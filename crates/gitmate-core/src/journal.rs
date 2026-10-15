//! Operation journal: one TOML file per applied (or attempted) plan.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsutil;
use crate::plan::{Action, ConfigChange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Writing started but did not finish (e.g. the process was killed).
    Pending,
    Applied,
    /// Writing failed; see `error` and whether it was reverted.
    Failed,
    /// Reversed by a later undo operation.
    Undone,
}

/// A copy of a file taken before an operation changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    pub file: PathBuf,
    /// Where the copy is; `None` if the file did not exist yet.
    pub copy: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub id: String,
    /// Position in the journal, assigned under the writer lock. Ids only
    /// have one-second resolution, so this is what orders operations.
    #[serde(default)]
    pub seq: u64,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
    pub action: Action,
    pub summary: String,
    pub status: OperationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_by: Option<String>,
    #[serde(default)]
    pub changes: Vec<ConfigChange>,
    #[serde(default)]
    pub backups: Vec<Backup>,
    /// FNV-1a hash (hex) of each written file right after the operation,
    /// used to tell the user that a file was also edited by someone else.
    #[serde(default)]
    pub file_hashes: Vec<(PathBuf, String)>,
}

impl OperationRecord {
    pub fn created_at_utc(&self) -> String {
        format_utc(self.created_at)
    }
}

/// Generate a unique operation id: `20261015T155501Z-3f9a1c`. Ids sort by
/// second only; [`OperationRecord::seq`] gives the exact order.
pub fn new_operation_id(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let mixed = u64::from(since.subsec_nanos()) ^ (u64::from(std::process::id()) << 20);
    let compact: String = format_utc(since.as_secs())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{compact}-{:06x}", mixed & 0xff_ffff)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format Unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (proleptic Gregorian, UTC).
pub fn format_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Reads and writes operation records under `state_dir/operations`.
#[derive(Debug, Clone)]
pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub fn new(dir: impl Into<PathBuf>) -> Journal {
        Journal { dir: dir.into() }
    }

    fn path_for(&self, id: &str) -> Result<PathBuf> {
        let valid =
            !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !valid {
            return Err(Error::OperationNotFound(id.to_string()));
        }
        Ok(self.dir.join(format!("{id}.toml")))
    }

    pub fn write(&self, record: &OperationRecord) -> Result<()> {
        fsutil::ensure_private_dir(&self.dir)?;
        let path = self.path_for(&record.id)?;
        let text = toml::to_string_pretty(record).map_err(|e| Error::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        fsutil::atomic_write(&path, text.as_bytes())
    }

    pub fn get(&self, id: &str) -> Result<OperationRecord> {
        let path = self.path_for(id)?;
        match fsutil::read_optional(&path)? {
            Some(bytes) => parse_record(&path, &bytes),
            None => Err(Error::OperationNotFound(id.to_string())),
        }
    }

    /// The sequence number for the next record. Call with the writer lock
    /// held.
    pub fn next_seq(&self) -> Result<u64> {
        Ok(self.list()?.iter().map(|r| r.seq).max().unwrap_or(0) + 1)
    }

    /// All records, oldest first.
    pub fn list(&self) -> Result<Vec<OperationRecord>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io("could not list", &self.dir, e)),
        };
        let mut records = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|e| Error::io("could not list", &self.dir, e))?
                .path();
            if path.extension().is_some_and(|e| e == "toml") {
                let bytes = std::fs::read(&path).map_err(|e| Error::io("could not read", &path, e))?;
                records.push(parse_record(&path, &bytes)?);
            }
        }
        records.sort_by(|a, b| (a.seq, &a.id).cmp(&(b.seq, &b.id)));
        Ok(records)
    }
}

fn parse_record(path: &Path, bytes: &[u8]) -> Result<OperationRecord> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Parse {
        path: path.to_path_buf(),
        message: "not valid UTF-8".into(),
    })?;
    toml::from_str(text).map_err(|e| Error::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_utc_dates() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_792_060_608), "2026-10-15T10:36:48Z");
    }

    #[test]
    fn operation_ids_sort_by_time() {
        let a = new_operation_id(UNIX_EPOCH + Duration::from_secs(1_792_060_608));
        let b = new_operation_id(UNIX_EPOCH + Duration::from_secs(1_792_060_609));
        assert!(a.starts_with("20261015T103648Z-"), "{a}");
        assert!(a < b);
    }

    #[test]
    fn journal_round_trip_and_rejects_path_like_ids() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("ops"));
        let record = OperationRecord {
            id: "20261015T095648Z-abc123".into(),
            seq: 1,
            created_at: 1,
            action: Action::Undo {
                operation: "x".into(),
            },
            summary: "s".into(),
            status: OperationStatus::Applied,
            error: None,
            undone_by: None,
            changes: vec![ConfigChange {
                file: PathBuf::from("/a b/.gitconfig"),
                key: "includeIf.gitdir:/w/.path".into(),
                before: vec![],
                after: vec!["/inc".into()],
            }],
            backups: vec![Backup {
                file: PathBuf::from("/a"),
                copy: None,
            }],
            file_hashes: vec![(PathBuf::from("/a"), "00ff".into())],
        };
        journal.write(&record).unwrap();
        assert_eq!(journal.get(&record.id).unwrap(), record);
        assert_eq!(journal.list().unwrap(), vec![record]);
        assert!(matches!(
            journal.get("../../etc/passwd"),
            Err(Error::OperationNotFound(_))
        ));
    }

    #[test]
    fn records_list_in_sequence_order_not_id_order() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("ops"));
        assert_eq!(journal.next_seq().unwrap(), 1);
        // Same second: the random suffix of the id says nothing about order.
        for (seq, id) in [(1, "20261015T095648Z-ffffff"), (2, "20261015T095648Z-000000")] {
            let record = OperationRecord {
                id: id.into(),
                seq,
                created_at: 1,
                action: Action::Undo {
                    operation: "x".into(),
                },
                summary: "s".into(),
                status: OperationStatus::Applied,
                error: None,
                undone_by: None,
                changes: vec![],
                backups: vec![],
                file_hashes: vec![],
            };
            journal.write(&record).unwrap();
        }
        let ids: Vec<String> = journal.list().unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["20261015T095648Z-ffffff", "20261015T095648Z-000000"]);
        assert_eq!(journal.next_seq().unwrap(), 3);
    }
}
