//! Running the `git` executable.
//!
//! Every invocation passes arguments as a vector (never through a shell),
//! disables interactive prompts, and forces the C locale so that Git's error
//! messages can be classified reliably.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::error::{Error, Result};

/// Minimum supported Git version (`rev-parse --path-format` arrived in 2.31).
pub const MIN_GIT_VERSION: (u32, u32) = (2, 31);
const MIN_GIT_VERSION_STR: &str = "2.31";

/// Environment variables that redirect Git away from normal repository
/// discovery. They are removed when GitMate targets a specific path, and
/// reported as overrides by `whoami`.
pub const REPOSITORY_ENV_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// Handle for running Git with a controlled environment.
#[derive(Debug, Clone)]
pub struct Git {
    program: PathBuf,
    env: Vec<(OsString, Option<OsString>)>,
}

/// Raw result of a Git invocation.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl GitOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stdout_trimmed(&self) -> String {
        String::from_utf8_lossy(&self.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_string()
    }
}

impl Default for Git {
    fn default() -> Self {
        Git::new("git")
    }
}

impl Git {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Git {
            program: program.into(),
            env: Vec::new(),
        }
    }

    /// Set an environment variable for every Git invocation made through this
    /// handle. Used by tests to point Git at a temporary HOME.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), Some(value.into())));
        self
    }

    /// Remove an inherited environment variable for every invocation.
    pub fn env_remove(mut self, key: impl Into<OsString>) -> Self {
        self.env.push((key.into(), None));
        self
    }

    /// Value of an environment variable as Git child processes will see it.
    pub fn effective_env(&self, key: &str) -> Option<OsString> {
        for (k, v) in self.env.iter().rev() {
            if k == key {
                return v.clone();
            }
        }
        std::env::var_os(key)
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// A Git command with this handle's environment.
    fn base_command(&self, cwd: Option<&Path>, strip_repo_env: bool) -> Command {
        let mut cmd = Command::new(&self.program);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        for (key, value) in &self.env {
            match value {
                Some(v) => cmd.env(key, v),
                None => cmd.env_remove(key),
            };
        }
        if strip_repo_env {
            for key in REPOSITORY_ENV_VARS {
                cmd.env_remove(key);
            }
        }
        cmd
    }

    /// A Git command whose output GitMate parses: C locale, no prompts.
    fn command(&self, cwd: Option<&Path>, strip_repo_env: bool) -> Command {
        let mut cmd = self.base_command(cwd, strip_repo_env);
        cmd.env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env_remove("GIT_TRACE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// Run Git with `args`. The caller decides how to interpret non-zero exit
    /// codes; only a failure to start the process is an error here.
    pub fn run<I, S>(&self, cwd: Option<&Path>, args: I) -> Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_inner(cwd, args, false)
    }

    /// Like [`Git::run`] but with repository-redirecting variables
    /// (`GIT_DIR`, `GIT_WORK_TREE`, ...) removed, so that `cwd` alone
    /// determines the repository.
    pub fn run_at<I, S>(&self, cwd: &Path, args: I) -> Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_inner(Some(cwd), args, true)
    }

    fn run_inner<I, S>(&self, cwd: Option<&Path>, args: I, strip_repo_env: bool) -> Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_os_string()).collect();
        let output = self
            .command(cwd, strip_repo_env)
            .args(&args)
            .output()
            .map_err(|e| Error::GitNotFound {
                program: self.program.display().to_string(),
                message: e.to_string(),
            })?;
        Ok(GitOutput {
            code: output.status.code(),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }

    /// Run Git and turn a non-zero exit into [`Error::GitFailed`].
    pub fn run_ok<I, S>(&self, cwd: Option<&Path>, args: I) -> Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_os_string()).collect();
        let out = self.run(cwd, &args)?;
        if out.success() {
            Ok(out)
        } else {
            Err(failed(&args, &out))
        }
    }

    /// Run Git attached to the user's terminal (progress output, password
    /// and passphrase prompts), with repository-redirecting variables
    /// removed. `summary` names the command in errors instead of the
    /// arguments, which may contain credentials.
    pub fn run_interactive<I, S>(&self, summary: &str, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let status = self
            .base_command(None, true)
            .args(args)
            .status()
            .map_err(|e| Error::GitNotFound {
                program: self.program.display().to_string(),
                message: e.to_string(),
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::GitFailed {
                args: summary.to_string(),
                code: status.code(),
                stderr: "see Git's output above".into(),
            })
        }
    }

    /// The installed Git version, checked against [`MIN_GIT_VERSION`].
    pub fn version(&self) -> Result<GitVersion> {
        let out = self.run_ok(None, ["version"])?;
        let text = out.stdout_trimmed();
        let version = parse_version(&text).ok_or_else(|| Error::GitFailed {
            args: "version".into(),
            code: out.code,
            stderr: format!("unrecognised version output: {text}"),
        })?;
        if (version.major, version.minor) < MIN_GIT_VERSION {
            return Err(Error::GitTooOld {
                found: version.text,
                required: MIN_GIT_VERSION_STR,
            });
        }
        Ok(version)
    }
}

pub(crate) fn failed(args: &[OsString], out: &GitOutput) -> Error {
    Error::GitFailed {
        args: args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" "),
        code: out.code,
        stderr: out.stderr.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// Full version string as printed by Git, e.g. `2.43.0`.
    pub text: String,
}

/// Parse the output of `git version` (e.g. `git version 2.43.0` or
/// `git version 2.39.3 (Apple Git-146)`).
pub fn parse_version(output: &str) -> Option<GitVersion> {
    let text = output
        .trim()
        .strip_prefix("git version ")?
        .split_whitespace()
        .next()?;
    let mut parts = text.split(['.', '-']);
    let mut number = || parts.next().and_then(|p| p.parse::<u32>().ok());
    let major = number()?;
    let minor = number()?;
    let patch = number().unwrap_or(0);
    let version_text = text.split(['-']).next().unwrap_or(text);
    let canonical: String = version_text.split('.').take(3).collect::<Vec<_>>().join(".");
    Some(GitVersion {
        major,
        minor,
        patch,
        text: canonical,
    })
}

/// A commit identity as printed by `git var GIT_AUTHOR_IDENT`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ident {
    pub name: String,
    pub email: String,
}

/// Parse `Name <email> 1700000000 +0000`.
pub fn parse_ident(output: &str) -> Option<Ident> {
    let line = output.lines().next()?;
    let close = line.rfind('>')?;
    let open = line[..close].rfind('<')?;
    let name = line[..open].trim_end().to_string();
    let email = line[open + 1..close].to_string();
    Some(Ident { name, email })
}

/// Configuration scope as reported by `git config --show-scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    System,
    Global,
    Local,
    Worktree,
    Command,
    Unknown,
}

impl Scope {
    fn parse(s: &str) -> Scope {
        match s {
            "system" => Scope::System,
            "global" => Scope::Global,
            "local" => Scope::Local,
            "worktree" => Scope::Worktree,
            "command" => Scope::Command,
            _ => Scope::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::System => "system",
            Scope::Global => "global",
            Scope::Local => "local",
            Scope::Worktree => "worktree",
            Scope::Command => "command",
            Scope::Unknown => "unknown",
        }
    }
}

/// Where a configuration value came from, as reported by `--show-origin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Origin {
    File(PathBuf),
    CommandLine,
    StandardInput,
    Blob(String),
    Other(String),
}

/// One value of a configuration key with its scope and origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigEntry {
    pub scope: Scope,
    pub origin: Origin,
    pub value: String,
}

/// Parse the output of `git config -z --show-scope --show-origin --get-all`:
/// repeated `scope NUL origin NUL value NUL` triples.
pub fn parse_scoped_entries(output: &[u8]) -> Vec<ConfigEntry> {
    let fields: Vec<&[u8]> = nul_records(output).collect();
    fields
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triple| ConfigEntry {
            scope: Scope::parse(&String::from_utf8_lossy(triple[0])),
            origin: parse_origin(triple[1]),
            value: String::from_utf8_lossy(triple[2]).into_owned(),
        })
        .collect()
}

fn parse_origin(raw: &[u8]) -> Origin {
    if let Some(path) = raw.strip_prefix(b"file:") {
        return Origin::File(path_from_bytes(path));
    }
    let text = String::from_utf8_lossy(raw);
    match text.as_ref() {
        "command line:" => Origin::CommandLine,
        "standard input:" => Origin::StandardInput,
        other => match other.strip_prefix("blob:") {
            Some(blob) => Origin::Blob(blob.to_string()),
            None => Origin::Other(other.to_string()),
        },
    }
}

/// Split NUL-terminated records, dropping an unterminated trailing fragment
/// (which can only come from truncated output).
fn nul_records(output: &[u8]) -> impl Iterator<Item = &[u8]> {
    let complete = output.iter().rposition(|&b| b == 0).map(|last| &output[..last]);
    complete.into_iter().flat_map(|body| body.split(|&b| b == 0))
}

/// Parse `git config -z --show-scope --show-origin --get-regexp`: repeated
/// `scope NUL origin NUL key LF value NUL` records.
pub fn parse_scoped_key_values(output: &[u8]) -> Vec<(String, ConfigEntry)> {
    let fields: Vec<&[u8]> = nul_records(output).collect();
    fields
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triple| {
            let (key, value) = split_key_value(triple[2]);
            (
                key,
                ConfigEntry {
                    scope: Scope::parse(&String::from_utf8_lossy(triple[0])),
                    origin: parse_origin(triple[1]),
                    value,
                },
            )
        })
        .collect()
}

fn split_key_value(record: &[u8]) -> (String, String) {
    match record.iter().position(|&b| b == b'\n') {
        Some(i) => (
            String::from_utf8_lossy(&record[..i]).into_owned(),
            String::from_utf8_lossy(&record[i + 1..]).into_owned(),
        ),
        None => (String::from_utf8_lossy(record).into_owned(), String::new()),
    }
}

/// Parse the output of `git config -z --get-regexp`: repeated
/// `key LF value NUL` records. A key without a value has no LF.
pub fn parse_key_values(output: &[u8]) -> Vec<(String, String)> {
    nul_records(output).map(split_key_value).collect()
}

/// Parse the output of `git config -z --get-all`: repeated `value NUL`.
pub fn parse_values(output: &[u8]) -> Vec<String> {
    nul_records(output)
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .collect()
}

#[cfg(unix)]
pub(crate) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
pub(crate) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_version() {
        let v = parse_version("git version 2.43.0").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (2, 43, 0));
        assert_eq!(v.text, "2.43.0");
    }

    #[test]
    fn parses_vendor_suffixed_versions() {
        let v = parse_version("git version 2.39.3 (Apple Git-146)").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (2, 39, 3));
        let v = parse_version("git version 2.45.1.windows.1").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (2, 45, 1));
        let v = parse_version("git version 2.50.0-rc1").unwrap();
        assert_eq!((v.major, v.minor), (2, 50));
    }

    #[test]
    fn rejects_non_version_output() {
        assert!(parse_version("hello").is_none());
        assert!(parse_version("git version").is_none());
    }

    #[test]
    fn parses_ident_with_spaces_and_unicode() {
        let id = parse_ident("Zoë  van der Berg <zoe@example.com> 1792060608 +0600").unwrap();
        assert_eq!(id.name, "Zoë  van der Berg");
        assert_eq!(id.email, "zoe@example.com");
    }

    #[test]
    fn rejects_malformed_ident() {
        assert!(parse_ident("no email here 123 +0000").is_none());
    }

    #[test]
    fn parses_scoped_entries_with_spaces_in_paths() {
        let raw = b"global\0file:/home/a b/.gitconfig\0Global Name\0command\0command line:\0x@y\0local\0file:.git/config\0v\0";
        let entries = parse_scoped_entries(raw);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].scope, Scope::Global);
        assert_eq!(
            entries[0].origin,
            Origin::File(PathBuf::from("/home/a b/.gitconfig"))
        );
        assert_eq!(entries[0].value, "Global Name");
        assert_eq!(entries[1].scope, Scope::Command);
        assert_eq!(entries[1].origin, Origin::CommandLine);
        assert_eq!(entries[2].origin, Origin::File(PathBuf::from(".git/config")));
    }

    #[test]
    fn parses_value_containing_newline() {
        let raw = b"global\0file:/h/.gitconfig\0line one\nline two\0";
        let entries = parse_scoped_entries(raw);
        assert_eq!(entries[0].value, "line one\nline two");
    }

    #[test]
    fn ignores_truncated_trailing_record() {
        let raw = b"global\0file:/h/.gitconfig\0a\0global\0file:/h";
        assert_eq!(parse_scoped_entries(raw).len(), 1);
    }

    #[test]
    fn parses_key_values() {
        let raw = b"includeif.gitdir:~/w o\"rk/.path\n/x y.inc\0core.bare\0";
        let kv = parse_key_values(raw);
        assert_eq!(
            kv,
            vec![
                (
                    "includeif.gitdir:~/w o\"rk/.path".to_string(),
                    "/x y.inc".to_string()
                ),
                ("core.bare".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn parses_scoped_key_values() {
        let raw = b"global\0file:/h/.gitconfig\0user.name\nGlobal Name\0local\0file:.git/config\0credential.helper\nstore\0";
        let kv = parse_scoped_key_values(raw);
        assert_eq!(kv.len(), 2);
        assert_eq!(kv[0].0, "user.name");
        assert_eq!(kv[0].1.value, "Global Name");
        assert_eq!(kv[1].1.scope, Scope::Local);
        assert_eq!(kv[1].0, "credential.helper");
    }

    #[test]
    fn parses_values() {
        assert_eq!(parse_values(b"a\0b c\0"), vec!["a", "b c"]);
        assert!(parse_values(b"").is_empty());
    }
}
