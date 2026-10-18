pub mod cli;
pub mod commands;
pub mod context;
pub mod error;
pub mod linker;
pub mod names;
pub mod output;
pub mod paths;
pub mod registry;
pub mod services;
pub mod skill;
pub mod store;
pub mod validate;
use crate::cli::{Command, ConfigCommand, GlobalCommand};
use crate::context::Context;
use crate::error::Error;

/// Dispatch a command with the invocation's shared dependencies.
/// Unfinished command bodies remain stubs until their respective work items.
pub fn run(command: Command, context: &mut Context<'_>) -> Result<(), Error> {
    let command = match command {
        Command::Init => return commands::init::run(context),
        Command::Attach {
            project,
            new,
            skills,
        } => return commands::attach::run(context, project.as_ref(), new.as_ref(), &skills),
        Command::Detach { .. } => "detach",
        Command::Status => return commands::status::run(context),
        Command::Add {
            skill,
            project,
            draft,
        } => return commands::pins::add(context, &skill, project.as_ref(), draft),
        Command::Remove { skill, project } => {
            return commands::pins::remove(context, &skill, project.as_ref());
        }
        Command::Global { command } => match command {
            GlobalCommand::Add { .. } => "global add",
            GlobalCommand::Remove { .. } => "global remove",
            GlobalCommand::List => "global list",
        },
        Command::Projects => return commands::projects::run(context),
        Command::Skills => return commands::skills::run(context),
        Command::Relink { all, project } => {
            return commands::pins::relink(context, all, project.as_ref());
        }
        Command::New {
            skill,
            from_template,
        } => return commands::new::run(context, &skill, from_template.as_deref()),
        Command::Edit { .. } => "edit",
        Command::Seal { skill, bump } => return commands::seal::run(context, &skill, bump),
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
    commands::init::ensure_home(context)?;

    Err(Error::NotImplemented {
        command: command.to_owned(),
        hint: "Run skillctl --help to see the command interface; execution is not implemented yet."
            .to_owned(),
    })
}
