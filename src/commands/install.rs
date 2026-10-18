//! Install skills from a Git URL or a local folder into the read-only store.
//!
//! The clone happens before the registry lock is taken (it needs no registry
//! state and may be slow). Every selected skill is validated before any of them
//! is staged, so a multi-skill install never stops halfway through the set.
use crate::{
    context::Context,
    error::Error,
    skill::Skill,
    sources::{self, Discovered, Source, git::Checkout},
    store::{
        drafts::{DocumentFs, rewrite},
        hash::content_hash,
        meta::{Metadata, Source as Provenance},
        stage::stage_version,
    },
    validate,
};
use semver::Version;
use std::{io, path::Path, time::SystemTime};

pub fn run(
    context: &mut Context<'_>,
    source: &str,
    reference: Option<&str>,
    subdir: Option<&Path>,
    all: bool,
    version: Option<&str>,
) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let requested = version
        .map(|text| {
            Version::parse(text).map_err(|error| {
                super::attach::usage(format!("Invalid --version {text:?}: {error}"))
            })
        })
        .transpose()?;
    let source = Source::classify(context.fs.as_ref(), source)?;
    let checkout = match &source {
        Source::Git { url } => Some(Checkout::new(context.git.as_ref(), url, reference)?),
        Source::Local(_) => None,
    };
    let root = match (&source, &checkout) {
        (Source::Local(path), _) => path.clone(),
        (Source::Git { .. }, Some(checkout)) => checkout.root(),
        (Source::Git { .. }, None) => unreachable!("git sources are cloned"),
    };
    let found = sources::discover(context.fs.as_ref(), &root, subdir)?;
    if found.is_empty() {
        return Err(super::attach::usage(format!(
            "No SKILL.md found under {}",
            root.display()
        )));
    }
    let selected: Vec<&Discovered> = if found.len() > 1 && !all {
        let items = found
            .iter()
            .map(|item| {
                item.subdir
                    .clone()
                    .unwrap_or_else(|| item.dir.display().to_string())
            })
            .collect::<Vec<_>>();
        context.prompt.set_answer_flag("--all");
        let picks = context.prompt.multi_select(
            "Select skills to install",
            &items,
            &vec![true; items.len()],
        )?;
        picks
            .into_iter()
            .map(|index| {
                found
                    .get(index)
                    .ok_or_else(|| super::attach::usage("Invalid skill choice"))
            })
            .collect::<Result<_, _>>()?
    } else {
        found.iter().collect()
    };
    if selected.is_empty() {
        return Err(super::attach::usage("No skills selected"));
    }
    super::pins::write_locked(context, |context| {
        let fs = context.fs.as_ref();
        let registry = super::load_registry(context)?;
        let allow = &registry.settings.validate.allow;
        let mut plans = Vec::new();
        let mut failures = Vec::new();
        for item in &selected {
            let name = item.name()?;
            // A declared version wins; --version only fills a missing one, and
            // the filled-in document is what gets validated, hashed and stored.
            let declared = Skill::load(fs, &item.dir)
                .ok()
                .and_then(|skill| skill.version().ok().flatten());
            let document = match (&declared, &requested) {
                (None, Some(requested)) => {
                    let path = item.dir.join("SKILL.md");
                    fs.read(&path)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .and_then(|text| rewrite(&text, None, Some(requested), false).ok())
                        .map(String::into_bytes)
                }
                _ => None,
            };
            let overlay = DocumentFs {
                fs,
                path: item.dir.join("SKILL.md"),
                document,
            };
            let findings = validate::run_all(&overlay, &item.dir, allow, false)?;
            if findings
                .iter()
                .any(|finding| finding.severity == validate::Severity::Error)
            {
                failures.push(validate::report::text(&findings));
                continue;
            }
            let version = match (declared, &requested) {
                (Some(declared), None) => declared,
                (Some(declared), Some(requested)) if declared == *requested => declared,
                (Some(declared), Some(requested)) => {
                    return Err(super::attach::usage(format!(
                        "{name} declares metadata.version {declared}; --version {requested} does not match"
                    )));
                }
                (None, Some(requested)) => requested.clone(),
                (None, None) => {
                    return Err(Error::Validation {
                        summary: format!("{name} has no metadata.version"),
                        hint: "Pass --version <semver> to record one in the stored copy, or add metadata.version to SKILL.md.".into(),
                    });
                }
            };
            plans.push((*item, name, version, overlay.document));
        }
        if !failures.is_empty() {
            return Err(Error::Validation {
                summary: failures.join("\n"),
                hint: "Fix the reported findings in the source and install again.".into(),
            });
        }
        let mut results = Vec::new();
        let mut lines = Vec::new();
        for (item, name, version, document) in plans {
            let overlay = DocumentFs {
                fs,
                path: item.dir.join("SKILL.md"),
                document,
            };
            let hash = content_hash(&overlay, &item.dir)?;
            let version_dir = context.home.version_dir(&name, &version);
            match fs.symlink_metadata(&version_dir) {
                Ok(_) => {
                    let existing = Metadata::load(fs, &version_dir.join("meta.json"))?;
                    if existing.content_hash == hash {
                        lines.push(format!("Already installed {name}@{version}"));
                        results.push(serde_json::json!({
                            "name": name, "version": version, "status": "already_installed", "path": version_dir
                        }));
                        continue;
                    }
                    return Err(Error::Io {
                        operation: format!("Install {name}@{version}"),
                        source: io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "this version is already installed with different content",
                        ),
                        hint: "Bump metadata.version in the source (or pass a new --version) and install again; skillctl gc removes unused versions.".into(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_error(&version_dir, source)),
            }
            let provenance = match &source {
                Source::Local(_) => Provenance::Local,
                Source::Git { url } => Provenance::Git {
                    url: url.clone(),
                    reference: reference.map(str::to_owned),
                    commit: checkout.as_ref().expect("git checkout").commit.clone(),
                    subdir: item.subdir.clone(),
                },
            };
            let metadata = Metadata {
                name: name.clone(),
                version: version.clone(),
                source: provenance,
                content_hash: hash,
                installed_at: SystemTime::now().into(),
                skillctl_version: Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| {
                    Error::Validation {
                        summary: error.to_string(),
                        hint: "Report this as a skillctl bug.".into(),
                    }
                })?,
            };
            let path = stage_version(&overlay, &context.home, &item.dir, &metadata)?;
            lines.push(format!(
                "{}{name}@{version} at {}",
                if context.flags.dry_run {
                    "Would install "
                } else {
                    "Installed "
                },
                path.display()
            ));
            results.push(serde_json::json!({
                "name": name, "version": version, "status": "installed", "path": path
            }));
        }
        context
            .output
            .result(
                &serde_json::json!({"installed": results, "dry_run": context.flags.dry_run}),
                &lines.join("\n"),
            )
            .map_err(super::output_error)
    })
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Read {}", path.display()),
        source,
        hint: "Check the source files and store permissions.".into(),
    }
}
