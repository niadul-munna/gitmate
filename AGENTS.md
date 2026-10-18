# AGENTS.md

Guidance for AI coding assistants working in this repository.

GitMate is a local Git identity manager. Profiles (name, email, optional SSH
key path) are applied per repository (`.git/config`) or per folder
(`includeIf "gitdir:…"` in the global Git config). Every Git config change is
previewed, journaled and undoable. One Rust core library serves two front
ends: the `gitmate` CLI and a Tauri 2 + React desktop app. Read
`docs/architecture.md`, `docs/git-behavior.md` and `SECURITY.md` before
changing how configuration is read or written.

## Commands

Requirements: Rust 1.90+ (edition 2024, pinned in `rust-toolchain.toml`),
Git 2.31+, and for the desktop app Node.js 24, pnpm 11 and the Tauri Linux
libraries listed in `CONTRIBUTING.md`.

```sh
# Core + CLI. The workspace's default-members exclude the desktop crate,
# so these work without WebKitGTK.
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test

# One test file, one test, or one module's unit tests
cargo test -p gitmate-core --test rollback
cargo test -p gitmate-core --test rollback undo_is_refused
cargo test -p gitmate-core --lib rules::tests
cargo test -p gitmate-cli --test cli

# Desktop frontend (run in apps/desktop)
pnpm install --frozen-lockfile
pnpm typecheck && pnpm lint && pnpm test && pnpm build
pnpm exec vitest run src/components/ProfileForm.test.tsx -t "fills the key field"

# Desktop app
cargo clippy -p gitmate-desktop --all-targets -- -D warnings
pnpm tauri dev                     # in apps/desktop
pnpm tauri build --bundles deb     # .deb lands in target/release/bundle/deb/
```

A plain `cargo build -p gitmate-desktop` loads the UI from the Vite dev
server (`devUrl`). For a binary with the frontend embedded, run `pnpm build`
first and add `--features tauri/custom-protocol`, or use `pnpm tauri build`.

CI (`.github/workflows/ci.yml`) runs these same checks and fails if a test
wrote to the runner's real Git config.

## Architecture

- `crates/gitmate-core` holds all behaviour and depends on no front-end
  crate. The `GitMate` facade (`src/service.rs`) is the only entry point both
  front ends use, so the CLI and the app cannot behave differently.
- `crates/gitmate-cli`: clap commands (`src/main.rs`) and text rendering
  (`src/render.rs`); `--json` prints the core's serde types.
- `apps/desktop/src-tauri`: thin Tauri commands over the facade
  (`src/lib.rs`).
- `apps/desktop/src`: React UI. Components reach the backend only through
  `src/lib/api.ts`; `src/lib/types.ts` mirrors the Rust serde output by hand,
  so keep the two in step.

How a Git config change flows:

1. `GitMate::plan_*` builds a `Plan` (`plan.rs`): exact file, key,
   before/after values, conflicts and warnings. Nothing is written.
2. The front end shows the plan. The desktop app also gets an opaque token
   (`pending.rs`) and can only apply a plan the Rust side computed and holds.
3. `GitMate::apply` → `Transactions::apply` (`transaction.rs`): writer lock,
   re-check every key (again just before writing), back up files, journal as
   `pending`, write with `git config --file`, read back, then mark `applied`,
   or revert and mark `failed`.
4. Undo builds the inverse plan. It refuses while a later operation changed
   the same key, and when a key no longer holds the value GitMate wrote.

Other key pieces:

- `config.rs`: targeted reads and writes. Appending uses `--add` and removing
  uses `--unset-all --fixed-value`, so a key's other values keep their
  position; this matters because the last matching `includeIf` wins.
- `rules.rs`: a folder rule is `includeIf.gitdir:<dir>/.path` pointing at the
  profile's generated include file (`includes.rs`). Rule directories are
  canonicalised, and glob characters are rejected before and after symlink
  resolution.
- `identity.rs`, `report.rs`, `doctor.rs`: which identity Git will use, where
  each value comes from, and what is wrong.
- `journal.rs`: one TOML file per operation, ordered by `seq`. Operation ids
  only have one-second resolution, so never order by id.

## Rules

- Git is the source of truth. Never parse Git config files or reimplement
  precedence; ask `git` (`git config -z --show-origin --show-scope`,
  `git var`, `git rev-parse`).
- Every write to Git config goes through a `Plan` and `GitMate::apply`. Only
  `user.name`, `user.email`, `core.sshCommand` and `includeIf.gitdir*.path`
  may be written; `plan::is_managed_key` enforces this at apply time. Profile
  edits are the one exception: they save `profiles.toml` and regenerate the
  include file directly.
- Run processes with argument vectors and put `--` before values. Never build
  shell strings; the only shell text GitMate produces is `core.sshCommand`,
  via `ssh::ssh_command_for_key` with single quoting.
- SSH private keys are inspected by file name and metadata only; never open
  them. Redact credentials in anything shown (see `remote.rs`).
- Tests never touch the real HOME. Rust tests use `Sandbox`
  (`crates/gitmate-core/tests/common/mod.rs`): a temporary HOME whose path
  contains a space and non-ASCII characters, with system config disabled and
  identity and repository-redirect variables removed. UI tests mock only the
  IPC boundary (`mockBackend` in `apps/desktop/src/test/fixtures.ts`, where
  unmocked commands reject) and query by role and label.
- Write the failing test first. Core behaviour is tested against real
  temporary Git repositories, not mocks.
- A new desktop command needs all of: `COMMANDS` in `src-tauri/build.rs`, its
  `allow-*` permission in `src-tauri/capabilities/default.json`,
  `generate_handler!` in `src-tauri/src/lib.rs`, the generated
  `src-tauri/permissions/autogenerated/*.toml` (committed), and the command
  count in `SECURITY.md`.
- When behaviour or a security decision changes, update `docs/` and
  `SECURITY.md` in the same change.

## Conventions

- Commit subjects are imperative and prefixed with the area when there is
  one: `core: …`, `cli: …`, `desktop: …`, `docs: …`, `ci: …`.
- `docs/releasing.md` lists every file that carries the version number.
