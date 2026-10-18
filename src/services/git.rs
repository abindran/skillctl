//! Read-only Git queries through the developer's Git binary.
//!
//! No shell is involved. Git 2.31+ is required for absolute exclude paths;
//! callers can use `version` to diagnose older installations.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use semver::Version;

use crate::error::Error;

/// Swappable Git queries. Non-work-tree folders (including bare repositories)
/// return `None`; missing Git, invalid directories and other failures are errors.
/// Cloning is added with the install work item, not this read-only interface.
pub trait Git {
    fn toplevel(&self, dir: &Path) -> Result<Option<PathBuf>, Error>;
    fn exclude_file(&self, dir: &Path) -> Result<Option<PathBuf>, Error>;
    fn version(&self) -> Result<Version, Error>;
}

#[derive(Debug, Clone)]
pub struct GitCli {
    program: OsString,
}

impl Default for GitCli {
    fn default() -> Self {
        Self::new()
    }
}

impl GitCli {
    pub fn new() -> Self {
        Self::with_program("git")
    }

    /// Select an explicit executable without changing process-wide PATH.
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn execute(&self, dir: Option<&Path>, args: &[&str]) -> Result<Output, Error> {
        let mut command = Command::new(&self.program);
        // -C must describe the repository, even if skillctl is launched by Git
        // with repository-selection environment variables in scope.
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_PREFIX",
        ] {
            command.env_remove(key);
        }
        command.env("LC_ALL", "C").env("GIT_OPTIONAL_LOCKS", "0");
        if let Some(dir) = dir {
            command.arg("-C").arg(dir);
        }
        command.args(args).output().map_err(|source| Error::Io {
            operation: format!("run Git {}", args.join(" ")),
            source,
            hint: "Install Git and ensure its executable is available on PATH".into(),
        })
    }

    fn in_work_tree(&self, dir: &Path) -> Result<bool, Error> {
        let output = self.execute(Some(dir), &["rev-parse", "--is-inside-work-tree"])?;
        if !output.status.success() {
            // Do not silently turn permission, corrupt-repository, unsafe-owner,
            // or invalid-directory failures into "not a repository".
            if output
                .stderr
                .starts_with(b"fatal: not a git repository (or any of the parent directories):")
                || output
                    .stderr
                    .starts_with(b"fatal: not a git repository (or any parent up to mount point")
            {
                return Ok(false);
            }
            return Err(command_error(dir, &output));
        }
        match line(&output.stdout) {
            b"true" => Ok(true),
            b"false" => Ok(false),
            _ => Err(invalid_output("work-tree status")),
        }
    }

    fn query_path(&self, dir: &Path, args: &[&str]) -> Result<Option<PathBuf>, Error> {
        if !self.in_work_tree(dir)? {
            return Ok(None);
        }
        let output = self.execute(Some(dir), args)?;
        if !output.status.success() {
            return Err(command_error(dir, &output));
        }
        let path = output_path(line(&output.stdout))?;
        if !path.is_absolute() {
            return Err(invalid_output(
                "absolute repository path (requires Git 2.31+)",
            ));
        }
        Ok(Some(path))
    }
}

impl Git for GitCli {
    fn toplevel(&self, dir: &Path) -> Result<Option<PathBuf>, Error> {
        self.query_path(dir, &["rev-parse", "--show-toplevel"])
    }

    fn exclude_file(&self, dir: &Path) -> Result<Option<PathBuf>, Error> {
        self.query_path(
            dir,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "info/exclude",
            ],
        )
    }

    fn version(&self) -> Result<Version, Error> {
        let output = self.execute(None, &["--version"])?;
        if !output.status.success() {
            return Err(command_error(Path::new("."), &output));
        }
        parse_version(&output.stdout)
    }
}

// Remove only Git's record terminator, never whitespace belonging to a path.
fn line(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[cfg(unix)]
fn output_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn output_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_output("UTF-8 path"))?;
    Ok(PathBuf::from(text.trim_end_matches('\r')))
}

fn parse_version(bytes: &[u8]) -> Result<Version, Error> {
    let token = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix("git version "))
        .and_then(|text| text.split_whitespace().next())
        .ok_or_else(|| invalid_output("version"))?;
    // Apple adds a separate vendor label; Git for Windows appends .windows.N.
    let core = token
        .split_once(".windows.")
        .map_or(token, |(core, _)| core);
    Version::parse(core).map_err(|_| invalid_output("version"))
}

fn command_error(dir: &Path, output: &Output) -> Error {
    Error::Io {
        operation: format!("query Git repository {}", dir.display()),
        source: io::Error::other(format!(
            "Git exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim_end()
        )),
        hint: "Check the repository, its permissions, and Git configuration; Git 2.31+ is required"
            .into(),
    }
}

fn invalid_output(expected: &str) -> Error {
    Error::Io {
        operation: format!("read Git {expected}"),
        source: io::Error::new(io::ErrorKind::InvalidData, "unexpected Git output"),
        hint: "Check that the configured Git executable is Git 2.31 or later".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_and_vendor_versions() {
        for text in [
            "git version 2.45.1\n",
            "git version 2.45.1 (Apple Git-154)\n",
            "git version 2.45.1.windows.1\r\n",
        ] {
            assert_eq!(
                parse_version(text.as_bytes()).unwrap(),
                Version::new(2, 45, 1)
            );
        }
        for text in [
            "",
            "git version",
            "other 2.45.1",
            "git version broken",
            "git version 2.45",
        ] {
            let error = parse_version(text.as_bytes()).unwrap_err();
            assert_eq!(error.exit_code(), 1);
            assert!(!error.hint().is_empty());
        }
    }

    #[test]
    fn preserves_path_whitespace() {
        assert_eq!(line(b"/tmp/repo \n"), b"/tmp/repo ");
        assert_eq!(line(b"/tmp/repo\n\n"), b"/tmp/repo\n");
    }
}
