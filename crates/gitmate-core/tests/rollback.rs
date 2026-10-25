//! Transactions: concurrency, failed writes, undo and undo conflicts.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::Sandbox;
use gitmate_core::Error;
use gitmate_core::fsutil::WriterLock;
use gitmate_core::journal::OperationStatus;
use gitmate_core::plan::{Action, AssignOptions, ConfigChange, Plan};
use gitmate_core::profile::Profile;

fn setup() -> (Sandbox, PathBuf) {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let repo = sb.repo("code/app");
    sb.app()
        .add_profile(Profile {
            id: "work".into(),
            name: "Work Person".into(),
            email: "work@acme.example".into(),
            label: None,
            ssh_key: None,
            signing_key: None,
            gpg_format: None,
            sign_commits: false,
        })
        .unwrap();
    (sb, repo)
}

#[test]
fn scenario_12_concurrent_modification_between_plan_and_apply() {
    let (sb, repo) = setup();
    let app = sb.app();
    let plan = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    // Another tool writes the same key after the preview was shown.
    sb.git_in(&repo, &["config", "user.email", "raced@example.com"]);

    let err = app.apply(&plan).unwrap_err();
    match err {
        Error::ConcurrentModification(mismatches) => {
            assert_eq!(mismatches[0].key, "user.email");
            assert_eq!(mismatches[0].found, vec!["raced@example.com"]);
        }
        other => panic!("unexpected {other}"),
    }
    assert_eq!(sb.author_email(&repo).as_deref(), Some("raced@example.com"));
    assert!(app.history().unwrap().is_empty(), "nothing was journaled");
}

#[test]
fn scenario_12_concurrent_gitmate_writer_is_refused() {
    let (sb, repo) = setup();
    let app = sb.app().with_lock_timeout(Duration::from_millis(100));
    let plan = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    let _held = WriterLock::acquire(&sb.paths.lock_file(), Duration::from_millis(100)).unwrap();
    assert!(matches!(app.apply(&plan), Err(Error::Locked)));
    assert_eq!(sb.author_email(&repo).as_deref(), Some("global@example.com"));
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Read-only directories do not stop root; skip those tests when running as root.
#[cfg(unix)]
fn can_be_denied(dir: &std::path::Path) -> bool {
    set_mode(dir, 0o555);
    let denied = std::fs::write(dir.join("probe"), b"x").is_err();
    let _ = std::fs::remove_file(dir.join("probe"));
    set_mode(dir, 0o755);
    denied
}

#[cfg(unix)]
#[test]
fn scenario_11_failed_write_changes_nothing_and_is_journaled() {
    let (sb, repo) = setup();
    let git_dir = repo.join(".git");
    if !can_be_denied(&git_dir) {
        eprintln!("running as root; skipping");
        return;
    }
    let app = sb.app();
    let plan = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    let before = sb.read(&git_dir.join("config"));
    set_mode(&git_dir, 0o555); // git cannot create config.lock
    let result = app.apply(&plan);
    set_mode(&git_dir, 0o755);

    match result {
        Err(Error::ApplyFailed { reverted, .. }) => assert!(reverted),
        other => panic!("expected ApplyFailed, got {other:?}"),
    }
    assert_eq!(sb.read(&git_dir.join("config")), before);
    let history = app.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, OperationStatus::Failed);
}

#[cfg(unix)]
#[test]
fn scenario_11_partial_failure_reverts_earlier_changes() {
    let (sb, repo) = setup();
    let other = sb.repo("code/other");
    let other_git = other.join(".git");
    if !can_be_denied(&other_git) {
        eprintln!("running as root; skipping");
        return;
    }
    let app = sb.app();
    // A two-file transaction where the second write fails.
    let plan = Plan::new(
        Action::Assign {
            repository: repo.clone(),
            profile: "work".into(),
            ssh: false,
            sign: false,
        },
        "two files".into(),
        vec![
            ConfigChange {
                file: repo.join(".git/config"),
                key: "user.email".into(),
                before: vec![],
                after: vec!["first@example.com".into()],
            },
            ConfigChange {
                file: other_git.join("config"),
                key: "user.email".into(),
                before: vec![],
                after: vec!["second@example.com".into()],
            },
        ],
    );
    set_mode(&other_git, 0o555);
    let result = app.apply(&plan);
    set_mode(&other_git, 0o755);

    assert!(
        matches!(result, Err(Error::ApplyFailed { reverted: true, .. })),
        "{result:?}"
    );
    assert_eq!(
        sb.author_email(&repo).as_deref(),
        Some("global@example.com"),
        "first write reverted"
    );
}

#[test]
fn apply_refuses_keys_outside_the_managed_set() {
    let (sb, repo) = setup();
    let plan = Plan::new(
        Action::Assign {
            repository: repo.clone(),
            profile: "work".into(),
            ssh: false,
            sign: false,
        },
        "sneaky".into(),
        vec![ConfigChange {
            file: repo.join(".git/config"),
            key: "core.hooksPath".into(),
            before: vec![],
            after: vec!["/tmp/evil".into()],
        }],
    );
    assert!(sb.app().apply(&plan).is_err());
    assert!(
        sb.git
            .run_at(&repo, ["config", "core.hooksPath"])
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn scenario_13_undo_restores_previous_values_exactly() {
    let (sb, repo) = setup();
    sb.git_in(&repo, &["config", "user.email", "previous@example.com"]);
    let app = sb.app();
    let applied = app
        .apply(&app.plan_assign(&repo, "work", AssignOptions::default()).unwrap())
        .unwrap()
        .operation
        .unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("work@acme.example"));
    // Unrelated edits made afterwards must survive the undo.
    sb.git_in(&repo, &["config", "core.autocrlf", "input"]);

    let undo = app.plan_undo(&applied.id).unwrap();
    assert!(undo.conflicts.is_empty(), "{:?}", undo.conflicts);
    let undone = app.apply(&undo).unwrap().operation.unwrap();

    assert_eq!(sb.author_email(&repo).as_deref(), Some("previous@example.com"));
    assert!(
        sb.git
            .run_at(&repo, ["config", "--local", "user.name"])
            .unwrap()
            .stdout
            .is_empty(),
        "added key removed"
    );
    assert_eq!(sb.git_in(&repo, &["config", "core.autocrlf"]), "input");

    let history = app.history().unwrap();
    let original = history.iter().find(|r| r.id == applied.id).unwrap();
    assert_eq!(original.status, OperationStatus::Undone);
    assert_eq!(original.undone_by.as_deref(), Some(undone.id.as_str()));
    assert!(app.plan_undo(&applied.id).is_err(), "cannot undo twice");
}

#[test]
fn scenario_13_undo_of_rule_restores_global_file_byte_for_byte() {
    let (sb, _) = setup();
    let app = sb.app();
    let before = sb.read(&sb.global_config());
    let op = app
        .apply(&app.plan_add_rule(&sb.dir("work"), "work").unwrap())
        .unwrap()
        .operation
        .unwrap();
    assert_ne!(sb.read(&sb.global_config()), before);
    app.apply(&app.plan_undo(&op.id).unwrap()).unwrap();
    assert_eq!(sb.read(&sb.global_config()), before);
}

#[test]
fn scenario_14_undo_refuses_when_values_changed_externally() {
    let (sb, repo) = setup();
    let app = sb.app();
    let op = app
        .apply(&app.plan_assign(&repo, "work", AssignOptions::default()).unwrap())
        .unwrap()
        .operation
        .unwrap();
    sb.git_in(&repo, &["config", "user.email", "edited-by-hand@example.com"]);
    let config_before = sb.read(&repo.join(".git/config"));

    let undo = app.plan_undo(&op.id).unwrap();
    assert!(!undo.conflicts.is_empty());
    match app.apply(&undo) {
        Err(Error::RollbackConflict { conflicts, .. }) => {
            assert_eq!(conflicts[0].key, "user.email");
            assert_eq!(conflicts[0].expected, vec!["work@acme.example"]);
            assert_eq!(conflicts[0].found, vec!["edited-by-hand@example.com"]);
        }
        other => panic!("expected RollbackConflict, got {other:?}"),
    }
    assert_eq!(
        sb.read(&repo.join(".git/config")),
        config_before,
        "nothing restored"
    );
    let original = app
        .history()
        .unwrap()
        .into_iter()
        .find(|r| r.id == op.id)
        .unwrap();
    assert_eq!(original.status, OperationStatus::Applied);
}

#[test]
fn backups_are_private_copies_of_affected_files() {
    let (sb, repo) = setup();
    let app = sb.app();
    let before = sb.read(&repo.join(".git/config"));
    let op = app
        .apply(&app.plan_assign(&repo, "work", AssignOptions::default()).unwrap())
        .unwrap()
        .operation
        .unwrap();
    assert_eq!(op.backups.len(), 1);
    let copy = op.backups[0].copy.as_ref().unwrap();
    assert_eq!(std::fs::read_to_string(copy).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(copy).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn undo_of_unknown_operation_is_an_error() {
    let (sb, _) = setup();
    assert!(matches!(
        sb.app().plan_undo("nope"),
        Err(Error::OperationNotFound(_))
    ));
}

fn assign(sb: &Sandbox, repo: &std::path::Path, profile: &str) -> gitmate_core::journal::OperationRecord {
    let app = sb.app();
    app.apply(&app.plan_assign(repo, profile, AssignOptions::default()).unwrap())
        .unwrap()
        .operation
        .unwrap()
}

#[test]
fn undo_is_refused_while_a_later_operation_changed_the_same_key() {
    let (sb, repo) = setup();
    let app = sb.app();
    let first = assign(&sb, &repo, "work");
    sb.git_in(&repo, &["config", "user.email", "custom@example.com"]);
    let second = assign(&sb, &repo, "work");

    let err = app.plan_undo(&first.id).unwrap_err();
    assert!(matches!(err, Error::NotUndoable { .. }), "{err}");
    assert!(err.to_string().contains(&second.id), "{err}");
    assert_eq!(sb.author_email(&repo).as_deref(), Some("work@acme.example"));

    // Undoing the later operation first gives the hand-set value back.
    app.apply(&app.plan_undo(&second.id).unwrap()).unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("custom@example.com"));
    // The earlier operation's value is gone, so its undo now reports that.
    assert!(!app.plan_undo(&first.id).unwrap().conflicts.is_empty());
}

#[test]
fn undone_later_operations_do_not_block_undo() {
    let (sb, repo) = setup();
    let app = sb.app();
    app.add_profile(Profile {
        id: "oss".into(),
        name: "Oss Person".into(),
        email: "oss@example.org".into(),
        label: None,
        ssh_key: None,
        signing_key: None,
        gpg_format: None,
        sign_commits: false,
    })
    .unwrap();
    let first = assign(&sb, &repo, "work");
    let second = assign(&sb, &repo, "oss");
    app.apply(&app.plan_undo(&second.id).unwrap()).unwrap();

    let undo = app.plan_undo(&first.id).unwrap();
    assert!(undo.conflicts.is_empty(), "{:?}", undo.conflicts);
    app.apply(&undo).unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("global@example.com"));
}

#[test]
fn later_operations_on_other_keys_do_not_block_undo() {
    let (sb, repo) = setup();
    let app = sb.app();
    let first = assign(&sb, &repo, "work");
    app.apply(&app.plan_add_rule(&sb.dir("elsewhere"), "work").unwrap())
        .unwrap();
    let undo = app.plan_undo(&first.id).unwrap();
    assert!(undo.conflicts.is_empty(), "{:?}", undo.conflicts);
}
