//! The `GitMate` facade used by the CLI and the desktop app.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::doctor::{self, DoctorOptions, DoctorReport};
use crate::error::{Error, FieldError, Result};
use crate::fsutil::WriterLock;
use crate::git::{Git, Ident, Scope};
use crate::identity;
use crate::includes;
use crate::journal::OperationRecord;
use crate::paths::AppPaths;
use crate::plan::{Action, AssignOptions, ConfigChange, Issue, Plan};
use crate::profile::{self, Profile, ProfileInput, ProfileStore};
use crate::remote::{self, Transport};
use crate::repo::{self, RepoInfo};
use crate::report::{self, RepoReport, ReportInput};
use crate::rules::{self, Rule};
use crate::ssh::{self, KeyStatus};
use crate::transaction::Transactions;

const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Entry point for every GitMate operation.
#[derive(Debug, Clone)]
pub struct GitMate {
    git: Git,
    paths: AppPaths,
    lock_timeout: Duration,
}

/// Result of saving a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileSaved {
    pub profile: Profile,
    /// Non-blocking problems, e.g. SSH key permissions.
    pub warnings: Vec<Issue>,
}

/// Result of applying a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApplyOutcome {
    /// The recorded operation; `None` when the plan was a no-op.
    pub operation: Option<OperationRecord>,
    /// For assignments: the identity Git now uses in the repository.
    pub effective: Option<Ident>,
    /// Things worth telling the user after applying.
    pub notes: Vec<String>,
}

/// A profile together with where it is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileUsage {
    pub profile: Profile,
    pub rules: Vec<Rule>,
    /// Repositories this profile was assigned to with `use` (most recent
    /// first), from the operation journal.
    pub repositories: Vec<PathBuf>,
    pub ssh_key: Option<ssh::KeyCheck>,
}

/// SSH keys a profile could use, found in `~/.ssh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SshKeyChoices {
    /// The `~/.ssh` directory, where a file picker should start.
    pub dir: PathBuf,
    pub keys: Vec<SshKeyChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SshKeyChoice {
    /// The key's path as a profile stores it, e.g. `~/.ssh/id_ed25519`.
    pub path: PathBuf,
    /// Profiles that already use this key.
    pub used_by: Vec<String>,
}

impl GitMate {
    pub fn new(git: Git, paths: AppPaths) -> Self {
        GitMate {
            git,
            paths,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
        }
    }

    /// Facade for the real environment of the current process.
    pub fn from_env() -> Result<Self> {
        let git = Git::default();
        let paths = AppPaths::resolve(|k| std::env::var_os(k))?;
        Ok(GitMate::new(git, paths))
    }

    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    pub fn git(&self) -> &Git {
        &self.git
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    fn tx(&self) -> Transactions<'_> {
        Transactions {
            git: &self.git,
            paths: &self.paths,
            lock_timeout: self.lock_timeout,
        }
    }

    fn store(&self) -> ProfileStore {
        ProfileStore::new(self.paths.profiles_file())
    }

    fn lock(&self) -> Result<WriterLock> {
        self.tx().lock()
    }

    // ----- profiles -------------------------------------------------------

    pub fn profiles(&self) -> Result<Vec<Profile>> {
        self.store().load()
    }

    pub fn profile(&self, id: &str) -> Result<Profile> {
        self.profiles()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::ProfileNotFound(id.to_string()))
    }

    /// A profile with its rules, assigned repositories and key status.
    pub fn profile_usage(&self, id: &str) -> Result<ProfileUsage> {
        let profile = self.profile(id)?;
        let rules = self
            .rules()?
            .into_iter()
            .filter(|r| r.profile.as_deref() == Some(id))
            .collect();
        let mut repositories: Vec<PathBuf> = Vec::new();
        for op in self.history()?.iter().rev() {
            if let Action::Assign {
                repository,
                profile: p,
                ..
            } = &op.action
                && op.status == crate::journal::OperationStatus::Applied
                && p == id
                && !repositories.contains(repository)
            {
                repositories.push(repository.clone());
            }
        }
        let ssh_key = profile
            .ssh_key
            .as_ref()
            .map(|k| ssh::check_key(k, &self.paths.home));
        Ok(ProfileUsage {
            profile,
            rules,
            repositories,
            ssh_key,
        })
    }

    /// Field-level validation including SSH key checks (metadata only).
    fn validate_profile(&self, profile: &Profile, others: &[Profile]) -> (Vec<FieldError>, Vec<Issue>) {
        let mut errors = profile::validate(profile);
        let mut warnings = Vec::new();
        if let Some(other) = others
            .iter()
            .find(|o| o.id != profile.id && o.matches_email(&profile.email))
        {
            errors.push(FieldError {
                field: "email",
                message: format!("is already used by profile '{}'", other.id),
            });
        }
        if let Some(key) = profile
            .ssh_key
            .as_ref()
            .filter(|_| errors.iter().all(|e| e.field != "ssh_key"))
        {
            let check = ssh::check_key(key, &self.paths.home);
            match &check.status {
                KeyStatus::Ok => {}
                KeyStatus::PermissionsTooOpen { .. } => warnings.push(Issue::new(
                    "ssh_key_permissions",
                    check.problem().unwrap_or_default(),
                )),
                KeyStatus::Missing => errors.push(FieldError {
                    field: "ssh_key",
                    message: format!("{} does not exist", check.path.display()),
                }),
                KeyStatus::NotAFile => errors.push(FieldError {
                    field: "ssh_key",
                    message: format!("{} is not a regular file", check.path.display()),
                }),
                KeyStatus::Inaccessible { message } => errors.push(FieldError {
                    field: "ssh_key",
                    message: format!("cannot be inspected: {message}"),
                }),
            }
        }
        (errors, warnings)
    }

    pub fn add_profile(&self, profile: Profile) -> Result<ProfileSaved> {
        let profile = Profile::from_input(&profile.id, profile.to_input());
        let _lock = self.lock()?;
        let mut profiles = self.profiles()?;
        if profiles.iter().any(|p| p.id == profile.id) {
            return Err(Error::ProfileExists(profile.id));
        }
        let (errors, warnings) = self.validate_profile(&profile, &profiles);
        if !errors.is_empty() {
            return Err(Error::InvalidProfile(errors));
        }
        includes::write(&self.git, &self.paths, &profile)?;
        profiles.push(profile.clone());
        self.store().save(&profiles)?;
        Ok(ProfileSaved { profile, warnings })
    }

    /// Replace a profile's fields. Rule-matched repositories pick up the new
    /// values immediately because the include file is regenerated.
    pub fn update_profile(&self, id: &str, input: ProfileInput) -> Result<ProfileSaved> {
        let profile = Profile::from_input(id, input);
        let _lock = self.lock()?;
        let mut profiles = self.profiles()?;
        let index = profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| Error::ProfileNotFound(id.to_string()))?;
        let (errors, warnings) = self.validate_profile(&profile, &profiles);
        if !errors.is_empty() {
            return Err(Error::InvalidProfile(errors));
        }
        includes::write(&self.git, &self.paths, &profile)?;
        profiles[index] = profile.clone();
        self.store().save(&profiles)?;
        Ok(ProfileSaved { profile, warnings })
    }

    /// Private keys in `~/.ssh` and the profiles already using each one.
    pub fn ssh_key_choices(&self) -> Result<SshKeyChoices> {
        let home = &self.paths.home;
        let dir = home.join(".ssh");
        let profiles = self.profiles()?;
        let keys = ssh::find_private_keys(&dir)
            .into_iter()
            .map(|key| SshKeyChoice {
                used_by: profiles
                    .iter()
                    .filter(|p| {
                        p.ssh_key.as_ref().is_some_and(|k| {
                            crate::paths::normalize_lexically(&crate::paths::expand_tilde(k, home)) == key
                        })
                    })
                    .map(|p| p.id.clone())
                    .collect(),
                path: PathBuf::from(self.paths.display(&key)),
            })
            .collect();
        Ok(SshKeyChoices { dir, keys })
    }

    /// Remove a profile. Refused while a rule still includes it.
    pub fn remove_profile(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut profiles = self.profiles()?;
        let index = profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| Error::ProfileNotFound(id.to_string()))?;
        let using: Vec<String> = self
            .rules()?
            .into_iter()
            .filter(|r| r.profile.as_deref() == Some(id))
            .map(|r| r.directory.map(|d| self.paths.display(&d)).unwrap_or(r.condition))
            .collect();
        if !using.is_empty() {
            return Err(Error::ProfileInUse {
                id: id.to_string(),
                rules: using,
            });
        }
        profiles.remove(index);
        self.store().save(&profiles)?;
        includes::remove(&self.paths, id)
    }

    // ----- inspection -----------------------------------------------------

    /// Everything GitMate knows about the identity used in `dir`.
    pub fn inspect(&self, dir: &Path) -> Result<RepoReport> {
        let dir = repo::validate_directory(dir)?;
        let repository = repo::discover(&self.git, &dir)?;
        let identity = identity::inspect(&self.git, &dir)?;
        let profiles = self.profiles()?;
        let rules = self.rules()?;
        let history = self.history()?;
        let (remotes, ssh_entries, credentials) = match &repository {
            Some(_) => {
                let ssh_entries = self
                    .git
                    .scoped_get_all(&dir, "core.sshCommand")?
                    .into_iter()
                    .map(|e| identity::resolve_origin(e, repository.as_ref(), &dir))
                    .collect();
                let credentials = self
                    .git
                    .scoped_get_regexp(&dir, r"^credential\..*(helper|username)$")?
                    .into_iter()
                    .map(|(k, mut e)| {
                        if k.ends_with(".helper") {
                            e.value = remote::redact_helper(&e.value);
                        }
                        let e = identity::resolve_origin(e, repository.as_ref(), &dir);
                        (remote::redact_credential_key(&k), e)
                    })
                    .collect();
                (remote::list_remotes(&self.git, &dir)?, ssh_entries, credentials)
            }
            None => (Vec::new(), Vec::new(), Vec::new()),
        };
        Ok(report::build(ReportInput {
            path: dir,
            repository,
            identity,
            profiles: &profiles,
            rules: &rules,
            history: &history,
            remotes,
            ssh_entries,
            git_ssh_command: self
                .git
                .effective_env("GIT_SSH_COMMAND")
                .map(|v| v.to_string_lossy().into_owned()),
            credentials,
            paths: &self.paths,
        }))
    }

    fn require_repo(&self, dir: &Path) -> Result<(PathBuf, RepoInfo)> {
        let dir = repo::validate_directory(dir)?;
        match repo::discover(&self.git, &dir)? {
            Some(info) => Ok((dir, info)),
            None => Err(Error::NotARepository { path: dir }),
        }
    }

    // ----- assignment -----------------------------------------------------

    /// Plan writing `profile`'s identity into the repository containing `dir`.
    pub fn plan_assign(&self, dir: &Path, profile_id: &str, options: AssignOptions) -> Result<Plan> {
        let (dir, repo) = self.require_repo(dir)?;
        let profile = self.profile(profile_id)?;
        let file = repo.local_config();
        let current = |key: &str| self.git.file_get_all(&file, key);

        let mut changes = vec![
            ConfigChange {
                file: file.clone(),
                key: "user.name".into(),
                before: current("user.name")?,
                after: vec![profile.name.clone()],
            },
            ConfigChange {
                file: file.clone(),
                key: "user.email".into(),
                before: current("user.email")?,
                after: vec![profile.email.clone()],
            },
        ];
        let mut conflicts = Vec::new();
        let mut warnings = Vec::new();

        if options.ssh {
            match &profile.ssh_key {
                None => conflicts.push(Issue::new(
                    "no_ssh_key",
                    format!("Profile '{}' has no SSH key; add one with `gitmate profile edit {} --ssh-key <path>`", profile.id, profile.id),
                )),
                Some(key) => {
                    let check = ssh::check_key(key, &self.paths.home);
                    if let Some(problem) = check.problem().filter(|_| !check.is_usable()) {
                        let code = if matches!(check.status, KeyStatus::PermissionsTooOpen { .. }) {
                            "ssh_key_permissions"
                        } else {
                            "ssh_key_unusable"
                        };
                        if code == "ssh_key_permissions" {
                            warnings.push(Issue::new(code, problem));
                        } else {
                            conflicts.push(Issue::new(code, problem));
                        }
                    }
                    if !matches!(check.status, KeyStatus::Missing | KeyStatus::NotAFile | KeyStatus::Inaccessible { .. }) {
                        changes.push(ConfigChange {
                            file: file.clone(),
                            key: "core.sshCommand".into(),
                            before: current("core.sshCommand")?,
                            after: vec![ssh::ssh_command_for_key(&check.path)?],
                        });
                    }
                }
            }
        }

        if options.sign {
            match profile.signing_key_value(&self.paths.home) {
                None => conflicts.push(Issue::new(
                    "no_signing_key",
                    format!(
                        "Profile '{}' has no signing key; add one with `gitmate profile edit {} --signing-key <key>`",
                        profile.id, profile.id
                    ),
                )),
                Some(key) => {
                    changes.push(ConfigChange {
                        file: file.clone(),
                        key: "user.signingKey".into(),
                        before: current("user.signingKey")?,
                        after: vec![key],
                    });
                    if let Some(format) = &profile.gpg_format {
                        changes.push(ConfigChange {
                            file: file.clone(),
                            key: "gpg.format".into(),
                            before: current("gpg.format")?,
                            after: vec![format.clone()],
                        });
                    }
                    changes.push(ConfigChange {
                        file: file.clone(),
                        key: "commit.gpgsign".into(),
                        before: current("commit.gpgsign")?,
                        after: vec!["true".into()],
                    });
                }
            }
        }

        for change in &changes {
            if change.before.len() > 1 {
                warnings.push(Issue::new(
                    "multiple_values",
                    format!(
                        "{} has {} values in {}; all of them will be replaced",
                        change.key,
                        change.before.len(),
                        self.paths.display(&change.file)
                    ),
                ));
            }
        }

        let identity = identity::inspect(&self.git, &dir)?;
        for var in identity
            .env
            .iter()
            .filter(|v| v.name.starts_with("GIT_AUTHOR_") || v.name.starts_with("GIT_COMMITTER_"))
        {
            warnings.push(Issue::new(
                "env_override",
                format!(
                    "{} is set in this environment and will still override the repository config",
                    var.name
                ),
            ));
        }
        for key in ["author.name", "author.email", "committer.name", "committer.email"] {
            if identity.key(key).is_some() {
                warnings.push(Issue::new(
                    "author_committer_override",
                    format!(
                        "{key} is configured and takes precedence over user.{}",
                        key.split('.').nth(1).unwrap_or_default()
                    ),
                ));
            }
        }
        for key in ["user.name", "user.email"] {
            if let Some(entry) = identity.key(key).and_then(|k| {
                k.values
                    .iter()
                    .rev()
                    .find(|e| matches!(e.scope, Scope::Worktree | Scope::Command))
            }) {
                warnings.push(Issue::new(
                    "higher_scope_override",
                    format!(
                        "{key} is also set at {} scope, which overrides the repository config",
                        entry.scope.as_str()
                    ),
                ));
            }
        }
        if repo.is_linked_worktree {
            warnings.push(Issue::new(
                "shared_worktree_config",
                format!(
                    "This is a linked worktree; the change goes to {}, which every worktree of this repository shares",
                    self.paths.display(&file)
                ),
            ));
        }
        if options.ssh {
            let transports: Vec<Transport> = remote::list_remotes(&self.git, &dir)?
                .iter()
                .flat_map(|r| {
                    r.fetch
                        .iter()
                        .chain(&r.push)
                        .map(|u| u.transport)
                        .collect::<Vec<_>>()
                })
                .collect();
            if !transports.is_empty()
                && transports
                    .iter()
                    .all(|t| matches!(t, Transport::Https | Transport::Http))
            {
                warnings.push(Issue::new(
                    "https_remote",
                    "All remotes use HTTPS; core.sshCommand has no effect on them. HTTPS pushes authenticate through your credential helper",
                ));
            }
        }

        let mut plan = Plan::new(
            Action::Assign {
                repository: repo.root().to_path_buf(),
                profile: profile.id.clone(),
                ssh: options.ssh,
                sign: options.sign,
            },
            format!(
                "Assign profile '{}' to {}",
                profile.id,
                self.paths.display(repo.root())
            ),
            changes,
        );
        plan.conflicts = conflicts;
        if !plan.is_noop() {
            plan.warnings = warnings;
        }
        Ok(plan)
    }

    // ----- rules ----------------------------------------------------------

    /// All `includeIf` rules in the global config, managed or not.
    pub fn rules(&self) -> Result<Vec<Rule>> {
        rules::list(&self.git, &self.paths)
    }

    /// Plan a rule that applies `profile_id` to repositories under `dir`.
    pub fn plan_add_rule(&self, dir: &Path, profile_id: &str) -> Result<Plan> {
        let profile = self.profile(profile_id)?;
        let target = rules::rule_directory(dir, &self.paths.home)?;
        let condition = rules::condition_for(&target.resolved);
        let file = rules::target_file(&self.git, &self.paths.home);
        let key = format!("includeIf.{condition}.path");
        let include = self.paths.include_file(&profile.id);
        let include_value = include.to_string_lossy().into_owned();
        let shown_dir = self.paths.display(&target.resolved);

        let existing = self.rules()?;
        let mut conflicts = Vec::new();
        let mut warnings = Vec::new();
        let mut noop = false;
        for rule in existing.iter().filter(|r| r.names_directory(&target.resolved)) {
            match &rule.profile {
                Some(id) if *id == profile.id => noop = true,
                Some(id) => conflicts.push(Issue::new(
                    "existing_rule",
                    format!(
                        "{shown_dir} already uses profile '{id}'; remove that rule first with `gitmate rules remove {shown_dir}`"
                    ),
                )),
                None => conflicts.push(Issue::new(
                    "existing_include",
                    format!(
                        "{} already has an includeIf for {shown_dir} (path = {}) that GitMate does not manage; it will not be changed",
                        self.paths.display(&rule.file),
                        rule.include_path
                    ),
                )),
            }
        }

        if target.requested != target.resolved {
            warnings.push(Issue::new(
                "symlink_resolved",
                format!(
                    "{} is a symbolic link; the rule names its real location {shown_dir} because Git matches repositories by their real path",
                    self.paths.display(&target.requested)
                ),
            ));
        }
        if !target.exists {
            warnings.push(Issue::new(
                "directory_missing",
                format!("{shown_dir} does not exist yet"),
            ));
        }
        for rule in &existing {
            let Some(other) = rule.real_directory() else {
                continue;
            };
            if rule.profile.as_deref() == Some(profile.id.as_str()) || rule.names_directory(&target.resolved)
            {
                continue;
            }
            let owner = match &rule.profile {
                Some(id) => format!("profile '{id}'"),
                None => format!(
                    "an includeIf in {} that GitMate does not manage, path = {}",
                    self.paths.display(&rule.file),
                    rule.include_path
                ),
            };
            if target.resolved.starts_with(&other) {
                warnings.push(Issue::new(
                    "overrides_broader_rule",
                    format!(
                        "Inside {shown_dir}, this rule overrides the broader rule for {} ({owner}) because it is added after it",
                        self.paths.display(&other)
                    ),
                ));
            } else if other.starts_with(&target.resolved) {
                let remedy = if rule.is_managed() {
                    "Re-add that rule afterwards to keep it in effect"
                } else {
                    "Move that includeIf below the new one to keep it in effect"
                };
                warnings.push(Issue::new(
                    "shadows_specific_rule",
                    format!(
                        "This broader rule is added after the rule for {} ({owner}) and will override it there. {remedy}",
                        self.paths.display(&other)
                    ),
                ));
            }
        }

        let before = self.git.file_get_all(&file, &key)?;
        let mut plan = Plan::new(
            Action::AddRule {
                directory: target.resolved.clone(),
                profile: profile.id.clone(),
            },
            format!("Use profile '{}' for repositories under {shown_dir}", profile.id),
            if noop {
                Vec::new()
            } else {
                vec![ConfigChange {
                    file,
                    key,
                    after: before.iter().cloned().chain([include_value]).collect(),
                    before,
                }]
            },
        );
        plan.conflicts = conflicts;
        if !plan.is_noop() {
            plan.warnings = warnings;
            plan.ensure_include = Some(profile.id);
        }
        Ok(plan)
    }

    /// Plan removing the GitMate rule for `dir`.
    pub fn plan_remove_rule(&self, dir: &Path) -> Result<Plan> {
        let target = rules::rule_directory(dir, &self.paths.home)?;
        let matching: Vec<Rule> = self
            .rules()?
            .into_iter()
            .filter(|r| {
                r.names_directory(&target.resolved) || r.directory.as_ref() == Some(&target.requested)
            })
            .collect();
        let Some(rule) = matching.iter().rev().find(|r| r.is_managed()).cloned() else {
            return Err(match matching.last() {
                Some(unmanaged) => Error::PlanHasConflicts(vec![format!(
                    "the includeIf for {} in {} is not managed by GitMate; edit it by hand",
                    self.paths.display(&target.requested),
                    self.paths.display(&unmanaged.file)
                )]),
                None => Error::RuleNotFound(self.paths.display(&target.requested)),
            });
        };
        let profile = rule.profile.clone().unwrap_or_default();
        let key = rule.key();
        let before = self.git.file_get_all(&rule.file, &key)?;
        let after: Vec<String> = before
            .iter()
            .filter(|v| **v != rule.include_path)
            .cloned()
            .collect();
        let directory = rule.directory.clone().unwrap_or_default();
        Ok(Plan::new(
            Action::RemoveRule {
                directory: directory.clone(),
                profile: Some(profile.clone()),
            },
            format!(
                "Stop using profile '{profile}' for repositories under {}",
                self.paths.display(&directory)
            ),
            vec![ConfigChange {
                file: rule.file,
                key,
                before,
                after,
            }],
        ))
    }

    // ----- applying and undo ----------------------------------------------

    /// Apply a plan produced by this facade.
    pub fn apply(&self, plan: &Plan) -> Result<ApplyOutcome> {
        let tx = self.tx();
        if let Action::Undo { operation } = &plan.action
            && !plan.conflicts.is_empty()
        {
            let (_, conflicts) = tx.plan_undo(operation)?;
            if !conflicts.is_empty() {
                return Err(Error::RollbackConflict {
                    op_id: operation.clone(),
                    conflicts,
                });
            }
        }
        let record = tx.apply(plan, || match &plan.ensure_include {
            Some(id) => {
                let profile = self.profile(id)?;
                if !includes::is_current(&self.git, &self.paths, &profile)? {
                    includes::write(&self.git, &self.paths, &profile)?;
                }
                Ok(())
            }
            None => Ok(()),
        })?;

        let mut outcome = ApplyOutcome {
            operation: record,
            effective: None,
            notes: Vec::new(),
        };
        if let Action::Assign {
            repository, profile, ..
        } = &plan.action
            && let Ok(report) = identity::inspect(&self.git, repository)
        {
            if let (Some(author), Ok(p)) = (&report.author, self.profile(profile))
                && !p.matches_email(&author.email)
            {
                let source = match &report.email_source {
                    Some(identity::Source::Environment { var }) => {
                        format!("{var} is set in the environment")
                    }
                    Some(identity::Source::Config { key, scope, .. }) => {
                        format!("{key} at {} scope takes precedence", scope.as_str())
                    }
                    _ => "another setting takes precedence".into(),
                };
                outcome.notes.push(format!(
                    "The repository now says {}, but Git will still use {} because {source}",
                    p.email, author.email
                ));
            }
            outcome.effective = report.author;
        }
        Ok(outcome)
    }

    /// All recorded operations, oldest first.
    pub fn history(&self) -> Result<Vec<OperationRecord>> {
        self.tx().journal().list()
    }

    /// Plan reversing operation `id`. Keys changed outside GitMate since
    /// are reported as conflicts and block applying.
    pub fn plan_undo(&self, id: &str) -> Result<Plan> {
        Ok(self.tx().plan_undo(id)?.0)
    }

    /// Most recent operation that can still be undone.
    pub fn last_undoable(&self) -> Result<Option<OperationRecord>> {
        Ok(self.history()?.into_iter().rev().find(|op| {
            op.status == crate::journal::OperationStatus::Applied && !matches!(op.action, Action::Undo { .. })
        }))
    }

    // ----- diagnostics ----------------------------------------------------

    pub fn doctor(&self, dir: Option<&Path>) -> Result<DoctorReport> {
        self.doctor_with(dir, DoctorOptions::default())
    }

    pub fn doctor_with(&self, dir: Option<&Path>, options: DoctorOptions) -> Result<DoctorReport> {
        use crate::report::{Finding, Severity};
        let mut findings = Vec::new();
        let git_version = match self.git.version() {
            Ok(v) => Some(v),
            Err(e) => {
                findings.push(Finding::new(Severity::Error, "git_unusable", e.to_string()));
                None
            }
        };
        let profiles = match self.profiles() {
            Ok(p) => p,
            Err(e) => {
                findings.push(Finding::new(
                    Severity::Error,
                    "profiles_unreadable",
                    e.to_string(),
                ));
                Vec::new()
            }
        };
        if profiles.is_empty() && findings.is_empty() {
            findings.push(
                Finding::new(Severity::Info, "no_profiles", "No profiles yet")
                    .hint("Create one with `gitmate profile add <id> --name <name> --email <email>`"),
            );
        }
        let rules = if git_version.is_some() {
            self.rules()?
        } else {
            Vec::new()
        };
        findings.extend(doctor::profile_findings(&self.paths, &profiles));
        findings.extend(doctor::rule_findings(&self.git, &self.paths, &profiles, &rules));
        findings.extend(doctor::history_findings(&self.history().unwrap_or_default()));

        let repository = match (dir, &git_version) {
            (Some(d), Some(_)) => {
                let report = self.inspect(d)?;
                findings.extend(report.findings.iter().cloned());
                Some(report)
            }
            _ => None,
        };

        let mut ssh_hosts = Vec::new();
        if options.ssh_config
            && let Some(report) = &repository
        {
            let mut hosts: Vec<String> = report
                .remotes
                .iter()
                .flat_map(|r| r.fetch.iter().chain(&r.push))
                .filter(|u| u.transport == Transport::Ssh)
                .filter_map(|u| u.host.clone())
                .collect();
            hosts.sort();
            hosts.dedup();
            for host in hosts {
                match ssh::resolve_host(&host, None) {
                    Ok(cfg) => ssh_hosts.push(cfg),
                    Err(e) => findings.push(Finding::new(
                        Severity::Warning,
                        "ssh_config_unreadable",
                        e.to_string(),
                    )),
                }
            }
        }

        // Duplicates can arise when the repository report repeats a profile
        // finding; keep the first of each message.
        let mut seen = Vec::new();
        findings.retain(|f| {
            let key = (f.code, f.message.clone());
            let fresh = !seen.contains(&key);
            seen.push(key);
            fresh
        });
        findings.sort_by_key(|f| f.severity);
        Ok(DoctorReport {
            git_version,
            global_config: rules::target_file(&self.git, &self.paths.home),
            paths: self.paths.clone(),
            profile_count: profiles.len(),
            rules,
            repository,
            ssh_hosts,
            findings,
        })
    }
}
