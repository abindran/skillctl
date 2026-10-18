use clap::CommandFactory;
use clap_complete::Shell;

use crate::{cli::Cli, context::Context, error::Error};

/// Print a completion script for `shell` on stdout. Nothing else is touched:
/// no home folder, no registry, no lock.
pub fn run(context: &mut Context<'_>, shell: Shell) -> Result<(), Error> {
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "skillctl", &mut script);
    let script = String::from_utf8(script).map_err(|error| Error::Io {
        operation: "Render completion script".into(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        hint: "Report this as a skillctl bug.".into(),
    })?;
    context
        .output
        .document(script.trim_end_matches('\n'), true)
        .map_err(super::output_error)
}
