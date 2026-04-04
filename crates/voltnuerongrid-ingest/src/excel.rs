#![forbid(unsafe_code)]

use crate::{
    ConnectorDescriptor, ConnectorDirection, IngestFormat, IngestRecord, IngestionConnector,
};
use calamine::{open_workbook_auto_from_rs, Data, Reader, Sheets};
use serde_json::{Map, Number, Value};
use std::io::Cursor;

/// Connector for Excel `.xlsx` workbooks (first worksheet only).
///
/// The first row is treated as headers. The first column holds the record key; remaining columns
/// are serialized as JSON using the header names from the first row.
#[derive(Debug, Clone)]
pub struct ExcelConnector {
    descriptor: ConnectorDescriptor,
    records: Vec<IngestRecord>,
}

impl ExcelConnector {
    pub fn new(id: &str, display_name: &str) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: id.to_string(),
                display_name: display_name.to_string(),
                format: IngestFormat::Excel,
                direction: ConnectorDirection::Inbound,
            },
            records: Vec::new(),
        }
    }

    /// Parse XLSX bytes into in-memory records.
    pub fn load_xlsx_bytes(&mut self, bytes: &[u8]) -> Result<usize, String> {
        self.records.clear();
        let cursor = Cursor::new(bytes.to_vec());
        let mut workbook: Sheets<_> =
            open_workbook_auto_from_rs(cursor).map_err(|e| format!("excel: {e}"))?;
        let sheet = workbook
            .sheet_names()
            .first()
            .ok_or_else(|| "workbook has no worksheets".to_string())?
            .to_string();
        let range = workbook
            .worksheet_range(&sheet)
            .map_err(|e| format!("excel worksheet: {e}"))?;

        let mut rows = range.rows();
        let header = rows.next().ok_or_else(|| "worksheet is empty".to_string())?;
        if header.is_empty() {
            return Err("header row is empty".to_string());
        }

        let headers: Vec<String> = header
            .iter()
            .map(|cell| cell_string(cell))
            .collect::<Result<_, _>>()?;

        for row in rows {
            let key = row
                .get(0)
                .map(cell_string)
                .transpose()?
                .unwrap_or_default()
                .trim()
                .to_string();
            if key.is_empty() {
                continue;
            }

            let mut map = Map::new();
            for (idx, header_name) in headers.iter().enumerate().skip(1) {
                let value = row
                    .get(idx)
                    .map(data_to_json)
                    .transpose()?
                    .unwrap_or(Value::Null);
                map.insert(header_name.clone(), value);
            }

            let payload = serde_json::to_string(&Value::Object(map)).map_err(|e| e.to_string())?;
            self.records.push(IngestRecord { key, payload });
        }

        Ok(self.records.len())
    }
}

impl IngestionConnector for ExcelConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        self.records.iter().take(max_items).cloned().collect()
    }
}

fn cell_string(cell: &Data) -> Result<String, String> {
    Ok(match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        Data::Float(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::DateTime(value) => value.to_string(),
        Data::DateTimeIso(value) => value.clone(),
        Data::DurationIso(value) => value.clone(),
        Data::Error(err) => return Err(format!("excel cell error: {err:?}")),
    })
}

fn data_to_json(cell: &Data) -> Result<Value, String> {
    Ok(match cell {
        Data::Empty => Value::Null,
        Data::String(value) => Value::String(value.clone()),
        Data::Float(value) => Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Data::Int(value) => Value::Number(Number::from(*value)),
        Data::Bool(value) => Value::Bool(*value),
        Data::DateTime(value) => Value::String(value.to_string()),
        Data::DateTimeIso(value) => Value::String(value.clone()),
        Data::DurationIso(value) => Value::String(value.clone()),
        Data::Error(err) => return Err(format!("excel cell error: {err:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::{Format, Workbook};

    fn sample_xlsx_bytes() -> Vec<u8> {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        let header = Format::new().set_bold();
        sheet.write_string_with_format(0, 0, "id", &header).unwrap();
        sheet.write_string_with_format(0, 1, "name", &header).unwrap();
        sheet.write_number(1, 0, 1).unwrap();
        sheet.write_string(1, 1, "Alice").unwrap();
        sheet.write_number(2, 0, 2).unwrap();
        sheet.write_string(2, 1, "Bob").unwrap();
        workbook.save_to_buffer().expect("buffer")
    }

    #[test]
    fn loads_simple_xlsx_rows() {
        let bytes = sample_xlsx_bytes();
        let mut connector = ExcelConnector::new("xlsx-test", "Excel Test");
        let count = connector.load_xlsx_bytes(&bytes).expect("load");
        assert_eq!(count, 2);
        let batch = connector.read_batch(10);
        assert_eq!(batch[0].key, "1");
        assert_eq!(batch[0].payload, r#"{"name":"Alice"}"#);
        assert_eq!(batch[1].key, "2");
        assert_eq!(batch[1].payload, r#"{"name":"Bob"}"#);
    }

    #[test]
    fn descriptor_reflects_excel_format() {
        let conn = ExcelConnector::new("xlsx-desc", "Excel Desc");
        assert_eq!(conn.descriptor().format, IngestFormat::Excel);
    }
}
