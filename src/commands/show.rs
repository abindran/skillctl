//! `show <name>[@<version>|@draft]`: the root document of a stored version
//! (default: the newest stable one) or of a draft, with the files beside it.
//! Read-only. The macOS app renders this under the selected version, so the
//! app never has to know where the store keeps things.
use crate::{
    artifact::Kind, context::Context, error::Error, names::SkillName, services::fs::Fs, store,
};
use semver::Version;
use serde::Serialize;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// Path relative to the artifact folder.
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Shown {
    pub name: SkillName,
    /// A semantic version, or `draft`.
    pub version: String,
    pub kind: Kind,
    pub dir: PathBuf,
    /// The root document's file name (`SKILL.md`, `HOOK.md`, …).
    pub document: String,
    pub content: String,
    pub files: Vec<Entry>,
}

enum Target {
    Draft,
    Version(Version),
    Latest,
}

fn parse_target(target: &str) -> Result<(SkillName, Target), Error> {
    let (name, rest) = match target.split_once('@') {
        Some((name, rest)) => (name, Some(rest)),
        None => (target, None),
    };
    let name: SkillName = name.parse().map_err(|_| Error::Usage {
        message: format!("{name:?} is not an artifact name"),
        hint: "Use lowercase letters, digits and hyphens, optionally followed by @<version> or @draft.".into(),
    })?;
    let which =
        match rest {
            None => Target::Latest,
            Some("draft") => Target::Draft,
            Some(text) => Target::Version(text.parse().map_err(|_| {
                Error::Usage {
                message: format!("{text:?} is not a semantic version"),
                hint: "Write the version as in `skillctl artifacts`, for example 1.2.0, or @draft."
                    .into(),
            }
            })?),
        };
    Ok((name, which))
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("reading {}", path.display()),
        source,
        hint: "Check the folder and its permissions.".into(),
    }
}

/// Every regular file under `dir`, relative to `root`, sorted.
fn walk(fs: &dyn Fs, root: &Path, dir: &Path, out: &mut Vec<Entry>) -> Result<(), Error> {
    for entry in fs.read_dir(dir).map_err(|error| io_error(dir, error))? {
        let info = fs
            .symlink_metadata(&entry)
            .map_err(|error| io_error(&entry, error))?;
        if info.is_dir() {
            walk(fs, root, &entry, out)?;
        } else if info.is_file() {
            out.push(Entry {
                path: entry
                    .strip_prefix(root)
                    .unwrap_or(&entry)
                    .to_string_lossy()
                    .into_owned(),
                bytes: info.len(),
            });
        }
    }
    Ok(())
}

pub fn load(context: &Context<'_>, target: &str) -> Result<Shown, Error> {
    let (name, which) = parse_target(target)?;
    let fs = context.fs.as_ref();
    let (dir, version) = match which {
        Target::Draft => (context.home.draft_dir(&name), "draft".to_owned()),
        Target::Version(version) => (context.home.store_dir(&name, &version), version.to_string()),
        Target::Latest => {
            if store::list_versions(fs, &context.home, &name)?.is_empty() {
                return Err(Error::Usage {
                    message: format!("No stored version of {name}"),
                    hint: "Run skillctl artifacts to see what is in the store, or show <name>@draft for a draft.".into(),
                });
            }
            let version = store::latest_stable(fs, &context.home, &name)?;
            (context.home.store_dir(&name, &version), version.to_string())
        }
    };
    let exists = fs
        .symlink_metadata(&dir)
        .map(|info| info.is_dir())
        .unwrap_or(false);
    if !exists {
        return Err(Error::Usage {
            message: format!("{name}@{version} is not in the store or drafts"),
            hint: "Run skillctl artifacts and skillctl drafts to see what exists.".into(),
        });
    }
    let Some(kind) = Kind::detect(fs, &dir)? else {
        return Err(Error::Usage {
            message: format!("{} has no root document", dir.display()),
            hint: "A folder needs SKILL.md, HOOK.md, RULE.mdc, COMMAND.md, AGENT.md or SCRIPT.md."
                .into(),
        });
    };
    let document_path = dir.join(kind.document());
    let content = String::from_utf8_lossy(
        &fs.read(&document_path)
            .map_err(|error| io_error(&document_path, error))?,
    )
    .into_owned();
    let mut files = Vec::new();
    walk(fs, &dir, &dir, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Shown {
        name,
        version,
        kind,
        dir,
        document: kind.document().to_owned(),
        content,
        files,
    })
}

pub fn run(context: &mut Context<'_>, target: &str) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let shown = super::read_locked(context, || load(context, target))?;
    let text = shown.content.clone();
    context
        .output
        .result(&shown, &text)
        .map_err(super::output_error)
}
