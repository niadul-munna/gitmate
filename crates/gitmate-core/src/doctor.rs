//! `gitmate doctor`: environment-wide checks plus an optional repository
//! report.

use std::path::PathBuf;

use serde::Serialize;

use crate::git::{Git, GitVersion};
use crate::includes;
use crate::journal::{OperationRecord, OperationStatus};
use crate::paths::AppPaths;
use crate::profile::Profile;
use crate::report::{Finding, RepoReport, Severity};
use crate::rules::{self, Rule};
use crate::ssh::{self, SshHostConfig};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoctorOptions {
    /// Run `ssh -G` for SSH remote hosts of the inspected repository. This
    /// evaluates the user's ssh_config (including `Match exec`), so it is
    /// opt-in. It never connects to the host.
    pub ssh_config: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub git_version: Option<GitVersion>,
    pub paths: AppPaths,
    /// Global file GitMate writes rules to.
    pub global_config: PathBuf,
    pub profile_count: usize,
    pub rules: Vec<Rule>,
    pub repository: Option<RepoReport>,
    pub ssh_hosts: Vec<SshHostConfig>,
    pub findings: Vec<Finding>,
}

impl DoctorReport {
    pub fn worst(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).min()
    }
}

pub(crate) fn rule_findings(
    git: &Git,
    paths: &AppPaths,
    profiles: &[Profile],
    rules: &[Rule],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for rule in rules.iter().filter(|r| r.is_managed()) {
        let id = rule.profile.as_deref().unwrap_or_default();
        let dir = rule
            .directory
            .as_deref()
            .map(|d| paths.display(d))
            .unwrap_or_else(|| rule.condition.clone());
        match profiles.iter().find(|p| p.id == id) {
            None => findings.push(
                Finding::new(
                    Severity::Error,
                    "rule_missing_profile",
                    format!("The rule for {dir} includes profile '{id}', which no longer exists"),
                )
                .hint(format!("Remove it with `gitmate rules remove {dir}`")),
            ),
            Some(profile) => {
                if !matches!(includes::is_current(git, paths, profile), Ok(true)) {
                    findings.push(
                        Finding::new(
                            Severity::Error,
                            "include_stale",
                            format!(
                                "The include file for profile '{id}' ({}) is missing or out of date, so the rule for {dir} does not apply the profile's current values",
                                paths.display(&paths.include_file(id))
                            ),
                        )
                        .hint(format!("Run `gitmate profile edit {id}` (with no changes) to regenerate it")),
                    );
                }
            }
        }
        if let Some(d) = &rule.directory
            && !d.exists()
        {
            findings.push(Finding::new(
                Severity::Info,
                "rule_directory_missing",
                format!("The directory {dir} for profile '{id}' does not exist yet"),
            ));
        }
    }

    // Rules shadowed by a broader rule that appears later in the same file.
    for (i, specific) in rules.iter().enumerate() {
        for broad in rules.iter().skip(i + 1) {
            if broad.file != specific.file {
                continue;
            }
            if let (Some(s), Some(b)) = (&specific.directory, &broad.directory)
                && s != b
                && s.starts_with(b)
                && broad.profile != specific.profile
            {
                findings.push(
                        Finding::new(
                            Severity::Warning,
                            "rule_shadowed",
                            format!(
                                "The rule for {} comes after the more specific rule for {}, so it overrides it for every repository under {}",
                                paths.display(b),
                                paths.display(s),
                                paths.display(s)
                            ),
                        )
                        .hint(format!(
                            "Remove and re-add the rule for {} so it comes last",
                            paths.display(s)
                        )),
                    );
            }
        }
    }

    // user.name / user.email written after an includeIf rule in the same
    // global file override that rule.
    for file in rules::global_files(git, &paths.home) {
        let Ok(entries) = git.file_list(&file) else {
            continue;
        };
        let first_rule = entries
            .iter()
            .position(|(k, _)| k.starts_with("includeif.gitdir") && k.ends_with(".path"));
        if let Some(first) = first_rule
            && let Some((key, _)) = entries
                .iter()
                .skip(first + 1)
                .find(|(k, _)| k == "user.email" || k == "user.name")
        {
            findings.push(
                    Finding::new(
                        Severity::Warning,
                        "global_identity_after_rule",
                        format!(
                            "{} sets {key} after an includeIf rule, which overrides the rule's identity in every matching repository",
                            paths.display(&file)
                        ),
                    )
                    .hint("Move the [user] section above the includeIf sections"),
                );
        }
    }
    findings
}

pub(crate) fn profile_findings(paths: &AppPaths, profiles: &[Profile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for profile in profiles {
        if let Some(key) = &profile.ssh_key {
            let check = ssh::check_key(key, &paths.home);
            if let Some(problem) = check.problem() {
                findings.push(Finding::new(
                    Severity::Error,
                    "ssh_key_problem",
                    format!("Profile '{}': {problem}", profile.id),
                ));
            }
        }
    }
    findings
}

pub(crate) fn history_findings(history: &[OperationRecord]) -> Vec<Finding> {
    history
        .iter()
        .filter_map(|op| match op.status {
            OperationStatus::Pending => Some(
                Finding::new(
                    Severity::Warning,
                    "interrupted_operation",
                    format!(
                        "Operation {} ({}) was interrupted before it finished",
                        op.id, op.summary
                    ),
                )
                .hint(format!(
                    "Review it with `gitmate history` and roll it back with `gitmate undo {}`",
                    op.id
                )),
            ),
            OperationStatus::Failed
                if op
                    .error
                    .as_deref()
                    .is_some_and(|e| e.contains("NOT fully reverted")) =>
            {
                Some(
                    Finding::new(
                        Severity::Error,
                        "failed_operation",
                        format!(
                            "Operation {} ({}) failed and was not fully reverted",
                            op.id, op.summary
                        ),
                    )
                    .hint(format!(
                        "Run `gitmate undo {}`; backups are listed in `gitmate history --json`",
                        op.id
                    )),
                )
            }
            _ => None,
        })
        .collect()
}
