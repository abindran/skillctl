//! Per-invocation dependencies shared by command handlers.
//!
//! Construction resolves paths but does not initialize the home or touch state.
//! Git queries use the real binary by default and can be replaced in tests.

use std::io::{self, Write};

use crate::cli::{Cli, Command};
use crate::error::Error;
use crate::output::Output;
use crate::paths::Paths;
use crate::services::fs::{DryRunFs, Fs, RealFs};
use crate::services::git::{Git, GitCli};
use crate::services::prompt::{Prompter, TermPrompter};

/// Global execution policy, independent of command-specific arguments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub yes: bool,
    pub json: bool,
    pub dry_run: bool,
    pub quiet: bool,
    pub verbose: bool,
}

impl From<&Cli> for Flags {
    fn from(cli: &Cli) -> Self {
        Self {
            yes: cli.yes,
            json: cli.json,
            dry_run: cli.dry_run,
            quiet: cli.quiet,
            verbose: cli.verbose,
        }
    }
}

/// Output with injectable writers (including borrowed test buffers).
pub type ContextOutput<'a> = Output<Box<dyn Write + 'a>, Box<dyn Write + 'a>>;

/// Built once by the binary and borrowed by every command.
///
/// Tests may inject services that borrow local state; no global environment or
/// process-wide output redirection is required.
pub struct Context<'a> {
    pub home: Paths,
    pub fs: Box<dyn Fs + 'a>,
    pub prompt: Box<dyn Prompter + 'a>,
    pub git: Box<dyn Git + 'a>,
    pub output: ContextOutput<'a>,
    pub flags: Flags,
}

impl<'a> Context<'a> {
    /// Assemble a context from explicit services, without filesystem mutations.
    /// Flags configure the output mode in one place for real and test contexts.
    pub fn new(
        home: Paths,
        fs: Box<dyn Fs + 'a>,
        prompt: Box<dyn Prompter + 'a>,
        stdout: impl Write + 'a,
        stderr: impl Write + 'a,
        flags: Flags,
    ) -> Self {
        Self {
            home,
            fs,
            prompt,
            git: Box::new(GitCli::new()),
            output: Output::new(Box::new(stdout), Box::new(stderr), flags.json, flags.quiet),
            flags,
        }
    }
}

impl Context<'static> {
    /// Select real services and resolve --home / SKILLCTL_HOME / ~/.skills.
    /// Dry-run swaps only the filesystem; commands must route writes through it.
    pub fn from_cli(cli: &Cli) -> Result<Self, Error> {
        let flags = Flags::from(cli);
        let fs: Box<dyn Fs> = if flags.dry_run {
            Box::new(DryRunFs::new())
        } else {
            Box::new(RealFs)
        };
        let answer_flag = match &cli.command {
            Command::Attach { .. } => "--project",
            Command::Install { .. } => "--all",
            Command::Gc { .. } | Command::Config { .. } => "--yes",
            // Handlers with multiple questions can replace the prompter or set
            // a per-question flag once their interactive workflows are added.
            _ => "an explicit command option (see --help)",
        };
        Ok(Self::new(
            Paths::resolve(cli.home.as_deref())?,
            fs,
            Box::new(TermPrompter::new(answer_flag).with_yes(flags.yes)),
            io::stdout(),
            io::stderr(),
            flags,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::prompt::{Answer, ScriptedPrompter};
    use clap::Parser;

    #[test]
    fn injected_services_and_output_drive_a_command() {
        fn command(ctx: &mut Context<'_>) -> Result<(), Error> {
            if ctx.prompt.confirm("Create the home?", false)? {
                ctx.fs.create_dir(ctx.home.home()).unwrap();
            }
            ctx.output
                .result(&serde_json::json!({"ok": true}), "Created")
                .unwrap();
            Ok(())
        }

        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("skills");
        let fs = DryRunFs::new();
        let prompt = ScriptedPrompter::new([Answer::Confirm(true)]);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        {
            let mut ctx = Context::new(
                Paths::resolve(Some(&home)).unwrap(),
                Box::new(fs),
                Box::new(prompt),
                &mut stdout,
                &mut stderr,
                Flags {
                    json: true,
                    dry_run: true,
                    ..Flags::default()
                },
            );
            command(&mut ctx).unwrap();
            assert_eq!(ctx.home.home(), home);
        }
        assert!(!home.exists());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&stdout).unwrap()["ok"],
            true
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn production_context_selects_dry_run_and_propagates_flags() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("missing");
        let cli = Cli::try_parse_from([
            "skillctl",
            "--home",
            home.to_str().unwrap(),
            "--dry-run",
            "--yes",
            "--json",
            "--verbose",
            "gc",
        ])
        .unwrap();
        let ctx = Context::from_cli(&cli).unwrap();
        assert_eq!(
            ctx.flags,
            Flags {
                yes: true,
                json: true,
                dry_run: true,
                quiet: false,
                verbose: true,
            }
        );
        assert_eq!(ctx.home.home(), home);
        assert!(!home.exists());
        ctx.fs.create_dir(&home).unwrap();
        assert!(!home.exists());
        assert!(ctx.prompt.confirm("Continue?", false).unwrap());
    }

    #[test]
    fn production_context_uses_real_filesystem_without_creating_state_on_build() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("missing");
        let cli = Cli::try_parse_from([
            "skillctl",
            "--home",
            home.to_str().unwrap(),
            "--quiet",
            "status",
        ])
        .unwrap();
        let ctx = Context::from_cli(&cli).unwrap();
        assert!(ctx.flags.quiet);
        assert!(!ctx.flags.dry_run);
        assert!(!home.exists());
        ctx.fs.create_dir(&home).unwrap();
        assert!(home.is_dir());
    }

    #[test]
    fn dry_run_dispatch_receives_context_without_creating_home() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("missing");
        let prompt = ScriptedPrompter::new([]);
        let mut ctx = Context::new(
            Paths::resolve(Some(&home)).unwrap(),
            Box::new(DryRunFs::new()),
            Box::new(prompt),
            Vec::new(),
            Vec::new(),
            Flags {
                dry_run: true,
                ..Flags::default()
            },
        );
        crate::run(Command::Status, &mut ctx).unwrap();
        assert!(!home.exists());
    }
}
