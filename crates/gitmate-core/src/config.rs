//! Targeted reads and writes of Git configuration through the `git config`
//! command. GitMate never serialises a Git config file itself.

use std::ffi::OsString;
use std::path::Path;

use crate::error::{Error, Result};
use crate::git::{self, ConfigEntry, Git};

/// `git config` exit code for "key not found" on reads.
const EXIT_NOT_FOUND: i32 = 1;
/// `git config` exit code for "nothing to unset".
const EXIT_NOTHING_TO_UNSET: i32 = 5;

/// `--file` paths must be absolute so they never depend on where Git runs.
fn require_absolute(file: &Path) -> Result<()> {
    if file.is_absolute() {
        Ok(())
    } else {
        Err(Error::InvalidPath {
            path: file.to_path_buf(),
            reason: "must be an absolute path".into(),
        })
    }
}

impl Git {
    fn run_config(&self, args: Vec<OsString>) -> Result<git::GitOutput> {
        // `git config --file` needs no repository; strip repository
        // redirection so an inherited GIT_DIR cannot interfere, and run from
        // the filesystem root rather than a shared directory such as /tmp.
        let cwd = Path::new(std::path::MAIN_SEPARATOR_STR);
        self.run_at(cwd, &args)
    }

    /// Every value of `key` in exactly `file`, ignoring includes. A missing
    /// file or key yields an empty list.
    pub fn file_get_all(&self, file: &Path, key: &str) -> Result<Vec<String>> {
        require_absolute(file)?;
        let args: Vec<OsString> = vec![
            "config".into(),
            "--file".into(),
            file.into(),
            "--no-includes".into(),
            "-z".into(),
            "--get-all".into(),
            "--".into(),
            key.into(),
        ];
        let out = self.run_config(args.clone())?;
        match out.code {
            Some(0) => Ok(git::parse_values(&out.stdout)),
            Some(EXIT_NOT_FOUND) if out.stderr.is_empty() => Ok(Vec::new()),
            _ => Err(git::failed(&args, &out)),
        }
    }

    /// All `key=value` entries of exactly `file`, in file order, ignoring
    /// includes. A missing file yields an empty list.
    pub fn file_list(&self, file: &Path) -> Result<Vec<(String, String)>> {
        require_absolute(file)?;
        if !file.exists() {
            return Ok(Vec::new());
        }
        let args: Vec<OsString> = vec![
            "config".into(),
            "--file".into(),
            file.into(),
            "--no-includes".into(),
            "-z".into(),
            "--list".into(),
        ];
        let out = self.run_config(args.clone())?;
        if !out.success() {
            return Err(git::failed(&args, &out));
        }
        Ok(git::parse_key_values(&out.stdout))
    }

    /// Make `key` in exactly `file` hold `values` (in order); an empty list
    /// removes the key. Values that stay are not rewritten: appending uses
    /// `--add` and removing uses `--unset-all --fixed-value`, so other
    /// values of the key keep their place in the file (the order of
    /// `includeIf` sections decides which one wins).
    pub fn file_set_all(&self, file: &Path, key: &str, values: &[String]) -> Result<()> {
        let current = self.file_get_all(file, key)?;
        if current == values {
            return Ok(());
        }
        let run = |op: &[&str], value: Option<&String>| -> Result<()> {
            let mut args: Vec<OsString> = vec!["config".into(), "--file".into(), file.into()];
            args.extend(op.iter().map(OsString::from));
            args.push("--".into());
            args.push(key.into());
            args.extend(value.map(OsString::from));
            let out = self.run_config(args.clone())?;
            match out.code {
                Some(0) => Ok(()),
                Some(EXIT_NOTHING_TO_UNSET) if op[0] == "--unset-all" => Ok(()),
                _ => Err(git::failed(&args, &out)),
            }
        };

        if values.is_empty() {
            return run(&["--unset-all"], None);
        }
        if let Some(added) = values.strip_prefix(current.as_slice()) {
            return added.iter().try_for_each(|v| run(&["--add"], Some(v)));
        }
        let removed: Vec<&String> = current.iter().filter(|v| !values.contains(v)).collect();
        if current.iter().filter(|v| values.contains(v)).eq(values.iter()) {
            let mut done: Vec<&String> = Vec::new();
            for value in removed {
                if !done.contains(&value) {
                    run(&["--unset-all", "--fixed-value"], Some(value))?;
                    done.push(value);
                }
            }
            return Ok(());
        }
        run(&["--replace-all"], Some(&values[0]))?;
        values[1..].iter().try_for_each(|v| run(&["--add"], Some(v)))
    }

    /// Every value of `key` as Git sees it from `dir` (with includes and
    /// conditional includes evaluated), in the order Git reads them; the last
    /// one is effective for single-valued keys.
    pub fn scoped_get_all(&self, dir: &Path, key: &str) -> Result<Vec<ConfigEntry>> {
        let args: Vec<OsString> = vec![
            "config".into(),
            "-z".into(),
            "--show-scope".into(),
            "--show-origin".into(),
            "--get-all".into(),
            "--".into(),
            key.into(),
        ];
        let out = self.run_at(dir, &args)?;
        match out.code {
            Some(0) => Ok(git::parse_scoped_entries(&out.stdout)),
            Some(EXIT_NOT_FOUND) if out.stderr.is_empty() => Ok(Vec::new()),
            _ => Err(git::failed(&args, &out)),
        }
    }

    /// Every entry whose key matches `regex`, as Git sees it from `dir`.
    pub fn scoped_get_regexp(&self, dir: &Path, regex: &str) -> Result<Vec<(String, ConfigEntry)>> {
        let args: Vec<OsString> = vec![
            "config".into(),
            "-z".into(),
            "--show-scope".into(),
            "--show-origin".into(),
            "--get-regexp".into(),
            "--".into(),
            regex.into(),
        ];
        let out = self.run_at(dir, &args)?;
        match out.code {
            Some(0) => Ok(git::parse_scoped_key_values(&out.stdout)),
            Some(EXIT_NOT_FOUND) if out.stderr.is_empty() => Ok(Vec::new()),
            _ => Err(git::failed(&args, &out)),
        }
    }
}
