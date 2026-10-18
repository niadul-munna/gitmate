# Releasing

GitMate is released from tags. Nothing is published automatically: the
`Release build` workflow builds and uploads artifacts to the workflow run, and
a maintainer attaches them to a GitHub release by hand.

## Before the first public release

Decisions the maintainers make once, before the first public release:

- [x] `repository` in `[workspace.package]` in `Cargo.toml` is
      `https://github.com/niadul-munna/gitmate`.
- [x] The bundle identifier in `apps/desktop/src-tauri/tauri.conf.json` is
      `io.github.niadul-munna.gitmate`. It must never change after release:
      the app's data directory is named after it.
- [x] The enforcement contact in `CODE_OF_CONDUCT.md` is
      niadulislammunna@gmail.com.
- [x] *Private vulnerability reporting* is enabled in the GitHub repository
      settings (SECURITY.md points to it).

## Each release

1. Make sure `main` is green in CI.
2. Update the version in **all** of:
   - `Cargo.toml` (`[workspace.package] version`, and the
     `gitmate-core` version in `[workspace.dependencies]`)
   - `apps/desktop/package.json`
   - `apps/desktop/src-tauri/tauri.conf.json`
3. Run the full local check:

   ```sh
   cargo fmt --all --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   cargo clippy -p gitmate-desktop --all-targets -- -D warnings
   (cd apps/desktop && pnpm install --frozen-lockfile && pnpm typecheck && pnpm lint && pnpm test && pnpm build)
   ```

4. Smoke-test on a real machine, in a throwaway HOME:

   ```sh
   cargo build --release -p gitmate-cli
   GP="$PWD/target/release/gitmate"
   export HOME=$(mktemp -d) XDG_CONFIG_HOME= XDG_STATE_HOME=
   "$GP" profile add work --name "Test" --email test@example.com
   git init "$HOME/work/app" && cd "$HOME/work/app"
   "$GP" rules add ~/work work --yes
   "$GP" whoami
   "$GP" undo --yes
   ```

5. Commit the version bump, tag it `vX.Y.Z`, and push the tag.
6. Download the `gitmate-linux-x86_64` artifact from the *Release build*
   run. It contains the CLI binary, the `.deb`, the `.AppImage` and
   `SHA256SUMS`.
7. Install the `.deb` and the AppImage on a clean Linux machine and run
   through the smoke test again with the desktop app.
8. Create a GitHub release for the tag, attach the artifacts and checksums,
   and list user-visible changes. Only list platforms that were actually
   tested for that release.

## Platform claims

v0.1.0 is verified on Linux only (Pop!_OS 24.04 / Ubuntu 24.04 base,
Git 2.43, x86_64). macOS and Windows code paths exist (config directories,
`gitdir/i:` rules, path handling) but are untested; do not advertise them
until CI covers them.
