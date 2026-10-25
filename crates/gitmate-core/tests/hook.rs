//! The optional pre-commit hook: where it is written, which hooks it never
//! touches, and which identity problems make it refuse a commit.

mod common;

use std::path::PathBuf;

use common::Sandbox;
use gitmate_core::Error;
use gitmate_core::plan::AssignOptions;
use gitmate_core::profile::Profile;

fn profile(id: &str, email: &str) -> Profile {
    Profile {
        id: id.into(),
        name: format!("{id} person"),
        email: email.into(),
        label: None,
        ssh_key: None,
        signing_key: None,
        gpg_format: None,
        sign_commits: false,
        remote_patterns: Vec::new(),
    }
}

fn setup() -> (Sandbox, PathBuf) {
    let sb = Sandbox::new();
    let repo = sb.repo("code/app");
    sb.app()
        .add_profile(profile("work", "work@acme.example"))
        .unwrap();
    (sb, repo)
}

#[test]
fn install_writes_an_executable_hook_and_uninstall_removes_it() {
    let (sb, repo) = setup();
    let app = sb.app();
    let hook = repo.join(".git/hooks/pre-commit");

    let status = app.hook_status(&repo).unwrap();
    assert_eq!(status.path, hook);
    assert!(!status.installed && !status.managed && status.hooks_path.is_none());

    let status = app.install_hook(&repo, false).unwrap();
    assert!(status.installed && status.is_active());
    assert_eq!(app.hook_status(&repo).unwrap(), status);
    assert!(sb.read(&hook).contains("exec gitmate hook check"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    // Installing again just rewrites GitMate's own hook.
    app.install_hook(&repo, false).unwrap();

    let status = app.uninstall_hook(&repo).unwrap();
    assert!(!status.installed);
    assert!(!hook.exists());
    // Nothing left to remove is not an error.
    app.uninstall_hook(&repo).unwrap();
}

#[test]
fn a_hook_gitmate_did_not_write_is_kept_unless_install_is_forced() {
    let (sb, repo) = setup();
    let app = sb.app();
    let hook = repo.join(".git/hooks/pre-commit");
    let theirs = "#!/bin/sh\nexec make lint\n";
    std::fs::write(&hook, theirs).unwrap();

    let status = app.hook_status(&repo).unwrap();
    assert!(status.installed && !status.managed);
    assert!(matches!(
        app.install_hook(&repo, false),
        Err(Error::HookConflict { .. })
    ));
    assert!(matches!(
        app.uninstall_hook(&repo),
        Err(Error::HookNotManaged { .. })
    ));
    assert_eq!(sb.read(&hook), theirs);

    app.install_hook(&repo, true).unwrap();
    assert!(app.hook_status(&repo).unwrap().managed);
}

#[test]
fn install_is_refused_when_core_hooks_path_sends_git_elsewhere() {
    let (sb, repo) = setup();
    sb.set_global("core.hooksPath", "~/hooks");
    let app = sb.app();

    let status = app.hook_status(&repo).unwrap();
    assert_eq!(status.hooks_path, Some(sb.home.join("hooks")));
    match app.install_hook(&repo, true) {
        Err(Error::HooksPathOverridden { hooks_path }) => assert_eq!(hooks_path, sb.home.join("hooks")),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!repo.join(".git/hooks/pre-commit").exists());
    assert!(!sb.home.join("hooks").exists());

    // Pointing core.hooksPath at the default directory changes nothing.
    sb.set_global("core.hooksPath", repo.join(".git/hooks").to_str().unwrap());
    assert!(app.install_hook(&repo, false).unwrap().is_active());
}

#[test]
fn linked_worktrees_share_the_main_repository_hook() {
    let (sb, repo) = setup();
    let wt = sb.home.join("code/app-wt");
    sb.git_in(&repo, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let app = sb.app();

    let status = app.install_hook(&wt, false).unwrap();
    assert_eq!(status.path, repo.join(".git/hooks/pre-commit"));
    assert!(app.hook_status(&repo).unwrap().is_active());
}

#[test]
fn commit_check_refuses_only_identity_problems_in_this_repository() {
    let (sb, repo) = setup();
    let app = sb.app();
    app.apply(&app.plan_assign(&repo, "work", AssignOptions::default()).unwrap())
        .unwrap();
    assert!(app.commit_check(&repo).unwrap().is_empty());

    // Another profile whose SSH key has gone is a doctor error, but no reason
    // to refuse a commit here.
    let key = sb.dir(".ssh").join("id_gone");
    std::fs::write(&key, "not a key").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut gone = profile("gone", "gone@example.org");
    gone.ssh_key = Some(key.clone());
    app.add_profile(gone).unwrap();
    std::fs::remove_file(&key).unwrap();
    assert!(app.commit_check(&repo).unwrap().is_empty());

    sb.git_in(&repo, &["config", "user.email", "oops@example.com"]);
    let codes: Vec<_> = app.commit_check(&repo).unwrap().iter().map(|f| f.code).collect();
    assert_eq!(codes, ["profile_mismatch"]);
}

#[test]
fn commit_check_refuses_an_author_a_remote_pattern_says_is_wrong() {
    let (sb, repo) = setup();
    let app = sb.app();
    let mut acme = profile("acme", "me@acme-corp.example");
    acme.remote_patterns = vec!["github.com:acme/".into()];
    app.add_profile(acme).unwrap();
    sb.git_in(&repo, &["remote", "add", "origin", "git@github.com:acme/app.git"]);
    sb.git_in(&repo, &["config", "user.name", "Work Person"]);
    sb.git_in(&repo, &["config", "user.email", "work@acme.example"]);

    let codes: Vec<_> = app.commit_check(&repo).unwrap().iter().map(|f| f.code).collect();
    assert_eq!(codes, ["remote_profile_mismatch"]);
}

#[test]
fn hook_commands_need_a_repository() {
    let sb = Sandbox::new();
    let plain = sb.dir("plain");
    let app = sb.app();
    assert!(matches!(
        app.hook_status(&plain),
        Err(Error::NotARepository { .. })
    ));
    assert!(matches!(
        app.install_hook(&plain, false),
        Err(Error::NotARepository { .. })
    ));
    assert!(matches!(
        app.commit_check(&plain),
        Err(Error::NotARepository { .. })
    ));
}
