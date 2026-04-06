"""Fix two corrupt code regions in main.rs."""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# ── Fix 1: planner_path block missing loop body ──────────────────────────────
old1 = (
    "        for stmt + S3-WS1-05: vectorized OLAP executor dispatch \u2014 if planner says olap/hybrid,\n"
    "    // run filter_batch (predicate pushdown from WHERE clause) then aggregate_batch over a\n"
    "    // columnar scan of the committed PagedRowStore snapshot.\n"
    "    let olap_agg_results: Option<Vec<OlapVecAggResult>> =\n"
    "        if matches!(planner_path.as_deref(), Some(\"olap\") | Some(\"hybrid\")) {\n"
    "            use voltnuerongrid_store::columnar::\n"
    "                vectorized_scan, aggregate_batch, filter_batch, VectorizedAggOp,\n"
    "            };\n"
    "            use voltnuerongrid_sql::{parse_one, Statement};\n"
    "            let rs = state.row_store.lock().expect(\"row_store lock olap dispatch\");\n"
    "            let snapshot_xid = rs.current_xid();"
)

# Check exact text around corruption
idx = content.find("        for stmt + S3-WS1-05:")
if idx == -1:
    print("ERROR: corruption 1 anchor not found")
    exit(1)

# Get the region to replace
region = content[idx: idx + 600]
print("=== Corruption 1 region ===")
print(repr(region[:300]))
