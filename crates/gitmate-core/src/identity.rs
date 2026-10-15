//! Effective commit identity and where each part of it comes from.

use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::git::{ConfigEntry, Git, Ident, Origin, Scope, parse_ident};
use crate::repo::{self, RepoInfo};

/// Config keys that influence the commit identity.
pub const IDENTITY_KEYS: &[&str] = &[
    "user.name",
    "user.email",
    "author.name",
    "author.email",
    "committer.name",
    "committer.email",
    "user.useConfigOnly",
];

/// Environment variables reported by `whoami`. Values of the ones marked
/// secret-capable are never shown.
pub const REPORTED_ENV_VARS: &[(&str, bool)] = &[
    ("GIT_AUTHOR_NAME", false),
    ("GIT_AUTHOR_EMAIL", false),
    ("GIT_COMMITTER_NAME", false),
    ("GIT_COMMITTER_EMAIL", false),
    ("EMAIL", false),
    ("GIT_CONFIG_GLOBAL", false),
    ("GIT_CONFIG_SYSTEM", false),
    ("GIT_CONFIG_NOSYSTEM", false),
    ("GIT_CONFIG_COUNT", false),
    ("GIT_CONFIG_PARAMETERS", true),
    ("GIT_SSH_COMMAND", false),
    ("GIT_SSH", false),
    ("GIT_DIR", false),
    ("GIT_WORK_TREE", false),
    ("GIT_COMMON_DIR", false),
];

/// All values of one key, in the order Git reads them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyReport {
    pub key: String,
    pub values: Vec<ConfigEntry>,
}

impl KeyReport {
    /// The value Git uses for a single-valued key: the last one read.
    pub fn effective(&self) -> Option<&ConfigEntry> {
        self.values.last()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvVar {
    pub name: String,
    /// `None` when the value is withheld because it may contain secrets.
    pub value: Option<String>,
}

/// Which mechanism supplied a part of the identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Environment {
        var: String,
    },
    Config {
        key: String,
        scope: Scope,
        origin: Origin,
    },
    /// Git derived it from the system user/hostname.
    AutoDetected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdentityReport {
    /// Author Git would record right now (`git var GIT_AUTHOR_IDENT`).
    pub author: Option<Ident>,
    /// Committer Git would record right now.
    pub committer: Option<Ident>,
    /// Git's explanation when no identity can be determined.
    pub problem: Option<String>,
    pub name_source: Option<Source>,
    pub email_source: Option<Source>,
    /// Identity-related keys that have at least one value.
    pub keys: Vec<KeyReport>,
    /// Relevant environment variables that are set.
    pub env: Vec<EnvVar>,
}

impl IdentityReport {
    pub fn key(&self, key: &str) -> Option<&KeyReport> {
        self.keys.iter().find(|k| k.key.eq_ignore_ascii_case(key))
    }

    /// True when Git has an identity that was explicitly configured (by
    /// config or environment), not guessed from the system user and host.
    pub fn is_configured(&self) -> bool {
        self.author.is_some()
            && !matches!(self.email_source, Some(Source::AutoDetected))
            && !matches!(self.name_source, Some(Source::AutoDetected))
    }
}

/// Inspect the identity Git would use for a commit made in `dir`.
pub fn inspect(git: &Git, dir: &Path) -> Result<IdentityReport> {
    let repo = repo::discover(git, dir)?;
    let mut keys = Vec::new();
    for key in IDENTITY_KEYS {
        let values: Vec<ConfigEntry> = git
            .scoped_get_all(dir, key)?
            .into_iter()
            .map(|entry| resolve_origin(entry, repo.as_ref(), dir))
            .collect();
        if !values.is_empty() {
            keys.push(KeyReport {
                key: (*key).to_string(),
                values,
            });
        }
    }

    let (author, author_problem) = ident(git, dir, "GIT_AUTHOR_IDENT")?;
    let (committer, _) = ident(git, dir, "GIT_COMMITTER_IDENT")?;

    let env = REPORTED_ENV_VARS
        .iter()
        .filter_map(|(name, secret)| {
            git.effective_env(name).map(|value| EnvVar {
                name: (*name).to_string(),
                value: (!secret).then(|| value.to_string_lossy().into_owned()),
            })
        })
        .collect();

    let mut report = IdentityReport {
        author,
        committer,
        problem: author_problem,
        name_source: None,
        email_source: None,
        keys,
        env,
    };
    if report.author.is_some() {
        report.name_source = Some(attribute(git, &report, "name"));
        report.email_source = Some(attribute(git, &report, "email"));
    }
    Ok(report)
}

/// Run `git var <var>`; an unknown identity is a normal result, not an error.
fn ident(git: &Git, dir: &Path, var: &str) -> Result<(Option<Ident>, Option<String>)> {
    let out = git.run_at(dir, ["var", var])?;
    if out.success()
        && let Some(ident) = parse_ident(&out.stdout_trimmed())
    {
        return Ok((Some(ident), None));
    }
    let reason = out
        .stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("fatal:"))
        .map(|l| l.trim_start_matches("fatal:").trim().to_string())
        .unwrap_or_else(|| "Git could not determine an identity".to_string());
    Ok((None, Some(format!("No commit identity is configured ({reason})"))))
}

/// Explain which mechanism supplied the author's `field` ("name"/"email"),
/// following the precedence documented in `git-commit-tree(1)`. The value
/// itself always comes from `git var`.
fn attribute(git: &Git, report: &IdentityReport, field: &str) -> Source {
    let env_var = format!("GIT_AUTHOR_{}", field.to_ascii_uppercase());
    if git.effective_env(&env_var).is_some() {
        return Source::Environment { var: env_var };
    }
    for key in [format!("author.{field}"), format!("user.{field}")] {
        if let Some(entry) = report.key(&key).and_then(KeyReport::effective) {
            return Source::Config {
                key,
                scope: entry.scope,
                origin: entry.origin.clone(),
            };
        }
    }
    if field == "email" && git.effective_env("EMAIL").is_some() {
        return Source::Environment { var: "EMAIL".into() };
    }
    Source::AutoDetected
}

/// Git prints repository-local origins relative to the directory it works
/// from: the top of the working tree, or the Git directory for bare
/// repositories (`file:.git/config`, `file:.git/extra`, `file:config`). Map
/// them to the exact file, falling back to the scope's own config file.
pub(crate) fn resolve_origin(mut entry: ConfigEntry, repo: Option<&RepoInfo>, dir: &Path) -> ConfigEntry {
    if let Origin::File(path) = &entry.origin
        && path.is_relative()
    {
        let resolved = match repo {
            Some(r) => {
                let candidate = crate::paths::normalize_lexically(&r.root().join(path));
                match entry.scope {
                    Scope::Local if !candidate.is_file() => r.local_config(),
                    Scope::Worktree if !candidate.is_file() => r.worktree_config(),
                    _ => candidate,
                }
            }
            None => dir.join(path),
        };
        entry.origin = Origin::File(resolved);
    }
    entry
}
