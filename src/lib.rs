pub mod cli;
pub mod error;
pub mod names;
pub mod output;
pub mod paths;
pub mod services;
use crate::cli::{Cli, Command, ConfigCommand, GlobalCommand};
use crate::error::Error;

pub fn run(cli: Cli) -> Result<(), Error> {
    let command = match cli.command {
        Command::Init => "init",
        Command::Attach { .. } => "attach",
        Command::Detach { .. } => "detach",
        Command::Status => "status",
        Command::Add { .. } => "add",
        Command::Remove { .. } => "remove",
        Command::Global { command } => match command {
            GlobalCommand::Add { .. } => "global add",
            GlobalCommand::Remove { .. } => "global remove",
            GlobalCommand::List => "global list",
        },
        Command::Projects => "projects",
        Command::Skills => "skills",
        Command::Relink { .. } => "relink",
        Command::New { .. } => "new",
        Command::Edit { .. } => "edit",
        Command::Seal { .. } => "seal",
        Command::Install { .. } => "install",
        Command::Validate { .. } => "validate",
        Command::Doctor { .. } => "doctor",
        Command::Gc { .. } => "gc",
        Command::Config { command } => match command {
            ConfigCommand::Get { .. } => "config get",
            ConfigCommand::Set { .. } => "config set",
        },
        Command::Completions { .. } => "completions",
    };

    Err(Error::NotImplemented {
        command: command.to_owned(),
        hint: "Run skillctl --help to see the command interface; execution is not implemented yet."
            .to_owned(),
    })
}
