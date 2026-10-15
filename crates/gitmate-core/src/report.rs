//! Repository reports and diagnostic findings.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::git::{ConfigEntry, Origin, Scope};
use crate::identity::{IdentityReport, Source};
use crate::journal::{OperationRecord, OperationStatus};
use crate::paths::AppPaths;
use crate::plan::Action;
use crate::profile::Profile;
use crate::remote::{Remote, Transport};
use crate::repo::RepoInfo;
use crate::rules::Rule;
use crate::ssh::{self, KeyCheck};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// A diagnostic: what is wrong (or notable) and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Finding {
    pub fn new(severity: Severity, code: &'static str, message: impl Into<String>) -> Finding {
        Finding {
            severity,
            code,
            message: message.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Finding {
        self.hint = Some(hint.into());
        self
    }
}

/// The profile whose email matches the effective author email.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileMatch {
    pub id: String,
    pub label: Option<String>,
    /// Whether the effective name also equals the profile's name.
    pub name_matches: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectedReason {
    /// `gitmate use` assigned the profile to this repository.
    Assignment { operation: String },
    /// A GitMate rule covers this repository.
    Rule { directory: PathBuf },
}

/// The profile GitMate configured for this repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expected {
    pub profile: String,
    pub reason: ExpectedReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SshStatus {
    /// The SSH command Git will run, if not plain `ssh`.
    pub command: Option<String>,
    /// Where `command` comes from, e.g. `GIT_SSH_COMMAND` or a config file.
    pub command_source: Option<String>,
    /// Key check for the relevant profile's SSH key, if it has one.
    pub profile_key: Option<KeyCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoReport {
    /// Directory that was inspected.
    pub path: PathBuf,
    pub repository: Option<RepoInfo>,
    pub identity: IdentityReport,
    pub profile: Option<ProfileMatch>,
    /// The GitMate rule that supplied identity values, if any.
    pub rule: Option<Rule>,
    pub expected: Option<Expected>,
    pub remotes: Vec<Remote>,
    pub ssh: SshStatus,
    /// `credential.*helper` and `credential.*username` entries.
    pub credentials: Vec<(String, ConfigEntry)>,
    pub findings: Vec<Finding>,
}

/// Inputs gathered by the facade.
pub(crate) struct ReportInput<'a> {
    pub path: PathBuf,
    pub repository: Option<RepoInfo>,
    pub identity: IdentityReport,
    pub profiles: &'a [Profile],
    pub rules: &'a [Rule],
    pub history: &'a [OperationRecord],
    pub remotes: Vec<Remote>,
    pub ssh_entries: Vec<ConfigEntry>,
    pub git_ssh_command: Option<String>,
    pub credentials: Vec<(String, ConfigEntry)>,
    pub paths: &'a AppPaths,
}

fn origin_file(entry: &ConfigEntry) -> Option<&Path> {
    match &entry.origin {
        Origin::File(p) => Some(p),
        _ => None,
    }
}

fn describe_source(source: &Source, paths: &AppPaths) -> String {
    match source {
        Source::Environment { var } => format!("the {var} environment variable"),
        Source::Config { key, scope, origin } => match origin {
            Origin::File(p) => format!("{key} in {} ({} scope)", paths.display(p), scope.as_str()),
            Origin::CommandLine => format!(
                "{key} set on the command line or via GIT_CONFIG_* ({} scope)",
                scope.as_str()
            ),
            other => format!("{key} from {other:?}"),
        },
        Source::AutoDetected => "Git's guess from your system user and host name".to_string(),
    }
}

pub(crate) fn build(input: ReportInput<'_>) -> RepoReport {
    let ReportInput {
        path,
        repository,
        identity,
        profiles,
        rules,
        history,
        remotes,
        ssh_entries,
        git_ssh_command,
        credentials,
        paths,
    } = input;
    let mut findings = Vec::new();

    let profile_match = identity.author.as_ref().and_then(|author| {
        profiles
            .iter()
            .find(|p| p.matches_email(&author.email))
            .map(|p| ProfileMatch {
                id: p.id.clone(),
                label: p.label.clone(),
                name_matches: p.name == author.name,
            })
    });

    // Managed rule whose include file supplied a user.* value (the last one
    // read is the one in effect among rules).
    let supplying_rule = ["user.email", "user.name"].iter().find_map(|key| {
        identity.key(key).and_then(|k| {
            k.values.iter().rev().find_map(|entry| {
                let file = origin_file(entry)?;
                rules.iter().find(|r| {
                    r.profile.as_ref().is_some_and(|id| {
                        crate::paths::normalize_lexically(file)
                            == crate::paths::normalize_lexically(&paths.include_file(id))
                    })
                })
            })
        })
    });

    let assignment = repository.as_ref().and_then(|repo| {
        let local = repo.local_config();
        history.iter().rev().find_map(|op| match &op.action {
            Action::Assign { profile, .. }
                if op.status == OperationStatus::Applied && op.changes.iter().any(|c| c.file == local) =>
            {
                Some(Expected {
                    profile: profile.clone(),
                    reason: ExpectedReason::Assignment {
                        operation: op.id.clone(),
                    },
                })
            }
            _ => None,
        })
    });

    // A managed rule that covers the repository's git dir lexically but that
    // Git did not apply.
    let covering_rule = repository.as_ref().and_then(|repo| {
        rules
            .iter()
            .filter(|r| r.is_managed() && (r.covers(&repo.git_dir) || r.covers(repo.root())))
            .max_by_key(|r| (r.file.clone(), r.position))
    });

    let expected = assignment.or_else(|| {
        supplying_rule.or(covering_rule).map(|rule| Expected {
            profile: rule.profile.clone().unwrap_or_default(),
            reason: ExpectedReason::Rule {
                directory: rule.directory.clone().unwrap_or_default(),
            },
        })
    });

    // Identity state.
    if repository.is_none() {
        findings.push(Finding::new(
            Severity::Info,
            "not_a_repository",
            format!(
                "{} is not inside a Git repository; only configuration that applies everywhere is shown (directory rules apply inside repositories only)",
                paths.display(&path)
            ),
        ));
    }
    match (&identity.author, &identity.email_source) {
        (None, _) => findings.push(
            Finding::new(
                Severity::Error,
                "identity_unset",
                identity
                    .problem
                    .clone()
                    .unwrap_or_else(|| "No commit identity is configured".into()),
            )
            .hint("Assign a profile with `gitmate use <profile>` or add a directory rule with `gitmate rules add`"),
        ),
        (Some(author), Some(Source::AutoDetected)) => findings.push(
            Finding::new(
                Severity::Warning,
                "identity_guessed",
                format!("Git is guessing the identity {} <{}> from your system user and host name", author.name, author.email),
            )
            .hint("Configure an identity so commits are attributed correctly"),
        ),
        _ => {}
    }

    if let (Some(expected), Some(author)) = (&expected, &identity.author) {
        let matches_expected = profile_match.as_ref().is_some_and(|m| m.id == expected.profile);
        if !matches_expected {
            let want = profiles.iter().find(|p| p.id == expected.profile);
            let why = match &expected.reason {
                ExpectedReason::Assignment { .. } => "it was assigned to this repository".to_string(),
                ExpectedReason::Rule { directory } => {
                    format!("the rule for {} covers it", paths.display(directory))
                }
            };
            let cause = identity
                .email_source
                .as_ref()
                .map(|s| format!(" The email comes from {}.", describe_source(s, paths)))
                .unwrap_or_default();
            let mut finding = Finding::new(
                Severity::Error,
                "profile_mismatch",
                format!(
                    "Commits here would be authored as {} <{}>, but GitMate expects profile '{}'{} because {why}.{cause}",
                    author.name,
                    author.email,
                    expected.profile,
                    want.map(|p| format!(" ({})", p.email)).unwrap_or_default(),
                ),
            );
            finding = match &identity.email_source {
                Some(Source::Environment { var }) => finding.hint(format!("Unset {var} in your shell")),
                Some(Source::Config { scope: Scope::Local, .. }) => {
                    finding.hint(format!("Run `gitmate use {}` to assign it to this repository", expected.profile))
                }
                Some(Source::Config { scope: Scope::Global, origin, .. })
                    if !matches!(origin, Origin::File(p) if p.starts_with(paths.includes_dir())) =>
                {
                    finding.hint("A global user.email defined after the includeIf section overrides the rule; move it above the rule or remove it")
                }
                _ => finding.hint(format!("Run `gitmate use {}` to assign it explicitly", expected.profile)),
            };
            if want.is_none() {
                finding = finding.hint(format!(
                    "Profile '{}' no longer exists; remove or re-create the rule",
                    expected.profile
                ));
            }
            findings.push(finding);
        }
    } else if expected.is_none()
        && profile_match.is_none()
        && let Some(author) = &identity.author
    {
        findings.push(Finding::new(
            Severity::Info,
            "no_matching_profile",
            format!(
                "The identity {} <{}> does not belong to any GitMate profile",
                author.name, author.email
            ),
        ));
    }

    if let (Some(rule), Some(_)) = (covering_rule, &repository)
        && supplying_rule.is_none()
        && identity.author.is_some()
        && matches!(
            expected.as_ref().map(|e| &e.reason),
            Some(ExpectedReason::Rule { .. })
        )
    {
        findings.push(
                Finding::new(
                    Severity::Warning,
                    "rule_not_applied",
                    format!(
                        "The rule for {} appears to cover this repository, but Git did not apply it",
                        rule.directory.as_deref().map(|d| paths.display(d)).unwrap_or_default()
                    ),
                )
                .hint("The repository may be reached through a symlink or a differently-cased path; `git config --show-origin user.email` shows what Git used"),
            );
    }

    if let Some(m) = &profile_match
        && !m.name_matches
        && let (Some(p), Some(author)) = (profiles.iter().find(|p| p.id == m.id), &identity.author)
    {
        findings.push(Finding::new(
            Severity::Warning,
            "name_differs",
            format!(
                "The email matches profile '{}', but the name is \"{}\" instead of \"{}\"",
                m.id, author.name, p.name
            ),
        ));
    }

    for var in identity
        .env
        .iter()
        .filter(|v| v.name.starts_with("GIT_AUTHOR_") || v.name.starts_with("GIT_COMMITTER_"))
    {
        findings.push(
            Finding::new(
                Severity::Warning,
                "env_override",
                format!(
                    "{} is set in the environment and overrides Git configuration",
                    var.name
                ),
            )
            .hint(format!("Unset {} unless you set it on purpose", var.name)),
        );
    }
    for key in ["author.name", "author.email", "committer.name", "committer.email"] {
        if let Some(entry) = identity.key(key).and_then(|k| k.effective()) {
            findings.push(Finding::new(
                Severity::Warning,
                "author_committer_override",
                format!(
                    "{key} is set{} and takes precedence over user.{}",
                    origin_file(entry)
                        .map(|p| format!(" in {}", paths.display(p)))
                        .unwrap_or_default(),
                    key.split('.').nth(1).unwrap_or_default()
                ),
            ));
        }
    }

    // SSH and transport.
    let relevant_profile = expected
        .as_ref()
        .map(|e| e.profile.as_str())
        .or(profile_match.as_ref().map(|m| m.id.as_str()))
        .and_then(|id| profiles.iter().find(|p| p.id == id));
    let profile_key =
        relevant_profile.and_then(|p| p.ssh_key.as_ref().map(|k| ssh::check_key(k, &paths.home)));
    if let Some(check) = &profile_key
        && let Some(problem) = check.problem()
    {
        findings.push(Finding::new(Severity::Error, "ssh_key_problem", problem));
    }

    let (command, command_source) = if let Some(cmd) = git_ssh_command {
        (
            Some(cmd),
            Some("the GIT_SSH_COMMAND environment variable".to_string()),
        )
    } else if let Some(entry) = ssh_entries.last() {
        (
            Some(entry.value.clone()),
            Some(match &entry.origin {
                Origin::File(p) => format!(
                    "core.sshCommand in {} ({} scope)",
                    paths.display(p),
                    entry.scope.as_str()
                ),
                _ => format!("core.sshCommand ({} scope)", entry.scope.as_str()),
            }),
        )
    } else {
        (None, None)
    };

    let fetch_transports: Vec<Transport> = remotes
        .iter()
        .flat_map(|r| r.fetch.iter().chain(&r.push))
        .map(|u| u.transport)
        .collect();
    if let Some(p) = relevant_profile
        && let Some(key) = &p.ssh_key
    {
        let http_only = !fetch_transports.is_empty()
            && fetch_transports
                .iter()
                .all(|t| matches!(t, Transport::Https | Transport::Http));
        if http_only {
            let helper = credentials
                .iter()
                .rfind(|(k, _)| k.ends_with("helper"))
                .map(|(_, e)| format!(" (credential helper: {})", e.value))
                .unwrap_or_default();
            findings.push(
                    Finding::new(
                        Severity::Warning,
                        "https_remote_ssh_profile",
                        format!(
                            "Profile '{}' authenticates with the SSH key {}, but this repository's remotes use HTTPS, so pushes use stored HTTPS credentials{helper} instead",
                            p.id,
                            paths.display(key)
                        ),
                    )
                    .hint("Switch the remote to SSH (`git remote set-url origin git@host:owner/repo.git`) if you want the profile's key to be used"),
                );
        }
        if let Ok(expected_cmd) = ssh::ssh_command_for_key(&paths.expand_tilde(key)) {
            match &command {
                Some(actual) if actual != &expected_cmd => findings.push(Finding::new(
                    Severity::Warning,
                    "ssh_command_mismatch",
                    format!(
                        "Git will authenticate with `{actual}` from {}, not with profile '{}' key",
                        command_source.clone().unwrap_or_default(),
                        p.id
                    ),
                )),
                None if fetch_transports.contains(&Transport::Ssh) => findings.push(
                    Finding::new(
                        Severity::Info,
                        "ssh_key_not_pinned",
                        format!(
                            "SSH remotes here use your default SSH configuration, not profile '{}' key",
                            p.id
                        ),
                    )
                    .hint(format!(
                        "Run `gitmate use {} --ssh` to pin the key for this repository",
                        p.id
                    )),
                ),
                _ => {}
            }
        }
    }

    for remote in &remotes {
        for url in remote.fetch.iter().chain(&remote.push) {
            if url.embedded_credentials {
                findings.push(
                    Finding::new(
                        Severity::Warning,
                        "embedded_credentials",
                        format!(
                            "Remote '{}' has credentials embedded in its URL ({})",
                            remote.name, url.display
                        ),
                    )
                    .hint("Remove them from the URL and use a credential helper instead"),
                );
                break;
            }
        }
    }

    findings.sort_by_key(|f| f.severity);
    RepoReport {
        path,
        repository,
        identity,
        profile: profile_match,
        rule: supplying_rule.cloned(),
        expected,
        remotes,
        ssh: SshStatus {
            command,
            command_source,
            profile_key,
        },
        credentials,
        findings,
    }
}
