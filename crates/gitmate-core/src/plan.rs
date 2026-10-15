//! Change plans: what GitMate intends to write, shown before anything is
//! written.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What a plan (and the operation recorded from it) does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Write a profile's identity into one repository's local config.
    Assign {
        repository: PathBuf,
        profile: String,
        #[serde(default)]
        ssh: bool,
    },
    /// Add an `includeIf "gitdir:<directory>/"` rule.
    AddRule { directory: PathBuf, profile: String },
    /// Remove a GitMate-managed rule.
    RemoveRule {
        directory: PathBuf,
        profile: Option<String>,
    },
    /// Reverse a previous operation.
    Undo { operation: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignOptions {
    /// Also set the repository's `core.sshCommand` to use the profile's key.
    pub ssh: bool,
}

/// One key in one exact file going from `before` to `after` (all values, in
/// order; empty means unset).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigChange {
    pub file: PathBuf,
    /// Key as given to `git config` (subsection case preserved).
    pub key: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Add,
    Replace,
    Remove,
    Unchanged,
}

impl ConfigChange {
    pub fn kind(&self) -> ChangeKind {
        match (self.before.is_empty(), self.after.is_empty()) {
            _ if self.before == self.after => ChangeKind::Unchanged,
            (true, false) => ChangeKind::Add,
            (false, true) => ChangeKind::Remove,
            _ => ChangeKind::Replace,
        }
    }

    /// The inverse change, used by undo.
    pub fn inverse(&self) -> ConfigChange {
        ConfigChange {
            file: self.file.clone(),
            key: self.key.clone(),
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A conflict (blocks applying) or a warning (needs attention).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub message: String,
}

impl Issue {
    pub fn new(code: &'static str, message: impl Into<String>) -> Issue {
        Issue {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub action: Action,
    /// One-line description, e.g. "Assign profile 'work' to ~/code/app".
    pub summary: String,
    /// Changes that will be made, in order. Unchanged keys are omitted.
    pub changes: Vec<ConfigChange>,
    /// Problems that prevent applying the plan.
    pub conflicts: Vec<Issue>,
    /// Things the user should know before confirming.
    pub warnings: Vec<Issue>,
    /// Profile whose include file must exist and be current before applying.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensure_include: Option<String>,
}

impl Plan {
    pub fn new(action: Action, summary: String, changes: Vec<ConfigChange>) -> Plan {
        Plan {
            action,
            summary,
            changes: changes
                .into_iter()
                .filter(|c| c.kind() != ChangeKind::Unchanged)
                .collect(),
            conflicts: Vec::new(),
            warnings: Vec::new(),
            ensure_include: None,
        }
    }

    /// Nothing to write: the configuration already matches.
    pub fn is_noop(&self) -> bool {
        self.changes.is_empty()
    }

    /// Distinct files the plan writes, in first-use order.
    pub fn files(&self) -> Vec<&Path> {
        let mut files: Vec<&Path> = Vec::new();
        for change in &self.changes {
            if !files.contains(&change.file.as_path()) {
                files.push(&change.file);
            }
        }
        files
    }

    /// Whether a front end must ask before applying. Pure additions to a
    /// single repository's local config apply directly; anything that
    /// replaces or removes existing values, touches a non-repository file
    /// (e.g. the global config), or carries warnings needs confirmation.
    pub fn requires_confirmation(&self) -> bool {
        if self.is_noop() {
            return false;
        }
        let repo_local_only = matches!(self.action, Action::Assign { .. });
        !self.warnings.is_empty()
            || !repo_local_only
            || self.changes.iter().any(|c| c.kind() != ChangeKind::Add)
    }
}

/// Config keys GitMate is allowed to write. Anything else is refused at
/// apply time, as a second line of defence behind plan construction.
pub(crate) fn is_managed_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    if matches!(lower.as_str(), "user.name" | "user.email" | "core.sshcommand") {
        return true;
    }
    // includeIf.<condition>.path where the condition is a gitdir match.
    lower.starts_with("includeif.gitdir") && lower.ends_with(".path")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(before: &[&str], after: &[&str]) -> ConfigChange {
        ConfigChange {
            file: PathBuf::from("/f"),
            key: "user.email".into(),
            before: before.iter().map(|s| s.to_string()).collect(),
            after: after.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn change_kinds() {
        assert_eq!(change(&[], &["a"]).kind(), ChangeKind::Add);
        assert_eq!(change(&["a"], &[]).kind(), ChangeKind::Remove);
        assert_eq!(change(&["a"], &["b"]).kind(), ChangeKind::Replace);
        assert_eq!(change(&["a"], &["a"]).kind(), ChangeKind::Unchanged);
        assert_eq!(change(&["a", "b"], &["a"]).kind(), ChangeKind::Replace);
    }

    #[test]
    fn unchanged_keys_are_dropped_from_plans() {
        let plan = Plan::new(
            Action::Undo {
                operation: "x".into(),
            },
            "s".into(),
            vec![change(&["a"], &["a"])],
        );
        assert!(plan.is_noop());
        assert!(!plan.requires_confirmation());
    }

    #[test]
    fn managed_keys() {
        assert!(is_managed_key("user.email"));
        assert!(is_managed_key("core.sshCommand"));
        assert!(is_managed_key("includeIf.gitdir:/home/a b/.path"));
        assert!(is_managed_key("includeIf.gitdir/i:C:/w/.path"));
        assert!(!is_managed_key("core.hooksPath"));
        assert!(!is_managed_key("includeIf.onbranch:main.path"));
        assert!(!is_managed_key("alias.x"));
    }
}
