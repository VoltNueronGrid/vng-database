//! Background Parquet flush — periodically exports OLTP row data to Parquet
//! files on disk so DataFusion can query them without a full in-memory scan.
//!
//! File layout:  `{data_dir}/parquet/{db_name}/{table_name}.parquet`
//!               If db_name is empty, uses `_default` as the directory name.
//!
//! The flush function groups rows by (db, table) then writes one Parquet file
//! per table. DataFusion in `execute_oltp_select` prefers Parquet files when
//! they exist (they are more recent or equal to the in-memory snapshot).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

/// Flush all rows from the in-memory row store to Parquet files.
/// Returns the number of tables flushed.
pub(crate) fn flush_rows_to_parquet(
    rows: &[(String, voltnuerongrid_store::mvcc::RowData)],
    data_dir: &str,
) -> usize {
    // Group rows by (db, table).
    // Row keys are either "db.table:row_id" (db-scoped) or "table:row_id" (no DB).
    let mut by_db_table: HashMap<(String, String), Vec<&voltnuerongrid_store::mvcc::RowData>> =
        HashMap::new();

    for (key, data) in rows {
        let (db, table) = parse_db_table_from_key(key);
        by_db_table.entry((db, table)).or_default().push(data);
    }

    let mut flushed = 0;
    for ((db, table), table_rows) in &by_db_table {
        let dir_name = if db.is_empty() { "_default" } else { db.as_str() };
        let dir = PathBuf::from(data_dir).join("parquet").join(dir_name);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("[parquet-flush] failed to create dir {}: {e}", dir.display());
            continue;
        }
        let parquet_path = dir.join(format!("{table}.parquet"));
        match write_rows_to_parquet(&parquet_path, table_rows) {
            Ok(n) => {
                tracing::debug!(
                    target: "vng.parquet",
                    table = %table,
                    rows = n,
                    path = %parquet_path.display(),
                    "flushed table to parquet"
                );
                flushed += 1;
            }
            Err(e) => {
                eprintln!("[parquet-flush] error writing {}: {e}", parquet_path.display());
            }
        }
    }
    flushed
}

/// Parse (db, table) from a storage key.
/// Key formats: "db.table:row_id" → ("db", "table")
///              "table:row_id"    → ("", "table")
fn parse_db_table_from_key(key: &str) -> (String, String) {
    // Find the colon that separates table-part from row-id.
    let colon = key.rfind(':').unwrap_or(key.len());
    let table_part = &key[..colon];
    // If there's a dot before the colon, the part before the dot is the db.
    if let Some(dot) = table_part.find('.') {
        let db = table_part[..dot].to_string();
        let table = table_part[dot + 1..].to_string();
        (db, table)
    } else {
        (String::new(), table_part.to_string())
    }
}

/// Write a slice of RowData maps to a Parquet file.
/// Discovers the column schema from all rows (union of all column names).
fn write_rows_to_parquet(
    path: &Path,
    rows: &[&voltnuerongrid_store::mvcc::RowData],
) -> Result<usize, Box<dyn std::error::Error>> {
    if rows.is_empty() {
        return Ok(0);
    }

    // Collect all column names across all rows (sorted for deterministic schema).
    let col_names: Vec<String> = rows
        .iter()
        .flat_map(|r| r.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    // Build Arrow schema: all columns are Utf8 strings (we store everything as strings).
    let fields: Vec<Field> = col_names
        .iter()
        .map(|name| Field::new(name.as_str(), DataType::Utf8, true))
        .collect();
    let schema = Arc::new(Schema::new(fields));

    // Build column arrays.
    let arrays: Vec<ArrayRef> = col_names
        .iter()
        .map(|col| {
            let values: Vec<Option<&str>> = rows
                .iter()
                .map(|row| row.get(col.as_str()).map(|s| s.as_str()))
                .collect();
            Arc::new(StringArray::from(values)) as ArrayRef
        })
        .collect();

    let batch = RecordBatch::try_new(schema.clone(), arrays)?;

    let file = std::fs::File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_db_table_key_with_db() {
        let (db, table) = parse_db_table_from_key("mydb.users:row-1");
        assert_eq!(db, "mydb");
        assert_eq!(table, "users");
    }

    #[test]
    fn parse_db_table_key_no_db() {
        let (db, table) = parse_db_table_from_key("orders:row-42");
        assert_eq!(db, "");
        assert_eq!(table, "orders");
    }

    #[test]
    fn flush_empty_rows_returns_zero() {
        let result = flush_rows_to_parquet(&[], "/tmp/vng-test-parquet");
        assert_eq!(result, 0);
    }
}
