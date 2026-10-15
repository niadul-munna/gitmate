//! Remote URLs and the transport each one uses.

use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::git::Git;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Ssh,
    Https,
    Http,
    Git,
    File,
    Local,
    Other,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Ssh => "ssh",
            Transport::Https => "https",
            Transport::Http => "http",
            Transport::Git => "git",
            Transport::File => "file",
            Transport::Local => "local",
            Transport::Other => "other",
        }
    }
}

/// A remote URL, safe to display: credentials embedded in HTTP(S) URLs, and
/// passwords in any other URL, are replaced with `***`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteUrl {
    pub display: String,
    pub transport: Transport,
    pub host: Option<String>,
    pub user: Option<String>,
    /// True when an HTTP(S) URL carries a username or token, or any other
    /// URL carries a password.
    pub embedded_credentials: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Remote {
    pub name: String,
    pub fetch: Vec<RemoteUrl>,
    pub push: Vec<RemoteUrl>,
}

/// Classify a URL the way Git does (see "GIT URLS" in `git-clone(1)`).
pub fn classify_url(url: &str) -> RemoteUrl {
    if let Some((scheme, rest)) = url.split_once("://") {
        return classify_scheme_url(url, &scheme.to_ascii_lowercase(), rest);
    }
    // Git treats `host:path` as scp-like SSH unless a slash comes before the
    // first colon, or it is a DOS drive prefix (`C:\...`).
    let colon = url.find(':');
    let slash = url.find(['/', '\\']);
    let dos_drive = url.len() >= 2 && url.as_bytes()[0].is_ascii_alphabetic() && url.as_bytes()[1] == b':';
    let local = match colon {
        None => true,
        Some(c) => slash.is_some_and(|s| s < c) || dos_drive,
    };
    if local {
        return RemoteUrl {
            display: url.to_string(),
            transport: Transport::Local,
            host: None,
            user: None,
            embedded_credentials: false,
        };
    }
    let authority = &url[..colon.unwrap_or(url.len())];
    let (user, host) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user.to_string()), host),
        None => (None, authority),
    };
    RemoteUrl {
        display: url.to_string(),
        transport: Transport::Ssh,
        host: Some(host.trim_start_matches('[').trim_end_matches(']').to_string()),
        user,
        embedded_credentials: false,
    }
}

fn classify_scheme_url(url: &str, scheme: &str, rest: &str) -> RemoteUrl {
    let transport = match scheme {
        "ssh" | "git+ssh" | "ssh+git" => Transport::Ssh,
        "https" => Transport::Https,
        "http" => Transport::Http,
        "git" => Transport::Git,
        "file" => Transport::File,
        _ => Transport::Other,
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    let host = if let Some(bracketed) = hostport.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        hostport.split(':').next().unwrap_or_default()
    };
    let is_http = matches!(transport, Transport::Https | Transport::Http);
    // HTTP(S) tokens are often given as the username, so the whole userinfo
    // is hidden there; elsewhere only the password is.
    let shown_userinfo = match userinfo {
        Some(_) if is_http => Some("***".to_string()),
        Some(u) => u.split_once(':').map(|(user, _)| format!("{user}:***")),
        None => None,
    };
    let embedded_credentials = shown_userinfo.is_some();
    let display = match &shown_userinfo {
        Some(shown) => format!(
            "{}://{shown}@{}{}",
            &url[..scheme.len()],
            hostport,
            &rest[authority_end..]
        ),
        None => url.to_string(),
    };
    RemoteUrl {
        display,
        transport,
        host: (!host.is_empty()).then(|| host.to_string()),
        user: if is_http {
            None
        } else {
            userinfo.map(|u| u.split(':').next().unwrap_or_default().to_string())
        },
        embedded_credentials,
    }
}

/// A credential helper as it is safe to show: inline shell helpers (`!...`)
/// and helper arguments can carry tokens, so only the helper's name is kept.
pub fn redact_helper(value: &str) -> String {
    let value = value.trim();
    if value.starts_with('!') {
        return "!<shell command hidden>".into();
    }
    match value.split_once(char::is_whitespace) {
        Some((name, _)) => format!("{name} <arguments hidden>"),
        None => value.to_string(),
    }
}

/// A `credential.<url>.<name>` key with any credentials in `<url>` hidden.
pub fn redact_credential_key(key: &str) -> String {
    let Some(rest) = key.strip_prefix("credential.") else {
        return key.to_string();
    };
    match rest.rsplit_once('.') {
        Some((url, name)) if url.contains("://") => {
            format!("credential.{}.{name}", classify_url(url).display)
        }
        _ => key.to_string(),
    }
}

/// List remotes with fetch and push URLs, after `insteadOf` rewriting (Git
/// applies that rewriting in `git remote get-url`).
pub fn list_remotes(git: &Git, repo_dir: &Path) -> Result<Vec<Remote>> {
    let names = git.run_at(repo_dir, ["remote"])?;
    if !names.success() {
        return Ok(Vec::new());
    }
    let mut remotes = Vec::new();
    for name in String::from_utf8_lossy(&names.stdout).lines() {
        let urls = |push: bool| -> Result<Vec<RemoteUrl>> {
            let mut args = vec!["remote", "get-url", "--all"];
            if push {
                args.push("--push");
            }
            args.push("--");
            args.push(name);
            let out = git.run_at(repo_dir, &args)?;
            Ok(if out.success() {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(classify_url)
                    .collect()
            } else {
                Vec::new()
            })
        };
        remotes.push(Remote {
            name: name.to_string(),
            fetch: urls(false)?,
            push: urls(true)?,
        });
    }
    Ok(remotes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_scp_like_ssh() {
        let u = classify_url("git@github.com:acme/app.git");
        assert_eq!(u.transport, Transport::Ssh);
        assert_eq!(u.host.as_deref(), Some("github.com"));
        assert_eq!(u.user.as_deref(), Some("git"));
        assert_eq!(u.display, "git@github.com:acme/app.git");
        let alias = classify_url("gh-work:acme/app.git");
        assert_eq!(alias.transport, Transport::Ssh);
        assert_eq!(alias.host.as_deref(), Some("gh-work"));
    }

    #[test]
    fn classifies_ssh_scheme_urls() {
        for url in [
            "ssh://git@gitlab.com:2222/a/b.git",
            "git+ssh://git@gitlab.com/a/b",
            "ssh+git://gitlab.com/a/b",
        ] {
            let u = classify_url(url);
            assert_eq!(u.transport, Transport::Ssh, "{url}");
            assert_eq!(u.host.as_deref(), Some("gitlab.com"), "{url}");
        }
    }

    #[test]
    fn redacts_credentials_in_https_urls() {
        let u = classify_url("https://ada:ghp_SECRET@github.com/acme/app.git");
        assert_eq!(u.transport, Transport::Https);
        assert_eq!(u.display, "https://***@github.com/acme/app.git");
        assert!(u.embedded_credentials);
        assert_eq!(u.host.as_deref(), Some("github.com"));
        let token_only = classify_url("https://ghp_SECRET@github.com/acme/app.git");
        assert!(!token_only.display.contains("SECRET"));
        let clean = classify_url("https://github.com/acme/app.git");
        assert!(!clean.embedded_credentials);
        assert_eq!(clean.display, "https://github.com/acme/app.git");
    }

    #[test]
    fn redacts_passwords_in_other_scheme_urls() {
        let ssh = classify_url("ssh://git:hunter2@example.com/r.git");
        assert_eq!(ssh.display, "ssh://git:***@example.com/r.git");
        assert_eq!(ssh.user.as_deref(), Some("git"));
        assert!(ssh.embedded_credentials);
        let ftp = classify_url("ftp://bob:s3cr3t@example.com/x");
        assert!(!ftp.display.contains("s3cr3t"), "{}", ftp.display);
        assert!(ftp.embedded_credentials);
        let user_only = classify_url("ssh://git@example.com/r.git");
        assert_eq!(user_only.display, "ssh://git@example.com/r.git");
        assert!(!user_only.embedded_credentials);
    }

    #[test]
    fn redacts_credential_helpers_and_keys() {
        assert_eq!(redact_helper("osxkeychain"), "osxkeychain");
        assert_eq!(
            redact_helper("store --file /home/a/creds"),
            "store <arguments hidden>"
        );
        assert!(!redact_helper("!f() { echo password=ghp_X; }; f").contains("ghp_X"));
        assert_eq!(
            redact_credential_key("credential.https://bob:tok@example.com.helper"),
            "credential.https://***@example.com.helper"
        );
        assert_eq!(redact_credential_key("credential.helper"), "credential.helper");
    }

    #[test]
    fn classifies_other_transports() {
        assert_eq!(
            classify_url("http://example.com/r.git").transport,
            Transport::Http
        );
        assert_eq!(classify_url("git://example.com/r.git").transport, Transport::Git);
        assert_eq!(classify_url("file:///srv/r.git").transport, Transport::File);
        assert_eq!(classify_url("/srv/git/r.git").transport, Transport::Local);
        assert_eq!(classify_url("../sibling").transport, Transport::Local);
        assert_eq!(classify_url("./a:b").transport, Transport::Local);
        assert_eq!(classify_url("dir/with:colon").transport, Transport::Local);
        assert_eq!(classify_url("C:\\repos\\r").transport, Transport::Local);
        assert_eq!(classify_url("https://[::1]:8443/r").host.as_deref(), Some("::1"));
    }
}
