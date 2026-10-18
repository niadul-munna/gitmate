// Shapes returned by the Rust side (serde output of gitmate-core types).

export interface Profile {
  id: string;
  name: string;
  email: string;
  label?: string;
  ssh_key?: string;
}

export type KeyStatus =
  | { status: "ok" }
  | { status: "missing" }
  | { status: "not_a_file" }
  | { status: "permissions_too_open"; mode: number }
  | { status: "inaccessible"; message: string };

export interface KeyCheck {
  configured: string;
  path: string;
  status: KeyStatus;
  public_key_present: boolean;
}

export interface Rule {
  file: string;
  condition: string;
  directory: string | null;
  case_insensitive: boolean;
  include_path: string;
  profile: string | null;
  position: number;
}

/** SSH private keys found in ~/.ssh, by name only. */
export interface SshKeyChoices {
  /** The ~/.ssh directory, where the file picker starts. */
  dir: string;
  keys: { path: string; used_by: string[] }[];
}

export interface ProfileUsage {
  profile: Profile;
  rules: Rule[];
  repositories: string[];
  ssh_key: KeyCheck | null;
}

export type Scope = "system" | "global" | "local" | "worktree" | "command" | "unknown";

export type Origin =
  | { kind: "file"; detail: string }
  | { kind: "command_line" }
  | { kind: "standard_input" }
  | { kind: "blob"; detail: string }
  | { kind: "other"; detail: string };

export interface ConfigEntry {
  scope: Scope;
  origin: Origin;
  value: string;
}

export interface KeyReport {
  key: string;
  values: ConfigEntry[];
}

export interface EnvVar {
  name: string;
  value: string | null;
}

export type Source =
  | { kind: "environment"; var: string }
  | { kind: "config"; key: string; scope: Scope; origin: Origin }
  | { kind: "auto_detected" };

export interface Ident {
  name: string;
  email: string;
}

export interface IdentityReport {
  author: Ident | null;
  committer: Ident | null;
  problem: string | null;
  name_source: Source | null;
  email_source: Source | null;
  keys: KeyReport[];
  env: EnvVar[];
}

export interface RepoInfo {
  git_dir: string;
  common_dir: string;
  work_tree: string | null;
  is_bare: boolean;
  is_linked_worktree: boolean;
}

export interface ProfileMatch {
  id: string;
  label: string | null;
  name_matches: boolean;
}

export interface Expected {
  profile: string;
  reason: { kind: "assignment"; operation: string } | { kind: "rule"; directory: string };
}

export type Transport = "ssh" | "https" | "http" | "git" | "file" | "local" | "other";

export interface RemoteUrl {
  display: string;
  transport: Transport;
  host: string | null;
  user: string | null;
  embedded_credentials: boolean;
}

export interface Remote {
  name: string;
  fetch: RemoteUrl[];
  push: RemoteUrl[];
}

export interface SshStatus {
  command: string | null;
  command_source: string | null;
  profile_key: KeyCheck | null;
}

export type Severity = "error" | "warning" | "info";

export interface Finding {
  severity: Severity;
  code: string;
  message: string;
  hint?: string;
}

export interface RepoReport {
  path: string;
  repository: RepoInfo | null;
  identity: IdentityReport;
  profile: ProfileMatch | null;
  rule: Rule | null;
  expected: Expected | null;
  remotes: Remote[];
  ssh: SshStatus;
  credentials: [string, ConfigEntry][];
  findings: Finding[];
}

export type Action =
  | { kind: "assign"; repository: string; profile: string; ssh: boolean }
  | { kind: "add_rule"; directory: string; profile: string }
  | { kind: "remove_rule"; directory: string; profile: string | null }
  | { kind: "undo"; operation: string };

export interface ConfigChange {
  file: string;
  key: string;
  before: string[];
  after: string[];
}

export interface Issue {
  code: string;
  message: string;
}

export interface Plan {
  action: Action;
  summary: string;
  changes: ConfigChange[];
  conflicts: Issue[];
  warnings: Issue[];
  ensure_include?: string;
}

export interface PlanTicket {
  token: string;
  plan: Plan;
  requires_confirmation: boolean;
}

export type OperationStatus = "pending" | "applied" | "failed" | "undone";

export interface OperationRecord {
  id: string;
  /** Journal order; ids only have one-second resolution. */
  seq: number;
  created_at: number;
  action: Action;
  summary: string;
  status: OperationStatus;
  error?: string;
  undone_by?: string;
  changes: ConfigChange[];
  backups: { file: string; copy: string | null }[];
}

export interface ApplyOutcome {
  operation: OperationRecord | null;
  effective: Ident | null;
  notes: string[];
}

export interface GitVersion {
  major: number;
  minor: number;
  patch: number;
  text: string;
}

export interface AppPaths {
  config_dir: string;
  state_dir: string;
  home: string;
}

export interface AppInfo {
  version: string;
  git_version: GitVersion | null;
  git_error: string | null;
  paths: AppPaths;
  global_config: string;
}

export interface DoctorReport {
  git_version: GitVersion | null;
  paths: AppPaths;
  global_config: string;
  profile_count: number;
  rules: Rule[];
  repository: RepoReport | null;
  findings: Finding[];
}

export interface ProfileSaved {
  profile: Profile;
  warnings: Issue[];
}

export interface FieldError {
  field: string;
  message: string;
}

export interface CommandError {
  code: string;
  message: string;
  fields: FieldError[];
  details: string[];
}

export interface ProfileFields {
  name: string;
  email: string;
  label: string | null;
  ssh_key: string | null;
}
