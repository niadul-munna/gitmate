//! Repository discovery, delegated to `git rev-parse`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::git::Git;

/// A repository as Git discovered it from some directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoInfo {
    /// `$GIT_DIR`. For a linked worktree: `<main>/.git/worktrees/<name>`.
    pub git_dir: PathBuf,
    /// `$GIT_COMMON_DIR`, holding the shared `config`.
    pub common_dir: PathBuf,
    /// Top-level of the working tree; `None` for bare repositories.
    pub work_tree: Option<PathBuf>,
    pub is_bare: bool,
    pub is_linked_worktree: bool,
}

impl RepoInfo {
    /// The repository-local config file (shared by all worktrees).
    pub fn local_config(&self) -> PathBuf {
        self.common_dir.join("config")
    }

    /// The per-worktree config file (used only with
    /// `extensions.worktreeConfig`).
    pub fn worktree_config(&self) -> PathBuf {
        self.git_dir.join("config.worktree")
    }

    /// Directory that best identifies the repository to a person.
    pub fn root(&self) -> &Path {
        self.work_tree.as_deref().unwrap_or(&self.git_dir)
    }
}

/// Validate a directory path received from a user or from the desktop UI.
pub fn validate_directory(path: &Path) -> Result<PathBuf> {
    let invalid = |reason: String| Error::InvalidPath {
        path: path.to_path_buf(),
        reason,
    };
    let text = path.as_os_str();
    if text.is_empty() {
        return Err(invalid("is empty".into()));
    }
    if text.as_encoded_bytes().iter().any(|b| b.is_ascii_control()) {
        return Err(invalid("contains control characters".into()));
    }
    if !path.is_absolute() {
        return Err(invalid("must be an absolute path".into()));
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(crate::paths::normalize_lexically(path)),
        Ok(_) => Err(invalid("is not a directory".into())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(invalid("does not exist".into())),
        Err(e) => Err(invalid(e.to_string())),
    }
}

/// Discover the repository containing `dir`. Returns `Ok(None)` outside a
/// repository. Git's `safe.directory` ownership check is respected and
/// reported as [`Error::UnsafeRepository`].
pub fn discover(git: &Git, dir: &Path) -> Result<Option<RepoInfo>> {
    let out = git.run_at(
        dir,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
            "--is-bare-repository",
        ],
    )?;
    if !out.success() {
        return classify_failure(dir, &out.stderr).map(|()| None);
    }
    // The output is line-based, so a path containing a newline would be
    // split; insist on exactly three lines and absolute paths.
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let unexpected = || Error::GitFailed {
        args: "rev-parse".into(),
        code: out.code,
        stderr: format!("unexpected output: {stdout:?}"),
    };
    let [git_dir, common_dir, bare] = stdout.lines().collect::<Vec<_>>()[..] else {
        return Err(unexpected());
    };
    let is_bare = bare == "true";
    let git_dir = PathBuf::from(git_dir);
    let common_dir = PathBuf::from(common_dir);
    if !git_dir.is_absolute() || !common_dir.is_absolute() {
        return Err(unexpected());
    }

    let work_tree = if is_bare {
        None
    } else {
        // Fails when `dir` is inside the .git directory itself; that is not a
        // working tree, so leave it unset rather than guess.
        let out = git.run_at(dir, ["rev-parse", "--path-format=absolute", "--show-toplevel"])?;
        let top = out.stdout_trimmed();
        (out.success() && !top.contains('\n'))
            .then(|| PathBuf::from(top))
            .filter(|p| p.is_absolute())
    };

    Ok(Some(RepoInfo {
        is_linked_worktree: git_dir != common_dir,
        git_dir,
        common_dir,
        work_tree,
        is_bare,
    }))
}

/// `Ok(())` when Git simply found no repository.
fn classify_failure(dir: &Path, stderr: &str) -> Result<()> {
    if stderr.contains("not a git repository") {
        Ok(())
    } else if stderr.contains("dubious ownership") || stderr.contains("safe.directory") {
        Err(Error::UnsafeRepository {
            path: dir.to_path_buf(),
            message: stderr
                .lines()
                .next()
                .unwrap_or(stderr)
                .trim_start_matches("fatal: ")
                .to_string(),
        })
    } else {
        Err(Error::GitFailed {
            args: "rev-parse --git-dir".into(),
            code: None,
            stderr: stderr.to_string(),
        })
    }
}
