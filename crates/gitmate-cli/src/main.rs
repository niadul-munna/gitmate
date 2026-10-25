//! `gitmate`: manage Git identities from the command line.

mod render;

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use gitmate_core::doctor::DoctorOptions;
use gitmate_core::plan::{AssignOptions, Plan};
use gitmate_core::profile::{Profile, ProfileInput};
use gitmate_core::report::Severity;
use gitmate_core::{Error, GitMate};

use crate::render::Style;

#[derive(Parser)]
#[command(
    name = "gitmate",
    version,
    about = "One machine. Every Git identity. Zero confusion.",
    long_about = "GitMate manages Git commit identities (user.name / user.email) per repository \
                  and per directory, shows exactly which configuration Git uses and why, and \
                  previews every change before writing it.\n\n\
                  Changing the commit identity does not change which account authenticates a push: \
                  that is decided by your SSH key or HTTPS credential helper.",
    after_help = "Environment:\n  \
                  GITMATE_CONFIG_DIR  Override where profiles are stored\n  \
                  GITMATE_STATE_DIR   Override where history and backups are stored\n  \
                  NO_COLOR            Disable colored output"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create, inspect, change and delete profiles
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Show the identity Git uses here and where it comes from
    Whoami {
        /// Directory to inspect (default: current directory)
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// One-line summary (profile id or author name and email)
        #[arg(long, short = 's')]
        short: bool,
    },
    /// Print the active profile for a shell prompt (silent outside repositories)
    Prompt {
        /// Directory to inspect (default: current directory)
        path: Option<PathBuf>,
        /// Output with {profile}, {name} and {email} filled in
        #[arg(long, short = 'f', default_value = "{profile}")]
        format: String,
        /// Printed instead outside a repository, or when a value the format
        /// uses is not set
        #[arg(long)]
        fallback: Option<String>,
    },
    /// Assign a profile to the current repository (writes .git/config)
    Use {
        /// Profile id
        profile: String,
        /// Repository directory (default: current directory)
        #[arg(long, value_name = "PATH")]
        repo: Option<PathBuf>,
        /// Also pin the profile's SSH key with core.sshCommand for this repository
        #[arg(long)]
        ssh: bool,
        /// Also configure commit signing (user.signingKey, gpg.format, commit.gpgsign)
        #[arg(long)]
        sign: bool,
        #[command(flatten)]
        apply: ApplyFlags,
    },
    /// Manage directory rules (includeIf "gitdir:..." in your global config)
    #[command(subcommand)]
    Rules(RulesCommand),
    /// Check profiles, rules and (optionally) a repository for problems
    Doctor {
        /// Repository directory to check as well (default: current directory if it is a repository)
        path: Option<PathBuf>,
        /// Also resolve SSH remote hosts with `ssh -G` (reads ~/.ssh/config; never connects)
        #[arg(long)]
        ssh_config: bool,
        #[arg(long)]
        json: bool,
    },
    /// List changes GitMate has made
    History {
        #[arg(long)]
        json: bool,
    },
    /// Reverse a change GitMate made (default: the most recent one)
    Undo {
        /// Operation id from `gitmate history`
        operation: Option<String>,
        #[command(flatten)]
        apply: ApplyFlags,
    },
}

#[derive(Args, Clone, Copy)]
struct ApplyFlags {
    /// Show the planned change without writing anything
    #[arg(long)]
    dry_run: bool,
    /// Apply without asking for confirmation (for scripts)
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// Create a profile
    Add {
        /// Short id: lowercase letters, digits, '-' and '_'
        id: String,
        /// Commit author name (user.name)
        #[arg(long)]
        name: String,
        /// Commit author email (user.email)
        #[arg(long)]
        email: String,
        /// Description shown in lists, e.g. "Acme (work GitHub)"
        #[arg(long)]
        label: Option<String>,
        /// Path to this identity's SSH private key (only the path is stored)
        #[arg(long, value_name = "PATH")]
        ssh_key: Option<PathBuf>,
        /// Signing key for user.signingKey: GPG key id or SSH key path
        #[arg(long, value_name = "KEY")]
        signing_key: Option<String>,
        /// Signature format for gpg.format: openpgp, ssh or x509
        #[arg(long, value_name = "FORMAT")]
        gpg_format: Option<String>,
        /// Folder rules for this profile also set commit.gpgsign = true
        #[arg(long)]
        sign_commits: bool,
    },
    /// List profiles
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show a profile with its rules and repositories
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Change a profile's fields
    Edit {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long, conflicts_with = "clear_label")]
        label: Option<String>,
        #[arg(long)]
        clear_label: bool,
        #[arg(long, value_name = "PATH", conflicts_with = "clear_ssh_key")]
        ssh_key: Option<PathBuf>,
        #[arg(long)]
        clear_ssh_key: bool,
        #[arg(long, value_name = "KEY", conflicts_with = "clear_signing_key")]
        signing_key: Option<String>,
        #[arg(long)]
        clear_signing_key: bool,
        #[arg(long, value_name = "FORMAT", conflicts_with = "clear_gpg_format")]
        gpg_format: Option<String>,
        #[arg(long)]
        clear_gpg_format: bool,
        #[arg(long, conflicts_with = "no_sign_commits")]
        sign_commits: bool,
        #[arg(long)]
        no_sign_commits: bool,
        #[command(flatten)]
        apply: ApplyFlags,
    },
    /// Delete a profile
    Remove {
        id: String,
        #[command(flatten)]
        apply: ApplyFlags,
    },
}

#[derive(Subcommand)]
enum RulesCommand {
    /// Use a profile for every repository under a directory
    Add {
        /// Directory (repositories anywhere below it match)
        dir: PathBuf,
        /// Profile id
        profile: String,
        #[command(flatten)]
        apply: ApplyFlags,
    },
    /// List includeIf rules in your global Git config
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove the GitMate rule for a directory
    Remove {
        dir: PathBuf,
        #[command(flatten)]
        apply: ApplyFlags,
    },
}

/// Failure that has already been reported to the user.
struct Reported;

type CliResult = Result<ExitCode, Reported>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::detect();
    let json = matches!(
        &cli.command,
        Command::Whoami { json: true, .. }
            | Command::Doctor { json: true, .. }
            | Command::History { json: true }
            | Command::Profile(ProfileCommand::List { json: true } | ProfileCommand::Show { json: true, .. })
            | Command::Rules(RulesCommand::List { json: true })
    );
    let app = match GitMate::from_env() {
        Ok(app) => app,
        Err(e) => {
            report_error(&e, json, style);
            return ExitCode::FAILURE;
        }
    };
    match run(&app, cli.command, style) {
        Ok(code) => code,
        Err(Reported) => ExitCode::FAILURE,
    }
}

fn run(app: &GitMate, command: Command, style: Style) -> CliResult {
    let fail = |e: Error, json: bool| {
        report_error(&e, json, style);
        Reported
    };
    match command {
        Command::Profile(cmd) => profile_command(app, cmd, style),
        Command::Whoami { path, json, short } => {
            let dir = resolve(app, path.as_deref());
            let report = app.inspect(&dir).map_err(|e| fail(e, json))?;
            if json {
                print_json(&report);
            } else if short {
                render::whoami_short(&report);
            } else {
                render::whoami(app.paths(), &report, style);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Prompt {
            path,
            format,
            fallback,
        } => {
            // A prompt is redrawn constantly; errors only set the exit code.
            let dir = resolve(app, path.as_deref());
            let report = app.inspect(&dir);
            render::prompt(report.as_ref().ok(), &format, fallback.as_deref());
            Ok(if report.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Use {
            profile,
            repo,
            ssh,
            sign,
            apply,
        } => {
            let dir = resolve(app, repo.as_deref());
            let plan = app
                .plan_assign(&dir, &profile, AssignOptions { ssh, sign })
                .map_err(|e| fail(e, false))?;
            apply_plan(app, &plan, apply, style)
        }
        Command::Rules(RulesCommand::Add { dir, profile, apply }) => {
            let plan = app
                .plan_add_rule(&resolve(app, Some(&dir)), &profile)
                .map_err(|e| fail(e, false))?;
            apply_plan(app, &plan, apply, style)
        }
        Command::Rules(RulesCommand::Remove { dir, apply }) => {
            let plan = app
                .plan_remove_rule(&resolve(app, Some(&dir)))
                .map_err(|e| fail(e, false))?;
            apply_plan(app, &plan, apply, style)
        }
        Command::Rules(RulesCommand::List { json }) => {
            let rules = app.rules().map_err(|e| fail(e, json))?;
            if json {
                print_json(&rules);
            } else {
                render::rules(app.paths(), &rules, style);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor {
            path,
            ssh_config,
            json,
        } => {
            let dir = match path {
                Some(p) => Some(resolve(app, Some(&p))),
                None => std::env::current_dir()
                    .ok()
                    .filter(|d| gitmate_core::repo::discover(app.git(), d).is_ok_and(|r| r.is_some())),
            };
            let report = app
                .doctor_with(dir.as_deref(), DoctorOptions { ssh_config })
                .map_err(|e| fail(e, json))?;
            if json {
                print_json(&report);
            } else {
                render::doctor(app.paths(), &report, style);
            }
            Ok(if report.worst() == Some(Severity::Error) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::History { json } => {
            let history = app.history().map_err(|e| fail(e, json))?;
            if json {
                print_json(&history);
            } else {
                render::history(&history, style);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Undo { operation, apply } => {
            let id = match operation {
                Some(id) => id,
                None => match app.last_undoable().map_err(|e| fail(e, false))? {
                    Some(op) => op.id,
                    None => {
                        println!("Nothing to undo.");
                        return Ok(ExitCode::SUCCESS);
                    }
                },
            };
            let plan = app.plan_undo(&id).map_err(|e| fail(e, false))?;
            apply_plan(app, &plan, apply, style)
        }
    }
}

fn profile_command(app: &GitMate, cmd: ProfileCommand, style: Style) -> CliResult {
    let fail = |e: Error, json: bool| {
        report_error(&e, json, style);
        Reported
    };
    match cmd {
        ProfileCommand::Add {
            id,
            name,
            email,
            label,
            ssh_key,
            signing_key,
            gpg_format,
            sign_commits,
        } => {
            let saved = app
                .add_profile(Profile {
                    id,
                    name,
                    email,
                    label,
                    ssh_key: ssh_key.map(key_path),
                    signing_key,
                    gpg_format,
                    sign_commits,
                })
                .map_err(|e| fail(e, false))?;
            println!("Created profile '{}'.", saved.profile.id);
            for w in &saved.warnings {
                println!("{} {}", style.warn("warning:"), w.message);
            }
            println!(
                "Next: `gitmate use {id}` in a repository, or `gitmate rules add <dir> {id}`.",
                id = saved.profile.id
            );
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::List { json } => {
            let profiles = app.profiles().map_err(|e| fail(e, json))?;
            if json {
                print_json(&profiles);
            } else {
                render::profiles(app.paths(), &profiles, style);
            }
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Show { id, json } => {
            let usage = app.profile_usage(&id).map_err(|e| fail(e, json))?;
            if json {
                print_json(&usage);
            } else {
                render::profile_usage(app.paths(), &usage, style);
            }
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Edit {
            id,
            name,
            email,
            label,
            clear_label,
            ssh_key,
            clear_ssh_key,
            signing_key,
            clear_signing_key,
            gpg_format,
            clear_gpg_format,
            sign_commits,
            no_sign_commits,
            apply,
        } => {
            let usage = app.profile_usage(&id).map_err(|e| fail(e, false))?;
            let old = usage.profile.clone();
            let input = ProfileInput {
                name: name.unwrap_or_else(|| old.name.clone()),
                email: email.unwrap_or_else(|| old.email.clone()),
                label: if clear_label {
                    None
                } else {
                    label.or_else(|| old.label.clone())
                },
                ssh_key: if clear_ssh_key {
                    None
                } else {
                    ssh_key.map(key_path).or_else(|| old.ssh_key.clone())
                },
                signing_key: if clear_signing_key {
                    None
                } else {
                    signing_key.or_else(|| old.signing_key.clone())
                },
                gpg_format: if clear_gpg_format {
                    None
                } else {
                    gpg_format.or_else(|| old.gpg_format.clone())
                },
                sign_commits: (sign_commits || old.sign_commits) && !no_sign_commits,
            };
            let new = Profile::from_input(&id, input.clone());
            render::profile_diff(app.paths(), &old, &new, &usage.rules, style);
            if apply.dry_run {
                println!("\n{}", style.dim("(dry run: nothing was changed)"));
                return Ok(ExitCode::SUCCESS);
            }
            if !usage.rules.is_empty() && old != new && !confirm(apply, style)? {
                return Ok(ExitCode::FAILURE);
            }
            let saved = app.update_profile(&id, input).map_err(|e| fail(e, false))?;
            for w in &saved.warnings {
                println!("{} {}", style.warn("warning:"), w.message);
            }
            println!("Saved profile '{id}'.");
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Remove { id, apply } => {
            let usage = app.profile_usage(&id).map_err(|e| fail(e, false))?;
            println!(
                "Delete profile '{id}' ({} <{}>)?",
                usage.profile.name, usage.profile.email
            );
            println!(
                "  Removes it from {} and deletes {}.",
                app.paths().display(&app.paths().profiles_file()),
                app.paths().display(&app.paths().include_file(&id))
            );
            if !usage.repositories.is_empty() {
                println!(
                    "  Repositories assigned with `use` keep their .git/config values: {}",
                    usage
                        .repositories
                        .iter()
                        .map(|p| app.paths().display(p))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if apply.dry_run {
                println!("\n{}", style.dim("(dry run: nothing was changed)"));
                return Ok(ExitCode::SUCCESS);
            }
            if !confirm(apply, style)? {
                return Ok(ExitCode::FAILURE);
            }
            app.remove_profile(&id).map_err(|e| fail(e, false))?;
            println!("Deleted profile '{id}'.");
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Show a plan, ask for confirmation when needed, and apply it.
fn apply_plan(app: &GitMate, plan: &Plan, flags: ApplyFlags, style: Style) -> CliResult {
    render::plan(app.paths(), plan, style);
    if plan.is_noop() {
        if plan.conflicts.is_empty() {
            println!("Already up to date; nothing to change.");
            return Ok(ExitCode::SUCCESS);
        }
        return Ok(ExitCode::FAILURE);
    }
    if !plan.conflicts.is_empty() {
        eprintln!(
            "{} the plan has conflicts; nothing was changed.",
            style.error("error:")
        );
        return Err(Reported);
    }
    if flags.dry_run {
        println!("\n{}", style.dim("(dry run: nothing was changed)"));
        return Ok(ExitCode::SUCCESS);
    }
    if plan.requires_confirmation() && !confirm(flags, style)? {
        return Ok(ExitCode::FAILURE);
    }
    match app.apply(plan) {
        Ok(outcome) => {
            render::outcome(&outcome, style);
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            report_error(&e, false, style);
            Err(Reported)
        }
    }
}

/// Ask "Apply? [y/N]" on a terminal. Without a terminal, refuse unless
/// `--yes` was given.
fn confirm(flags: ApplyFlags, style: Style) -> Result<bool, Reported> {
    if flags.yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        eprintln!(
            "{} confirmation required, but stdin is not a terminal. Review the plan above and re-run with --yes to apply it, or --dry-run to only preview.",
            style.error("error:")
        );
        return Err(Reported);
    }
    print!("\nApply these changes? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return Err(Reported);
    }
    let yes = matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if !yes {
        println!("Cancelled; nothing was changed.");
    }
    Ok(yes)
}

/// Make a user-supplied path absolute against the current directory.
fn resolve(app: &GitMate, path: Option<&Path>) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match path {
        None => cwd,
        Some(p) if p.starts_with("~") => app.paths().expand_tilde(p),
        Some(p) if p.is_absolute() => p.to_path_buf(),
        Some(p) => cwd.join(p),
    }
}

/// SSH key paths are stored as given when absolute or `~/...`; relative paths
/// are made absolute against the current directory.
fn key_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() || path.starts_with("~") {
        path
    } else {
        std::env::current_dir().map(|cwd| cwd.join(&path)).unwrap_or(path)
    }
}

fn print_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("error: could not encode JSON: {e}"),
    }
}

fn report_error(error: &Error, json: bool, style: Style) {
    if json {
        let body = serde_json::json!({ "error": { "code": error.code(), "message": error.to_string() } });
        eprintln!("{body}");
        return;
    }
    eprintln!("{} {error}", style.error("error:"));
    match error {
        Error::InvalidProfile(fields) => {
            for f in fields {
                eprintln!("  {}: {}", f.field, f.message);
            }
        }
        Error::ConcurrentModification(list) | Error::RollbackConflict { conflicts: list, .. } => {
            for m in list {
                eprintln!(
                    "  {} in {}: expected {}, found {}",
                    m.key,
                    m.file.display(),
                    render::values(&m.expected),
                    render::values(&m.found)
                );
            }
        }
        Error::PlanHasConflicts(messages) => {
            for m in messages {
                eprintln!("  {m}");
            }
        }
        _ => {}
    }
}
