//! Repository discovery and effective identity inspection against real Git.

mod common;

use common::Sandbox;
use gitmate_core::git::{Origin, Scope};
use gitmate_core::identity::{self, Source};
use gitmate_core::repo;

#[test]
fn global_identity_is_reported_with_its_origin() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let repo_dir = sb.repo("code/app");

    let report = identity::inspect(&sb.git, &repo_dir).unwrap();
    let author = report.author.as_ref().unwrap();
    assert_eq!(author.name, "Global Person");
    assert_eq!(author.email, "global@example.com");
    match &report.email_source {
        Some(Source::Config { key, scope, origin }) => {
            assert_eq!(key, "user.email");
            assert_eq!(*scope, Scope::Global);
            assert_eq!(*origin, Origin::File(sb.global_config()));
        }
        other => panic!("unexpected source {other:?}"),
    }
}

#[test]
fn scenario_2_local_identity_overrides_global() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let repo_dir = sb.repo("code/app");
    sb.git_in(&repo_dir, &["config", "user.email", "local@example.com"]);

    let report = identity::inspect(&sb.git, &repo_dir.join(".")).unwrap();
    assert_eq!(report.author.as_ref().unwrap().email, "local@example.com");
    assert_eq!(report.author.as_ref().unwrap().name, "Global Person");

    let email = report.key("user.email").unwrap();
    assert_eq!(email.values.len(), 2, "both values are listed in read order");
    assert_eq!(email.values[0].scope, Scope::Global);
    let effective = email.effective().unwrap();
    assert_eq!(effective.scope, Scope::Local);
    // Git prints the local origin relative to the repo; it must be resolved.
    assert_eq!(effective.origin, Origin::File(repo_dir.join(".git/config")));
}

#[test]
fn scenario_6_unset_identity_is_reported_not_guessed() {
    let sb = Sandbox::new();
    let repo_dir = sb.repo("code/app");
    let report = identity::inspect(&sb.git, &repo_dir).unwrap();
    // Depending on the host's FQDN, Git either fails or guesses from
    // user@hostname. A guess must never count as configured.
    assert!(!report.is_configured());
    match &report.author {
        None => assert!(
            report.problem.as_deref().unwrap().contains("identity"),
            "{:?}",
            report.problem
        ),
        Some(_) => assert_eq!(report.email_source, Some(Source::AutoDetected)),
    }

    // With user.useConfigOnly Git never guesses.
    sb.set_global("user.useConfigOnly", "true");
    let report = identity::inspect(&sb.git, &repo_dir).unwrap();
    assert!(report.author.is_none());
    assert!(
        report.problem.as_deref().unwrap().contains("identity"),
        "{:?}",
        report.problem
    );
    assert!(report.email_source.is_none());
}

#[test]
fn scenario_15_outside_a_repository() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let plain = sb.dir("not a repo/sub");

    assert!(repo::discover(&sb.git, &plain).unwrap().is_none());
    let report = identity::inspect(&sb.git, &plain).unwrap();
    assert_eq!(report.author.unwrap().email, "global@example.com");
}

#[test]
fn scenario_16_environment_variables_override_config() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let repo_dir = sb.repo("code/app");
    let git = sb.git.clone().env("GIT_AUTHOR_EMAIL", "env@example.com").env(
        "GIT_CONFIG_PARAMETERS",
        "'http.extraheader'='Authorization: secret'",
    );

    let report = identity::inspect(&git, &repo_dir).unwrap();
    assert_eq!(report.author.as_ref().unwrap().email, "env@example.com");
    assert_eq!(
        report.email_source,
        Some(Source::Environment {
            var: "GIT_AUTHOR_EMAIL".into()
        })
    );
    assert!(
        report
            .env
            .iter()
            .any(|v| v.name == "GIT_AUTHOR_EMAIL" && v.value.as_deref() == Some("env@example.com"))
    );
    // GIT_CONFIG_PARAMETERS is reported by name only; its value can hold secrets.
    let params = report
        .env
        .iter()
        .find(|v| v.name == "GIT_CONFIG_PARAMETERS")
        .unwrap();
    assert_eq!(params.value, None);
    // Committer is not affected by GIT_AUTHOR_EMAIL.
    assert_eq!(report.committer.as_ref().unwrap().email, "global@example.com");
}

#[test]
fn author_email_key_beats_user_email() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    sb.set_global("author.email", "author@example.com");
    let repo_dir = sb.repo("code/app");
    let report = identity::inspect(&sb.git, &repo_dir).unwrap();
    assert_eq!(report.author.as_ref().unwrap().email, "author@example.com");
    assert!(matches!(&report.email_source, Some(Source::Config { key, .. }) if key == "author.email"));
}

#[test]
fn command_scope_config_is_attributed() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let repo_dir = sb.repo("code/app");
    let git = sb
        .git
        .clone()
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "user.email")
        .env("GIT_CONFIG_VALUE_0", "cmd@example.com");
    let report = identity::inspect(&git, &repo_dir).unwrap();
    assert_eq!(report.author.as_ref().unwrap().email, "cmd@example.com");
    assert!(matches!(
        &report.email_source,
        Some(Source::Config {
            scope: Scope::Command,
            ..
        })
    ));
}

#[test]
fn scenario_10_paths_with_spaces_and_unicode() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Zoë Ó Briain");
    sb.set_global("user.email", "zoe@example.com");
    let repo_dir = sb.repo("projets clients/équipe ünïcode/app 1");
    let info = repo::discover(&sb.git, &repo_dir.join(".")).unwrap().unwrap();
    assert_eq!(info.work_tree.as_deref(), Some(repo_dir.as_path()));
    let report = identity::inspect(&sb.git, &repo_dir).unwrap();
    assert_eq!(report.author.unwrap().name, "Zoë Ó Briain");
}

#[test]
fn scenario_4_nested_repository_is_discovered_innermost_first() {
    let sb = Sandbox::new();
    let outer = sb.repo("code/outer");
    let inner = sb.repo("code/outer/vendor/inner");
    let deep = inner.join("src/deep");
    std::fs::create_dir_all(&deep).unwrap();

    let found = repo::discover(&sb.git, &deep).unwrap().unwrap();
    assert_eq!(found.work_tree.as_deref(), Some(inner.as_path()));
    let found = repo::discover(&sb.git, &outer.join("vendor")).unwrap().unwrap();
    assert_eq!(found.work_tree.as_deref(), Some(outer.as_path()));
}

#[test]
fn scenario_9_linked_worktree_shares_local_config() {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let main = sb.repo("code/main repo");
    let wt = sb.home.join("elsewhere/wt one");
    sb.git_in(
        &main,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    sb.git_in(&main, &["config", "user.email", "shared@example.com"]);

    let info = repo::discover(&sb.git, &wt).unwrap().unwrap();
    assert!(info.is_linked_worktree);
    assert_eq!(info.work_tree.as_deref(), Some(wt.as_path()));
    assert_eq!(info.common_dir, main.join(".git"));
    assert_eq!(info.local_config(), main.join(".git/config"));

    let report = identity::inspect(&sb.git, &wt).unwrap();
    assert_eq!(report.author.unwrap().email, "shared@example.com");
}

#[test]
fn bare_repository_is_recognised() {
    let sb = Sandbox::new();
    let bare = sb.dir("srv/app.git");
    sb.git_in(&bare, &["init", "-q", "--bare"]);
    let info = repo::discover(&sb.git, &bare).unwrap().unwrap();
    assert!(info.is_bare);
    assert!(info.work_tree.is_none());
    assert_eq!(info.local_config(), bare.join("config"));
}

#[test]
fn inherited_git_dir_does_not_redirect_discovery() {
    let sb = Sandbox::new();
    let a = sb.repo("code/a");
    let b = sb.repo("code/b");
    let git = sb.git.clone().env("GIT_DIR", b.join(".git"));
    let info = repo::discover(&git, &a).unwrap().unwrap();
    assert_eq!(info.work_tree.as_deref(), Some(a.as_path()));
    let report = identity::inspect(&git, &a).unwrap();
    assert!(report.env.iter().any(|v| v.name == "GIT_DIR"));
}

#[test]
fn directory_validation_rejects_bad_input() {
    let sb = Sandbox::new();
    assert!(repo::validate_directory(std::path::Path::new("relative/path")).is_err());
    assert!(repo::validate_directory(&sb.home.join("missing")).is_err());
    let file = sb.home.join("file.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(repo::validate_directory(&file).is_err());
    assert!(repo::validate_directory(&sb.home).is_ok());
}

#[cfg(unix)]
#[test]
fn directories_with_control_characters_are_rejected() {
    let sb = Sandbox::new();
    let dir = sb.repo("line\nbreak");
    assert!(repo::validate_directory(&dir).is_err());
    assert!(repo::discover(&sb.git, &dir).is_err());
    assert!(sb.app().inspect(&dir).is_err());
}

#[test]
fn included_local_values_are_attributed_to_the_included_file() {
    let sb = Sandbox::new();
    let repo_dir = sb.repo("code/app");
    std::fs::write(
        repo_dir.join(".git/extra"),
        "[user]\n\temail = extra@example.com\n",
    )
    .unwrap();
    sb.git_in(&repo_dir, &["config", "include.path", "extra"]);
    let sub = sb.dir("code/app/src/deep");
    for dir in [&repo_dir, &sub] {
        let report = identity::inspect(&sb.git, dir).unwrap();
        let entry = report.key("user.email").unwrap().effective().unwrap();
        assert_eq!(
            entry.origin,
            Origin::File(repo_dir.join(".git/extra")),
            "from {}",
            dir.display()
        );
    }
}

#[test]
fn credential_helper_secrets_are_not_reported() {
    let sb = Sandbox::new();
    let repo_dir = sb.repo("code/app");
    sb.git_in(
        &repo_dir,
        &[
            "config",
            "credential.helper",
            "!f() { echo password=ghp_SECRETTOKEN; }; f",
        ],
    );
    sb.git_in(
        &repo_dir,
        &[
            "config",
            "credential.https://bob:hunter2@example.com.helper",
            "store --file /home/bob/creds",
        ],
    );
    sb.git_in(
        &repo_dir,
        &["remote", "add", "origin", "ssh://git:hunter2@example.com/r.git"],
    );
    let report = sb.app().inspect(&repo_dir).unwrap();
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("SECRETTOKEN"), "{json}");
    assert!(!json.contains("hunter2"), "{json}");
    assert!(!report.credentials.is_empty(), "helpers are still listed");
}
