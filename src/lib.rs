pub mod cli;
pub mod context;
pub mod error;
pub mod names;
pub mod output;
pub mod paths;
pub mod registry;
pub mod services;
use crate::cli::{Command, ConfigCommand, GlobalCommand};
use crate::context::Context;
use crate::error::Error;

/// Dispatch a command with the invocation's shared dependencies.
/// Command bodies remain stubs until their respective work items.
pub fn run(command: Command, _context: &mut Context<'_>) -> Result<(), Error> {
    let command = match command {
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
