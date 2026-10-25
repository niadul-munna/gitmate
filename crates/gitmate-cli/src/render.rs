//! Human-readable output.

use std::io::IsTerminal;

use gitmate_core::doctor::DoctorReport;
use gitmate_core::git::Origin;
use gitmate_core::hook::HookStatus;
use gitmate_core::identity::Source;
use gitmate_core::journal::{OperationRecord, OperationStatus};
use gitmate_core::paths::AppPaths;
use gitmate_core::plan::{ChangeKind, Plan};
use gitmate_core::profile::Profile;
use gitmate_core::report::{ExpectedReason, Finding, RepoReport, Severity};
use gitmate_core::rules::Rule;
use gitmate_core::{ApplyOutcome, ProfileUsage};

/// ANSI styling, enabled only for terminals and when `NO_COLOR` is unset.
#[derive(Clone, Copy)]
pub struct Style {
    color: bool,
}

impl Style {
    pub fn detect() -> Style {
        Style {
            color: std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        }
    }

    fn paint(self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(self, t: &str) -> String {
        self.paint("1", t)
    }
    pub fn dim(self, t: &str) -> String {
        self.paint("2", t)
    }
    pub fn error(self, t: &str) -> String {
        self.paint("1;31", t)
    }
    pub fn warn(self, t: &str) -> String {
        self.paint("33", t)
    }
    pub fn added(self, t: &str) -> String {
        self.paint("32", t)
    }
    pub fn removed(self, t: &str) -> String {
        self.paint("31", t)
    }
}

pub fn values(values: &[String]) -> String {
    match values {
        [] => "(unset)".into(),
        [one] => format!("\"{one}\""),
        many => format!(
            "[{}]",
            many.iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn row(style: Style, label: &str, value: impl AsRef<str>) {
    println!("{}  {}", style.bold(&format!("{label:<11}")), value.as_ref());
}

pub fn plan(paths: &AppPaths, plan: &Plan, style: Style) {
    println!("{}", style.bold(&plan.summary));
    for file in plan.files() {
        println!("\n  {}", paths.display(file));
        for change in plan.changes.iter().filter(|c| c.file == file) {
            let verb = match change.kind() {
                ChangeKind::Add => "add",
                ChangeKind::Replace => "replace",
                ChangeKind::Remove => "remove",
                ChangeKind::Unchanged => "keep",
            };
            println!(
                "    {} {}",
                display_key(&change.key),
                style.dim(&format!("({verb})"))
            );
            for v in &change.before {
                if !change.after.contains(v) {
                    println!("      {}", style.removed(&format!("- {v}")));
                }
            }
            if change.before.is_empty() {
                println!("      {}", style.dim("- (unset)"));
            }
            for v in &change.after {
                if !change.before.contains(v) {
                    println!("      {}", style.added(&format!("+ {v}")));
                }
            }
            if change.after.is_empty() {
                println!("      {}", style.removed("+ (unset)"));
            }
        }
    }
    if let Some(id) = &plan.ensure_include {
        println!(
            "\n  {}",
            style.dim(&format!(
                "Includes {} (generated from profile '{id}')",
                paths.display(&paths.include_file(id))
            ))
        );
    }
    if !plan.conflicts.is_empty() {
        println!("\n{}", style.error("Conflicts (nothing will be written):"));
        for c in &plan.conflicts {
            println!("  - {}", c.message);
        }
    }
    if !plan.warnings.is_empty() {
        println!("\n{}", style.warn("Please review:"));
        for w in &plan.warnings {
            println!("  - {}", w.message);
        }
    }
}

/// Show `includeIf.<condition>.path` the way it appears in a config file.
fn display_key(key: &str) -> String {
    match key
        .strip_prefix("includeIf.")
        .and_then(|r| r.strip_suffix(".path"))
    {
        Some(condition) => format!("[includeIf \"{condition}\"] path"),
        None => key.to_string(),
    }
}

pub fn outcome(outcome: &ApplyOutcome, style: Style) {
    match &outcome.operation {
        Some(op) => println!(
            "\nDone. Recorded as {} (undo with `gitmate undo {}`).",
            style.bold(&op.id),
            op.id
        ),
        None => println!("\nNothing to change."),
    }
    if let Some(author) = &outcome.effective {
        println!(
            "Commits here will be authored as {} <{}>.",
            author.name, author.email
        );
    }
    for note in &outcome.notes {
        println!("{} {note}", style.warn("note:"));
    }
}

fn source_text(paths: &AppPaths, source: &Source) -> String {
    match source {
        Source::Environment { var } => format!("environment variable {var}"),
        Source::Config { key, scope, origin } => match origin {
            Origin::File(p) => format!("{key} in {} ({})", paths.display(p), scope.as_str()),
            Origin::CommandLine => format!("{key} from the command line / GIT_CONFIG_* ({})", scope.as_str()),
            other => format!("{key} ({other:?})"),
        },
        Source::AutoDetected => "guessed by Git from your user and host name".into(),
    }
}

/// Where a value came from, ignoring which key carried it.
fn location(source: &Source) -> String {
    match source {
        Source::Environment { .. } => "env".into(),
        Source::Config { scope, origin, .. } => format!("{scope:?}{origin:?}"),
        Source::AutoDetected => "auto".into(),
    }
}

pub fn whoami(paths: &AppPaths, report: &RepoReport, style: Style) {
    match &report.repository {
        Some(repo) => {
            let mut kind = Vec::new();
            if repo.is_bare {
                kind.push("bare".to_string());
            }
            if repo.is_linked_worktree {
                kind.push(format!(
                    "worktree of {}",
                    paths.display(repo.common_dir.parent().unwrap_or(&repo.common_dir))
                ));
            }
            let suffix = if kind.is_empty() {
                String::new()
            } else {
                format!("  ({})", kind.join(", "))
            };
            row(
                style,
                "Repository",
                format!("{}{suffix}", paths.display(repo.root())),
            );
        }
        None => row(
            style,
            "Directory",
            format!("{}  (not inside a Git repository)", paths.display(&report.path)),
        ),
    }
    match &report.identity.author {
        Some(a) => row(style, "Author", format!("{} <{}>", a.name, a.email)),
        None => row(style, "Author", style.error("not configured")),
    }
    if let Some(source) = &report.identity.email_source {
        row(style, "Email from", source_text(paths, source));
    }
    if let Some(source) = &report.identity.name_source
        && Some(location(source)) != report.identity.email_source.as_ref().map(location)
    {
        row(style, "Name from", source_text(paths, source));
    }
    if let (Some(c), Some(a)) = (&report.identity.committer, &report.identity.author)
        && c != a
    {
        row(style, "Committer", format!("{} <{}>", c.name, c.email));
    }
    match (&report.profile, &report.expected) {
        (Some(m), _) => row(
            style,
            "Profile",
            format!(
                "{}{}",
                m.id,
                m.label
                    .as_ref()
                    .map(|l| style.dim(&format!("  {l}")))
                    .unwrap_or_default()
            ),
        ),
        (None, _) => row(style, "Profile", style.dim("none matches")),
    }
    if let Some(expected) = &report.expected {
        let why = match &expected.reason {
            ExpectedReason::Assignment { operation } => format!("assigned with `use` ({operation})"),
            ExpectedReason::Rule { directory } => format!("rule for {}", paths.display(directory)),
        };
        row(style, "Expected", format!("{}  ({why})", expected.profile));
    }
    match (&report.ssh.command, &report.ssh.command_source) {
        (Some(cmd), Some(src)) => row(style, "SSH", format!("{cmd}  ({src})")),
        _ if report.repository.is_some() => row(style, "SSH", style.dim("default ssh configuration")),
        _ => {}
    }
    for remote in &report.remotes {
        for url in &remote.fetch {
            row(
                style,
                "Remote",
                format!("{}  {}  {}", remote.name, url.transport.as_str(), url.display),
            );
        }
    }
    for (key, entry) in report.credentials.iter().filter(|(k, _)| k.ends_with("helper")) {
        row(
            style,
            "Credential",
            format!("{key} = {}  ({})", entry.value, entry.scope.as_str()),
        );
    }
    findings(&report.findings, style);
}

pub fn whoami_short(report: &RepoReport) {
    if report.repository.is_none() {
        println!("not inside a Git repository");
        return;
    }
    if let Some(profile) = &report.profile {
        println!("{}", profile.id);
    } else if let Some(author) = &report.identity.author {
        println!("{} <{}>", author.name, author.email);
    } else {
        println!("(unset)");
    }
}

/// One line for a shell prompt: `format` with `{profile}`, `{name}` and
/// `{email}` filled in. Outside a repository, or when the format uses a value
/// that is not set, prints `fallback` (or nothing).
pub fn prompt(report: Option<&RepoReport>, format: &str, fallback: Option<&str>) {
    let line = report.filter(|r| r.repository.is_some()).and_then(|r| {
        let author = r.identity.author.as_ref();
        fill_placeholders(
            format,
            &[
                ("{profile}", r.profile.as_ref().map(|p| p.id.as_str())),
                ("{name}", author.map(|a| a.name.as_str())),
                ("{email}", author.map(|a| a.email.as_str())),
            ],
        )
    });
    if let Some(line) = line.as_deref().or(fallback) {
        println!("{line}");
    }
}

/// Replace placeholders in one pass, so a value is never re-scanned. `None`
/// when the template uses a placeholder whose value is missing.
fn fill_placeholders(template: &str, values: &[(&str, Option<&str>)]) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        match values
            .iter()
            .find(|(placeholder, _)| rest.starts_with(placeholder))
        {
            Some((placeholder, value)) => {
                out.push_str((*value)?);
                rest = &rest[placeholder.len()..];
            }
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

pub fn hook_status(paths: &AppPaths, status: &HookStatus, style: Style) {
    let state = match (status.installed, status.managed) {
        (false, _) => "not installed".to_string(),
        (true, true) => "GitMate's hook".to_string(),
        (true, false) => style.warn("a hook GitMate did not install"),
    };
    row(
        style,
        "Pre-commit",
        format!("{}  ({state})", paths.display(&status.path)),
    );
    if let Some(dir) = &status.hooks_path {
        row(
            style,
            "Hooks path",
            format!(
                "{}  {}",
                paths.display(dir),
                style.warn("core.hooksPath: Git runs hooks from here, not from the file above")
            ),
        );
    }
    if !status.installed && status.hooks_path.is_none() {
        println!("\nInstall it with `gitmate hook install`.");
    }
}

/// Why the pre-commit hook refused a commit (on stderr, which Git shows).
pub fn commit_refused(findings: &[Finding], style: Style) {
    eprintln!("{} GitMate refused this commit:", style.error("error:"));
    for f in findings {
        eprintln!("  {}", f.message);
        if let Some(hint) = &f.hint {
            eprintln!("  {}", style.dim(&format!("hint: {hint}")));
        }
    }
    eprintln!(
        "{}",
        style.dim(
            "Skip this check once with `git commit --no-verify`; remove it with `gitmate hook uninstall`."
        )
    );
}

pub fn findings(list: &[Finding], style: Style) {
    if list.is_empty() {
        return;
    }
    println!();
    for f in list {
        let tag = match f.severity {
            Severity::Error => style.error("error  "),
            Severity::Warning => style.warn("warning"),
            Severity::Info => style.dim("info   "),
        };
        println!("{tag}  {}  {}", f.message, style.dim(&format!("[{}]", f.code)));
        if let Some(hint) = &f.hint {
            println!("         {}", style.dim(&format!("hint: {hint}")));
        }
    }
}

pub fn profiles(paths: &AppPaths, profiles: &[Profile], style: Style) {
    if profiles.is_empty() {
        println!("No profiles yet. Create one with:");
        println!("  gitmate profile add work --name \"Your Name\" --email you@company.example");
        return;
    }
    let width = profiles.iter().map(|p| p.id.len()).max().unwrap_or(2).max(2);
    for p in profiles {
        let mut line = format!(
            "{}  {} <{}>",
            style.bold(&format!("{:<width$}", p.id)),
            p.name,
            p.email
        );
        if let Some(label) = &p.label {
            line.push_str(&style.dim(&format!("  {label}")));
        }
        if let Some(key) = &p.ssh_key {
            line.push_str(&style.dim(&format!("  ssh: {}", paths.display(&paths.expand_tilde(key)))));
        }
        println!("{line}");
    }
}

pub fn profile_usage(paths: &AppPaths, usage: &ProfileUsage, style: Style) {
    let p = &usage.profile;
    row(style, "Profile", &p.id);
    row(style, "Name", &p.name);
    row(style, "Email", &p.email);
    if let Some(label) = &p.label {
        row(style, "Label", label);
    }
    if let Some(check) = &usage.ssh_key {
        let status = check.problem().unwrap_or_else(|| "ok".into());
        row(
            style,
            "SSH key",
            format!("{}  ({status})", paths.display(&check.path)),
        );
    }
    if let Some(key) = &p.signing_key {
        row(style, "Signing key", key);
    }
    if let Some(format) = &p.gpg_format {
        row(style, "Sign format", format);
    }
    if p.sign_commits {
        row(style, "Sign commits", "yes, in folder rules");
    }
    if !p.remote_patterns.is_empty() {
        row(style, "Remotes", p.remote_patterns.join(", "));
    }
    row(style, "Include", paths.display(&paths.include_file(&p.id)));
    if usage.rules.is_empty() {
        row(style, "Rules", style.dim("none"));
    }
    for rule in &usage.rules {
        row(
            style,
            "Rule",
            rule.directory
                .as_deref()
                .map(|d| paths.display(d))
                .unwrap_or_else(|| rule.condition.clone()),
        );
    }
    for repo in &usage.repositories {
        row(style, "Assigned", paths.display(repo));
    }
}

pub fn profile_diff(paths: &AppPaths, old: &Profile, new: &Profile, rules: &[Rule], style: Style) {
    println!("{}", style.bold(&format!("Edit profile '{}'", old.id)));
    let show = |label: &str, a: String, b: String| {
        if a == b {
            println!("  {label:<8} {}", style.dim(&a));
        } else {
            println!("  {label:<8} {}  ->  {}", style.removed(&a), style.added(&b));
        }
    };
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "(none)".into());
    let key = |v: &Option<std::path::PathBuf>| {
        v.as_ref()
            .map(|k| k.display().to_string())
            .unwrap_or_else(|| "(none)".into())
    };
    show("name", old.name.clone(), new.name.clone());
    show("email", old.email.clone(), new.email.clone());
    show("label", opt(&old.label), opt(&new.label));
    show("ssh key", key(&old.ssh_key), key(&new.ssh_key));
    show("signing", opt(&old.signing_key), opt(&new.signing_key));
    show("format", opt(&old.gpg_format), opt(&new.gpg_format));
    show("sign", old.sign_commits.to_string(), new.sign_commits.to_string());
    let patterns = |v: &[String]| {
        if v.is_empty() {
            "(none)".into()
        } else {
            v.join(", ")
        }
    };
    show(
        "remotes",
        patterns(&old.remote_patterns),
        patterns(&new.remote_patterns),
    );
    if !rules.is_empty() && old != new {
        println!(
            "\n  Repositories under {} pick up the new values immediately ({} is regenerated).",
            rules
                .iter()
                .filter_map(|r| r.directory.as_deref().map(|d| paths.display(d)))
                .collect::<Vec<_>>()
                .join(", "),
            paths.display(&paths.include_file(&old.id))
        );
    }
}

pub fn rules(paths: &AppPaths, rules: &[Rule], style: Style) {
    if rules.is_empty() {
        println!("No includeIf rules in your global Git config.");
        println!("Add one with: gitmate rules add ~/work work");
        return;
    }
    for rule in rules {
        let target = rule
            .directory
            .as_deref()
            .map(|d| format!("{}/", paths.display(d)))
            .unwrap_or_else(|| rule.condition.clone());
        match &rule.profile {
            Some(id) => println!("{}  ->  {}", style.bold(&target), id),
            None => println!(
                "{}  ->  {}  {}",
                target,
                rule.include_path,
                style.dim("(not managed by GitMate)")
            ),
        }
    }
}

pub fn doctor(paths: &AppPaths, report: &DoctorReport, style: Style) {
    row(
        style,
        "Git",
        report
            .git_version
            .as_ref()
            .map(|v| v.text.clone())
            .unwrap_or_else(|| style.error("not usable")),
    );
    row(
        style,
        "Profiles",
        format!(
            "{}  ({})",
            report.profile_count,
            paths.display(&paths.profiles_file())
        ),
    );
    row(style, "Global file", paths.display(&report.global_config));
    row(
        style,
        "Rules",
        format!(
            "{} managed, {} other",
            report.rules.iter().filter(|r| r.is_managed()).count(),
            report.rules.iter().filter(|r| !r.is_managed()).count()
        ),
    );
    if let Some(repo) = &report.repository {
        row(
            style,
            "Repository",
            paths.display(repo.repository.as_ref().map(|r| r.root()).unwrap_or(&repo.path)),
        );
        if let Some(a) = &repo.identity.author {
            row(style, "Author", format!("{} <{}>", a.name, a.email));
        }
    }
    for host in &report.ssh_hosts {
        row(
            style,
            "SSH host",
            format!(
                "{} -> {}  user {}  identities-only {}  keys: {}",
                host.host,
                host.hostname.as_deref().unwrap_or("?"),
                host.user.as_deref().unwrap_or("?"),
                if host.identities_only { "yes" } else { "no" },
                host.identity_files.join(", ")
            ),
        );
    }
    if report.findings.is_empty() {
        println!("\nNo problems found.");
    } else {
        findings(&report.findings, style);
    }
}

pub fn history(history: &[OperationRecord], style: Style) {
    if history.is_empty() {
        println!("GitMate has not changed anything yet.");
        return;
    }
    for op in history.iter().rev() {
        let status = match op.status {
            OperationStatus::Applied => style.added("applied"),
            OperationStatus::Undone => style.dim("undone "),
            OperationStatus::Failed => style.error("failed "),
            OperationStatus::Pending => style.warn("pending"),
        };
        println!(
            "{}  {}  {}  {}",
            style.bold(&op.id),
            status,
            style.dim(&op.created_at_utc()),
            op.summary
        );
        if let Some(err) = &op.error {
            println!("    {}", style.dim(err));
        }
    }
}
