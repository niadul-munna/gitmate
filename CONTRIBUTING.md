# Contributing to GitMate

Thanks for helping. GitMate edits people's Git configuration, so the bar
for correctness is high and changes come with tests.

## Ground rules

* **Git is the source of truth.** Read effective values, origins and remote
  URLs through the `git` executable. Do not add a Git config parser or
  re-implement precedence rules (see [docs/git-behavior.md](docs/git-behavior.md)).
* **Every write is a plan first.** New mutations produce a `Plan` that front
  ends preview; they go through `GitMate::apply`, which handles locking,
  re-checking, journaling, revert and undo. Never write Git config directly.
* **No shell strings.** Run processes with argument vectors.
* **Never touch the real HOME in tests.** Use the `Sandbox` in
  `crates/gitmate-core/tests/common/mod.rs` (Rust) or the IPC mocks in
  `apps/desktop/src/test/` (UI). CI fails loudly if a test escapes.
* **Keep dependencies minimal** and explain new ones in the pull request.

## Development setup

Requirements: Rust (stable, 1.90 or newer), Git 2.31 or newer, and for the
desktop app Node.js 24, pnpm 11 and the
[Tauri Linux prerequisites](https://tauri.app/start/prerequisites/)
(`libwebkit2gtk-4.1-dev`, `libssl-dev`, `libxdo-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `build-essential`).

```sh
# Core library and CLI (no system libraries needed)
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test

# Desktop frontend
cd apps/desktop
pnpm install --frozen-lockfile
pnpm typecheck && pnpm lint && pnpm test && pnpm build

# Desktop app (needs the Tauri prerequisites)
cargo clippy -p gitmate-desktop --all-targets -- -D warnings
pnpm tauri dev
```

The workspace's `default-members` exclude the desktop crate, so `cargo test`
works on machines without WebKitGTK. CI builds the desktop crate separately.

## Tests

* Write the failing test first, then the code.
* Core behaviour is tested against real temporary Git repositories:
  `tests/inspect.rs`, `assign.rs`, `rules.rs`, `rollback.rs`, `profiles.rs`.
* The CLI is tested end to end by running the built binary in a temporary
  HOME (`crates/gitmate-cli/tests/cli.rs`).
* UI tests (Vitest + Testing Library) mock only the IPC boundary and query by
  role and label, which also keeps the UI accessible.

## Pull requests

* Keep changes focused; describe the user-visible effect and how you tested it.
* Update `docs/` when behaviour or a security decision changes.
* Make sure `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, and the
  frontend checks all pass.

By contributing you agree that your contributions are licensed under the MIT
license (see [LICENSE](LICENSE)), and you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
