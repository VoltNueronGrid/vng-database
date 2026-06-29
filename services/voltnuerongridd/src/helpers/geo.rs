//! Geospatial engine — WKT parsing, PostGIS-compatible spatial predicates,
//! and an R-tree spatial index via the `rstar` crate.
//!
//! Supported functions (PostGIS-compatible subset):
//! - [`parse_wkt_point`]         — `POINT(x y)` / `POINT(x,y)`
//! - [`parse_envelope`]          — `ENVELOPE(minx,miny,maxx,maxy)` / `ST_MakeEnvelope(...)`
//! - [`st_distance`]             — Euclidean distance between two WKT points
//! - [`st_within`]               — point inside bounding-box envelope
//! - [`st_contains`]             — envelope contains point
//! - [`st_intersects`]           — any overlap between two geometries
//! - [`GeoIndex`]                — per-table R-tree for fast spatial queries

use std::collections::HashMap;
use rstar::{RTree, RTreeObject, PointDistance, AABB};

// ── R-tree item type ──────────────────────────────────────────────────────────

/// A named 2-D point that can be stored in an R-tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IndexedPoint {
    /// `[longitude/x, latitude/y]`
    pub(crate) coords: [f64; 2],
    /// The row key this geometry belongs to.
    pub(crate) row_key: String,
}

impl RTreeObject for IndexedPoint {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.coords)
    }
}

impl PointDistance for IndexedPoint {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        let dx = self.coords[0] - point[0];
        let dy = self.coords[1] - point[1];
        dx * dx + dy * dy
    }
}

// ── WKT parsing ───────────────────────────────────────────────────────────────

/// Parse `POINT(x y)` or `POINT(x, y)` (case-insensitive) into `[x, y]`.
pub(crate) fn parse_wkt_point(wkt: &str) -> Option<[f64; 2]> {
    let upper = wkt.trim().to_ascii_uppercase();
    let content = upper
        .trim()
        .strip_prefix("POINT")?
        .trim()
        .strip_prefix('(')?
        .trim_end()
        .strip_suffix(')')?;
    let parts: Vec<&str> = content
        .split(|c| c == ' ' || c == ',')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let x = parts[0].parse::<f64>().ok()?;
    let y = parts[1].parse::<f64>().ok()?;
    Some([x, y])
}

/// Parse a bounding-box envelope.
///
/// Accepted formats:
/// - `ENVELOPE(min_x,min_y,max_x,max_y)`
/// - `ST_MakeEnvelope(min_x,min_y,max_x,max_y)`
/// - `ST_MakeEnvelope(min_x,min_y,max_x,max_y,srid)` (SRID ignored)
///
/// Returns `(lower_left, upper_right)` as `([min_x, min_y], [max_x, max_y])`.
pub(crate) fn parse_envelope(wkt: &str) -> Option<([f64; 2], [f64; 2])> {
    let upper = wkt.trim().to_ascii_uppercase();
    let inner = if let Some(s) = upper.strip_prefix("ENVELOPE") {
        s.trim().strip_prefix('(')?.strip_suffix(')')?
    } else if let Some(s) = upper.strip_prefix("ST_MAKEENVELOPE") {
        s.trim().strip_prefix('(')?.strip_suffix(')')?
    } else {
        return None;
    };
    let parts: Vec<f64> = inner
        .split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    // Convention: min_x, min_y, max_x, max_y
    Some(([parts[0], parts[1]], [parts[2], parts[3]]))
}

// ── Spatial predicate functions ───────────────────────────────────────────────

/// Euclidean distance between two WKT points (or 0 if either cannot be parsed).
pub(crate) fn st_distance(wkt1: &str, wkt2: &str) -> f64 {
    let p1 = parse_wkt_point(wkt1).unwrap_or([0.0, 0.0]);
    let p2 = parse_wkt_point(wkt2).unwrap_or([0.0, 0.0]);
    let dx = p1[0] - p2[0];
    let dy = p1[1] - p2[1];
    (dx * dx + dy * dy).sqrt()
}

/// Returns `true` if the WKT point `geom_wkt` lies within the bounding box
/// described by `envelope_wkt`.
pub(crate) fn st_within(geom_wkt: &str, envelope_wkt: &str) -> bool {
    let pt = match parse_wkt_point(geom_wkt) {
        Some(p) => p,
        None => return false,
    };
    let (lo, hi) = match parse_envelope(envelope_wkt) {
        Some(e) => e,
        None => return false,
    };
    pt[0] >= lo[0] && pt[0] <= hi[0] && pt[1] >= lo[1] && pt[1] <= hi[1]
}

/// Returns `true` if the bounding-box `envelope_wkt` contains the point
/// `geom_wkt` (alias of `st_within` with swapped arguments).
pub(crate) fn st_contains(envelope_wkt: &str, geom_wkt: &str) -> bool {
    st_within(geom_wkt, envelope_wkt)
}

/// Minimal `ST_Intersects`: for two points they must be identical; for a point
/// and an envelope, delegates to `st_within`.
pub(crate) fn st_intersects(wkt1: &str, wkt2: &str) -> bool {
    // Try point-in-envelope in both directions.
    if st_within(wkt1, wkt2) || st_within(wkt2, wkt1) {
        return true;
    }
    // Fallback: identical points.
    parse_wkt_point(wkt1) == parse_wkt_point(wkt2)
}

// ── Spatial index ─────────────────────────────────────────────────────────────

/// Per-table R-tree spatial index.
///
/// Points are stored in a flat `Vec` as the canonical source; the R-tree is
/// built lazily via `rstar::RTree::bulk_load` and invalidated whenever a
/// point is inserted or removed.
#[derive(Default)]
pub(crate) struct GeoIndex {
    points: HashMap<String, Vec<IndexedPoint>>,
    /// Lazily built R-trees, keyed by table name.
    trees: HashMap<String, RTree<IndexedPoint>>,
}

impl GeoIndex {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Insert or update a WKT point geometry for a row.
    pub(crate) fn insert_point(
        &mut self,
        table: &str,
        row_key: &str,
        wkt: &str,
    ) -> Result<(), String> {
        let coords = parse_wkt_point(wkt)
            .ok_or_else(|| format!("geo_parse_error: cannot parse WKT point '{wkt}'"))?;
        let list = self.points.entry(table.to_string()).or_default();
        list.retain(|p| p.row_key != row_key);
        list.push(IndexedPoint { coords, row_key: row_key.to_string() });
        // Invalidate cached R-tree.
        self.trees.remove(table);
        Ok(())
    }

    /// Remove a row's geometry from the index.
    pub(crate) fn remove_point(&mut self, table: &str, row_key: &str) {
        if let Some(list) = self.points.get_mut(table) {
            list.retain(|p| p.row_key != row_key);
            self.trees.remove(table);
        }
    }

    /// Return the row keys of all points whose coordinates lie within the
    /// supplied bounding box `[min_x, min_y]` → `[max_x, max_y]`.
    pub(crate) fn within_envelope(
        &mut self,
        table: &str,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Vec<String> {
        self.ensure_tree(table);
        match self.trees.get(table) {
            None => vec![],
            Some(tree) => tree
                .locate_in_envelope(&AABB::from_corners([min_x, min_y], [max_x, max_y]))
                .map(|pt| pt.row_key.clone())
                .collect(),
        }
    }

    /// Return the K nearest row keys to a query point, with their distances.
    pub(crate) fn nearest_k(
        &mut self,
        table: &str,
        x: f64,
        y: f64,
        k: usize,
    ) -> Vec<(String, f64)> {
        self.ensure_tree(table);
        match self.trees.get(table) {
            None => vec![],
            Some(tree) => tree
                .nearest_neighbor_iter(&[x, y])
                .take(k)
                .map(|pt| {
                    let dist = pt.distance_2(&[x, y]).sqrt();
                    (pt.row_key.clone(), dist)
                })
                .collect(),
        }
    }

    pub(crate) fn point_count(&self, table: &str) -> usize {
        self.points.get(table).map(|v| v.len()).unwrap_or(0)
    }

    /// Build (or rebuild) the R-tree for a table from the canonical point list.
    fn ensure_tree(&mut self, table: &str) {
        if self.trees.contains_key(table) {
            return;
        }
        if let Some(pts) = self.points.get(table) {
            let tree = RTree::bulk_load(pts.clone());
            self.trees.insert(table.to_string(), tree);
        }
    }
}
