//! Applying plans: lock, re-check, back up, journal, write, verify, and
//! revert on failure. See `docs/architecture.md`.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::error::{Error, Result, ValueMismatch};
use crate::fsutil::{self, WriterLock};
use crate::git::Git;
use crate::journal::{self, Backup, Journal, OperationRecord, OperationStatus};
use crate::paths::AppPaths;
use crate::plan::{self, Action, ConfigChange, Issue, Plan};

/// Number of most recent operations whose file backups are kept.
const BACKUP_RETENTION: usize = 25;

pub(crate) struct Transactions<'a> {
    pub git: &'a Git,
    pub paths: &'a AppPaths,
    pub lock_timeout: Duration,
}

/// Keys whose current value differs from `expected(change)`.
fn mismatches(
    git: &Git,
    changes: &[ConfigChange],
    expected: impl Fn(&ConfigChange) -> &Vec<String>,
) -> Result<Vec<ValueMismatch>> {
    let mut out = Vec::new();
    for change in changes {
        let found = git.file_get_all(&change.file, &change.key)?;
        if &found != expected(change) {
            out.push(ValueMismatch {
                file: change.file.clone(),
                key: change.key.clone(),
                expected: expected(change).clone(),
                found,
            });
        }
    }
    Ok(out)
}

impl Transactions<'_> {
    pub fn journal(&self) -> Journal {
        Journal::new(self.paths.operations_dir())
    }

    pub fn lock(&self) -> Result<WriterLock> {
        WriterLock::acquire(&self.paths.lock_file(), self.lock_timeout)
    }

    /// Apply `plan`. `prepare` runs under the lock after the re-check and
    /// before any Git config write (used to regenerate include files).
    /// Returns `None` for a no-op plan.
    pub fn apply(
        &self,
        plan: &Plan,
        prepare: impl FnOnce() -> Result<()>,
    ) -> Result<Option<OperationRecord>> {
        if let Some(bad) = plan.changes.iter().find(|c| !plan::is_managed_key(&c.key)) {
            return Err(Error::PlanHasConflicts(vec![format!(
                "{} is not a key GitMate manages",
                bad.key
            )]));
        }
        if !plan.conflicts.is_empty() {
            return Err(Error::PlanHasConflicts(
                plan.conflicts.iter().map(|c| c.message.clone()).collect(),
            ));
        }
        if plan.is_noop() {
            return Ok(None);
        }

        let _lock = self.lock()?;
        let changed = mismatches(self.git, &plan.changes, |c| &c.before)?;
        if !changed.is_empty() {
            return Err(Error::ConcurrentModification(changed));
        }
        prepare()?;

        let journal = self.journal();
        let id = journal::new_operation_id(SystemTime::now());
        let backups = self.back_up(&id, &plan.files())?;
        let mut record = OperationRecord {
            id: id.clone(),
            seq: journal.next_seq()?,
            created_at: journal::now_secs(),
            action: plan.action.clone(),
            summary: plan.summary.clone(),
            status: OperationStatus::Pending,
            error: None,
            undone_by: None,
            changes: plan.changes.clone(),
            backups,
            file_hashes: Vec::new(),
        };
        journal.write(&record)?;

        // Re-check once more right before writing: `prepare`, the backup and
        // the journal write all take time, and an edit made meanwhile by
        // another tool must not be overwritten.
        let changed = match mismatches(self.git, &plan.changes, |c| &c.before) {
            Ok(changed) => changed,
            Err(err) => return Err(self.fail(&mut record, &[], err.to_string())),
        };
        if !changed.is_empty() {
            record.status = OperationStatus::Failed;
            record.error = Some("configuration changed before writing; nothing was written".into());
            let _ = journal.write(&record);
            return Err(Error::ConcurrentModification(changed));
        }

        for (i, change) in plan.changes.iter().enumerate() {
            if let Err(err) = self.git.file_set_all(&change.file, &change.key, &change.after) {
                return Err(self.fail(&mut record, &plan.changes[..=i], err.to_string()));
            }
        }
        let wrong = match mismatches(self.git, &plan.changes, |c| &c.after) {
            Ok(wrong) => wrong,
            Err(err) => {
                let message = format!("verification failed: {err}");
                return Err(self.fail(&mut record, &plan.changes, message));
            }
        };
        if !wrong.is_empty() {
            let message = format!(
                "verification failed: {} reads back {:?}, expected {:?}",
                wrong[0].key, wrong[0].found, wrong[0].expected
            );
            return Err(self.fail(&mut record, &plan.changes, message));
        }

        record.status = OperationStatus::Applied;
        record.file_hashes = plan
            .files()
            .into_iter()
            .map(|f| (f.to_path_buf(), file_hash(f)))
            .collect();
        let recorded = journal.write(&record).and_then(|()| match &plan.action {
            // Still under the lock, so no other writer sees the original
            // operation as undoable after its undo has been applied.
            Action::Undo { operation } => self.mark_undone(operation, &id),
            _ => Ok(()),
        });
        if let Err(err) = recorded {
            return Err(Error::JournalFailed {
                op_id: id,
                message: err.to_string(),
            });
        }
        self.prune_backups();
        Ok(Some(record))
    }

    /// Revert `applied` (in reverse order), record the failure, and build the
    /// error to return.
    fn fail(&self, record: &mut OperationRecord, applied: &[ConfigChange], message: String) -> Error {
        for change in applied.iter().rev() {
            let _ = self.git.file_set_all(&change.file, &change.key, &change.before);
        }
        let reverted = mismatches(self.git, applied, |c| &c.before).is_ok_and(|m| m.is_empty());
        record.status = OperationStatus::Failed;
        record.error = Some(if reverted {
            format!("{message} (reverted)")
        } else {
            format!("{message} (NOT fully reverted)")
        });
        let message = match self.journal().write(record) {
            Ok(()) => message,
            Err(err) => format!("{message}; recording the failure in the history also failed: {err}"),
        };
        Error::ApplyFailed {
            op_id: record.id.clone(),
            message,
            reverted,
        }
    }

    fn back_up(&self, id: &str, files: &[&Path]) -> Result<Vec<Backup>> {
        let dir = self.paths.backups_dir().join(id);
        let mut backups = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let copy = match fsutil::read_optional(file)? {
                Some(bytes) => {
                    fsutil::ensure_private_dir(&self.paths.state_dir)?;
                    fsutil::ensure_private_dir(&self.paths.backups_dir())?;
                    fsutil::ensure_private_dir(&dir)?;
                    let name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "config".into());
                    let dest = dir.join(format!("{i}-{name}"));
                    fsutil::atomic_write(&dest, &bytes)?;
                    Some(dest)
                }
                None => None,
            };
            backups.push(Backup {
                file: file.to_path_buf(),
                copy,
            });
        }
        Ok(backups)
    }

    /// Keep backups only for the most recent operations. Failures here are
    /// not fatal: stale backups are harmless.
    fn prune_backups(&self) {
        let Ok(entries) = std::fs::read_dir(self.paths.backups_dir()) else {
            return;
        };
        let mut dirs: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        dirs.sort();
        let excess = dirs.len().saturating_sub(BACKUP_RETENTION);
        for dir in dirs.into_iter().take(excess) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Build the plan that reverses operation `id`, with conflicts for every
    /// key that no longer holds the value GitMate wrote.
    pub fn plan_undo(&self, id: &str) -> Result<(Plan, Vec<ValueMismatch>)> {
        let record = self.journal().get(id)?;
        let not_undoable = |reason: String| Error::NotUndoable {
            op_id: id.to_string(),
            reason,
        };
        if let Action::Undo { .. } = record.action {
            return Err(not_undoable(
                "it is itself an undo; run the original command again instead".into(),
            ));
        }
        let partial = match record.status {
            OperationStatus::Applied => false,
            OperationStatus::Pending | OperationStatus::Failed => true,
            OperationStatus::Undone => {
                return Err(not_undoable(format!(
                    "it was already undone{}",
                    record
                        .undone_by
                        .as_deref()
                        .map(|u| format!(" by {u}"))
                        .unwrap_or_default()
                )));
            }
        };

        // Undo must go newest first for any key a later operation also
        // changed: values alone cannot tell whose write is in effect.
        let history = self.journal().list()?;
        let later = history.iter().skip_while(|op| op.id != record.id).skip(1);
        for op in later
            .filter(|op| op.status != OperationStatus::Undone && !matches!(op.action, Action::Undo { .. }))
        {
            for theirs in op.changes.iter().filter(|c| {
                record
                    .changes
                    .iter()
                    .any(|mine| mine.file == c.file && mine.key == c.key)
            }) {
                let in_effect = op.status == OperationStatus::Applied
                    || self.git.file_get_all(&theirs.file, &theirs.key)? == theirs.after;
                if in_effect {
                    return Err(not_undoable(format!(
                        "the later operation {} ({}) also changed {}; undo that one first",
                        op.id, op.summary, theirs.key
                    )));
                }
            }
        }

        let mut changes = Vec::new();
        let mut conflicts = Vec::new();
        for change in record.changes.iter().rev() {
            let found = self.git.file_get_all(&change.file, &change.key)?;
            if found == change.after {
                changes.push(change.inverse());
            } else if partial && found == change.before {
                // Never written (interrupted or already reverted).
            } else {
                conflicts.push(ValueMismatch {
                    file: change.file.clone(),
                    key: change.key.clone(),
                    expected: change.after.clone(),
                    found,
                });
            }
        }
        if partial && changes.is_empty() && conflicts.is_empty() {
            return Err(not_undoable("none of its changes are in effect".into()));
        }

        let mut plan = Plan::new(
            Action::Undo {
                operation: record.id.clone(),
            },
            format!("Undo: {}", record.summary),
            changes,
        );
        for m in &conflicts {
            plan.conflicts.push(Issue::new(
                "changed_externally",
                format!(
                    "{} in {} is now {} but GitMate set it to {}; it was changed outside GitMate",
                    m.key,
                    m.file.display(),
                    show_values(&m.found),
                    show_values(&m.expected)
                ),
            ));
        }
        for (file, hash) in &record.file_hashes {
            if file_hash(file) != *hash && conflicts.iter().all(|c| &c.file != file) {
                plan.warnings.push(Issue::new(
                    "file_edited_since",
                    format!(
                        "{} was also edited after this operation; only GitMate's keys are restored and other edits are kept",
                        file.display()
                    ),
                ));
            }
        }
        Ok((plan, conflicts))
    }

    /// Mark `id` as undone by `by`.
    pub fn mark_undone(&self, id: &str, by: &str) -> Result<()> {
        let journal = self.journal();
        let mut record = journal.get(id)?;
        record.status = OperationStatus::Undone;
        record.undone_by = Some(by.to_string());
        journal.write(&record)
    }
}

pub(crate) fn show_values(values: &[String]) -> String {
    match values {
        [] => "(unset)".to_string(),
        [one] => format!("\"{one}\""),
        many => format!(
            "[{}]",
            many.iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn file_hash(file: &Path) -> String {
    match std::fs::read(file) {
        Ok(bytes) => format!("{:016x}", fsutil::fnv1a64(&bytes)),
        Err(_) => "missing".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn edits_made_while_preparing_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            config_dir: dir.path().join("config"),
            state_dir: dir.path().join("state"),
            home: dir.path().to_path_buf(),
        };
        let git = Git::default().env("GIT_CONFIG_NOSYSTEM", "1");
        let file = dir.path().join("repo.config");
        let tx = Transactions {
            git: &git,
            paths: &paths,
            lock_timeout: Duration::from_secs(1),
        };
        let plan = Plan::new(
            Action::Assign {
                repository: PathBuf::from("/r"),
                profile: "work".into(),
                ssh: false,
            },
            "assign".into(),
            vec![ConfigChange {
                file: file.clone(),
                key: "user.email".into(),
                before: vec![],
                after: vec!["work@acme.example".into()],
            }],
        );

        let result = tx.apply(&plan, || {
            git.file_set_all(&file, "user.email", &["raced@example.com".to_string()])
        });

        assert!(
            matches!(result, Err(Error::ConcurrentModification(_))),
            "{result:?}"
        );
        assert_eq!(
            git.file_get_all(&file, "user.email").unwrap(),
            vec!["raced@example.com"]
        );
    }
}
