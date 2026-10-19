pub mod advisory;
pub mod attach;
pub mod completions;
pub mod config;
pub mod detach;
pub mod doctor;
pub mod edit;
pub mod gc;
pub mod global;
pub mod hook;
pub mod init;
pub mod install;
pub mod lint;
pub mod mcp;
pub mod new;
pub mod pins;
pub mod projects;
pub mod seal;
pub mod skills;
pub mod status;
pub mod telemetry;
pub mod validate;

use crate::{
    context::Context,
    error::Error,
    registry::{Registry, lock::RegistryLock},
};

fn output_error(source: std::io::Error) -> Error {
    Error::Io {
        operation: "Write command output".into(),
        source,
        hint: "Check the output stream and try again.".into(),
    }
}

fn read_locked<T>(
    context: &Context<'_>,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    if context.flags.dry_run {
        operation()
    } else {
        RegistryLock::open(&context.home.registry_lock())?.with_shared(operation)
    }
}

/// Read-only commands tolerate a home folder whose registry was never written.
fn load_registry_or_default(context: &Context<'_>) -> Result<Registry, Error> {
    match context.fs.symlink_metadata(&context.home.registry()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        _ => Registry::load(context.fs.as_ref(), &context.home.registry()),
    }
}

fn load_registry(context: &Context<'_>) -> Result<Registry, Error> {
    if context.flags.dry_run
        && matches!(context.fs.symlink_metadata(&context.home.registry()), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        Ok(Registry::default())
    } else {
        Registry::load(context.fs.as_ref(), &context.home.registry())
    }
}

#[cfg(test)]
mod tests;
