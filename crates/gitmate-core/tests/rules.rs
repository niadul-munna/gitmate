//! Directory rules implemented as `includeIf "gitdir:..."` sections.

mod common;

use std::path::Path;

use common::Sandbox;
use gitmate_core::Error;
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

fn setup() -> Sandbox {
    let sb = Sandbox::new();
    sb.set_global("user.name", "Global Person");
    sb.set_global("user.email", "global@example.com");
    let app = sb.app();
    app.add_profile(profile("work", "work@acme.example")).unwrap();
    app.add_profile(profile("personal", "me@home.example")).unwrap();
    sb
}

#[test]
fn scenario_3_rule_applies_inside_directory_only() {
    let sb = setup();
    let app = sb.app();
    let work_dir = sb.dir("work area");
    let inside = sb.repo("work area/client app");
    let outside = sb.repo("misc/tool");

    let plan = app.plan_add_rule(&work_dir, "work").unwrap();
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].file, sb.global_config());
    assert!(
        plan.requires_confirmation(),
        "global config changes always need confirmation"
    );
    app.apply(&plan).unwrap();

    assert_eq!(sb.author_email(&inside).as_deref(), Some("work@acme.example"));
    assert_eq!(sb.author_email(&outside).as_deref(), Some("global@example.com"));
    // Pre-existing global settings survive.
    assert_eq!(
        sb.git_in(&sb.home, &["config", "--global", "user.email"]),
        "global@example.com"
    );

    let rules = app.rules().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].profile.as_deref(), Some("work"));
    assert_eq!(rules[0].directory.as_deref(), Some(work_dir.as_path()));

    let report = app.inspect(&inside).unwrap();
    assert_eq!(report.profile.as_ref().unwrap().id, "work");
    assert_eq!(report.rule.as_ref().unwrap().profile.as_deref(), Some("work"));
}

#[test]
fn scenario_1_two_independent_profiles() {
    let sb = setup();
    let app = sb.app();
    let work_repo = sb.repo("work/api");
    let personal_repo = sb.repo("personal/blog");
    app.apply(&app.plan_add_rule(&sb.home.join("work"), "work").unwrap())
        .unwrap();
    app.apply(&app.plan_add_rule(&sb.home.join("personal"), "personal").unwrap())
        .unwrap();

    assert_eq!(sb.author_email(&work_repo).as_deref(), Some("work@acme.example"));
    assert_eq!(
        sb.author_email(&personal_repo).as_deref(),
        Some("me@home.example")
    );
    assert_eq!(app.inspect(&work_repo).unwrap().profile.unwrap().id, "work");
    assert_eq!(
        app.inspect(&personal_repo).unwrap().profile.unwrap().id,
        "personal"
    );
}

#[test]
fn scenario_4_nested_rules_most_specific_last_wins() {
    let sb = setup();
    let app = sb.app();
    let general = sb.repo("work/internal");
    let nested = sb.repo("work/oss/lib");
    app.apply(&app.plan_add_rule(&sb.home.join("work"), "work").unwrap())
        .unwrap();

    let plan = app.plan_add_rule(&sb.home.join("work/oss"), "personal").unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.code == "overrides_broader_rule"),
        "{:?}",
        plan.warnings
    );
    app.apply(&plan).unwrap();

    assert_eq!(sb.author_email(&general).as_deref(), Some("work@acme.example"));
    assert_eq!(sb.author_email(&nested).as_deref(), Some("me@home.example"));
}

#[test]
fn broader_rule_added_after_a_specific_one_is_warned() {
    let sb = setup();
    let app = sb.app();
    sb.dir("work/oss");
    app.apply(&app.plan_add_rule(&sb.home.join("work/oss"), "personal").unwrap())
        .unwrap();
    let plan = app.plan_add_rule(&sb.home.join("work"), "work").unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.code == "shadows_specific_rule"),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn scenario_5_existing_unmanaged_include_for_same_directory_is_a_conflict() {
    let sb = setup();
    let app = sb.app();
    let dir = sb.dir("work");
    let mine = sb.home.join("my-work.inc");
    std::fs::write(&mine, "[user]\n\temail = handmade@example.com\n").unwrap();
    let key = format!("includeIf.gitdir:{}/.path", dir.display());
    sb.set_global(&key, mine.to_str().unwrap());
    let before = sb.read(&sb.global_config());

    let plan = app.plan_add_rule(&dir, "work").unwrap();
    assert!(
        plan.conflicts.iter().any(|c| c.code == "existing_include"),
        "{:?}",
        plan.conflicts
    );
    assert!(matches!(app.apply(&plan), Err(Error::PlanHasConflicts(_))));
    assert_eq!(sb.read(&sb.global_config()), before, "nothing was written");

    let rules = app.rules().unwrap();
    assert_eq!(rules.len(), 1);
    assert!(
        rules[0].profile.is_none(),
        "handmade rules are listed but not managed"
    );
}

#[test]
fn scenario_17_adding_the_same_rule_twice_is_a_no_op() {
    let sb = setup();
    let app = sb.app();
    let dir = sb.dir("work");
    app.apply(&app.plan_add_rule(&dir, "work").unwrap()).unwrap();
    let again = app.plan_add_rule(&dir, "work").unwrap();
    assert!(again.is_noop());
    assert!(again.conflicts.is_empty());
}

#[test]
fn reassigning_a_directory_to_another_profile_is_a_conflict() {
    let sb = setup();
    let app = sb.app();
    let dir = sb.dir("work");
    app.apply(&app.plan_add_rule(&dir, "work").unwrap()).unwrap();
    let plan = app.plan_add_rule(&dir, "personal").unwrap();
    assert!(
        plan.conflicts.iter().any(|c| c.code == "existing_rule"),
        "{:?}",
        plan.conflicts
    );
}

#[cfg(unix)]
#[test]
fn rules_are_written_on_the_resolved_directory() {
    let sb = setup();
    let app = sb.app();
    let real = sb.dir("real work");
    let link = sb.home.join("work-link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let repo_via_real = sb.repo("real work/app");

    let plan = app.plan_add_rule(&link, "work").unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.code == "symlink_resolved"),
        "{:?}",
        plan.warnings
    );
    app.apply(&plan).unwrap();
    assert_eq!(
        sb.author_email(&repo_via_real).as_deref(),
        Some("work@acme.example")
    );
    assert_eq!(
        sb.author_email(&link.join("app")).as_deref(),
        Some("work@acme.example")
    );
}

#[test]
fn scenario_9_rule_matches_worktree_of_repo_inside_directory() {
    let sb = setup();
    let app = sb.app();
    let main = sb.repo("work/main");
    let wt = sb.home.join("scratch/wt");
    sb.git_in(
        &main,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    app.apply(&app.plan_add_rule(&sb.home.join("work"), "work").unwrap())
        .unwrap();
    // Documented Git behaviour: the worktree's gitdir lives inside the main repo.
    assert_eq!(sb.author_email(&wt).as_deref(), Some("work@acme.example"));
}

#[test]
fn removing_a_rule_restores_previous_behaviour() {
    let sb = setup();
    let app = sb.app();
    let dir = sb.dir("work");
    let repo = sb.repo("work/app");
    let before = sb.read(&sb.global_config());
    app.apply(&app.plan_add_rule(&dir, "work").unwrap()).unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("work@acme.example"));

    let plan = app.plan_remove_rule(&dir).unwrap();
    assert_eq!(plan.changes.len(), 1);
    assert!(plan.changes[0].after.is_empty());
    app.apply(&plan).unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("global@example.com"));
    assert_eq!(sb.read(&sb.global_config()), before);
    assert!(matches!(app.plan_remove_rule(&dir), Err(Error::RuleNotFound(_))));
}

#[test]
fn profile_in_use_by_a_rule_cannot_be_removed() {
    let sb = setup();
    let app = sb.app();
    app.apply(&app.plan_add_rule(&sb.dir("work"), "work").unwrap())
        .unwrap();
    assert!(matches!(
        app.remove_profile("work"),
        Err(Error::ProfileInUse { .. })
    ));
}

#[test]
fn editing_a_profile_updates_rule_matched_repositories() {
    let sb = setup();
    let app = sb.app();
    let repo = sb.repo("work/app");
    app.apply(&app.plan_add_rule(&sb.home.join("work"), "work").unwrap())
        .unwrap();
    let mut input = gitmate_core::profile::ProfileInput {
        name: "Work Person".into(),
        email: "new-work@acme.example".into(),
        ..Default::default()
    };
    input.label = Some("Acme".into());
    app.update_profile("work", input).unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("new-work@acme.example"));
}

#[test]
fn glob_characters_in_rule_directories_are_rejected() {
    let sb = setup();
    let err = sb.app().plan_add_rule(&sb.dir("weird[dir]"), "work").unwrap_err();
    assert!(matches!(err, Error::InvalidPath { .. }), "{err}");
}

#[test]
fn global_identity_after_a_rule_is_detected() {
    let sb = Sandbox::new();
    let app = sb.app();
    app.add_profile(profile("work", "work@acme.example")).unwrap();
    let repo = sb.repo("work/app");
    app.apply(&app.plan_add_rule(&sb.home.join("work"), "work").unwrap())
        .unwrap();
    // A user.email written *after* the includeIf section overrides it.
    sb.set_global("user.email", "late@example.com");
    assert_eq!(sb.author_email(&repo).as_deref(), Some("late@example.com"));
    let report = app.inspect(&repo).unwrap();
    assert!(
        report.findings.iter().any(|f| f.code == "profile_mismatch"),
        "{:?}",
        report.findings
    );
    let doctor = app.doctor(None).unwrap();
    assert!(
        doctor
            .findings
            .iter()
            .any(|f| f.code == "global_identity_after_rule"),
        "{:?}",
        doctor.findings
    );
}

#[cfg(unix)]
#[test]
fn glob_characters_reached_through_a_symlink_are_rejected() {
    let sb = setup();
    let real = sb.dir("data/[ab]proj");
    let link = sb.home.join("work");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = sb.app().plan_add_rule(&link, "work").unwrap_err();
    assert!(matches!(err, Error::InvalidPath { .. }), "{err}");
}

#[test]
fn unmanaged_include_spelled_differently_is_still_a_conflict() {
    for spelling in ["gitdir:~/work/", "gitdir:~/work/**"] {
        let sb = setup();
        let dir = sb.dir("work");
        let mine = sb.home.join("my-work.inc");
        std::fs::write(&mine, "[user]\n\temail = handmade@example.com\n").unwrap();
        sb.set_global(&format!("includeIf.{spelling}.path"), mine.to_str().unwrap());

        let plan = sb.app().plan_add_rule(&dir, "work").unwrap();
        assert!(
            plan.conflicts.iter().any(|c| c.code == "existing_include"),
            "{spelling}: {:?}",
            plan.conflicts
        );
    }
}

#[cfg(unix)]
#[test]
fn unmanaged_include_through_a_symlink_is_a_conflict() {
    let sb = setup();
    let real = sb.dir("real/work");
    let link = sb.home.join("work-link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mine = sb.home.join("my-work.inc");
    std::fs::write(&mine, "[user]\n\temail = handmade@example.com\n").unwrap();
    sb.set_global(
        &format!("includeIf.gitdir:{}/.path", link.display()),
        mine.to_str().unwrap(),
    );

    let plan = sb.app().plan_add_rule(&real, "work").unwrap();
    assert!(
        plan.conflicts.iter().any(|c| c.code == "existing_include"),
        "{:?}",
        plan.conflicts
    );
}

#[test]
fn overlapping_unmanaged_rules_are_warned() {
    let sb = setup();
    let mine = sb.home.join("my-work.inc");
    std::fs::write(&mine, "[user]\n\temail = handmade@example.com\n").unwrap();
    sb.set_global("includeIf.gitdir:~/work/.path", mine.to_str().unwrap());

    let plan = sb
        .app()
        .plan_add_rule(&sb.dir("work/client"), "personal")
        .unwrap();
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    assert!(
        plan.warnings.iter().any(|w| w.code == "overrides_broader_rule"),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn removing_a_rule_leaves_other_values_of_the_same_key_in_place() {
    let sb = setup();
    let app = sb.app();
    let dir = sb.dir("work");
    let repo = sb.repo("work/app");
    let team = sb.home.join("team.inc");
    std::fs::write(&team, "[user]\n\temail = team@example.com\n").unwrap();
    let condition = gitmate_core::rules::condition_for(&gitmate_core::paths::canonicalize(&dir).unwrap());
    let managed = sb.paths.include_file("work");
    // A backslash starts an escape in a Git config value.
    let slashed = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let head = format!(
        "[includeIf \"{condition}\"]\n\tpath = {}\n[user]\n\tname = Global Person\n\temail = global@example.com\n",
        slashed(&team)
    );
    std::fs::write(
        sb.global_config(),
        format!(
            "{head}[includeIf \"{condition}\"]\n\tpath = {}\n",
            slashed(&managed)
        ),
    )
    .unwrap();
    assert_eq!(sb.author_email(&repo).as_deref(), Some("work@acme.example"));

    app.apply(&app.plan_remove_rule(&dir).unwrap()).unwrap();

    assert_eq!(
        sb.read(&sb.global_config()),
        head,
        "only the managed entry is removed"
    );
    assert_eq!(sb.author_email(&repo).as_deref(), Some("global@example.com"));
}
