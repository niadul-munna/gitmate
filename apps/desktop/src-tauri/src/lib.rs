//! Tauri glue for GitMate.
//!
//! The webview is untrusted. Every command takes plain data, validates it in
//! Rust, and calls the same core facade the CLI uses. Mutations are two-step:
//! `plan_*` returns a preview plus an opaque token, and `apply_plan(token)`
//! applies only a plan this process computed. Git runs on blocking worker
//! threads so the UI never stalls.

use std::path::PathBuf;
use std::time::Duration;

use gitmate_core::doctor::{DoctorOptions, DoctorReport};
use gitmate_core::error::FieldError;
use gitmate_core::git::GitVersion;
use gitmate_core::journal::OperationRecord;
use gitmate_core::paths::AppPaths;
use gitmate_core::pending::{PendingPlans, parse_untrusted_path};
use gitmate_core::plan::{AssignOptions, Plan};
use gitmate_core::profile::{Profile, ProfileInput};
use gitmate_core::report::RepoReport;
use gitmate_core::rules::Rule;
use gitmate_core::{ApplyOutcome, Error, GitMate, ProfileSaved, ProfileUsage, SshKeyChoices};
use serde::{Deserialize, Serialize};
use tauri::State;

/// Plans expire if the user does not confirm them within this time.
const PLAN_TTL: Duration = Duration::from_secs(15 * 60);

struct AppState {
    core: Result<GitMate, String>,
    pending: PendingPlans,
}

/// Error shape returned to the frontend.
#[derive(Debug, Serialize)]
struct CommandError {
    code: &'static str,
    message: String,
    /// Field-level problems for forms.
    fields: Vec<FieldError>,
    /// Extra lines, e.g. conflicting keys.
    details: Vec<String>,
}

impl From<Error> for CommandError {
    fn from(error: Error) -> Self {
        let mut fields = Vec::new();
        let mut details = Vec::new();
        match &error {
            Error::InvalidProfile(f) => fields = f.clone(),
            Error::ConcurrentModification(list) | Error::RollbackConflict { conflicts: list, .. } => {
                details = list
                    .iter()
                    .map(|m| {
                        format!(
                            "{} in {}: expected {:?}, found {:?}",
                            m.key,
                            m.file.display(),
                            m.expected,
                            m.found
                        )
                    })
                    .collect();
            }
            Error::PlanHasConflicts(list) => details = list.clone(),
            _ => {}
        }
        CommandError {
            code: error.code(),
            message: error.to_string(),
            fields,
            details,
        }
    }
}

impl CommandError {
    fn simple(code: &'static str, message: impl Into<String>) -> Self {
        CommandError {
            code,
            message: message.into(),
            fields: Vec::new(),
            details: Vec::new(),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

/// Run `f` with the core facade on a blocking worker thread.
async fn with_core<T, F>(state: &State<'_, AppState>, f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(GitMate) -> Result<T, Error> + Send + 'static,
{
    let core = state
        .core
        .clone()
        .map_err(|message| CommandError::simple("startup_failed", message))?;
    tauri::async_runtime::spawn_blocking(move || f(core))
        .await
        .map_err(|e| CommandError::simple("internal", e.to_string()))?
        .map_err(CommandError::from)
}

fn untrusted_path(state: &State<'_, AppState>, input: &str) -> CmdResult<PathBuf> {
    let home = match &state.core {
        Ok(core) => core.paths().home.clone(),
        Err(message) => return Err(CommandError::simple("startup_failed", message.clone())),
    };
    parse_untrusted_path(input, &home).map_err(CommandError::from)
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    git_version: Option<GitVersion>,
    git_error: Option<String>,
    paths: AppPaths,
    global_config: PathBuf,
}

#[tauri::command]
async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    with_core(&state, |core| {
        let (git_version, git_error) = match core.git().version() {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        };
        Ok(AppInfo {
            version: env!("CARGO_PKG_VERSION"),
            git_version,
            git_error,
            global_config: gitmate_core::rules::target_file(core.git(), &core.paths().home),
            paths: core.paths().clone(),
        })
    })
    .await
}

#[tauri::command]
async fn list_profiles(state: State<'_, AppState>) -> CmdResult<Vec<Profile>> {
    with_core(&state, |core| core.profiles()).await
}

#[tauri::command]
async fn get_profile(state: State<'_, AppState>, id: String) -> CmdResult<ProfileUsage> {
    with_core(&state, move |core| core.profile_usage(&id)).await
}

/// SSH private keys in `~/.ssh` (found by name; never opened), to fill the
/// profile form's key field.
#[tauri::command]
async fn list_ssh_keys(state: State<'_, AppState>) -> CmdResult<SshKeyChoices> {
    with_core(&state, |core| core.ssh_key_choices()).await
}

#[derive(Deserialize)]
struct NewProfile {
    id: String,
    name: String,
    email: String,
    label: Option<String>,
    ssh_key: Option<String>,
}

#[tauri::command]
async fn create_profile(state: State<'_, AppState>, profile: NewProfile) -> CmdResult<ProfileSaved> {
    with_core(&state, move |core| {
        core.add_profile(Profile {
            id: profile.id,
            name: profile.name,
            email: profile.email,
            label: profile.label,
            ssh_key: profile
                .ssh_key
                .filter(|k| !k.trim().is_empty())
                .map(PathBuf::from),
        })
    })
    .await
}

#[derive(Deserialize)]
struct EditProfile {
    name: String,
    email: String,
    label: Option<String>,
    ssh_key: Option<String>,
}

#[tauri::command]
async fn update_profile(
    state: State<'_, AppState>,
    id: String,
    profile: EditProfile,
) -> CmdResult<ProfileSaved> {
    with_core(&state, move |core| {
        core.update_profile(
            &id,
            ProfileInput {
                name: profile.name,
                email: profile.email,
                label: profile.label,
                ssh_key: profile
                    .ssh_key
                    .filter(|k| !k.trim().is_empty())
                    .map(PathBuf::from),
            },
        )
    })
    .await
}

#[tauri::command]
async fn delete_profile(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_core(&state, move |core| core.remove_profile(&id)).await
}

#[tauri::command]
async fn inspect_repository(state: State<'_, AppState>, path: String) -> CmdResult<RepoReport> {
    let path = untrusted_path(&state, &path)?;
    with_core(&state, move |core| core.inspect(&path)).await
}

#[tauri::command]
async fn run_doctor(state: State<'_, AppState>, path: Option<String>) -> CmdResult<DoctorReport> {
    let path = path.map(|p| untrusted_path(&state, &p)).transpose()?;
    with_core(&state, move |core| {
        core.doctor_with(path.as_deref(), DoctorOptions::default())
    })
    .await
}

#[tauri::command]
async fn list_rules(state: State<'_, AppState>) -> CmdResult<Vec<Rule>> {
    with_core(&state, |core| core.rules()).await
}

#[tauri::command]
async fn list_history(state: State<'_, AppState>) -> CmdResult<Vec<OperationRecord>> {
    with_core(&state, |core| core.history()).await
}

/// A plan preview and the token that applies it.
#[derive(Serialize)]
struct PlanTicket {
    token: String,
    plan: Plan,
    requires_confirmation: bool,
}

fn ticket(state: &State<'_, AppState>, plan: Plan) -> PlanTicket {
    let requires_confirmation = plan.requires_confirmation();
    let token = state.pending.insert(plan.clone());
    PlanTicket {
        token,
        plan,
        requires_confirmation,
    }
}

#[tauri::command]
async fn plan_assign(
    state: State<'_, AppState>,
    path: String,
    profile: String,
    ssh: bool,
) -> CmdResult<PlanTicket> {
    let path = untrusted_path(&state, &path)?;
    let plan = with_core(&state, move |core| {
        core.plan_assign(&path, &profile, AssignOptions { ssh })
    })
    .await?;
    Ok(ticket(&state, plan))
}

#[tauri::command]
async fn plan_add_rule(
    state: State<'_, AppState>,
    directory: String,
    profile: String,
) -> CmdResult<PlanTicket> {
    let directory = untrusted_path(&state, &directory)?;
    let plan = with_core(&state, move |core| core.plan_add_rule(&directory, &profile)).await?;
    Ok(ticket(&state, plan))
}

#[tauri::command]
async fn plan_remove_rule(state: State<'_, AppState>, directory: String) -> CmdResult<PlanTicket> {
    let directory = untrusted_path(&state, &directory)?;
    let plan = with_core(&state, move |core| core.plan_remove_rule(&directory)).await?;
    Ok(ticket(&state, plan))
}

#[tauri::command]
async fn plan_undo(state: State<'_, AppState>, operation: String) -> CmdResult<PlanTicket> {
    let plan = with_core(&state, move |core| core.plan_undo(&operation)).await?;
    Ok(ticket(&state, plan))
}

#[tauri::command]
async fn apply_plan(state: State<'_, AppState>, token: String) -> CmdResult<ApplyOutcome> {
    let plan = state.pending.take(&token).ok_or_else(|| {
        CommandError::simple(
            "plan_expired",
            "This preview is no longer valid. Close it and review the change again.",
        )
    })?;
    with_core(&state, move |core| core.apply(&plan)).await
}

#[tauri::command]
fn discard_plan(state: State<'_, AppState>, token: String) {
    state.pending.discard(&token);
}

pub fn run() {
    let state = AppState {
        core: GitMate::from_env().map_err(|e| e.to_string()),
        pending: PendingPlans::new(PLAN_TTL),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_profiles,
            get_profile,
            list_ssh_keys,
            create_profile,
            update_profile,
            delete_profile,
            inspect_repository,
            run_doctor,
            list_rules,
            list_history,
            plan_assign,
            plan_add_rule,
            plan_remove_rule,
            plan_undo,
            apply_plan,
            discard_plan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GitMate");
}
