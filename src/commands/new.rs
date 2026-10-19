use crate::{
    artifact::Kind, context::Context, error::Error, names::SkillName, registry::lock::RegistryLock,
    store::drafts,
};

pub fn run(
    context: &mut Context<'_>,
    name: &SkillName,
    template: Option<&str>,
    kind: Kind,
) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let operation = || {
        drafts::new(
            context.fs.as_ref(),
            &context.home,
            name,
            template,
            kind,
            context.flags.dry_run,
        )
    };
    let path = if context.flags.dry_run {
        operation()?
    } else {
        RegistryLock::open(&context.home.registry_lock())?.with_exclusive(operation)?
    };
    let message = format!(
        "{}{kind} draft {} at {}",
        if context.flags.dry_run {
            "Would create "
        } else {
            "Created "
        },
        name,
        path.display()
    );
    context
        .output
        .result(
            &serde_json::json!({"name": name, "kind": kind, "path": path, "dry_run": context.flags.dry_run}),
            &message,
        )
        .map_err(super::output_error)
}
