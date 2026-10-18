//! End-to-end tests of the `gitmate` binary in an isolated HOME.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Env {
    _root: tempfile::TempDir,
    home: PathBuf,
}

const STRIPPED: &[&str] = &[
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "EMAIL",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_SSH_COMMAND",
    "GITMATE_CONFIG_DIR",
    "GITMATE_STATE_DIR",
    "NO_COLOR",
];

impl Env {
    fn new() -> Env {
        let root = tempfile::Builder::new().prefix("gitmate cli ").tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        assert!(home.starts_with(std::env::temp_dir()));
        Env { _root: root, home }
    }

    fn command(&self, program: &str, dir: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(dir)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null());
        for var in STRIPPED {
            cmd.env_remove(var);
        }
        cmd
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_gitmate"), dir)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run(dir, args);
        assert!(
            out.status.success(),
            "gitmate {} failed\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.command("git", dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn repo(&self, rel: &str) -> PathBuf {
        let dir = self.home.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q"]);
        dir
    }

    fn json(&self, dir: &Path, args: &[&str]) -> serde_json::Value {
        serde_json::from_str(&self.ok(dir, args)).unwrap()
    }
}

fn add_work_profile(env: &Env) {
    env.ok(
        &env.home,
        &[
            "profile",
            "add",
            "work",
            "--name",
            "Work Person",
            "--email",
            "work@acme.example",
            "--label",
            "Acme",
        ],
    );
}

#[test]
fn help_lists_every_command() {
    let env = Env::new();
    let help = env.ok(&env.home, &["--help"]);
    for cmd in ["profile", "whoami", "use", "rules", "doctor", "history", "undo"] {
        assert!(help.contains(cmd), "missing {cmd} in help:\n{help}");
    }
    let profile_help = env.ok(&env.home, &["profile", "--help"]);
    for cmd in ["add", "list", "show", "edit", "remove"] {
        assert!(profile_help.contains(cmd));
    }
}

#[test]
fn help_environment_descriptions_line_up() {
    let env = Env::new();
    let help = env.ok(&env.home, &["--help"]);
    let block: Vec<&str> = help
        .lines()
        .skip_while(|l| *l != "Environment:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(block.len(), 3, "{help}");
    // Column where the description starts: after the indent, the variable
    // name and the padding.
    let starts: Vec<usize> = block
        .iter()
        .map(|line| {
            let name_end = line.len() - line.trim_start().len() + line.trim_start().find(' ').unwrap();
            line.len() - line[name_end..].trim_start().len()
        })
        .collect();
    assert!(starts.iter().all(|s| *s == starts[0]), "{block:#?}");
}

#[test]
fn profile_lifecycle_with_json() {
    let env = Env::new();
    add_work_profile(&env);
    let list = env.json(&env.home, &["profile", "list", "--json"]);
    assert_eq!(list[0]["id"], "work");
    assert_eq!(list[0]["email"], "work@acme.example");

    let shown = env.json(&env.home, &["profile", "show", "work", "--json"]);
    assert_eq!(shown["profile"]["label"], "Acme");

    env.ok(
        &env.home,
        &["profile", "edit", "work", "--email", "new@acme.example"],
    );
    let shown = env.json(&env.home, &["profile", "show", "work", "--json"]);
    assert_eq!(shown["profile"]["email"], "new@acme.example");
    assert_eq!(
        shown["profile"]["name"], "Work Person",
        "unchanged fields are kept"
    );

    env.ok(&env.home, &["profile", "remove", "work", "--yes"]);
    assert_eq!(
        env.json(&env.home, &["profile", "list", "--json"]),
        serde_json::json!([])
    );
}

#[test]
fn invalid_profile_reports_field_errors() {
    let env = Env::new();
    let out = env.run(
        &env.home,
        &["profile", "add", "Bad Id", "--name", "", "--email", "nope"],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("id:") && err.contains("name:") && err.contains("email:"),
        "{err}"
    );
}

#[test]
fn use_then_whoami_then_undo() {
    let env = Env::new();
    env.git(&env.home, &["config", "--global", "user.name", "Global"]);
    env.git(
        &env.home,
        &["config", "--global", "user.email", "global@example.com"],
    );
    add_work_profile(&env);
    let repo = env.repo("code/my app");

    // Pure additions apply without a prompt, even without a terminal.
    let out = env.ok(&repo, &["use", "work"]);
    assert!(out.contains("+ work@acme.example"), "{out}");

    let who = env.json(&repo, &["whoami", "--json"]);
    assert_eq!(who["identity"]["author"]["email"], "work@acme.example");
    assert_eq!(who["profile"]["id"], "work");

    let human = env.ok(&repo, &["whoami"]);
    assert!(
        human.contains("work@acme.example") && human.contains("work"),
        "{human}"
    );

    let history = env.json(&env.home, &["history", "--json"]);
    assert_eq!(history.as_array().unwrap().len(), 1);

    // Undo needs confirmation; without a terminal it is refused...
    let refused = env.run(&repo, &["undo"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    assert_eq!(env.git(&repo, &["config", "user.email"]), "work@acme.example");
    // ...and applied with --yes.
    env.ok(&repo, &["undo", "--yes"]);
    assert_eq!(env.git(&repo, &["config", "user.email"]), "global@example.com");
}

#[test]
fn overwriting_existing_values_requires_confirmation() {
    let env = Env::new();
    add_work_profile(&env);
    let repo = env.repo("code/app");
    env.git(&repo, &["config", "user.email", "mine@example.com"]);

    let out = env.run(&repo, &["use", "work"]);
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("- mine@example.com") && stdout.contains("+ work@acme.example"),
        "{stdout}"
    );
    assert_eq!(env.git(&repo, &["config", "user.email"]), "mine@example.com");

    env.ok(&repo, &["use", "work", "--yes"]);
    assert_eq!(env.git(&repo, &["config", "user.email"]), "work@acme.example");
}

#[test]
fn dry_run_never_writes() {
    let env = Env::new();
    add_work_profile(&env);
    let repo = env.repo("code/app");
    let work = env.home.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let before = std::fs::read_to_string(repo.join(".git/config")).unwrap();

    let out = env.ok(&repo, &["use", "work", "--dry-run"]);
    assert!(out.contains("dry run"), "{out}");
    env.ok(
        &env.home,
        &["rules", "add", work.to_str().unwrap(), "work", "--dry-run"],
    );
    env.ok(
        &env.home,
        &["profile", "edit", "work", "--name", "X", "--dry-run"],
    );

    assert_eq!(std::fs::read_to_string(repo.join(".git/config")).unwrap(), before);
    assert!(!env.home.join(".gitconfig").exists());
    assert_eq!(
        env.json(&env.home, &["profile", "show", "work", "--json"])["profile"]["name"],
        "Work Person"
    );
    assert_eq!(env.json(&env.home, &["history", "--json"]), serde_json::json!([]));
}

#[test]
fn rules_add_list_remove() {
    let env = Env::new();
    add_work_profile(&env);
    let repo = env.repo("work/api");
    env.ok(&env.home, &["rules", "add", "work", "work", "--yes"]);
    assert_eq!(env.git(&repo, &["config", "user.email"]), "work@acme.example");

    let rules = env.json(&env.home, &["rules", "list", "--json"]);
    assert_eq!(rules[0]["profile"], "work");

    env.ok(&env.home, &["rules", "remove", "work", "--yes"]);
    assert_eq!(
        env.json(&env.home, &["rules", "list", "--json"]),
        serde_json::json!([])
    );
}

#[test]
fn whoami_outside_repository_and_doctor_exit_codes() {
    let env = Env::new();
    let plain = env.home.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let who = env.ok(&plain, &["whoami"]);
    assert!(who.contains("not inside a Git repository"), "{who}");

    // No identity at all is an error-level finding: doctor exits 1.
    let repo = env.repo("code/app");
    env.git(&env.home, &["config", "--global", "user.useConfigOnly", "true"]);
    let out = env.run(&repo, &["doctor"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("identity_unset"));

    env.git(&env.home, &["config", "--global", "user.name", "G"]);
    env.git(&env.home, &["config", "--global", "user.email", "g@example.com"]);
    let report = env.json(&repo, &["doctor", "--json"]);
    assert!(report["git_version"]["major"].as_u64().unwrap() >= 2);
}

#[test]
fn errors_are_readable() {
    let env = Env::new();
    let plain = env.home.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    add_work_profile(&env);
    let out = env.run(&plain, &["use", "work"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.starts_with("error: ") && err.contains("not inside a Git repository"),
        "{err}"
    );

    let out = env.run(&plain, &["profile", "show", "missing"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("no profile named 'missing'"));
}
