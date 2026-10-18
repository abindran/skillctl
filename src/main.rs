use clap::Parser;
use std::process::ExitCode;

use skillctl::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match skillctl::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::from(1)
        }
    }
}
