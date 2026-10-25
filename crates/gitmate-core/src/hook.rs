//! The optional pre-commit hook (`gitmate hook install`).
//!
//! The hook is a fixed script that runs `gitmate hook check`, which refuses a
//! commit whose author is not the profile GitMate expects. It is the only
//! file GitMate writes inside a repository. A hook GitMate did not write is
//! replaced only when forced and never removed, and nothing is written when
//! `core.hooksPath` makes Git run hooks from somewhere else.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::fsutil;
use crate::git::Git;
use crate::paths::normalize_lexically;
use crate::repo::RepoInfo;

/// Line that marks a hook file as GitMate's.
const MARKER: &str = "# gitmate:pre-commit";

/// Without `gitmate` on PATH (a GUI client's environment, or GitMate was
/// uninstalled) the hook lets the commit through instead of blocking every
/// commit in the repository.
const SCRIPT: &str = "#!/bin/sh
# gitmate:pre-commit
# Installed by `gitmate hook install`; remove it with `gitmate hook uninstall`.
# Refuses commits whose author is not the profile GitMate expects here.
# Skip it once with `git commit --no-verify`.
command -v gitmate >/dev/null 2>&1 || exit 0
exec gitmate hook check
";

/// Finding codes that make `gitmate hook check` refuse a commit: problems
/// with the identity the commit would record, nothing else.
pub const BLOCKING_FINDINGS: &[&str] = &[
    "identity_unset",
    "identity_guessed",
    "profile_mismatch",
    "remote_profile_mismatch",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookStatus {
    /// `hooks/pre-commit` in the repository's git directory (shared by its
    /// linked worktrees), where `install` writes.
    pub path: PathBuf,
    /// A file exists at `path`.
    pub installed: bool,
    /// That file is GitMate's hook.
    pub managed: bool,
    /// The directory Git runs hooks from instead, when `core.hooksPath`
    /// points elsewhere. A hook at `path` never runs then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks_path: Option<PathBuf>,
}

impl HookStatus {
    /// Whether Git runs GitMate's hook before each commit.
    pub fn is_active(&self) -> bool {
        self.managed && self.hooks_path.is_none()
    }
}

pub(crate) fn status(git: &Git, dir: &Path, repo: &RepoInfo) -> Result<HookStatus> {
    let default_dir = repo.common_dir.join("hooks");
    // Git resolves core.hooksPath (relative, `~`, worktrees) for us.
    let out = git.run_at(
        dir,
        ["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )?;
    let used = out.stdout_trimmed();
    if !out.success() || used.contains('\n') || !Path::new(&used).is_absolute() {
        return Err(Error::GitFailed {
            args: "rev-parse --git-path hooks".into(),
            code: out.code,
            stderr: format!("unexpected output: {used:?} {}", out.stderr),
        });
    }
    let used = PathBuf::from(used);
    let hooks_path = (normalize_lexically(&used) != normalize_lexically(&default_dir)).then_some(used);

    let path = default_dir.join("pre-commit");
    let (installed, managed) = match std::fs::read(&path) {
        Ok(bytes) => (true, String::from_utf8_lossy(&bytes).lines().any(|l| l == MARKER)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, false),
        Err(e) => return Err(Error::io("could not read", &path, e)),
    };
    Ok(HookStatus {
        path,
        installed,
        managed,
        hooks_path,
    })
}

pub(crate) fn install(git: &Git, dir: &Path, repo: &RepoInfo, force: bool) -> Result<HookStatus> {
    let status = status(git, dir, repo)?;
    if let Some(hooks_path) = status.hooks_path {
        return Err(Error::HooksPathOverridden { hooks_path });
    }
    if status.installed && !status.managed && !force {
        return Err(Error::HookConflict { path: status.path });
    }
    if let Some(hooks_dir) = status.path.parent() {
        std::fs::create_dir_all(hooks_dir).map_err(|e| Error::io("could not create", hooks_dir, e))?;
    }
    fsutil::atomic_replace_with(&status.path, |tmp| {
        std::fs::write(tmp, SCRIPT).map_err(|e| Error::io("could not write", tmp, e))?;
        make_executable(tmp)
    })?;
    Ok(HookStatus {
        installed: true,
        managed: true,
        ..status
    })
}

pub(crate) fn uninstall(git: &Git, dir: &Path, repo: &RepoInfo) -> Result<HookStatus> {
    let status = status(git, dir, repo)?;
    if !status.installed {
        return Ok(status);
    }
    if !status.managed {
        return Err(Error::HookNotManaged { path: status.path });
    }
    std::fs::remove_file(&status.path).map_err(|e| Error::io("could not remove", &status.path, e))?;
    Ok(HookStatus {
        installed: false,
        managed: false,
        ..status
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| Error::io("could not set permissions on", path, e))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}
