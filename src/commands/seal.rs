use crate::{
    cli::Bump, context::Context, error::Error, names::SkillName, registry::lock::RegistryLock,
    store::drafts,
};

pub fn run(context: &mut Context<'_>, name: &SkillName, bump: Option<Bump>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let operation = || {
        let registry = super::load_registry(context)?;
        drafts::seal_with_allow(
            context.fs.as_ref(),
            &context.home,
            name,
            bump,
            &registry.settings.validate.allow,
        )
    };
    let path = if context.flags.dry_run {
        operation()?
    } else {
        RegistryLock::open(&context.home.registry_lock())?.with_exclusive(operation)?
    };
    let message = format!(
        "{}{} at {}",
        if context.flags.dry_run {
            "Would seal "
        } else {
            "Sealed "
        },
        name,
        path.display()
    );
    context
        .output
        .result(
            &serde_json::json!({"name": name, "path": path, "dry_run": context.flags.dry_run}),
            &message,
        )
        .map_err(super::output_error)
}
