#![forbid(unsafe_code)]

use crate::{
    ConnectorDescriptor, ConnectorDirection, IngestFormat, IngestRecord, IngestionConnector,
};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float32Type, Float64Type, Int16Type, Int32Type, Int64Type};
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_schema::DataType;
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde_json::{Map, Number, Value};

/// Connector for Apache Parquet files.
///
/// The first column supplies the record key (stringified). Remaining columns are encoded as a
/// JSON object keyed by schema field names.
#[derive(Debug, Clone)]
pub struct ParquetConnector {
    descriptor: ConnectorDescriptor,
    records: Vec<IngestRecord>,
}

impl ParquetConnector {
    pub fn new(id: &str, display_name: &str) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: id.to_string(),
                display_name: display_name.to_string(),
                format: IngestFormat::Parquet,
                direction: ConnectorDirection::Inbound,
            },
            records: Vec::new(),
        }
    }

    /// Parse Parquet bytes into in-memory records.
    pub fn load_parquet_bytes(&mut self, bytes: &[u8]) -> Result<usize, String> {
        self.records.clear();
        let buffer = Bytes::copy_from_slice(bytes);
        let builder =
            ParquetRecordBatchReaderBuilder::try_new(buffer).map_err(|e| format!("parquet: {e}"))?;
        let mut reader = builder.build().map_err(|e| format!("parquet reader: {e}"))?;
        while let Some(batch) = reader.next() {
            let batch = batch.map_err(|e| format!("parquet batch: {e}"))?;
            self.append_batch(&batch)?;
        }
        Ok(self.records.len())
    }

    fn append_batch(&mut self, batch: &RecordBatch) -> Result<(), String> {
        if batch.num_columns() == 0 {
            return Err("parquet file has no columns".to_string());
        }
        let schema = batch.schema();
        let fields = schema.fields();
        for row in 0..batch.num_rows() {
            let key = scalar_as_string(batch.column(0), row)?;
            let payload = if batch.num_columns() == 1 {
                Value::Object(Map::new())
            } else {
                let mut map = Map::new();
                for col_idx in 1..batch.num_columns() {
                    let name = fields[col_idx].name().clone();
                    let val = scalar_to_json(batch.column(col_idx), row)?;
                    map.insert(name, val);
                }
                Value::Object(map)
            };
            let payload_str = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
            self.records.push(IngestRecord {
                key,
                payload: payload_str,
            });
        }
        Ok(())
    }
}

impl IngestionConnector for ParquetConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        self.records.iter().take(max_items).cloned().collect()
    }
}

fn scalar_as_string(col: &ArrayRef, row: usize) -> Result<String, String> {
    if col.is_null(row) {
        return Err("null value in parquet key column".to_string());
    }
    match col.data_type() {
        DataType::Utf8 => Ok(col.as_string::<i32>().value(row).to_string()),
        DataType::LargeUtf8 => Ok(col.as_string::<i64>().value(row).to_string()),
        DataType::Boolean => Ok(if col.as_boolean().value(row) {
            "true"
        } else {
            "false"
        }
        .to_string()),
        DataType::Int16 => Ok(col.as_primitive::<Int16Type>().value(row).to_string()),
        DataType::Int32 => Ok(col.as_primitive::<Int32Type>().value(row).to_string()),
        DataType::Int64 => Ok(col.as_primitive::<Int64Type>().value(row).to_string()),
        DataType::Float32 => Ok(f64::from(col.as_primitive::<Float32Type>().value(row)).to_string()),
        DataType::Float64 => Ok(col.as_primitive::<Float64Type>().value(row).to_string()),
        other => Err(format!("unsupported parquet key type: {other:?}")),
    }
}

fn scalar_to_json(col: &ArrayRef, row: usize) -> Result<Value, String> {
    if col.is_null(row) {
        return Ok(Value::Null);
    }
    match col.data_type() {
        DataType::Utf8 => Ok(Value::String(col.as_string::<i32>().value(row).to_string())),
        DataType::LargeUtf8 => Ok(Value::String(col.as_string::<i64>().value(row).to_string())),
        DataType::Boolean => Ok(Value::Bool(col.as_boolean().value(row))),
        DataType::Int16 => Ok(Value::Number(Number::from(col.as_primitive::<Int16Type>().value(row)))),
        DataType::Int32 => Ok(Value::Number(Number::from(col.as_primitive::<Int32Type>().value(row)))),
        DataType::Int64 => Ok(Value::Number(Number::from(col.as_primitive::<Int64Type>().value(row)))),
        DataType::Float32 => Ok(
            Number::from_f64(f64::from(col.as_primitive::<Float32Type>().value(row)))
                .map(Value::Number)
                .unwrap_or(Value::Null),
        ),
        DataType::Float64 => Ok(
            Number::from_f64(col.as_primitive::<Float64Type>().value(row))
                .map(Value::Number)
                .unwrap_or(Value::Null),
        ),
        other => Err(format!("unsupported parquet payload type: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Int32Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};
    use parquet::arrow::ArrowWriter;
    use std::sync::Arc;

    fn sample_parquet_bytes() -> Vec<u8> {
        let id = StringArray::from(vec!["a", "b"]);
        let amt = Int32Array::from(vec![10, 20]);
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("amount", DataType::Int32, false),
        ]));
        let batch = RecordBatch::try_new(schema.clone(), vec![
            Arc::new(id) as ArrayRef,
            Arc::new(amt) as ArrayRef,
        ])
        .expect("batch");
        let mut buffer = Vec::new();
        {
            let mut writer = ArrowWriter::try_new(&mut buffer, schema, None).expect("writer");
            writer.write(&batch).expect("write");
            writer.close().expect("close");
        }
        buffer
    }

    #[test]
    fn loads_simple_parquet_rows() {
        let bytes = sample_parquet_bytes();
        let mut connector = ParquetConnector::new("pq-test", "Parquet Test");
        let count = connector.load_parquet_bytes(&bytes).expect("load");
        assert_eq!(count, 2);
        let batch = connector.read_batch(10);
        assert_eq!(batch[0].key, "a");
        assert_eq!(batch[0].payload, r#"{"amount":10}"#);
        assert_eq!(batch[1].key, "b");
        assert_eq!(batch[1].payload, r#"{"amount":20}"#);
    }

    #[test]
    fn descriptor_reflects_parquet_format() {
        let conn = ParquetConnector::new("pq-desc", "PQ Desc");
        assert_eq!(conn.descriptor().format, IngestFormat::Parquet);
    }
}
