use std::path::PathBuf;

use serde::Serialize;

/// A single field-level validation problem, suitable for showing next to a form
/// field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

/// A key whose value is not what GitMate expected to find.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValueMismatch {
    pub file: PathBuf,
    pub key: String,
    pub expected: Vec<String>,
    pub found: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the git executable `{program}` could not be run: {message}")]
    GitNotFound { program: String, message: String },

    #[error("Git {found} is too old; GitMate requires Git {required} or newer")]
    GitTooOld { found: String, required: &'static str },

    #[error("`git {args}` failed{}: {stderr}", code.map(|c| format!(" (exit {c})")).unwrap_or_default())]
    GitFailed {
        args: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("{} is not inside a Git repository", path.display())]
    NotARepository { path: PathBuf },

    #[error(
        "Git refused to open {} because the repository is owned by another user. \
         GitMate does not change safe.directory for you. Git said: {message}",
        path.display()
    )]
    UnsafeRepository { path: PathBuf, message: String },

    #[error("invalid path {}: {reason}", path.display())]
    InvalidPath { path: PathBuf, reason: String },

    #[error("invalid profile: {}", .0.iter().map(|e| format!("{}: {}", e.field, e.message)).collect::<Vec<_>>().join("; "))]
    InvalidProfile(Vec<FieldError>),

    #[error("no profile named '{0}'")]
    ProfileNotFound(String),

    #[error("a profile named '{0}' already exists")]
    ProfileExists(String),

    #[error("profile '{id}' is used by {} rule(s) ({}); remove those rules first", rules.len(), rules.join(", "))]
    ProfileInUse { id: String, rules: Vec<String> },

    #[error("no GitMate rule for directory {0}")]
    RuleNotFound(String),

    #[error("the plan has {} unresolved conflict(s); nothing was changed", .0.len())]
    PlanHasConflicts(Vec<String>),

    #[error(
        "configuration changed since the plan was made ({} key(s) differ); nothing was changed. Re-run to see a fresh plan",
        .0.len()
    )]
    ConcurrentModification(Vec<ValueMismatch>),

    #[error(
        "cannot undo {op_id}: {} key(s) were changed outside GitMate since it ran; nothing was restored",
        conflicts.len()
    )]
    RollbackConflict {
        op_id: String,
        conflicts: Vec<ValueMismatch>,
    },

    #[error("cannot undo {op_id}: {reason}")]
    NotUndoable { op_id: String, reason: String },

    #[error("no operation '{0}' in history")]
    OperationNotFound(String),

    #[error("another GitMate process is changing configuration right now; try again")]
    Locked,

    #[error("{} already exists and was not installed by GitMate; use --force to replace it", path.display())]
    HookConflict { path: PathBuf },

    #[error("{} was not installed by GitMate, so it was left alone", path.display())]
    HookNotManaged { path: PathBuf },

    #[error(
        "Git runs hooks from {} (core.hooksPath), so a hook in the repository would never run; call `gitmate hook check` from the pre-commit hook there instead",
        hooks_path.display()
    )]
    HooksPathOverridden { hooks_path: PathBuf },

    #[error("applying {op_id} failed and {}: {message}", if *reverted { "the partial change was reverted" } else { "the partial change could NOT be fully reverted (see `gitmate history`)" })]
    ApplyFailed {
        op_id: String,
        message: String,
        reverted: bool,
    },

    #[error(
        "the change was applied, but recording {op_id} in the history failed: {message}; check `gitmate history`"
    )]
    JournalFailed { op_id: String, message: String },

    #[error("{} is not valid: {message}", path.display())]
    Parse { path: PathBuf, message: String },

    #[error("{action} {}: {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not determine the home directory; set HOME")]
    NoHomeDirectory,
}

impl Error {
    /// Stable machine-readable error code for JSON output and the desktop UI.
    pub fn code(&self) -> &'static str {
        match self {
            Error::GitNotFound { .. } => "git_not_found",
            Error::GitTooOld { .. } => "git_too_old",
            Error::GitFailed { .. } => "git_failed",
            Error::NotARepository { .. } => "not_a_repository",
            Error::UnsafeRepository { .. } => "unsafe_repository",
            Error::InvalidPath { .. } => "invalid_path",
            Error::InvalidProfile(_) => "invalid_profile",
            Error::ProfileNotFound(_) => "profile_not_found",
            Error::ProfileExists(_) => "profile_exists",
            Error::ProfileInUse { .. } => "profile_in_use",
            Error::RuleNotFound(_) => "rule_not_found",
            Error::PlanHasConflicts(_) => "plan_has_conflicts",
            Error::ConcurrentModification(_) => "concurrent_modification",
            Error::RollbackConflict { .. } => "rollback_conflict",
            Error::NotUndoable { .. } => "not_undoable",
            Error::OperationNotFound(_) => "operation_not_found",
            Error::Locked => "locked",
            Error::HookConflict { .. } => "hook_conflict",
            Error::HookNotManaged { .. } => "hook_not_managed",
            Error::HooksPathOverridden { .. } => "hooks_path_overridden",
            Error::ApplyFailed { .. } => "apply_failed",
            Error::JournalFailed { .. } => "journal_failed",
            Error::Parse { .. } => "parse_error",
            Error::Io { .. } => "io_error",
            Error::NoHomeDirectory => "no_home_directory",
        }
    }

    pub(crate) fn io(action: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
