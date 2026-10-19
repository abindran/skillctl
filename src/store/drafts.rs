//! Writable authoring and validated publication. Callers hold the exclusive registry lock.
use super::{
    hash::content_hash,
    meta::{Metadata, Source},
    stage::stage_version,
};
use crate::{
    artifact::Kind,
    cli::Bump,
    commands::init::{DEFAULT_HOOK_SCRIPT, DEFAULT_HOOK_TEMPLATE, DEFAULT_TEMPLATE},
    error::Error,
    names::SkillName,
    paths::Paths,
    services::fs::Fs,
    skill::{Skill, frontmatter},
    validate,
};
use semver::Version;
use serde_yaml_ng::Value;
use std::{
    io,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The template a kind scaffolds from when none is named.
fn default_template(kind: Kind) -> &'static str {
    match kind {
        Kind::Skill => "default",
        Kind::Hook => "hook",
    }
}

/// Scaffold a valid draft. Existing drafts are never overwritten, even if empty.
/// A skill draft gets `SKILL.md` plus empty `scripts/` and `references/`; a hook
/// draft gets `HOOK.md` plus every other regular file in the template folder
/// (the entry script), with `{{name}}` substituted and the exec bit preserved.
pub fn new(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    template: Option<&str>,
    kind: Kind,
    dry_run: bool,
) -> Result<PathBuf, Error> {
    let template = template.unwrap_or(default_template(kind));
    // Template names are single safe components, not arbitrary filesystem paths.
    SkillName::new(template).map_err(|_| {
        invalid("Invalid template name; use lowercase letters, digits and single hyphens")
    })?;
    let template_dir = paths.templates().join(template);
    for dir in [paths.templates(), template_dir.clone()] {
        real_dir(fs, &dir, dry_run)?;
    }
    let builtin = dry_run && template == default_template(kind);
    let path = template_dir.join(kind.document());
    let (document, mut extras) = match fs.symlink_metadata(&path) {
        Ok(info) if info.is_file() => {
            let document = String::from_utf8(op(fs.read(&path), &path)?)
                .map_err(|_| invalid("Template is not valid UTF-8"))?;
            (
                document,
                template_files(fs, &template_dir, kind.document())?,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound && builtin => match kind {
            Kind::Skill => (DEFAULT_TEMPLATE.into(), Vec::new()),
            Kind::Hook => (
                DEFAULT_HOOK_TEMPLATE.into(),
                vec![(
                    "hook.py".to_owned(),
                    DEFAULT_HOOK_SCRIPT.as_bytes().to_vec(),
                    true,
                )],
            ),
        },
        Err(error) => return Err(io_error(&path, error)),
        Ok(_) => {
            return Err(invalid(format!(
                "Template {} must be a regular file, not a symlink",
                kind.document()
            )));
        }
    };
    let document = document.replace("{{name}}", name.as_str());
    let document = rewrite(
        &document,
        Some(name),
        Some(&Version::new(0, 1, 0)),
        Some(kind),
    )?;
    for (_, bytes, _) in &mut extras {
        if let Ok(text) = std::str::from_utf8(bytes) {
            *bytes = text.replace("{{name}}", name.as_str()).into_bytes();
        }
    }
    let destination = paths.draft_dir(name);
    let parsed = Skill::parse_kind(&destination, &document, kind)?;
    let findings = match kind {
        Kind::Skill => validate::Rule::check(&validate::spec::SpecRules, &parsed),
        Kind::Hook => validate::Rule::check(&validate::hook::HookRules, &parsed),
    };
    require_valid(&findings)?;
    real_dir(fs, &paths.drafts(), dry_run)?;
    missing(fs, &destination)?;
    let mut created = Vec::new();
    let result = (|| {
        op(fs.create_dir(&destination), &destination)?;
        created.push((destination.clone(), true));
        if kind == Kind::Skill {
            for child in ["scripts", "references"] {
                let path = destination.join(child);
                op(fs.create_dir(&path), &path)?;
                created.push((path, true));
            }
        }
        let path = destination.join(kind.document());
        created.push((path.clone(), false));
        op(fs.write_atomic(&path, document.as_bytes()), &path)?;
        op(fs.set_mode(&path, 0o644), &path)?;
        for (file, bytes, executable) in &extras {
            let path = destination.join(file);
            created.push((path.clone(), false));
            op(fs.write_atomic(&path, bytes), &path)?;
            op(
                fs.set_mode(&path, if *executable { 0o755 } else { 0o644 }),
                &path,
            )?;
        }
        Ok(destination.clone())
    })();
    if result.is_err() {
        for (path, directory) in created.iter().rev() {
            let cleanup = if *directory {
                fs.remove_dir(path)
            } else {
                fs.remove_file(path)
            };
            if let Err(source) = cleanup {
                if source.kind() != io::ErrorKind::NotFound {
                    return Err(io_error(path, source));
                }
            }
        }
    }
    result
}

/// Regular files beside the template document, as (name, bytes, executable).
/// Subfolders and symlinks in a template are ignored.
fn template_files(
    fs: &dyn Fs,
    template_dir: &Path,
    document: &str,
) -> Result<Vec<(String, Vec<u8>, bool)>, Error> {
    let mut files = Vec::new();
    for entry in op(fs.read_dir(template_dir), template_dir)? {
        let Some(file) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file == document {
            continue;
        }
        let info = op(fs.symlink_metadata(&entry), &entry)?;
        if !info.is_file() || info.file_type().is_symlink() {
            continue;
        }
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            info.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        files.push((file.to_owned(), op(fs.read(&entry), &entry)?, executable));
    }
    Ok(files)
}

/// Seal using a read overlay for a requested bump: failed publication never edits
/// the draft. Only after staging succeeds is the original draft removed.
pub fn seal(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    bump: Option<Bump>,
) -> Result<PathBuf, Error> {
    seal_with_allow(fs, paths, name, bump, &[])
}

/// Seal with the registry's global org/harness suppression list.
pub fn seal_with_allow(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    bump: Option<Bump>,
    allow: &[String],
) -> Result<PathBuf, Error> {
    real_dir(fs, &paths.drafts(), false)?;
    let draft = paths.draft_dir(name);
    real_dir(fs, &draft, false)?;
    let findings = validate::run_all(fs, &draft, allow, false)?;
    require_valid(&findings)?;
    let kind = Kind::of(fs, &draft)?;
    let skill = Skill::load_kind(fs, &draft, kind)?;
    let mut version = skill.version()?.ok_or_else(|| Error::Validation {
        summary: "metadata.version is required to seal a draft".into(),
        hint: format!(
            "Set metadata.version to a semver string in the draft's {}.",
            kind.document()
        ),
    })?;
    let path = draft.join(kind.document());
    let document = if let Some(bump) = bump {
        version = bumped(version, bump)?;
        let bytes = op(fs.read(&path), &path)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| invalid(format!("{} is not valid UTF-8", kind.document())))?;
        Some(rewrite(text, None, Some(&version), None)?.into_bytes())
    } else {
        None
    };
    let overlay = DocumentFs { fs, path, document };
    let metadata = Metadata {
        name: name.clone(),
        version,
        kind,
        source: Source::Local,
        content_hash: content_hash(&overlay, &draft)?,
        installed_at: SystemTime::now().into(),
        skillctl_version: Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|error| invalid(error.to_string()))?,
    };
    let destination = stage_version(&overlay, paths, &draft, &metadata)?;
    remove_tree(fs, &draft).map_err(|error| Error::Io {
        operation: format!(
            "Version was published at {}, but draft cleanup failed: {error}",
            destination.display()
        ),
        source: io::Error::other(error.to_string()),
        hint: format!(
            "The sealed version is safe. Remove the remaining draft at {} before creating another.",
            draft.display()
        ),
    })?;
    Ok(destination)
}

/// Copy a stored version into a writable draft with its patch version bumped.
/// Without an explicit version the highest stable release is used (falling back
/// to the highest prerelease). An existing draft is replaced only with `force`.
pub fn edit(
    fs: &dyn Fs,
    paths: &Paths,
    name: &SkillName,
    version: Option<&Version>,
    force: bool,
) -> Result<PathBuf, Error> {
    let version = match version {
        Some(version) => version.clone(),
        None => super::latest_stable(fs, paths, name)
            .or_else(|error| super::list_versions(fs, paths, name)?.pop().ok_or(error))?,
    };
    super::resolve_pin(
        fs,
        paths,
        name,
        Some(&crate::registry::Pin::Version(version.clone())),
    )?;
    let source = paths.store_dir(name, &version);
    real_dir(fs, &paths.drafts(), false)?;
    let draft = paths.draft_dir(name);
    match fs.symlink_metadata(&draft) {
        Ok(_) if !force => {
            return Err(Error::Usage {
                message: format!("Draft {name} already exists at {}", draft.display()),
                hint: "Pass --force to replace it, or seal or remove the existing draft first."
                    .into(),
            });
        }
        Ok(_) => remove_tree(fs, &draft)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&draft, error)),
    }
    let next = bumped(version, Bump::Patch)?;
    let kind = Kind::of(fs, &source)?;
    let document_path = source.join(kind.document());
    let text = String::from_utf8(op(fs.read(&document_path), &document_path)?)
        .map_err(|_| invalid(format!("{} is not valid UTF-8", kind.document())))?;
    let document = rewrite(&text, None, Some(&next), None)?;
    let result = (|| {
        copy_tree(fs, &source, &draft)?;
        let path = draft.join(kind.document());
        op(fs.write_atomic(&path, document.as_bytes()), &path)?;
        op(fs.set_mode(&path, 0o644), &path)?;
        Ok(draft.clone())
    })();
    if result.is_err() {
        // Best effort: never leave a half-copied draft behind the error.
        let _ = remove_tree(fs, &draft);
    }
    result
}

/// Writable copy of a (read-only) store tree: folders 0755, files 0644 or 0755.
fn copy_tree(fs: &dyn Fs, from: &Path, to: &Path) -> Result<(), Error> {
    op(fs.create_dir(to), to)?;
    for entry in op(fs.read_dir(from), from)? {
        let info = op(fs.symlink_metadata(&entry), &entry)?;
        let target = to.join(entry.file_name().expect("directory entry"));
        if info.file_type().is_symlink() {
            return Err(invalid(format!(
                "{} is a symlink; stored versions never contain links",
                entry.display()
            )));
        } else if info.is_dir() {
            copy_tree(fs, &entry, &target)?;
        } else if info.is_file() {
            let bytes = op(fs.read(&entry), &entry)?;
            op(fs.write_atomic(&target, &bytes), &target)?;
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                if info.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }
            };
            #[cfg(not(unix))]
            let mode = 0o644;
            op(fs.set_mode(&target, mode), &target)?;
        }
    }
    Ok(())
}

fn require_valid(findings: &[validate::Finding]) -> Result<(), Error> {
    if findings
        .iter()
        .any(|finding| finding.severity == validate::Severity::Error)
    {
        return Err(Error::Validation {
            summary: validate::report::text(findings),
            hint: "Fix the reported findings before sealing or scaffolding.".into(),
        });
    }
    Ok(())
}

pub(crate) fn bumped(mut version: Version, bump: Bump) -> Result<Version, Error> {
    let increment = |number: u64| {
        number
            .checked_add(1)
            .ok_or_else(|| invalid("Version component overflow"))
    };
    match bump {
        Bump::Major => {
            version.major = increment(version.major)?;
            version.minor = 0;
            version.patch = 0;
        }
        Bump::Minor => {
            version.minor = increment(version.minor)?;
            version.patch = 0;
        }
        Bump::Patch => version.patch = increment(version.patch)?,
    }
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    Ok(version)
}

/// Rewrite frontmatter fields in place. `scaffold` names the kind being
/// scaffolded, which also fills in an empty description.
pub(crate) fn rewrite(
    document: &str,
    name: Option<&SkillName>,
    version: Option<&Version>,
    scaffold: Option<Kind>,
) -> Result<String, Error> {
    let (yaml, body) = frontmatter::split(document)?;
    let mut value: Value =
        serde_yaml_ng::from_str(yaml).map_err(|error| invalid(error.to_string()))?;
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| invalid("Template frontmatter must be a mapping"))?;
    if let Some(name) = name {
        map.insert(Value::from("name"), Value::from(name.as_str()));
    }
    if let Some(kind) = scaffold {
        if map
            .get(Value::from("description"))
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            map.insert(
                Value::from("description"),
                Value::from(match kind {
                    Kind::Skill => {
                        "Describe what this skill does and use when its specialized guidance is needed."
                    }
                    Kind::Hook => {
                        "Describe which lifecycle events this hook handles and what it does when they fire."
                    }
                }),
            );
        }
    }
    if let Some(version) = version {
        let metadata = map
            .entry(Value::from("metadata"))
            .or_insert_with(|| Value::Mapping(Default::default()));
        let metadata = metadata
            .as_mapping_mut()
            .ok_or_else(|| invalid("metadata must be a mapping"))?;
        metadata.insert(Value::from("version"), Value::from(version.to_string()));
    }
    let yaml = serde_yaml_ng::to_string(&value).map_err(|error| invalid(error.to_string()))?;
    Ok(format!("---\n{yaml}---\n{body}"))
}

pub(crate) fn remove_tree(fs: &dyn Fs, dir: &Path) -> Result<(), Error> {
    // Drafts copied from the read-only store keep 0555 folders; restore write
    // permission before deleting children so cleanup cannot stop halfway.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let info = op(fs.symlink_metadata(dir), dir)?;
        if info.is_dir() && info.permissions().mode() & 0o200 == 0 {
            op(fs.set_mode(dir, 0o755), dir)?;
        }
    }
    for path in op(fs.read_dir(dir), dir)? {
        let info = op(fs.symlink_metadata(&path), &path)?;
        if info.is_dir() {
            remove_tree(fs, &path)?;
        } else {
            op(fs.remove_file(&path), &path)?;
        }
    }
    op(fs.remove_dir(dir), dir)
}

fn real_dir(fs: &dyn Fs, path: &Path, allow_missing: bool) -> Result<(), Error> {
    match fs.symlink_metadata(path) {
        Ok(info) if info.is_dir() => Ok(()),
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
        Ok(_) => Err(invalid(format!(
            "{} must be a real directory",
            path.display()
        ))),
    }
}
fn missing(fs: &dyn Fs, path: &Path) -> Result<(), Error> {
    match fs.symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
        Ok(_) => Err(invalid(format!(
            "{} already exists; existing drafts are never overwritten",
            path.display()
        ))),
    }
}
fn op<T>(result: io::Result<T>, path: &Path) -> Result<T, Error> {
    result.map_err(|source| io_error(path, source))
}
fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Update draft {}", path.display()),
        source,
        hint: "Check the draft path and filesystem permissions.".into(),
    }
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Usage { message: message.into(), hint: "Check the template, draft and version; existing drafts and store versions are never overwritten.".into() }
}

/// Read overlay: one document is served from memory, everything else passes
/// through. Lets sealing and installing hash and copy a rewritten root
/// document without editing the source.
pub(crate) struct DocumentFs<'a> {
    pub(crate) fs: &'a dyn Fs,
    pub(crate) path: PathBuf,
    pub(crate) document: Option<Vec<u8>>,
}
impl Fs for DocumentFs<'_> {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        if path == self.path {
            if let Some(bytes) = &self.document {
                return Ok(bytes.clone());
            }
        }
        self.fs.read(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        self.fs.symlink_metadata(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.fs.read_link(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.fs.read_dir(path)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.fs.create_dir(path)
    }
    fn symlink(&self, target: &Path, link: &Path) -> io::Result<()> {
        self.fs.symlink(target, link)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.fs.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.fs.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.fs.remove_dir(path)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.fs.write_atomic(path, bytes)
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        self.fs.set_mode(path, mode)
    }
}
