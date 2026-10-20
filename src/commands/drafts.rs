//! `drafts`: every writable draft under `$SKILLCTL_HOME/drafts/` with the kind
//! detected from its root document and its path. Read-only; this is the
//! listing the authoring screens of the macOS app are built on, so the app
//! never has to look inside the home folder itself.
use crate::{artifact::Kind, context::Context, error::Error, names::SkillName};
use serde::Serialize;
use std::{io, path::PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub name: SkillName,
    /// `None` when the folder has no recognised root document yet.
    pub kind: Option<Kind>,
    pub path: PathBuf,
}

pub fn list(context: &Context<'_>) -> Result<Vec<Draft>, Error> {
    let fs = context.fs.as_ref();
    let root = context.home.drafts();
    match fs.symlink_metadata(&root) {
        Ok(info) if info.is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&root, error)),
    }
    let mut drafts = Vec::new();
    for entry in fs.read_dir(&root).map_err(|error| io_error(&root, error))? {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(name) = name.parse::<SkillName>() else {
            continue;
        };
        if !fs
            .symlink_metadata(&entry)
            .map(|info| info.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        drafts.push(Draft {
            kind: Kind::detect(fs, &entry)?,
            path: entry,
            name,
        });
    }
    drafts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(drafts)
}

fn io_error(path: &std::path::Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("reading {}", path.display()),
        source,
        hint: "Check the drafts folder and its permissions.".into(),
    }
}

pub fn text(drafts: &[Draft]) -> String {
    if drafts.is_empty() {
        return "No drafts.".to_owned();
    }
    drafts
        .iter()
        .map(|draft| {
            format!(
                "{} ({}) {}",
                draft.name,
                draft
                    .kind
                    .map_or_else(|| "no root document".to_owned(), |kind| kind.to_string()),
                draft.path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn run(context: &mut Context<'_>) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let drafts = super::read_locked(context, || list(context))?;
    context
        .output
        .result(&drafts, &text(&drafts))
        .map_err(super::output_error)
}
