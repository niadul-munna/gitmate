import type { Origin, Scope, Source } from "./types";

/** Show a path with the home directory as `~`. */
export function displayPath(path: string, home: string | undefined): string {
  if (!home) return path;
  const trimmed = home.replace(/\/+$/, "");
  if (path === trimmed) return "~";
  if (path.startsWith(trimmed + "/")) return "~" + path.slice(trimmed.length);
  return path;
}

export function originText(origin: Origin, home?: string): string {
  switch (origin.kind) {
    case "file":
      return displayPath(origin.detail, home);
    case "command_line":
      return "command line or GIT_CONFIG_* variables";
    case "standard_input":
      return "standard input";
    case "blob":
      return `blob ${origin.detail}`;
    case "other":
      return origin.detail;
  }
}

const scopeNames: Record<Scope, string> = {
  system: "System",
  global: "Global",
  local: "Repository",
  worktree: "Worktree",
  command: "Command",
  unknown: "Other",
};

export function scopeName(scope: Scope): string {
  return scopeNames[scope];
}

export function sourceText(source: Source, home?: string): string {
  switch (source.kind) {
    case "environment":
      return `${source.var} environment variable`;
    case "config":
      return `${source.key} in ${originText(source.origin, home)}`;
    case "auto_detected":
      return "guessed by Git from your user and host name";
  }
}

/** `2026-10-15 11:03` in local time. */
export function formatTime(unixSeconds: number): string {
  const d = new Date(unixSeconds * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Show `includeIf.<cond>.path` the way it appears in a config file. */
export function displayKey(key: string): string {
  const match = /^includeIf\.(.*)\.path$/.exec(key);
  return match ? `[includeIf "${match[1]}"] path` : key;
}

export function pluralize(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
