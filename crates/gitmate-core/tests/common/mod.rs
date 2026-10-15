//! Test sandbox: every test gets its own HOME, XDG directories and GitMate
//! directories inside a temporary directory whose path contains a space and
//! non-ASCII characters. Git is run with system config disabled and with every
//! inherited identity/config/redirect variable removed, so the developer's
//! real `~/.gitconfig`, `~/.ssh` and environment are never read or written.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use gitmate_core::GitMate;
use gitmate_core::git::Git;
use gitmate_core::paths::AppPaths;

const STRIPPED_VARS: &[&str] = &[
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    "EMAIL",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_TEMPLATE_DIR",
];

pub struct Sandbox {
    pub root: tempfile::TempDir,
    pub home: PathBuf,
    pub git: Git,
    pub paths: AppPaths,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        let root = tempfile::Builder::new()
            .prefix("gitmate tëst ")
            .tempdir()
            .expect("create temp dir");
        let home = root.path().join("home dir");
        let config_dir = home.join(".config/gitmate");
        let state_dir = home.join(".local/state/gitmate");
        std::fs::create_dir_all(&home).unwrap();

        let real_home = std::env::var_os("HOME").map(PathBuf::from);
        assert_ne!(
            Some(home.clone()),
            real_home,
            "sandbox must never use the real HOME"
        );
        assert!(
            home.starts_with(std::env::temp_dir()),
            "sandbox HOME must be under the temp dir"
        );

        let mut git = Git::default()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GITMATE_CONFIG_DIR", &config_dir)
            .env("GITMATE_STATE_DIR", &state_dir)
            // Keep `git init` quiet and deterministic regardless of host config.
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "init.defaultBranch")
            .env("GIT_CONFIG_VALUE_0", "main");
        for var in STRIPPED_VARS {
            if *var != "GIT_CONFIG_COUNT" {
                git = git.env_remove(*var);
            }
        }
        let paths = AppPaths::resolve(|k| git.effective_env(k)).unwrap();
        assert!(paths.config_dir.starts_with(&home));
        assert!(paths.state_dir.starts_with(&home));
        Sandbox {
            root,
            home,
            git,
            paths,
        }
    }

    /// A GitMate facade bound to this sandbox.
    pub fn app(&self) -> GitMate {
        GitMate::new(self.git.clone(), self.paths.clone())
    }

    pub fn global_config(&self) -> PathBuf {
        self.home.join(".gitconfig")
    }

    /// Run git in `dir` and return trimmed stdout; panics with stderr on failure.
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git.run_at(dir, args).expect("spawn git");
        assert!(
            out.success(),
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            out.stderr
        );
        out.stdout_trimmed()
    }

    pub fn set_global(&self, key: &str, value: &str) {
        self.git_in(&self.home, &["config", "--global", key, value]);
    }

    /// Create a directory (with parents) below HOME.
    pub fn dir(&self, rel: &str) -> PathBuf {
        let p = self.home.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// `git init` a repository at HOME/`rel` with one commit.
    pub fn repo(&self, rel: &str) -> PathBuf {
        let p = self.dir(rel);
        self.git_in(&p, &["init", "-q"]);
        self.git_in(
            &p,
            &[
                "-c",
                "user.name=Setup",
                "-c",
                "user.email=setup@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        p
    }

    /// Effective author email in `dir` according to `git var`.
    pub fn author_email(&self, dir: &Path) -> Option<String> {
        let out = self.git.run_at(dir, ["var", "GIT_AUTHOR_IDENT"]).unwrap();
        if !out.success() {
            return None;
        }
        let line = out.stdout_trimmed();
        let start = line.rfind('<')? + 1;
        let end = line.rfind('>')?;
        Some(line[start..end].to_string())
    }

    pub fn read(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }
}
