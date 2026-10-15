//! Assigning a profile to one repository (`gitmate use`).

mod common;

use std::path::PathBuf;

use common::Sandbox;
use gitmate_core::Error;
use gitmate_core::plan::{AssignOptions, ChangeKind};
use gitmate_core::profile::Profile;

fn profile(id: &str, name: &str, email: &str) -> Profile {
    Profile {
        id: id.into(),
        name: name.into(),
        email: email.into(),
        label: None,
        ssh_key: None,
    }
}

fn setup() -> (Sandbox, PathBuf) {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let repo = sb.repo("code/app");
    sb.app()
        .add_profile(profile("work", "Work Person", "work@acme.example"))
        .unwrap();
    (sb, repo)
}

#[test]
fn assigning_a_profile_writes_only_local_identity_keys() {
    let (sb, repo) = setup();
    sb.git_in(&repo, &["config", "core.autocrlf", "input"]);
    let global_before = sb.read(&sb.global_config());
    let app = sb.app();

    let plan = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    assert_eq!(plan.changes.len(), 2);
    for change in &plan.changes {
        assert_eq!(change.file, repo.join(".git/config"));
        assert_eq!(change.kind(), ChangeKind::Add);
    }
    assert!(
        !plan.requires_confirmation(),
        "pure additions to repo config apply directly"
    );

    let outcome = app.apply(&plan).unwrap();
    let record = outcome.operation.expect("an operation is recorded");
    assert_eq!(sb.author_email(&repo).as_deref(), Some("work@acme.example"));
    assert_eq!(outcome.effective.unwrap().email, "work@acme.example");
    // Unrelated settings and the global file are untouched.
    assert_eq!(sb.git_in(&repo, &["config", "--local", "core.autocrlf"]), "input");
    assert_eq!(sb.read(&sb.global_config()), global_before);
    assert_eq!(app.history().unwrap().last().unwrap().id, record.id);
}

#[test]
fn scenario_5_existing_local_identity_is_shown_and_needs_confirmation() {
    let (sb, repo) = setup();
    sb.git_in(&repo, &["config", "user.email", "old-local@example.com"]);
    let plan = sb
        .app()
        .plan_assign(&repo, "work", AssignOptions::default())
        .unwrap();
    let email = plan.changes.iter().find(|c| c.key == "user.email").unwrap();
    assert_eq!(email.kind(), ChangeKind::Replace);
    assert_eq!(email.before, vec!["old-local@example.com"]);
    assert_eq!(email.after, vec!["work@acme.example"]);
    assert!(plan.requires_confirmation());
}

#[test]
fn scenario_17_repeated_assignment_is_idempotent() {
    let (sb, repo) = setup();
    let app = sb.app();
    let first = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    app.apply(&first).unwrap();
    let history_len = app.history().unwrap().len();
    let config_before = sb.read(&repo.join(".git/config"));

    let second = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    assert!(second.is_noop());
    let outcome = app.apply(&second).unwrap();
    assert!(outcome.operation.is_none());
    assert_eq!(
        app.history().unwrap().len(),
        history_len,
        "no journal entry for a no-op"
    );
    assert_eq!(sb.read(&repo.join(".git/config")), config_before);
}

#[test]
fn scenario_15_assigning_outside_a_repository_fails_clearly() {
    let (sb, _) = setup();
    let plain = sb.dir("plain dir");
    let err = sb
        .app()
        .plan_assign(&plain, "work", AssignOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::NotARepository { .. }), "{err}");
}

#[test]
fn unknown_profile_is_rejected() {
    let (sb, repo) = setup();
    let err = sb
        .app()
        .plan_assign(&repo, "nope", AssignOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::ProfileNotFound(_)));
}

#[test]
fn scenario_16_environment_override_is_warned_and_reported_after_apply() {
    let (sb, repo) = setup();
    let git = sb.git.clone().env("GIT_AUTHOR_EMAIL", "env@example.com");
    let app = gitmate_core::GitMate::new(git, sb.paths.clone());
    let plan = app.plan_assign(&repo, "work", AssignOptions::default()).unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.code == "env_override"),
        "{:?}",
        plan.warnings
    );
    let outcome = app.apply(&plan).unwrap();
    assert_eq!(outcome.effective.unwrap().email, "env@example.com");
    assert!(
        outcome.notes.iter().any(|n| n.contains("GIT_AUTHOR_EMAIL")),
        "{:?}",
        outcome.notes
    );
}

#[test]
fn author_email_key_is_warned_because_it_beats_user_email() {
    let (sb, repo) = setup();
    sb.set_global("author.email", "author@example.com");
    let plan = sb
        .app()
        .plan_assign(&repo, "work", AssignOptions::default())
        .unwrap();
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.code == "author_committer_override"),
        "{:?}",
        plan.warnings
    );
}

#[cfg(unix)]
fn make_key(sb: &Sandbox, rel: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = sb.home.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"placeholder, never read").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn ssh_option_pins_the_profile_key_for_the_repository() {
    let (sb, repo) = setup();
    make_key(&sb, ".ssh/id work key");
    let mut p = profile("oss", "OSS Person", "oss@example.org");
    p.ssh_key = Some(PathBuf::from("~/.ssh/id work key"));
    let app = sb.app();
    app.add_profile(p).unwrap();
    sb.git_in(&repo, &["remote", "add", "origin", "git@github.com:acme/app.git"]);

    let plan = app
        .plan_assign(&repo, "oss", AssignOptions { ssh: true })
        .unwrap();
    let ssh = plan.changes.iter().find(|c| c.key == "core.sshCommand").unwrap();
    let expected = format!(
        "ssh -i '{}' -o IdentitiesOnly=yes",
        sb.home.join(".ssh/id work key").display()
    );
    assert_eq!(ssh.after, vec![expected.clone()]);
    app.apply(&plan).unwrap();
    assert_eq!(
        sb.git_in(&repo, &["config", "--local", "core.sshCommand"]),
        expected
    );
}

#[cfg(unix)]
#[test]
fn scenario_8_https_remote_with_ssh_profile_is_warned() {
    let (sb, repo) = setup();
    make_key(&sb, ".ssh/id_oss");
    let mut p = profile("oss", "OSS Person", "oss@example.org");
    p.ssh_key = Some(PathBuf::from("~/.ssh/id_oss"));
    let app = sb.app();
    app.add_profile(p).unwrap();
    sb.git_in(
        &repo,
        &["remote", "add", "origin", "https://github.com/acme/app.git"],
    );

    let plan = app
        .plan_assign(&repo, "oss", AssignOptions { ssh: true })
        .unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.code == "https_remote"),
        "{:?}",
        plan.warnings
    );

    let report = app.inspect(&repo).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.code == "https_remote_ssh_profile")
            || report.profile.is_none()
    );
}

#[cfg(unix)]
#[test]
fn scenario_7_ssh_option_with_missing_key_is_a_conflict() {
    let (sb, repo) = setup();
    let key = make_key(&sb, ".ssh/id_gone");
    let mut p = profile("gone", "Gone Person", "gone@example.org");
    p.ssh_key = Some(key.clone());
    let app = sb.app();
    app.add_profile(p).unwrap();
    std::fs::remove_file(&key).unwrap();

    let plan = app
        .plan_assign(&repo, "gone", AssignOptions { ssh: true })
        .unwrap();
    assert!(
        plan.conflicts.iter().any(|c| c.code == "ssh_key_unusable"),
        "{:?}",
        plan.conflicts
    );
    let err = app.apply(&plan).unwrap_err();
    assert!(matches!(err, Error::PlanHasConflicts(_)));
    assert!(
        sb.git
            .run_at(&repo, ["config", "--local", "user.email"])
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn ssh_option_without_a_profile_key_is_a_conflict() {
    let (sb, repo) = setup();
    let plan = sb
        .app()
        .plan_assign(&repo, "work", AssignOptions { ssh: true })
        .unwrap();
    assert!(plan.conflicts.iter().any(|c| c.code == "no_ssh_key"));
}

#[test]
fn scenario_9_assignment_in_a_linked_worktree_targets_shared_config() {
    let (sb, repo) = setup();
    let wt = sb.home.join("elsewhere/wt");
    sb.git_in(
        &repo,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    let app = sb.app();
    let plan = app.plan_assign(&wt, "work", AssignOptions::default()).unwrap();
    assert!(plan.changes.iter().all(|c| c.file == repo.join(".git/config")));
    assert!(plan.warnings.iter().any(|w| w.code == "shared_worktree_config"));
    app.apply(&plan).unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("work@acme.example"));
    assert_eq!(sb.author_email(&wt).as_deref(), Some("work@acme.example"));
}

#[test]
fn inspect_reports_the_matching_profile_and_assignment() {
    let (sb, repo) = setup();
    let app = sb.app();
    app.apply(&app.plan_assign(&repo, "work", AssignOptions::default()).unwrap())
        .unwrap();
    let report = app.inspect(&repo).unwrap();
    assert_eq!(report.profile.as_ref().map(|m| m.id.as_str()), Some("work"));
    assert_eq!(report.expected.as_ref().map(|e| e.profile.as_str()), Some("work"));
    assert!(
        report.findings.iter().all(|f| f.code != "profile_mismatch"),
        "{:?}",
        report.findings
    );

    // Someone changes the email by hand: GitMate reports the mismatch.
    sb.git_in(&repo, &["config", "user.email", "oops@example.com"]);
    let report = app.inspect(&repo).unwrap();
    assert!(report.profile.is_none());
    assert!(
        report.findings.iter().any(|f| f.code == "profile_mismatch"),
        "{:?}",
        report.findings
    );
}

#[cfg(unix)]
#[test]
fn https_remote_warning_names_the_key_relative_to_home() {
    let (sb, _) = setup();
    let key = make_key(&sb, ".ssh/id_work");
    let mut p = profile("keyed", "Keyed Person", "keyed@acme.example");
    p.ssh_key = Some(key);
    let app = sb.app();
    app.add_profile(p).unwrap();
    app.apply(&app.plan_add_rule(&sb.dir("keyed"), "keyed").unwrap())
        .unwrap();
    let repo = sb.repo("keyed/app");
    sb.git_in(
        &repo,
        &["remote", "add", "origin", "https://github.com/acme/app.git"],
    );

    let report = app.inspect(&repo).unwrap();
    let finding = report
        .findings
        .iter()
        .find(|f| f.code == "https_remote_ssh_profile")
        .expect("warning");
    assert!(finding.message.contains("~/.ssh/id_work"), "{}", finding.message);
}
