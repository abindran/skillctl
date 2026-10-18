use clap::Parser;
use std::io::{self, Write};
use std::process::ExitCode;

use skillctl::cli::Cli;
use skillctl::context::Context;
use skillctl::error::Error;
use skillctl::output::Output;

fn main() -> ExitCode {
    // Clap owns usage errors, help, and version output (including in JSON mode).
    let cli = Cli::parse();
    let mut context = match Context::from_cli(&cli) {
        Ok(context) => context,
        Err(error) => {
            // Even construction failures respect --json and --quiet.
            let result = Output::new(io::stdout(), io::stderr(), cli.json, cli.quiet).error(&error);
            return reported_error(&error, result);
        }
    };
    match skillctl::run(cli.command, &mut context) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let result = context.output.error(&error);
            reported_error(&error, result)
        }
    }
}

fn reported_error(error: &Error, result: io::Result<()>) -> ExitCode {
    if let Err(report_error) = result {
        // Best effort only: do not recurse or panic if stderr is also unavailable.
        let _ = writeln!(
            io::stderr(),
            "Error: could not write command output: {report_error}"
        );
        return ExitCode::FAILURE;
    }
    ExitCode::from(error.exit_code())
}
