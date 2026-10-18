use clap::Parser;
use std::io::{self, Write};
use std::process::ExitCode;

use skillctl::cli::Cli;
use skillctl::output::Output;

fn main() -> ExitCode {
    // Clap owns usage errors, help, and version output (including in JSON mode).
    let cli = Cli::parse();
    let (json, quiet) = (cli.json, cli.quiet);
    match skillctl::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let stdout = io::stdout();
            let stderr = io::stderr();
            let mut stderr = stderr.lock();
            let result = Output::new(stdout.lock(), &mut stderr, json, quiet).error(&err);
            if let Err(report_error) = result {
                // Best effort only: do not recurse or panic if stderr is also unavailable.
                let _ = writeln!(
                    stderr,
                    "Error: could not write command output: {report_error}"
                );
                return ExitCode::FAILURE;
            }
            ExitCode::from(err.exit_code())
        }
    }
}
