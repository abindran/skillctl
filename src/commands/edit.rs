use crate::{context::Context, error::Error, names::SkillName, store::drafts};
use semver::Version;

/// `edit <skill>[@<version>] [--force]`: copy a stored version into a writable
/// draft with a patch bump, then print the draft path.
pub fn run(context: &mut Context<'_>, skill: &str, force: bool) -> Result<(), Error> {
    let (name, version) = skill
        .split_once('@')
        .map_or((skill, None), |(name, version)| (name, Some(version)));
    let name: SkillName = name.parse()?;
    let version = version
        .map(|text| {
            Version::parse(text).map_err(|error| {
                super::attach::usage(format!(
                    "Invalid version {text:?}: {error}; drafts cannot be edited, only store versions"
                ))
            })
        })
        .transpose()?;
    super::pins::write_locked(context, |context| {
        let path = drafts::edit(
            context.fs.as_ref(),
            &context.home,
            &name,
            version.as_ref(),
            force,
        )?;
        let text = format!(
            "{}draft {name} at {}",
            if context.flags.dry_run {
                "Would create "
            } else {
                "Created "
            },
            path.display()
        );
        context
            .output
            .result(
                &serde_json::json!({"name": name, "path": path, "dry_run": context.flags.dry_run}),
                &text,
            )
            .map_err(super::output_error)
    })
}
