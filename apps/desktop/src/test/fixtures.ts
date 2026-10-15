// Test fixtures shaped like real backend responses. Used only in tests.

import { invoke } from "@tauri-apps/api/core";
import { vi } from "vitest";

import type { AppInfo, PlanTicket, Profile, RepoReport, Rule } from "../lib/types";

export const HOME = "/home/ada";

export const appInfo: AppInfo = {
  version: "0.1.0",
  git_version: { major: 2, minor: 43, patch: 0, text: "2.43.0" },
  git_error: null,
  paths: {
    config_dir: `${HOME}/.config/gitmate`,
    state_dir: `${HOME}/.local/state/gitmate`,
    home: HOME,
  },
  global_config: `${HOME}/.gitconfig`,
};

export const workProfile: Profile = {
  id: "work",
  name: "Ada Lovelace",
  email: "ada@acme.example",
  label: "Acme (work GitHub)",
  ssh_key: "~/.ssh/id_acme",
};

export const personalProfile: Profile = {
  id: "personal",
  name: "Ada Lovelace",
  email: "ada@home.example",
};

export const workRule: Rule = {
  file: `${HOME}/.gitconfig`,
  condition: `gitdir:${HOME}/work/`,
  directory: `${HOME}/work`,
  case_insensitive: false,
  include_path: `${HOME}/.config/gitmate/includes/work.gitconfig`,
  profile: "work",
  position: 2,
};

export const handmadeRule: Rule = {
  file: `${HOME}/.gitconfig`,
  condition: `gitdir:${HOME}/oss/`,
  directory: `${HOME}/oss`,
  case_insensitive: false,
  include_path: `${HOME}/oss.inc`,
  profile: null,
  position: 3,
};

export const repoReport: RepoReport = {
  path: `${HOME}/work/api`,
  repository: {
    git_dir: `${HOME}/work/api/.git`,
    common_dir: `${HOME}/work/api/.git`,
    work_tree: `${HOME}/work/api`,
    is_bare: false,
    is_linked_worktree: false,
  },
  identity: {
    author: { name: "Ada Lovelace", email: "wrong@example.com" },
    committer: { name: "Ada Lovelace", email: "wrong@example.com" },
    problem: null,
    name_source: {
      kind: "config",
      key: "user.name",
      scope: "global",
      origin: { kind: "file", detail: `${HOME}/.config/gitmate/includes/work.gitconfig` },
    },
    email_source: {
      kind: "config",
      key: "user.email",
      scope: "local",
      origin: { kind: "file", detail: `${HOME}/work/api/.git/config` },
    },
    keys: [
      {
        key: "user.email",
        values: [
          { scope: "global", origin: { kind: "file", detail: `${HOME}/.gitconfig` }, value: "ada@home.example" },
          {
            scope: "global",
            origin: { kind: "file", detail: `${HOME}/.config/gitmate/includes/work.gitconfig` },
            value: "ada@acme.example",
          },
          { scope: "local", origin: { kind: "file", detail: `${HOME}/work/api/.git/config` }, value: "wrong@example.com" },
        ],
      },
    ],
    env: [],
  },
  profile: null,
  rule: workRule,
  expected: { profile: "work", reason: { kind: "rule", directory: `${HOME}/work` } },
  remotes: [
    {
      name: "origin",
      fetch: [{ display: "git@github.com:acme/api.git", transport: "ssh", host: "github.com", user: "git", embedded_credentials: false }],
      push: [{ display: "git@github.com:acme/api.git", transport: "ssh", host: "github.com", user: "git", embedded_credentials: false }],
    },
  ],
  ssh: { command: null, command_source: null, profile_key: null },
  credentials: [],
  findings: [
    {
      severity: "error",
      code: "profile_mismatch",
      message:
        "Commits here would be authored as Ada Lovelace <wrong@example.com>, but GitMate expects profile 'work' (ada@acme.example) because the rule for ~/work covers it.",
      hint: "Run `gitmate use work` to assign it to this repository",
    },
  ],
};

export const assignTicket: PlanTicket = {
  token: "tok-123",
  requires_confirmation: true,
  plan: {
    action: { kind: "assign", repository: `${HOME}/work/api`, profile: "work", ssh: false },
    summary: "Assign profile 'work' to ~/work/api",
    changes: [
      { file: `${HOME}/work/api/.git/config`, key: "user.name", before: [], after: ["Ada Lovelace"] },
      { file: `${HOME}/work/api/.git/config`, key: "user.email", before: ["wrong@example.com"], after: ["ada@acme.example"] },
    ],
    conflicts: [],
    warnings: [{ code: "env_override", message: "GIT_AUTHOR_EMAIL is set in this environment and will still override the repository config" }],
  },
};

type Handler = (args: Record<string, unknown> | undefined) => unknown;

/** Route mocked `invoke` calls by command name. Unknown commands reject. */
export function mockBackend(handlers: Record<string, Handler | unknown>) {
  const mocked = vi.mocked(invoke);
  mocked.mockImplementation(async (cmd: string, args?: unknown) => {
    if (!(cmd in handlers)) throw { code: "unmocked", message: `unmocked command ${cmd}`, fields: [], details: [] };
    const handler = handlers[cmd];
    return typeof handler === "function" ? (handler as Handler)(args as Record<string, unknown>) : handler;
  });
  return mocked;
}
