pub mod artifact;
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
pub mod sources;
pub mod store;
pub mod validate;
use crate::cli::{Command, ConfigCommand};
use crate::context::Context;
use crate::error::Error;

/// Dispatch a command with the invocation's shared dependencies.
pub fn run(command: Command, context: &mut Context<'_>) -> Result<(), Error> {
    match command {
        Command::Init => commands::init::run(context),
        Command::Attach {
            project,
            new,
            skills,
            top_level,
            here,
        } => commands::attach::run(
            context,
            project.as_ref(),
            new.as_ref(),
            &skills,
            top_level,
            here,
        ),
        Command::Detach {
            keep_links,
            delete_project,
        } => commands::detach::run(context, keep_links, delete_project),
        Command::Status => commands::status::run(context),
        Command::Add {
            skill,
            project,
            draft,
        } => commands::pins::add(context, &skill, project.as_ref(), draft),
        Command::Remove { skill, project } => {
            commands::pins::remove(context, &skill, project.as_ref())
        }
        Command::Global { command } => commands::global::run(context, command),
        Command::Projects => commands::projects::run(context),
        Command::Skills => commands::skills::run(context),
        Command::Relink { all, project } => commands::pins::relink(context, all, project.as_ref()),
        Command::New {
            skill,
            from_template,
            kind,
        } => commands::new::run(context, &skill, from_template.as_deref(), kind),
        Command::Edit { skill, force } => commands::edit::run(context, &skill, force),
        Command::Seal { skill, bump } => commands::seal::run(context, &skill, bump),
        Command::Install {
            source,
            reference,
            path,
            all,
            version,
        } => commands::install::run(
            context,
            &source,
            reference.as_deref(),
            path.as_deref(),
            all,
            version.as_deref(),
        ),
        Command::Validate {
            target,
            format,
            strict,
            with_reference,
        } => commands::validate::run(context, target.as_deref(), format, strict, with_reference),
        Command::Doctor { fix } => commands::doctor::run(context, fix),
        Command::Gc { keep } => commands::gc::run(context, keep),
        Command::Config { command } => match command {
            ConfigCommand::Get { key } => commands::config::get(context, &key),
            ConfigCommand::Set { key, value } => commands::config::set(context, &key, &value),
        },
        Command::Completions { shell } => commands::completions::run(context, shell),
    }
}
