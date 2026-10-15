//! Directory rules: `[includeIf "gitdir:<dir>/"] path = <include file>`
//! sections in the global Git config.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::git::Git;
use crate::paths::{self, AppPaths};
use crate::profile;

/// One `includeIf.<condition>.path` entry in a global config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    /// File the rule lives in.
    pub file: PathBuf,
    /// Condition as written, e.g. `gitdir:/home/ada/work/`.
    pub condition: String,
    /// Directory for `gitdir:`/`gitdir/i:` conditions, `~` expanded and
    /// without the trailing slash. `None` for other condition types.
    pub directory: Option<PathBuf>,
    pub case_insensitive: bool,
    /// The `path` value as written.
    pub include_path: String,
    /// Set when the include file is a GitMate profile include file.
    pub profile: Option<String>,
    /// Position of the entry within `file` (0-based, in file order).
    pub position: usize,
}

impl Rule {
    pub fn is_managed(&self) -> bool {
        self.profile.is_some()
    }

    /// Key used with `git config` to address this rule.
    pub fn key(&self) -> String {
        format!("includeIf.{}.path", self.condition)
    }

    /// The rule's directory with symlinks resolved when it exists. Git
    /// matches a `gitdir:` pattern against both the path as given and its
    /// real path, so two spellings of one directory are the same rule.
    pub fn real_directory(&self) -> Option<PathBuf> {
        let dir = self.directory.as_ref()?;
        Some(std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()))
    }

    /// Whether this rule names `dir` (already resolved), however the rule
    /// spells it.
    pub fn names_directory(&self, dir: &Path) -> bool {
        let (Some(raw), Some(real)) = (&self.directory, self.real_directory()) else {
            return false;
        };
        let insensitive = self.case_insensitive || cfg!(any(target_os = "macos", windows));
        [raw.as_path(), real.as_path()].into_iter().any(|candidate| {
            if insensitive {
                candidate.to_string_lossy().to_lowercase() == dir.to_string_lossy().to_lowercase()
            } else {
                candidate == dir
            }
        })
    }

    /// Whether the rule's directory contains `path` (lexically).
    pub fn covers(&self, path: &Path) -> bool {
        match &self.directory {
            Some(dir) if self.case_insensitive => path
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&format!("{}/", dir.to_string_lossy().to_lowercase())),
            Some(dir) => path.starts_with(dir) && path != dir,
            None => false,
        }
    }
}

/// The files Git reads for the global scope, in read order.
pub fn global_files(git: &Git, home: &Path) -> Vec<PathBuf> {
    if let Some(explicit) = git.effective_env("GIT_CONFIG_GLOBAL").filter(|v| !v.is_empty()) {
        return vec![paths::expand_tilde(Path::new(&explicit), home)];
    }
    let xdg = git
        .effective_env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("git/config");
    vec![xdg, home.join(".gitconfig")]
}

/// The global file new rules are written to (see
/// [`paths::global_config_target`]).
pub fn target_file(git: &Git, home: &Path) -> PathBuf {
    paths::global_config_target(|k| git.effective_env(k), home)
}

/// Split a condition into its directory and case-sensitivity. Only
/// conditions that cover a whole directory tree have one: Git treats
/// `<dir>/` as `<dir>/**`, while a pattern without the trailing slash
/// matches a single `.git` directory.
pub fn parse_condition(condition: &str, home: &Path) -> (Option<PathBuf>, bool) {
    let (pattern, insensitive) = if let Some(p) = condition.strip_prefix("gitdir/i:") {
        (p, true)
    } else if let Some(p) = condition.strip_prefix("gitdir:") {
        (p, false)
    } else {
        return (None, false);
    };
    let Some(dir) = pattern
        .strip_suffix("/**")
        .or_else(|| pattern.strip_suffix('/'))
        .map(|d| d.trim_end_matches('/'))
    else {
        return (None, insensitive);
    };
    if dir.is_empty() || dir.contains(['*', '?', '[']) {
        return (None, insensitive);
    }
    let expanded = paths::expand_tilde(Path::new(dir), home);
    // Git prefixes other patterns with `**/` (or, for `./`, the including
    // file's directory); those do not name one directory.
    let drive = dir.len() >= 2 && dir.as_bytes()[0].is_ascii_alphabetic() && dir.as_bytes()[1] == b':';
    if !(expanded.is_absolute() || expanded.starts_with("/") || drive) {
        return (None, insensitive);
    }
    (Some(expanded), insensitive)
}

/// Which profile an include path belongs to, if it is a GitMate include
/// file. Relative include paths are relative to the including file.
pub fn profile_for_include(include: &str, including_file: &Path, app: &AppPaths) -> Option<String> {
    let mut path = paths::expand_tilde(Path::new(include), &app.home);
    if path.is_relative() {
        path = including_file.parent()?.join(path);
    }
    let path = paths::normalize_lexically(&path);
    if path.parent()? != paths::normalize_lexically(&app.includes_dir()) {
        return None;
    }
    if path.extension()? != "gitconfig" {
        return None;
    }
    let id = path.file_stem()?.to_str()?;
    profile::validate_id(id).is_none().then(|| id.to_string())
}

/// All `includeIf ... path` rules in the global config files.
pub fn list(git: &Git, app: &AppPaths) -> Result<Vec<Rule>> {
    let mut rules = Vec::new();
    for file in global_files(git, &app.home) {
        for (position, (key, value)) in git.file_list(&file)?.into_iter().enumerate() {
            let Some(condition) = key
                .strip_prefix("includeif.")
                .and_then(|rest| rest.strip_suffix(".path"))
            else {
                continue;
            };
            let (directory, case_insensitive) = parse_condition(condition, &app.home);
            rules.push(Rule {
                file: file.clone(),
                condition: condition.to_string(),
                directory,
                case_insensitive,
                profile: profile_for_include(&value, &file, app),
                include_path: value,
                position,
            });
        }
    }
    Ok(rules)
}

/// A directory ready to be written into a `gitdir:` condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDirectory {
    /// Directory as the user gave it, made absolute.
    pub requested: PathBuf,
    /// Directory the rule will name (symlinks resolved when it exists).
    pub resolved: PathBuf,
    pub exists: bool,
}

/// Validate and normalise a rule directory. Symlinks are resolved because
/// Git matches `gitdir:` against the repository's real path.
pub fn rule_directory(input: &Path, home: &Path) -> Result<RuleDirectory> {
    let invalid = |reason: &str| Error::InvalidPath {
        path: input.to_path_buf(),
        reason: reason.into(),
    };
    // Both the path as given and its resolved form are checked: the rule
    // names the resolved one, which a symlink can point anywhere.
    let check = |path: &Path, prefix: &str| -> Result<()> {
        let text = path
            .to_str()
            .ok_or_else(|| invalid(&format!("{prefix}is not valid UTF-8")))?;
        if text.chars().any(char::is_control) {
            return Err(invalid(&format!("{prefix}contains control characters")));
        }
        if text.contains(['*', '?', '[', ']']) || (cfg!(unix) && text.contains('\\')) {
            return Err(invalid(&format!(
                "{prefix}contains *, ?, [, ] or \\, which Git would treat as a pattern; rename the directory or write the rule by hand"
            )));
        }
        Ok(())
    };
    let expanded = paths::expand_tilde(input, home);
    if !expanded.is_absolute() {
        return Err(invalid("must be an absolute path"));
    }
    check(&expanded, "")?;
    let requested = paths::normalize_lexically(&expanded);
    if requested.parent().is_none() {
        return Err(invalid("must not be the filesystem root"));
    }
    match std::fs::canonicalize(&requested) {
        Ok(resolved) if resolved.is_dir() => {
            check(&resolved, &format!("resolves to {}, which ", resolved.display()))?;
            Ok(RuleDirectory {
                requested,
                resolved,
                exists: true,
            })
        }
        Ok(_) => Err(invalid("is not a directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RuleDirectory {
            resolved: requested.clone(),
            requested,
            exists: false,
        }),
        Err(e) => Err(invalid(&e.to_string())),
    }
}

/// The `includeIf` condition for a directory on this platform. macOS and
/// Windows file systems are case-insensitive by default, so `gitdir/i:` is
/// used there.
pub fn condition_for(dir: &Path) -> String {
    let text = dir.to_string_lossy().replace('\\', "/");
    let prefix = if cfg!(any(target_os = "macos", windows)) {
        "gitdir/i:"
    } else {
        "gitdir:"
    };
    format!("{prefix}{}/", text.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conditions() {
        let home = Path::new("/h");
        assert_eq!(
            parse_condition("gitdir:~/work/", home),
            (Some(PathBuf::from("/h/work")), false)
        );
        assert_eq!(
            parse_condition("gitdir/i:C:/w/", home),
            (Some(PathBuf::from("C:/w")), true)
        );
        assert_eq!(
            parse_condition("gitdir:~/work/**", home),
            (Some(PathBuf::from("/h/work")), false)
        );
        assert_eq!(parse_condition("gitdir:~/w/**/x/", home).0, None);
        assert_eq!(
            parse_condition("gitdir:/h/work", home).0,
            None,
            "no trailing slash"
        );
        assert_eq!(parse_condition("gitdir:work/", home).0, None, "matched anywhere");
        assert_eq!(parse_condition("gitdir:./work/", home).0, None);
        assert_eq!(parse_condition("onbranch:main", home), (None, false));
    }

    #[test]
    fn recognises_gitmate_include_files() {
        let app = AppPaths {
            config_dir: PathBuf::from("/h/.config/gitmate"),
            state_dir: PathBuf::from("/h/.local/state/gitmate"),
            home: PathBuf::from("/h"),
        };
        let gc = Path::new("/h/.gitconfig");
        assert_eq!(
            profile_for_include("/h/.config/gitmate/includes/work.gitconfig", gc, &app).as_deref(),
            Some("work")
        );
        assert_eq!(
            profile_for_include("~/.config/gitmate/includes/oss.gitconfig", gc, &app).as_deref(),
            Some("oss")
        );
        assert_eq!(profile_for_include("/h/other.inc", gc, &app), None);
        assert_eq!(
            profile_for_include("/h/.config/gitmate/includes/x.inc", gc, &app),
            None
        );
        assert_eq!(
            profile_for_include("/h/.config/gitmate/includes/BAD!.gitconfig", gc, &app),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn rule_coverage_is_component_wise() {
        let rule = Rule {
            file: PathBuf::from("/h/.gitconfig"),
            condition: "gitdir:/h/work/".into(),
            directory: Some(PathBuf::from("/h/work")),
            case_insensitive: false,
            include_path: String::new(),
            profile: None,
            position: 0,
        };
        assert!(rule.covers(Path::new("/h/work/app/.git")));
        assert!(!rule.covers(Path::new("/h/workshop/app/.git")));
        assert!(!rule.covers(Path::new("/h/work")));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn conditions_end_with_a_slash() {
        assert_eq!(condition_for(Path::new("/h/my work")), "gitdir:/h/my work/");
    }
}
