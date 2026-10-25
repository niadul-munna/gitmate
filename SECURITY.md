# Security policy

GitMate edits Git configuration on developer machines, so mistakes can
misattribute commits or change which SSH key Git uses. Please report
anything that could cause GitMate to write the wrong value, write somewhere
it should not, leak secrets, or run commands it should not.

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |

## Reporting a vulnerability

Please report privately using GitHub's **Report a vulnerability** button on the
repository's Security tab (private vulnerability reporting). Do not open a
public issue for security problems.

Include the GitMate version (`gitmate --version`), your OS, `git
--version`, and the smallest set of steps that reproduces the issue. **Redact
tokens, emails and private paths** from any configuration you paste.

You should receive an acknowledgement within 7 days. Fixes are released as
patch versions and credited in the release notes unless you prefer otherwise.

## What GitMate does and does not touch

* **Never stored or read:** passwords, access tokens, credential-helper data,
  SSH private key contents. Private keys are checked by file metadata only
  (exists, regular file, permissions). A signing key is stored as you give it
  (a key id, a path or a `key::` public key) and never read.
* **Written only after a preview:** `user.name`, `user.email` and, on request,
  `core.sshCommand` (`--ssh`) and `user.signingKey`, `gpg.format`,
  `commit.gpgsign` (`--sign`) in a repository's `.git/config`, and
  `includeIf "gitdir:…".path` entries in your global Git config. The core
  refuses to write any other key (enforced at apply time, not just in plan
  construction).
* **Owned by GitMate:** `profiles.toml`, generated include files (a profile's
  name, email and, if set, `core.sshCommand` and signing settings), the
  operation journal and backups, all created `0600` in `0700` directories.
* **Written only on request, outside Git config:** `hooks/pre-commit` in a
  repository's git directory (`gitmate hook install`). It is a fixed script
  that runs `gitmate hook check`, written `0755`, not journaled. GitMate
  replaces a hook it did not write only with `--force`, never removes one, and
  writes nothing when `core.hooksPath` sends Git elsewhere.
* **Never edited:** `~/.ssh/config`, credential helpers, `safe.directory`,
  `core.hooksPath`, or any Git setting outside the list above.
* **No network access, except `gitmate clone`.** That command runs `git clone`
  attached to your terminal, so Git's own prompts and credential helpers
  apply. Nothing else connects to GitHub, GitLab or any SSH host.
  `doctor --ssh-config` runs `ssh -G`, which only prints local client
  configuration.

## Threat model and mitigations

| Threat | Mitigation |
| --- | --- |
| Command injection through paths, names or remote hosts | Git and ssh are always run with argument vectors, never through a shell. Values follow `--`. SSH hosts are validated before `ssh -G` and passed after `--`. |
| `core.sshCommand` is executed by a shell | GitMate only builds it from a validated absolute key path, single-quoted (tested against a real `sh` with `$()`, backticks, quotes and spaces). It never runs a repository's `core.sshCommand`. |
| Malicious or unusual repository config | Values are read through `git config -z` (NUL-separated, no quoting ambiguity) and treated as data. Repository-redirecting variables (`GIT_DIR`, `GIT_WORK_TREE`, …) are stripped when GitMate targets a specific path. |
| Path traversal | Profile ids are restricted to `[a-z0-9_-]`, so include file names cannot escape the includes directory. Journal ids are validated the same way. |
| Symlink attacks on GitMate's files | Files are written to a new file created with `O_EXCL` and renamed into place, so a planted symlink is replaced, not followed. GitMate refuses to use group/world-writable state directories. |
| Glob or case surprises in rules | Rule directories containing `*`, `?`, `[`, `]` or `\` are rejected, both as typed and after symlinks are resolved; directories are canonicalised because Git matches repositories by real path. An existing rule for the same directory, however it is spelled (`~/work/`, `~/work/**`, through a symlink), is a conflict. |
| Concurrent modification | GitMate writers are serialised with an exclusive lock. Every planned key is re-read under the lock and again immediately before the first write; any difference from the preview aborts with nothing written. Git's own `config.lock` protects each write. Git has no compare-and-swap, so another tool editing the same key during the write itself (milliseconds) is not detected. |
| Partial failure | A failed write reverts the keys already changed and records the operation as failed. |
| Destructive rollback | Undo restores only GitMate's keys, and only if each still holds the value GitMate wrote; otherwise it refuses and lists the conflicts. It also refuses while a later operation that changed the same key is still applied, because equal values cannot show whose write is in effect. Whole-file backups are kept for manual recovery but are never restored automatically. |
| Git ownership checks | Git's `safe.directory` protection is respected; GitMate reports Git's refusal and never adds exceptions. |
| A pre-commit hook runs code on every commit | The hook GitMate installs is a constant script with no user-supplied content; it calls `gitmate` from `PATH` and lets the commit through when it is missing, so uninstalling GitMate never blocks commits. GitMate recognises its own hook by a marker line, and `hook check` only reads configuration. |
| Signing settings in shared files | `user.signingKey` and `gpg.format` are validated (no control characters; known formats only) and written with `git config --file` like every other value. GitMate never runs `gpg` or `ssh-keygen`. |
| Untrusted desktop webview | The Tauri webview can call only 17 app commands listed in `build.rs` and granted in `capabilities/default.json`, plus the native folder/file picker. No shell, filesystem, opener, HTTP or core window/event APIs are enabled. Inputs are validated in Rust. Changes to Git configuration require a plan token issued by the Rust side, so the webview can approve a computed plan but never submit files or keys to write. Profile commands take form fields only; saving an edited profile regenerates its include file, which takes effect at once in repositories covered by its rules (the form says so). A strict CSP is set. |
| Secrets in output | Credentials embedded in HTTP(S) remote URLs, and passwords in any other URL, are replaced with `***` before display and flagged; remote patterns are matched against that redacted form, and a failed `gitmate clone` is reported without repeating its URL. Credential helpers are shown by name only: inline shell helpers (`!…`) and helper arguments are hidden, as are credentials inside `credential.<url>` keys. The `GIT_CONFIG_PARAMETERS` variable is listed by name only; identity values it sets are shown like any other config value. |

## Backups

Before changing a file, GitMate copies it to
`$XDG_STATE_HOME/gitmate/backups/<operation>/` with mode `0600`. Your Git
config can contain sensitive values (for example tokens embedded in URLs), so
these copies carry the same sensitivity as the originals. Only the 25 most
recent operations keep backups.
