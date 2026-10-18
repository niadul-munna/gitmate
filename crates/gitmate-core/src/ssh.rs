//! SSH key checks and `core.sshCommand` construction.
//!
//! Private keys are inspected through filesystem metadata only. Their
//! contents are never opened.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::error::{Error, Result};

/// Result of checking a private key path without reading it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum KeyStatus {
    /// Exists, is a regular file, and has permissions ssh accepts.
    Ok,
    Missing,
    NotAFile,
    /// ssh refuses private keys readable by group or others.
    PermissionsTooOpen {
        mode: u32,
    },
    /// The path could not be inspected (e.g. a parent directory is not
    /// accessible).
    Inaccessible {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyCheck {
    /// Path as configured (may start with `~/`).
    pub configured: PathBuf,
    /// Path after `~` expansion.
    pub path: PathBuf,
    pub status: KeyStatus,
    /// Whether `<key>.pub` exists next to the private key.
    pub public_key_present: bool,
}

impl KeyCheck {
    pub fn is_usable(&self) -> bool {
        matches!(self.status, KeyStatus::Ok)
    }

    /// Human explanation of a non-OK status.
    pub fn problem(&self) -> Option<String> {
        let shown = self.configured.display();
        match &self.status {
            KeyStatus::Ok => None,
            KeyStatus::Missing => Some(format!("SSH key {shown} does not exist")),
            KeyStatus::NotAFile => Some(format!("SSH key {shown} is not a regular file")),
            KeyStatus::PermissionsTooOpen { mode } => Some(format!(
                "SSH key {shown} has mode {mode:o}; ssh refuses keys readable by others (run `chmod 600`)"
            )),
            KeyStatus::Inaccessible { message } => {
                Some(format!("SSH key {shown} cannot be inspected: {message}"))
            }
        }
    }
}

/// Private keys in `dir`, recognised by the `<name>.pub` file `ssh-keygen`
/// writes next to each one. Only file names and metadata are read, never
/// key contents. A missing or unreadable directory yields an empty list.
pub fn find_private_keys(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut keys: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_none_or(|ext| ext != "pub") && path.is_file())
        .filter(|path| {
            let mut public = path.clone().into_os_string();
            public.push(".pub");
            Path::new(&public).is_file()
        })
        .collect();
    keys.sort();
    keys
}

/// Inspect a private key path using metadata only.
pub fn check_key(configured: &Path, home: &Path) -> KeyCheck {
    let path = crate::paths::expand_tilde(configured, home);
    let status = match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeyStatus::Missing,
        Err(e) => KeyStatus::Inaccessible {
            message: e.to_string(),
        },
        Ok(meta) if !meta.is_file() => KeyStatus::NotAFile,
        Ok(meta) => permission_status(&meta),
    };
    let mut public = path.clone().into_os_string();
    public.push(".pub");
    KeyCheck {
        configured: configured.to_path_buf(),
        public_key_present: Path::new(&public).is_file(),
        path,
        status,
    }
}

#[cfg(unix)]
fn permission_status(meta: &std::fs::Metadata) -> KeyStatus {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        KeyStatus::PermissionsTooOpen { mode }
    } else {
        KeyStatus::Ok
    }
}

#[cfg(not(unix))]
fn permission_status(_meta: &std::fs::Metadata) -> KeyStatus {
    KeyStatus::Ok
}

/// Quote `s` for a POSIX shell using single quotes.
pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build the `core.sshCommand` value that makes Git authenticate with exactly
/// `key` (an expanded, absolute path). Git runs this value through a shell,
/// so the path is single-quoted and control characters are rejected.
pub fn ssh_command_for_key(key: &Path) -> Result<String> {
    let invalid = |reason: &str| Error::InvalidPath {
        path: key.to_path_buf(),
        reason: reason.into(),
    };
    let text = key.to_str().ok_or_else(|| invalid("is not valid UTF-8"))?;
    if text.chars().any(char::is_control) {
        return Err(invalid("contains control characters"));
    }
    if !key.is_absolute() {
        return Err(invalid("must be absolute"));
    }
    // Git for Windows also runs core.sshCommand through its POSIX shell, so
    // forward slashes and single quotes work on every platform.
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_string()
    };
    Ok(format!(
        "ssh -i {} -o IdentitiesOnly=yes",
        shell_single_quote(&text)
    ))
}

/// The SSH client settings `ssh -G` reports for a host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SshHostConfig {
    pub host: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub identities_only: bool,
    pub identity_files: Vec<String>,
}

/// Extract the fields GitMate shows from `ssh -G` output. Everything else
/// is discarded and never logged.
pub fn parse_ssh_g(host: &str, output: &str) -> SshHostConfig {
    let mut cfg = SshHostConfig {
        host: host.to_string(),
        ..SshHostConfig::default()
    };
    for line in output.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        match key {
            "hostname" => cfg.hostname = Some(value.to_string()),
            "user" => cfg.user = Some(value.to_string()),
            "identitiesonly" => cfg.identities_only = value == "yes",
            "identityfile" => cfg.identity_files.push(value.to_string()),
            _ => {}
        }
    }
    cfg
}

/// Whether `host` is safe to pass to `ssh` as a destination argument.
pub fn is_valid_ssh_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['-', '.'])
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Ask the local ssh client which configuration applies to `host`
/// (`ssh -G`). This does not open a network connection, but it does evaluate
/// the user's ssh_config, including any `Match exec` commands in it, so it
/// must only run on explicit user request.
pub fn resolve_host(host: &str, config_file: Option<&Path>) -> Result<SshHostConfig> {
    if !is_valid_ssh_host(host) {
        return Err(Error::InvalidPath {
            path: PathBuf::from(host),
            reason: "is not a valid SSH host name".into(),
        });
    }
    let mut args: Vec<OsString> = vec!["-G".into()];
    if let Some(file) = config_file {
        args.push("-F".into());
        args.push(file.into());
    }
    args.push("--".into());
    args.push(host.into());
    let output = Command::new("ssh")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| Error::io("could not run", "ssh", e))?;
    if !output.status.success() {
        return Err(Error::GitFailed {
            args: format!("ssh -G {host}"),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(parse_ssh_g(host, &String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    fn write_key(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, b"not a real key").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn checks_key_metadata_only() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh dir");
        fs::create_dir(&ssh).unwrap();

        let good = ssh.join("id_work");
        write_key(&good, 0o600);
        fs::write(ssh.join("id_work.pub"), b"ssh-ed25519 AAAA test").unwrap();
        let check = check_key(Path::new("~/.ssh dir/id_work"), home.path());
        assert_eq!(check.status, KeyStatus::Ok);
        assert!(check.public_key_present);
        assert_eq!(check.path, good);

        let open = ssh.join("id_open");
        write_key(&open, 0o644);
        assert_eq!(
            check_key(&open, home.path()).status,
            KeyStatus::PermissionsTooOpen { mode: 0o644 }
        );
        assert!(!check_key(&open, home.path()).public_key_present);

        assert_eq!(
            check_key(&ssh.join("nope"), home.path()).status,
            KeyStatus::Missing
        );
        assert_eq!(check_key(&ssh, home.path()).status, KeyStatus::NotAFile);
    }

    #[test]
    fn single_quotes_hostile_strings() {
        assert_eq!(shell_single_quote("plain"), "'plain'");
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
        assert_eq!(
            shell_single_quote("$(rm -rf ~)`x` \"y\""),
            "'$(rm -rf ~)`x` \"y\"'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ssh_command_survives_a_real_shell() {
        // The quoted path must reach the program as exactly one argument.
        let hostile = "/tmp/key dir/it's $(touch pwned) `x`/id";
        let command = ssh_command_for_key(Path::new(hostile)).unwrap();
        assert!(command.starts_with("ssh -i '"));
        assert!(command.ends_with(" -o IdentitiesOnly=yes"));
        let probe = command.replacen("ssh", "printf '%s\\n'", 1);
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&probe)
            .output()
            .unwrap();
        let args: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(args, vec!["-i", hostile, "-o", "IdentitiesOnly=yes"]);
    }

    #[test]
    fn ssh_command_rejects_control_characters_and_relative_paths() {
        assert!(ssh_command_for_key(Path::new("/tmp/a\nb")).is_err());
        assert!(ssh_command_for_key(Path::new("relative/id")).is_err());
    }

    #[test]
    fn parses_relevant_ssh_g_fields_only() {
        let out = "user git\nhostname github.com\nport 22\nidentitiesonly yes\nidentityfile ~/.ssh/id_work\nidentityfile ~/.ssh/id_other\nproxycommand secret-thing\n";
        let cfg = parse_ssh_g("gh-work", out);
        assert_eq!(cfg.host, "gh-work");
        assert_eq!(cfg.hostname.as_deref(), Some("github.com"));
        assert_eq!(cfg.user.as_deref(), Some("git"));
        assert!(cfg.identities_only);
        assert_eq!(cfg.identity_files, vec!["~/.ssh/id_work", "~/.ssh/id_other"]);
    }

    #[test]
    fn validates_ssh_hosts() {
        for ok in ["github.com", "gh-work", "10.0.0.1", "host_1.example"] {
            assert!(is_valid_ssh_host(ok), "{ok}");
        }
        for bad in ["", "-oProxyCommand=x", "a b", "a;b", "$(x)", "host\n", "[::1]"] {
            assert!(!is_valid_ssh_host(bad), "{bad:?}");
        }
    }

    #[test]
    fn resolve_host_reads_only_the_given_config() {
        if Command::new("ssh").arg("-V").output().is_err() {
            eprintln!("ssh not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config");
        fs::write(&cfg, "Host gh-work\n  HostName github.com\n  User git\n  IdentityFile \"/k/work key\"\n  IdentitiesOnly yes\n").unwrap();
        let resolved = resolve_host("gh-work", Some(&cfg)).unwrap();
        assert_eq!(resolved.hostname.as_deref(), Some("github.com"));
        assert!(resolved.identities_only);
        assert_eq!(resolved.identity_files, vec!["/k/work key"]);
        assert!(resolve_host("-oProxyCommand=touch /tmp/x", Some(&cfg)).is_err());
    }
}
