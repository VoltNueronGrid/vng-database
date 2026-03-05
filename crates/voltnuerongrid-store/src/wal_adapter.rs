use crate::WalRecord;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum WalAdapterError {
    Io(std::io::Error),
    CorruptRecord(String),
}

impl From<std::io::Error> for WalAdapterError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub trait WalAdapter {
    fn append(&self, record: &WalRecord) -> Result<(), WalAdapterError>;
    fn read_all(&self) -> Result<Vec<WalRecord>, WalAdapterError>;
    fn truncate(&self) -> Result<(), WalAdapterError>;
}

#[derive(Debug, Clone)]
pub struct FileWalAdapter {
    wal_path: PathBuf,
}

impl FileWalAdapter {
    pub fn new<P: AsRef<Path>>(wal_path: P) -> Result<Self, WalAdapterError> {
        let wal_path = wal_path.as_ref().to_path_buf();
        if let Some(parent) = wal_path.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;
        Ok(Self { wal_path })
    }

    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }
}

impl WalAdapter for FileWalAdapter {
    fn append(&self, record: &WalRecord) -> Result<(), WalAdapterError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.wal_path)?;
        let line = encode_record(record);
        writeln!(file, "{line}")?;
        file.flush()?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<WalRecord>, WalAdapterError> {
        if !self.wal_path.exists() {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.wal_path)?;
        }
        let file = OpenOptions::new().read(true).open(&self.wal_path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            records.push(decode_record(&line)?);
        }
        Ok(records)
    }

    fn truncate(&self) -> Result<(), WalAdapterError> {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.wal_path)?;
        Ok(())
    }
}

fn encode_record(record: &WalRecord) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        record.sequence,
        record.timestamp_epoch_ms,
        escape_field(&record.key),
        escape_field(&record.value)
    )
}

fn decode_record(line: &str) -> Result<WalRecord, WalAdapterError> {
    let parts: Vec<&str> = line.splitn(4, '\t').collect();
    if parts.len() != 4 {
        return Err(WalAdapterError::CorruptRecord(line.to_string()));
    }

    let sequence = parts[0]
        .parse::<u64>()
        .map_err(|_| WalAdapterError::CorruptRecord(line.to_string()))?;
    let timestamp_epoch_ms = parts[1]
        .parse::<u128>()
        .map_err(|_| WalAdapterError::CorruptRecord(line.to_string()))?;
    let key = unescape_field(parts[2]);
    let value = unescape_field(parts[3]);

    Ok(WalRecord {
        sequence,
        timestamp_epoch_ms,
        key,
        value,
    })
}

fn escape_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape_field(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_wal_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vng-wal-adapter-test-{}-{}.log",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn appends_and_reads_records_from_disk() {
        let wal_path = unique_wal_path();
        let adapter = FileWalAdapter::new(&wal_path).expect("adapter");

        adapter
            .append(&WalRecord {
                sequence: 1,
                timestamp_epoch_ms: 1000,
                key: "region".to_string(),
                value: "us-east-1".to_string(),
            })
            .expect("append first");
        adapter
            .append(&WalRecord {
                sequence: 2,
                timestamp_epoch_ms: 1001,
                key: "metric\tname".to_string(),
                value: "line1\nline2".to_string(),
            })
            .expect("append second");

        let records = adapter.read_all().expect("read all");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[1].key, "metric\tname");
        assert_eq!(records[1].value, "line1\nline2");

        let _ = fs::remove_file(adapter.wal_path());
    }

    #[test]
    fn truncates_wal_file() {
        let wal_path = unique_wal_path();
        let adapter = FileWalAdapter::new(&wal_path).expect("adapter");
        adapter
            .append(&WalRecord {
                sequence: 1,
                timestamp_epoch_ms: 1000,
                key: "k".to_string(),
                value: "v".to_string(),
            })
            .expect("append");

        adapter.truncate().expect("truncate");
        let records = adapter.read_all().expect("read");
        assert!(records.is_empty());

        let _ = fs::remove_file(adapter.wal_path());
    }
}
