//! Profiles: a named commit identity with an optional SSH key path.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, FieldError, Result};
use crate::fsutil;

pub const MAX_ID_LEN: usize = 32;
const MAX_NAME_LEN: usize = 128;
const MAX_EMAIL_LEN: usize = 254;
const MAX_LABEL_LEN: usize = 120;

/// A Git identity profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Short identifier: lowercase letters, digits, `-` and `_`.
    pub id: String,
    /// Value for `user.name`.
    pub name: String,
    /// Value for `user.email`.
    pub email: String,
    /// Optional human description, e.g. "Acme (work GitHub)".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional path to the SSH *private* key used for this identity. Only the
    /// path is stored; the key file is never read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<PathBuf>,
}

/// Editable fields of a profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileInput {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub ssh_key: Option<PathBuf>,
}

impl Profile {
    pub fn from_input(id: &str, input: ProfileInput) -> Profile {
        Profile {
            id: id.trim().to_string(),
            name: input.name.trim().to_string(),
            email: input.email.trim().to_string(),
            label: input
                .label
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty()),
            ssh_key: input.ssh_key.filter(|p| !p.as_os_str().is_empty()),
        }
    }

    /// Whether `email` belongs to this profile (case-insensitive, as mail
    /// domains are and as Git hosts treat them).
    pub fn matches_email(&self, email: &str) -> bool {
        self.email.eq_ignore_ascii_case(email.trim())
    }
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

pub fn validate_id(id: &str) -> Option<FieldError> {
    let message = if id.is_empty() {
        "is required".to_string()
    } else if id.len() > MAX_ID_LEN {
        format!("must be at most {MAX_ID_LEN} characters")
    } else if !id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        "must start with a lowercase letter or digit".to_string()
    } else if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        "may only contain lowercase letters, digits, '-' and '_'".to_string()
    } else {
        return None;
    };
    Some(FieldError { field: "id", message })
}

fn validate_name(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        Some("is required".into())
    } else if name.chars().count() > MAX_NAME_LEN {
        Some(format!("must be at most {MAX_NAME_LEN} characters"))
    } else if has_control(name) {
        Some("must not contain line breaks or control characters".into())
    } else if name.contains(['<', '>']) {
        Some("must not contain '<' or '>' (Git uses them to delimit the email)".into())
    } else {
        None
    }
}

fn validate_email(email: &str) -> Option<String> {
    if email.is_empty() {
        return Some("is required".into());
    }
    if email.len() > MAX_EMAIL_LEN {
        return Some(format!("must be at most {MAX_EMAIL_LEN} characters"));
    }
    if has_control(email) || email.chars().any(char::is_whitespace) || email.contains(['<', '>']) {
        return Some("must not contain spaces, '<', '>' or control characters".into());
    }
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() && !domain.contains('@') => None,
        _ => Some("must look like name@example.com".into()),
    }
}

fn validate_ssh_key(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    if has_control(&text) {
        return Some("must not contain control characters".into());
    }
    if !(path.is_absolute() || text.starts_with("~/")) {
        return Some("must be an absolute path or start with ~/".into());
    }
    if path.extension().is_some_and(|ext| ext == "pub") {
        return Some("points to a public key; use the private key path (it is never read)".into());
    }
    None
}

/// Validate a profile's fields without touching the filesystem.
pub fn validate(profile: &Profile) -> Vec<FieldError> {
    let mut errors = Vec::new();
    errors.extend(validate_id(&profile.id));
    let mut push = |field: &'static str, problem: Option<String>| {
        if let Some(message) = problem {
            errors.push(FieldError { field, message });
        }
    };
    push("name", validate_name(&profile.name));
    push("email", validate_email(&profile.email));
    push(
        "label",
        profile.label.as_deref().and_then(|l| {
            if l.chars().count() > MAX_LABEL_LEN {
                Some(format!("must be at most {MAX_LABEL_LEN} characters"))
            } else if has_control(l) {
                Some("must not contain line breaks or control characters".into())
            } else {
                None
            }
        }),
    );
    push("ssh_key", profile.ssh_key.as_deref().and_then(validate_ssh_key));
    errors
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilesDocument {
    version: u32,
    #[serde(default, rename = "profile")]
    profiles: Vec<Profile>,
}

const FORMAT_VERSION: u32 = 1;
const HEADER: &str = "# GitMate profiles. Prefer `gitmate profile edit` over editing by hand.\n\
                      # SSH keys are referenced by path only; key contents are never read or stored.\n\n";

/// Reads and writes `profiles.toml`.
#[derive(Debug, Clone)]
pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        ProfileStore { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load all profiles. A missing file means no profiles.
    pub fn load(&self) -> Result<Vec<Profile>> {
        let Some(bytes) = fsutil::read_optional(&self.path)? else {
            return Ok(Vec::new());
        };
        let parse_error = |message: String| Error::Parse {
            path: self.path.clone(),
            message,
        };
        let text = String::from_utf8(bytes).map_err(|_| parse_error("not valid UTF-8".into()))?;
        let doc: ProfilesDocument = toml::from_str(&text).map_err(|e| parse_error(e.to_string()))?;
        if doc.version != FORMAT_VERSION {
            return Err(parse_error(format!(
                "format version {} is not supported by this GitMate (expects {FORMAT_VERSION})",
                doc.version
            )));
        }
        let mut seen: Vec<&str> = Vec::new();
        for profile in &doc.profiles {
            if let Some(problem) = validate(profile).into_iter().next() {
                return Err(parse_error(format!(
                    "profile '{}': {}: {}",
                    profile.id, problem.field, problem.message
                )));
            }
            if seen.contains(&profile.id.as_str()) {
                return Err(parse_error(format!("profile '{}' is defined twice", profile.id)));
            }
            seen.push(&profile.id);
        }
        Ok(doc.profiles)
    }

    /// Atomically write all profiles. The caller holds the writer lock.
    pub fn save(&self, profiles: &[Profile]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fsutil::ensure_private_dir(parent)?;
        }
        let doc = ProfilesDocument {
            version: FORMAT_VERSION,
            profiles: profiles.to_vec(),
        };
        let body = toml::to_string_pretty(&doc).map_err(|e| Error::Parse {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        fsutil::atomic_write(&self.path, format!("{HEADER}{body}").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str, email: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            email: email.into(),
            label: None,
            ssh_key: None,
        }
    }

    fn fields(errors: Vec<FieldError>) -> Vec<&'static str> {
        errors.into_iter().map(|e| e.field).collect()
    }

    #[test]
    fn accepts_typical_profiles() {
        assert!(validate(&profile("work", "Ada Lovelace", "ada@acme.example")).is_empty());
        assert!(
            validate(&profile(
                "oss_2",
                "Zoë Ó Briain",
                "12345+zoe@users.noreply.github.com"
            ))
            .is_empty()
        );
    }

    #[test]
    fn rejects_ids_that_could_escape_the_includes_directory() {
        for bad in [
            "",
            "../x",
            "a/b",
            "Work",
            "-x",
            "a b",
            "a.b",
            &"x".repeat(MAX_ID_LEN + 1),
        ] {
            assert!(validate_id(bad).is_some(), "id {bad:?} should be rejected");
        }
        assert!(validate_id("client-7").is_none());
    }

    #[test]
    fn rejects_bad_names_and_emails() {
        assert_eq!(fields(validate(&profile("a", "", "a@b.c"))), vec!["name"]);
        assert_eq!(fields(validate(&profile("a", "Eve <x>", "a@b.c"))), vec!["name"]);
        assert_eq!(
            fields(validate(&profile("a", "Line\nBreak", "a@b.c"))),
            vec!["name"]
        );
        assert_eq!(fields(validate(&profile("a", "A", "no-at-sign"))), vec!["email"]);
        assert_eq!(fields(validate(&profile("a", "A", "two@@b.c"))), vec!["email"]);
        assert_eq!(fields(validate(&profile("a", "A", "sp ace@b.c"))), vec!["email"]);
        assert_eq!(fields(validate(&profile("a", "A", "@b.c"))), vec!["email"]);
        assert_eq!(fields(validate(&profile("a", "A", "a@"))), vec!["email"]);
        assert_eq!(fields(validate(&profile("a", "A", "<a@b.c>"))), vec!["email"]);
    }

    #[test]
    fn rejects_relative_or_public_ssh_key_paths() {
        let mut p = profile("a", "A", "a@b.c");
        p.ssh_key = Some(PathBuf::from("id_ed25519"));
        assert_eq!(fields(validate(&p)), vec!["ssh_key"]);
        p.ssh_key = Some(PathBuf::from("~/.ssh/id_ed25519.pub"));
        assert_eq!(fields(validate(&p)), vec!["ssh_key"]);
        p.ssh_key = Some(PathBuf::from("~/.ssh/id_ed25519"));
        assert!(validate(&p).is_empty());
        p.ssh_key = Some(PathBuf::from("/keys/with space/id"));
        assert!(validate(&p).is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(dir.path().join("cfg/profiles.toml"));
        assert!(store.load().unwrap().is_empty());
        let mut work = profile("work", "Ada \"The Countess\" Lovelace", "ada@acme.example");
        work.label = Some("Acme".into());
        work.ssh_key = Some(PathBuf::from("~/.ssh/id_acme"));
        let personal = profile("personal", "Ada", "ada@home.example");
        store.save(&[work.clone(), personal.clone()]).unwrap();
        assert_eq!(store.load().unwrap(), vec![work, personal]);
    }

    #[test]
    fn reports_hand_edit_mistakes_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        let store = ProfileStore::new(&path);

        std::fs::write(
            &path,
            "version = 1\n[[profile]]\nid = \"w\"\nname = \"W\"\nemial = \"w@x.y\"\n",
        )
        .unwrap();
        let err = store.load().unwrap_err().to_string();
        assert!(err.contains("emial"), "{err}");

        std::fs::write(
            &path,
            "version = 1\n[[profile]]\nid = \"W!\"\nname = \"W\"\nemail = \"w@x.y\"\n",
        )
        .unwrap();
        assert!(store.load().unwrap_err().to_string().contains("'W!'"));

        std::fs::write(&path, "version = 2\n").unwrap();
        assert!(store.load().unwrap_err().to_string().contains("version 2"));
    }
}
