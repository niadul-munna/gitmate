//! Profile storage through the facade.

mod common;

use std::path::PathBuf;

use common::Sandbox;
use gitmate_core::Error;
use gitmate_core::profile::{Profile, ProfileInput};

fn profile(id: &str, email: &str) -> Profile {
    Profile {
        id: id.into(),
        name: "Some Person".into(),
        email: email.into(),
        label: Some("Label".into()),
        ssh_key: None,
    }
}

#[test]
fn profiles_round_trip_and_generate_include_files() {
    let sb = Sandbox::new();
    let app = sb.app();
    app.add_profile(profile("work", "work@acme.example")).unwrap();
    app.add_profile(profile("personal", "me@home.example")).unwrap();
    let ids: Vec<String> = app.profiles().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, vec!["work", "personal"]);

    let include = sb.paths.include_file("work");
    assert_eq!(
        sb.git_in(
            &sb.home,
            &["config", "--file", include.to_str().unwrap(), "user.email"]
        ),
        "work@acme.example"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&sb.paths.config_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    // Creating profiles never touches the global Git config.
    assert!(!sb.global_config().exists());
}

#[test]
fn duplicate_ids_and_emails_are_rejected() {
    let sb = Sandbox::new();
    let app = sb.app();
    app.add_profile(profile("work", "work@acme.example")).unwrap();
    assert!(matches!(
        app.add_profile(profile("work", "x@y.z")),
        Err(Error::ProfileExists(_))
    ));
    match app.add_profile(profile("work2", "WORK@acme.example")) {
        Err(Error::InvalidProfile(errors)) => assert_eq!(errors[0].field, "email"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn scenario_7_invalid_ssh_key_path_is_rejected_with_field_error() {
    let sb = Sandbox::new();
    let app = sb.app();
    let mut p = profile("work", "work@acme.example");
    p.ssh_key = Some(PathBuf::from("~/.ssh/does-not-exist"));
    match app.add_profile(p) {
        Err(Error::InvalidProfile(errors)) => {
            assert_eq!(errors[0].field, "ssh_key");
            assert!(
                errors[0].message.contains("does not exist"),
                "{}",
                errors[0].message
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(app.profiles().unwrap().is_empty());
}

#[test]
fn update_and_remove() {
    let sb = Sandbox::new();
    let app = sb.app();
    app.add_profile(profile("work", "work@acme.example")).unwrap();
    app.update_profile(
        "work",
        ProfileInput {
            name: "Renamed".into(),
            email: "renamed@acme.example".into(),
            label: None,
            ssh_key: None,
        },
    )
    .unwrap();
    let p = app.profile("work").unwrap();
    assert_eq!(p.name, "Renamed");
    assert_eq!(p.label, None);

    app.remove_profile("work").unwrap();
    assert!(app.profiles().unwrap().is_empty());
    assert!(!sb.paths.include_file("work").exists());
    assert!(matches!(
        app.remove_profile("work"),
        Err(Error::ProfileNotFound(_))
    ));
}
