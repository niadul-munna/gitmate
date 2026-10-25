//! Locations of GitMate's own files and of the global Git config file.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

/// Where GitMate keeps its own data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppPaths {
    /// Profiles and generated include files.
    pub config_dir: PathBuf,
    /// Operation journal, backups and the writer lock.
    pub state_dir: PathBuf,
    /// The user's home directory, used for `~` expansion.
    pub home: PathBuf,
}

impl AppPaths {
    /// Resolve directories from environment variables, using `lookup` so that
    /// callers (and tests) control the environment explicitly.
    ///
    /// * `GITMATE_CONFIG_DIR` / `GITMATE_STATE_DIR` override everything.
    /// * Linux and other Unix: `$XDG_CONFIG_HOME/gitmate` (default
    ///   `~/.config/gitmate`) and `$XDG_STATE_HOME/gitmate` (default
    ///   `~/.local/state/gitmate`).
    /// * macOS: `~/Library/Application Support/GitMate` (state in a
    ///   `state` subdirectory).
    /// * Windows: `%APPDATA%\GitMate` and `%LOCALAPPDATA%\GitMate`.
    pub fn resolve(lookup: impl Fn(&str) -> Option<OsString>) -> Result<AppPaths> {
        let non_empty = |key: &str| lookup(key).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = non_empty("HOME")
            .or_else(|| non_empty("USERPROFILE"))
            .filter(|p| p.is_absolute())
            .ok_or(Error::NoHomeDirectory)?;

        let absolute = |p: PathBuf| if p.is_absolute() { Some(p) } else { None };

        let (default_config, default_state) = if cfg!(target_os = "macos") {
            let base = home.join("Library/Application Support/GitMate");
            (base.clone(), base.join("state"))
        } else if cfg!(windows) {
            let roaming = non_empty("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
            let local = non_empty("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
            (roaming.join("GitMate"), local.join("GitMate"))
        } else {
            let config = non_empty("XDG_CONFIG_HOME")
                .and_then(absolute)
                .unwrap_or_else(|| home.join(".config"));
            let state = non_empty("XDG_STATE_HOME")
                .and_then(absolute)
                .unwrap_or_else(|| home.join(".local/state"));
            (config.join("gitmate"), state.join("gitmate"))
        };

        Ok(AppPaths {
            config_dir: non_empty("GITMATE_CONFIG_DIR")
                .and_then(absolute)
                .unwrap_or(default_config),
            state_dir: non_empty("GITMATE_STATE_DIR")
                .and_then(absolute)
                .unwrap_or(default_state),
            home,
        })
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.config_dir.join("profiles.toml")
    }

    pub fn includes_dir(&self) -> PathBuf {
        self.config_dir.join("includes")
    }

    /// Include file generated for a profile. `id` must be a validated profile
    /// id (see [`crate::profile::validate_id`]), which cannot contain path
    /// separators.
    pub fn include_file(&self, id: &str) -> PathBuf {
        self.includes_dir().join(format!("{id}.gitconfig"))
    }

    pub fn operations_dir(&self) -> PathBuf {
        self.state_dir.join("operations")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.state_dir.join("backups")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.state_dir.join("lock")
    }

    /// Expand a leading `~/` (or a bare `~`) to the home directory.
    pub fn expand_tilde(&self, path: &Path) -> PathBuf {
        expand_tilde(path, &self.home)
    }

    /// Display form of a path with the home directory shown as `~`.
    pub fn display(&self, path: &Path) -> String {
        contract_tilde(path, &self.home)
    }
}

pub fn expand_tilde(path: &Path, home: &Path) -> PathBuf {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == "~" => home.join(components.as_path()),
        _ => path.to_path_buf(),
    }
}

pub fn contract_tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        // `~/` paths use `/` on every platform, as profiles.toml and Git
        // config spell them.
        Ok(rest) => {
            let parts: Vec<_> = rest.iter().map(|part| part.to_string_lossy()).collect();
            format!("~/{}", parts.join("/"))
        }
        Err(_) => path.display().to_string(),
    }
}

/// [`std::fs::canonicalize`] without the `\\?\` prefix it adds on Windows,
/// which Git does not understand and whose `?` and `\` are pattern
/// characters in a `gitdir:` rule.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let real = std::fs::canonicalize(path)?;
    if cfg!(windows)
        && let Some(plain) = real.to_str().and_then(without_verbatim_prefix)
    {
        return Ok(PathBuf::from(plain));
    }
    Ok(real)
}

/// The everyday spelling of a Windows verbatim path: `\\?\C:\x` is `C:\x`
/// and `\\?\UNC\server\share` is `\\server\share`. `None` for anything else.
fn without_verbatim_prefix(path: &str) -> Option<String> {
    let rest = path.strip_prefix(r"\\?\")?;
    if let Some(share) = rest.strip_prefix(r"UNC\") {
        return Some(format!(r"\\{share}"));
    }
    let drive = rest.as_bytes();
    (drive.len() >= 3 && drive[0].is_ascii_alphabetic() && drive[1] == b':' && drive[2] == b'\\')
        .then(|| rest.to_string())
}

/// Lexically normalise an absolute path: remove `.` components and resolve
/// `..` against preceding components. Symlinks are not consulted.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The file that `git config --global` writes to, following `git-config(1)`:
/// `GIT_CONFIG_GLOBAL` if set; otherwise `~/.gitconfig`, unless only
/// `$XDG_CONFIG_HOME/git/config` exists.
pub fn global_config_target(lookup: impl Fn(&str) -> Option<OsString>, home: &Path) -> PathBuf {
    if let Some(explicit) = lookup("GIT_CONFIG_GLOBAL").filter(|v| !v.is_empty()) {
        return expand_tilde(Path::new(&explicit), home);
    }
    let dot_gitconfig = home.join(".gitconfig");
    let xdg = lookup("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("git/config");
    if !dot_gitconfig.exists() && xdg.is_file() {
        xdg
    } else {
        dot_gitconfig
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn resolves_xdg_directories() {
        let p = AppPaths::resolve(env(&[
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", "/x/cfg"),
            ("XDG_STATE_HOME", "/x/st"),
        ]))
        .unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/x/cfg/gitmate"));
        assert_eq!(p.state_dir, PathBuf::from("/x/st/gitmate"));
        let p = AppPaths::resolve(env(&[("HOME", "/h")])).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/h/.config/gitmate"));
        assert_eq!(p.state_dir, PathBuf::from("/h/.local/state/gitmate"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn ignores_relative_xdg_paths_as_the_spec_requires() {
        let p = AppPaths::resolve(env(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "rel")])).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/h/.config/gitmate"));
    }

    #[cfg(unix)]
    #[test]
    fn explicit_overrides_win() {
        let p = AppPaths::resolve(env(&[
            ("HOME", "/h"),
            ("GITMATE_CONFIG_DIR", "/c"),
            ("GITMATE_STATE_DIR", "/s"),
        ]))
        .unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/c"));
        assert_eq!(p.state_dir, PathBuf::from("/s"));
        assert_eq!(
            p.include_file("work"),
            PathBuf::from("/c/includes/work.gitconfig")
        );
    }

    #[test]
    fn missing_home_is_an_error() {
        assert!(matches!(AppPaths::resolve(env(&[])), Err(Error::NoHomeDirectory)));
    }

    #[cfg(unix)]
    #[test]
    fn expands_and_contracts_tilde() {
        let home = Path::new("/home/zoë");
        assert_eq!(
            expand_tilde(Path::new("~/.ssh/id"), home),
            PathBuf::from("/home/zoë/.ssh/id")
        );
        assert_eq!(expand_tilde(Path::new("~"), home), PathBuf::from("/home/zoë"));
        assert_eq!(
            expand_tilde(Path::new("/abs/~/x"), home),
            PathBuf::from("/abs/~/x")
        );
        assert_eq!(
            expand_tilde(Path::new("~other/x"), home),
            PathBuf::from("~other/x")
        );
        assert_eq!(
            contract_tilde(Path::new("/home/zoë/work dir"), home),
            "~/work dir"
        );
        assert_eq!(contract_tilde(Path::new("/home/zoëx"), home), "/home/zoëx");
        assert_eq!(contract_tilde(Path::new("/home/zoë"), home), "~");
    }

    #[cfg(windows)]
    #[test]
    fn contracts_tilde_with_forward_slashes() {
        let home = Path::new(r"C:\Users\zoë");
        assert_eq!(
            contract_tilde(Path::new(r"C:\Users\zoë\.ssh\id"), home),
            "~/.ssh/id"
        );
    }

    #[test]
    fn drops_the_verbatim_prefix_of_windows_paths() {
        assert_eq!(
            without_verbatim_prefix(r"\\?\C:\Users\ada\work").as_deref(),
            Some(r"C:\Users\ada\work")
        );
        assert_eq!(
            without_verbatim_prefix(r"\\?\UNC\server\share\work").as_deref(),
            Some(r"\\server\share\work")
        );
        assert_eq!(without_verbatim_prefix(r"C:\Users\ada"), None);
        assert_eq!(without_verbatim_prefix(r"\\?\Volume{0b1c}\work"), None);
    }

    #[test]
    fn canonical_paths_have_no_verbatim_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let real = canonicalize(dir.path()).unwrap();
        assert!(real.is_absolute());
        assert!(!real.to_string_lossy().starts_with(r"\\?\"), "{}", real.display());
    }

    #[cfg(unix)]
    #[test]
    fn normalizes_dot_components() {
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/../c/")),
            PathBuf::from("/a/c")
        );
        assert_eq!(normalize_lexically(Path::new("/../a")), PathBuf::from("/a"));
    }

    #[test]
    fn global_target_prefers_git_config_global() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom.cfg");
        let target = global_config_target(
            env(&[("GIT_CONFIG_GLOBAL", custom.to_str().unwrap())]),
            dir.path(),
        );
        assert_eq!(target, custom);
    }

    #[test]
    fn global_target_uses_xdg_only_when_it_exists_alone() {
        let home = tempfile::tempdir().unwrap();
        let xdg = home.path().join("xdg");
        let lookup = || env(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]);
        // Neither exists: ~/.gitconfig
        assert_eq!(
            global_config_target(lookup(), home.path()),
            home.path().join(".gitconfig")
        );
        // Only XDG exists: XDG
        std::fs::create_dir_all(xdg.join("git")).unwrap();
        std::fs::write(xdg.join("git/config"), "").unwrap();
        assert_eq!(
            global_config_target(lookup(), home.path()),
            xdg.join("git/config")
        );
        // Both exist: ~/.gitconfig
        std::fs::write(home.path().join(".gitconfig"), "").unwrap();
        assert_eq!(
            global_config_target(lookup(), home.path()),
            home.path().join(".gitconfig")
        );
    }
}
