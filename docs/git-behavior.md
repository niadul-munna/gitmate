# How Git decides who you are

GitMate never re-implements Git's configuration rules. It asks Git. This
document records the behaviour GitMate relies on, as observed with Git
2.43 on Linux and as described in `git-config(1)`, `git-var(1)` and
`git-commit-tree(1)`. Every claim below is exercised by an integration test in
`crates/gitmate-core/tests/`.

## Four separate concerns

| Concern | What controls it | What GitMate does |
| --- | --- | --- |
| Commit identity | `user.name`, `user.email`, `author.*`, `committer.*`, `GIT_AUTHOR_*` / `GIT_COMMITTER_*` / `EMAIL` env vars | Reads, explains, and writes `user.name` / `user.email` |
| SSH authentication | remote URL, `core.sshCommand`, `GIT_SSH_COMMAND`, `GIT_SSH`, `~/.ssh/config` | Inspects; writes `core.sshCommand` only when asked; never edits `~/.ssh/config` |
| HTTPS authentication | `credential.helper`, `credential.<url>.*`, the helper's own store | Reports helpers and their origin; never calls a helper, never reads tokens |
| Precedence / origin | file scopes, include order, env vars, `-c` | Delegates to `git config --show-origin --show-scope` and `git var` |

Changing `user.email` changes the **author recorded in commits**. It does not
change which GitHub/GitLab account **authenticates** a push. That is decided by
the SSH key (SSH remotes) or the credential helper (HTTPS remotes).

## Effective identity

`git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT` print the identity
Git would put in a commit made right now, after applying every rule:

1. `GIT_AUTHOR_NAME` / `GIT_AUTHOR_EMAIL` (and committer equivalents) env vars
2. `author.name` / `author.email` (or `committer.*`) config keys
3. `user.name` / `user.email` config keys
4. `EMAIL` env var (email only)
5. Auto-detection from the system user and hostname, unless
   `user.useConfigOnly=true`

If no usable identity exists, `git var` exits 128 with
`Author identity unknown`. GitMate reports that as *unset*.

## Configuration scopes

For single-valued keys the **last value read wins**. Git reads, in order:

1. **system**: `$(prefix)/etc/gitconfig` (skipped with `GIT_CONFIG_NOSYSTEM=1`,
   replaced by `GIT_CONFIG_SYSTEM`)
2. **global**: `$XDG_CONFIG_HOME/git/config`, then `~/.gitconfig`; both are read,
   so `~/.gitconfig` wins. `GIT_CONFIG_GLOBAL` replaces both.
3. **local**: `$GIT_COMMON_DIR/config`. This is shared by all linked
   worktrees of a repository.
4. **worktree**: `$GIT_DIR/config.worktree`, only when
   `extensions.worktreeConfig=true`
5. **command**: `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n`,
   `GIT_CONFIG_PARAMETERS`, and `git -c key=value`

When **writing** with `--global`, Git writes `~/.gitconfig`. It writes
`$XDG_CONFIG_HOME/git/config` only if that file exists and `~/.gitconfig` does
not. `GIT_CONFIG_GLOBAL`, when set, is the only global file. GitMate
resolves the global target the same way and shows the exact file in every
change preview.

`git config -z --show-scope --show-origin --get-all <key>` lists every value in
read order as `scope NUL origin NUL value NUL`. Local origins, including files
pulled in with `[include]`, are reported relative to the top of the working
tree (`file:.git/config`, `file:.git/extra`), or to the git directory in a
bare repository (`file:config`), so GitMate resolves them against that
directory.

## Conditional includes

```ini
[includeIf "gitdir:~/work/"]
    path = ~/.config/gitmate/includes/work.gitconfig
```

* The included file is processed **at the position of the `includeIf` section**.
  A `[user]` section that appears later in the same file overrides it.
* A trailing `/` means "this directory and everything below it" (`**` is
  appended), so `gitdir:~/work/` and `gitdir:~/work/**` are the same rule.
  Without the trailing slash the pattern matches one `.git` directory only,
  and a pattern that does not start with `/`, `~/` or `./` gets `**/`
  prepended. GitMate treats only the first two forms as directory rules.
* `gitdir:` is case-sensitive; `gitdir/i:` is case-insensitive.
* For a **linked worktree** the matched git directory is
  `<main repo>/.git/worktrees/<name>`, so a worktree checked out *outside* the
  rule directory still matches if its main repository is inside it.
  (Verified: `tests/worktrees.rs`.)
* **Symlinks**: if `~/link -> ~/work`, a rule on the real path `~/work/` matches
  repositories opened through either path, but a rule written on `~/link/`
  does **not** match a repository opened through its real path. GitMate
  therefore writes rules on the canonical (resolved) directory.
* When several rules match (nested directories), all of them are included and
  the one that appears **last** in the file wins. GitMate appends new rules
  at the end of the file and warns when a broader rule would shadow a more
  specific one.
* `includeIf` conditions only apply inside a repository. Outside a repository
  only unconditional configuration is visible.

## Repository discovery

`git rev-parse --path-format=absolute --git-dir --git-common-dir
--is-bare-repository` (Git ≥ 2.31) gives exact locations without guessing.
`--show-toplevel` fails in bare repositories and is only queried for
non-bare ones. A nested repository is discovered by Git as the nearest
enclosing `.git`. Git's `safe.directory` ownership check is respected. When
Git refuses a repository, GitMate reports Git's own message and does not
add `safe.directory` entries.

## SSH

Git chooses the SSH program from `GIT_SSH_COMMAND`, then `core.sshCommand`,
then `GIT_SSH`, then `ssh`. Remote URLs are read with `git remote get-url
--all`, which applies `url.<base>.insteadOf` rewriting. `core.sshCommand` is run
**through a shell**, so GitMate builds it only from a validated key path,
single-quoted. It never executes a repository's `core.sshCommand`.

`ssh -G <host>` prints the client configuration that *would* be used,
including `identityfile` entries, without connecting. Note that `ssh` reads
`~/.ssh/config` from the passwd home directory, not `$HOME`, and that
`Match exec` blocks in that file run their commands during evaluation. For both
reasons GitMate runs `ssh -G` only when explicitly asked
(`gitmate doctor --ssh-config`).

## HTTPS

Credential helpers store tokens per host (optionally per path with
`credential.useHttpPath`). Setting `user.email` does not change which account a
helper returns. GitMate reports the configured helpers and
`credential.username` with their origin, explains the limitation, and never runs
`git credential` or a helper.
