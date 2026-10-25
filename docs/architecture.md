# Architecture

```
crates/gitmate-core   library: model, Git access, planning, transactions, diagnostics
crates/gitmate-cli    `gitmate` binary (clap), depends only on core
apps/desktop             Tauri 2 shell (src-tauri, depends on core) + React UI
```

The core has no dependency on Tauri, React or clap. Both front ends call the
same `GitMate` facade, so the CLI and the desktop app cannot drift apart in
behaviour.

## Principles

* **Git is the source of truth.** Effective values, origins, scopes, include
  resolution and remote URLs come from the `git` executable
  (see [git-behavior.md](git-behavior.md)). GitMate has no config parser.
* **Targeted edits only.** Every change to a Git config file is made with
  `git config --file <exact path> ...` invocations on one key. Appending a
  value uses `--add` and removing one uses `--unset-all --fixed-value`, so the
  key's other values keep their place (the order of `includeIf` sections
  decides which wins). Unrelated settings, comments and formatting are left
  to Git.
* **Plan, then apply.** Every change to Git configuration is first computed
  as a `Plan`: exact file, key, current values, proposed values, conflicts and
  warnings. Front ends render the plan; nothing is written until it is
  applied. (Profile edits are saved directly and regenerate the profile's
  include file. The optional pre-commit hook is the one file GitMate writes
  inside a repository; it is not Git configuration and not journaled.)
* **Recoverable.** Applying a plan records an operation journal entry and backs
  up affected files. `undo` reverses only GitMate's own changes and refuses
  if those keys were changed by someone else since.

## Data GitMate owns

| Path (Linux default) | Content |
| --- | --- |
| `$XDG_CONFIG_HOME/gitmate/profiles.toml` | Profiles (name, email, label, optional SSH key *path*, signing settings, remote patterns) |
| `$XDG_CONFIG_HOME/gitmate/includes/<id>.gitconfig` | Generated include file per profile, written with `git config --file` |
| `$XDG_STATE_HOME/gitmate/operations/<op>.toml` | Operation journal |
| `$XDG_STATE_HOME/gitmate/backups/<op>/` | Copies of affected config files (0600) |
| `$XDG_STATE_HOME/gitmate/lock` | Advisory lock serialising GitMate writers |

`GITMATE_CONFIG_DIR` and `GITMATE_STATE_DIR` override both locations.
Directories are created `0700`, files `0600`. All GitMate-owned files are
written to a fresh temporary file in the same directory and renamed into place.

## Core modules

| Module | Responsibility |
| --- | --- |
| `paths` | Config/state directory resolution, `~` expansion, global config target |
| `git` | Running Git with argument vectors, controlled environment, NUL-separated output parsing |
| `profile` | Profile model, validation, `ProfileStore` |
| `includes` | Generating per-profile include files |
| `repo` | Repository discovery (worktrees, bare, nested, safe.directory) |
| `identity` | Effective identity, per-key origins, environment overrides |
| `remote`, `ssh` | Remote transport classification, remote patterns, SSH key checks, `core.sshCommand` quoting, `ssh -G` |
| `hook` | The optional pre-commit hook: where Git runs hooks from, installing and removing GitMate's own, which findings refuse a commit |
| `rules` | Reading and planning `includeIf "gitdir:..."` rules |
| `plan` | `Plan`, `ConfigChange`, conflicts, warnings |
| `transaction`, `journal` | Locking, apply with verification, automatic revert on failure, undo |
| `doctor` | Diagnostics built from all of the above |

## How a change is applied

1. Take the exclusive GitMate lock.
2. Re-read the current value of every planned key from its exact file. If any
   value differs from what the plan showed, abort with a *concurrent
   modification* error. Nothing has been written.
3. Copy each affected file into `backups/<op>/`.
4. Write the journal entry with status `pending` and the next sequence
   number (operation ids only have one-second resolution, so the sequence
   number is what orders the history).
5. Re-read every planned key once more. If anything changed while steps 3–4
   ran, mark the operation `failed` with nothing written and abort.
6. Apply each key change with `git config --file`. Git takes its own
   `<file>.lock` for each write.
7. If any write fails, revert the keys already changed (reverse order),
   verify, and mark the operation `failed`.
8. Read every key back. Mark the operation `applied` only if all values
   match. For an undo, mark the original operation `undone` before releasing
   the lock.

**Undo** goes newest first: it refuses while a later operation that changed
one of the same keys is still applied, because matching values cannot show
whose write is in effect. It then checks that every key still holds exactly
the value GitMate wrote. Any difference is a conflict and nothing is
restored. Otherwise it restores
the recorded previous values key by key, verifies them, and records the undo
as its own operation. Backups exist for manual recovery and are never used for
automatic restore, because copying a whole file back would discard unrelated
edits.

## Desktop trust boundary

The webview is treated as untrusted. Tauri commands take plain data (paths,
profile ids, form fields), validate it in Rust, and return typed views. A
mutation is a two-step exchange: `plan_*` returns a preview and an opaque plan
token, and `apply_plan(token)` applies only a plan the Rust side computed and
still holds. The frontend can never submit file paths or config keys to
write. No shell, filesystem or opener plugin is enabled. The only plugin
permission is the native folder picker (`dialog:allow-open`), and its result
is validated like any other input.
