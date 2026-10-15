//! Plans waiting for confirmation, for front ends that separate preview and
//! apply across a trust boundary (the desktop webview).
//!
//! The untrusted side only ever holds an opaque token. Applying looks the plan
//! up here, so a front end can approve a plan the core computed, but can never
//! submit its own list of files and keys to write.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::plan::Plan;

/// Maximum number of plans held at once; the oldest is dropped beyond this.
const MAX_PENDING: usize = 32;
/// Longest path accepted from an untrusted caller.
const MAX_PATH_BYTES: usize = 4096;

pub struct PendingPlans {
    plans: Mutex<HashMap<String, (Plan, Instant)>>,
    counter: AtomicU64,
    salt: RandomState,
    ttl: Duration,
}

impl PendingPlans {
    pub fn new(ttl: Duration) -> PendingPlans {
        PendingPlans {
            plans: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
            salt: RandomState::new(),
            ttl,
        }
    }

    /// Store `plan` and return the token that refers to it.
    pub fn insert(&self, plan: Plan) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut a = self.salt.build_hasher();
        a.write_u64(n);
        let mut b = self.salt.build_hasher();
        b.write_u64(!n);
        let token = format!("{:016x}{:016x}", a.finish(), b.finish());

        let mut plans = self.plans.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        plans.retain(|_, (_, created)| now.duration_since(*created) < self.ttl);
        while plans.len() >= MAX_PENDING {
            let oldest = plans
                .iter()
                .min_by_key(|(_, (_, created))| *created)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(key) => plans.remove(&key),
                None => break,
            };
        }
        plans.insert(token.clone(), (plan, now));
        token
    }

    /// Remove and return the plan for `token`, if it exists and has not
    /// expired. Each token can be used once.
    pub fn take(&self, token: &str) -> Option<Plan> {
        let mut plans = self.plans.lock().unwrap_or_else(|p| p.into_inner());
        let (plan, created) = plans.remove(token)?;
        (created.elapsed() < self.ttl).then_some(plan)
    }

    /// Forget a plan the user cancelled.
    pub fn discard(&self, token: &str) {
        let _ = self.take(token);
    }

    pub fn len(&self) -> usize {
        self.plans.lock().map(|p| p.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Parse a path string received from an untrusted caller (e.g. the desktop
/// webview). Accepts absolute paths and `~/...`; rejects empty input, NUL and
/// other control characters, relative paths, and overlong input.
pub fn parse_untrusted_path(input: &str, home: &Path) -> Result<PathBuf> {
    let invalid = |reason: &str| Error::InvalidPath {
        path: PathBuf::from(
            input
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect::<String>(),
        ),
        reason: reason.into(),
    };
    if input.is_empty() {
        return Err(invalid("is empty"));
    }
    if input.len() > MAX_PATH_BYTES {
        return Err(invalid("is too long"));
    }
    if input.chars().any(char::is_control) {
        return Err(invalid("contains control characters"));
    }
    let path = crate::paths::expand_tilde(Path::new(input), home);
    if !path.is_absolute() {
        return Err(invalid("must be an absolute path"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Action;

    fn plan(name: &str) -> Plan {
        Plan::new(
            Action::Undo {
                operation: name.into(),
            },
            name.into(),
            Vec::new(),
        )
    }

    #[test]
    fn tokens_are_single_use() {
        let pending = PendingPlans::new(Duration::from_secs(60));
        let token = pending.insert(plan("a"));
        assert_eq!(pending.take(&token).unwrap().summary, "a");
        assert!(pending.take(&token).is_none());
    }

    #[test]
    fn tokens_are_distinct_and_unguessable_by_counting() {
        let pending = PendingPlans::new(Duration::from_secs(60));
        let a = pending.insert(plan("a"));
        let b = pending.insert(plan("b"));
        assert_ne!(a, b);
        assert!(a.len() >= 24, "{a}");
        assert!(pending.take("0").is_none());
        assert!(pending.take("").is_none());
    }

    #[test]
    fn expired_plans_cannot_be_applied() {
        let pending = PendingPlans::new(Duration::from_millis(1));
        let token = pending.insert(plan("a"));
        std::thread::sleep(Duration::from_millis(5));
        assert!(pending.take(&token).is_none());
    }

    #[test]
    fn the_number_of_pending_plans_is_bounded() {
        let pending = PendingPlans::new(Duration::from_secs(60));
        let first = pending.insert(plan("first"));
        for i in 0..MAX_PENDING {
            pending.insert(plan(&i.to_string()));
        }
        assert_eq!(pending.len(), MAX_PENDING);
        assert!(pending.take(&first).is_none(), "oldest plan was evicted");
    }

    #[cfg(unix)]
    #[test]
    fn untrusted_paths_are_validated() {
        let home = Path::new("/home/ada");
        assert_eq!(
            parse_untrusted_path("/srv/my repo", home).unwrap(),
            PathBuf::from("/srv/my repo")
        );
        assert_eq!(
            parse_untrusted_path("~/work", home).unwrap(),
            PathBuf::from("/home/ada/work")
        );
        for bad in ["", "relative/dir", "/a\0b", "/a\nb", "-rf"] {
            assert!(parse_untrusted_path(bad, home).is_err(), "{bad:?}");
        }
        assert!(parse_untrusted_path(&format!("/{}", "a".repeat(5000)), home).is_err());
    }
}
