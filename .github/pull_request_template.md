## What changes for users

<!-- One or two sentences. -->

## How it was tested

<!-- Commands you ran and new tests. Tests must use temporary HOMEs, never your real Git config. -->

- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
- [ ] Frontend: `pnpm typecheck && pnpm lint && pnpm test && pnpm build` (if the UI changed)
- [ ] Docs updated if behaviour or a security decision changed

## Safety

<!-- Does this add a new kind of write, a new key, a new process invocation, or a new Tauri command/permission? If so, explain why it is safe. -->
