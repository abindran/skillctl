pub mod cli;

use anyhow::{Result, anyhow};

use cli::Cli;

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        // Add one arm for every command defined in Cli.
        // Example:
        // SomeCommand => Err(anyhow!("some-command: not implemented")),
        command => Err(anyhow::anyhow!("{command:?}: not implemented")),
    }
}
