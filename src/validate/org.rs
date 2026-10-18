//! Organization quality and safety lints. All inspection is read-only; scripts are never run.
use super::{Finding, Rule, Severity, io_error};
use crate::{error::Error, services::fs::Fs, skill::Skill};
use pulldown_cmark::{Event, Parser, Tag};
use regex::Regex;
use std::{
    collections::BTreeSet,
    io,
    path::{Component, Path, PathBuf},
    sync::LazyLock,
};

pub struct OrgRules;

fn finding(
    skill: &Skill,
    id: &str,
    warning: bool,
    message: impl Into<String>,
    hint: &str,
) -> Finding {
    let mut finding = Finding::error(id, skill.dir.join("SKILL.md"), message, hint);
    if warning {
        finding.severity = Severity::Warning;
    }
    finding
}

impl Rule for OrgRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        if skill.version().ok().flatten().is_none() {
            findings.push(finding(
                skill,
                "O101",
                false,
                "metadata.version must be present and parse as semver",
                "Set metadata.version to a quoted semver string.",
            ));
        }
        if let Some(description) = skill.frontmatter.description.value() {
            if description.chars().count() < 80 {
                findings.push(finding(
                    skill,
                    "O102",
                    true,
                    "description is shorter than 80 characters",
                    "Explain the skill and when it should be used in at least 80 characters.",
                ));
            }
            if !description.to_lowercase().contains("use when") {
                findings.push(finding(
                    skill,
                    "O103",
                    true,
                    "description has no use when clause",
                    "Include a use when clause describing the trigger.",
                ));
            }
            if description.contains(['<', '>']) {
                findings.push(finding(
                    skill,
                    "O104",
                    false,
                    "description contains angle brackets",
                    "Remove < and > from description.",
                ));
            }
        }
        if skill.body.lines().count() > 500 {
            findings.push(finding(
                skill,
                "O110",
                true,
                "Body exceeds 500 lines",
                "Move detailed material into references/.",
            ));
        }
        if skill.body.chars().count() > 20_000 {
            findings.push(finding(
                skill,
                "O111",
                true,
                "Body exceeds 5,000 estimated tokens (characters / 4)",
                "Shorten the body and link supporting references.",
            ));
        }
        findings
    }

    fn check_fs(&self, fs: &dyn Fs, skill: &Skill) -> Result<Vec<Finding>, Error> {
        let mut findings = self.check(skill);
        let mut files = Vec::new();
        walk(fs, &skill.dir, &mut files, &mut findings)?;
        let refs = references(&skill.body);
        let mut referenced = BTreeSet::new();
        for reference in refs {
            let path = Path::new(&reference);
            if path.components().any(|part| part == Component::ParentDir)
                || reference.split('/').any(|part| part == "..")
            {
                findings.push(finding(
                    skill,
                    "O123",
                    false,
                    format!("Reference escapes the skill folder: {reference}"),
                    "Use relative paths without .. components.",
                ));
                continue;
            }
            let full = skill.dir.join(path);
            // Never follow symlink ancestors, even if the final component is a file.
            let mut prefix = skill.dir.clone();
            let mut exists = true;
            for part in path.components() {
                prefix.push(part);
                match fs.symlink_metadata(&prefix) {
                    Ok(info) if info.is_symlink() => {
                        exists = false;
                        break;
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        exists = false;
                        break;
                    }
                    Err(error) => return Err(io_error(&prefix, error)),
                }
            }
            if !exists {
                findings.push(finding(
                    skill,
                    "O120",
                    false,
                    format!("Referenced path does not exist or is a symlink: {reference}"),
                    "Create the referenced file or correct the path.",
                ));
            } else {
                referenced.insert(full);
            }
        }
        for path in files {
            let relative = path
                .strip_prefix(&skill.dir)
                .expect("walk stays within skill");
            let category = relative
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str())
                .unwrap_or("");
            if ["scripts", "references", "assets"].contains(&category)
                && !referenced.contains(&path)
            {
                let mut item = finding(
                    skill,
                    "O121",
                    true,
                    "Bundled file is not referenced by the body",
                    "Link this file from SKILL.md or remove it.",
                );
                item.path = path.clone();
                findings.push(item);
            }
            let bytes = fs.read(&path).map_err(|error| io_error(&path, error))?;
            let text = String::from_utf8_lossy(&bytes);
            if category == "references" && text.chars().count() > 40_000 {
                let mut item = finding(
                    skill,
                    "O112",
                    true,
                    "Reference exceeds 10,000 estimated tokens",
                    "Split the reference into smaller focused files.",
                );
                item.path = path.clone();
                findings.push(item);
            }
            if category == "scripts" {
                let info = fs
                    .symlink_metadata(&path)
                    .map_err(|error| io_error(&path, error))?;
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    info.permissions().mode() & 0o111 != 0
                };
                #[cfg(not(unix))]
                let executable = true;
                if !bytes.starts_with(b"#!") || !executable {
                    let mut item = finding(
                        skill,
                        "O130",
                        false,
                        "Script needs a shebang and executable permission",
                        "Add an interpreter shebang and chmod +x the script.",
                    );
                    item.path = path.clone();
                    findings.push(item);
                }
                for (offset, line) in text.lines().enumerate() {
                    let code = line.trim_start();
                    if code.starts_with('#') {
                        continue;
                    }
                    if INTERACTIVE.is_match(line) {
                        let mut item = finding(
                            skill,
                            "O131",
                            true,
                            "Script may request interactive terminal input",
                            "Pass explicit arguments or use the harness to ask the user.",
                        );
                        item.path = path.clone();
                        item.line = Some(offset + 1);
                        findings.push(item);
                    }
                }
                // Join shell continuations before checking download-and-execute patterns.
                let code = text
                    .lines()
                    .filter(|line| !line.trim_start().starts_with('#'))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .replace("\\\r\n", " ")
                    .replace("\\\n", " ");
                if DANGEROUS.is_match(&code) {
                    let mut item = finding(
                        skill,
                        "O132",
                        false,
                        "Script uses sudo or pipes a download into a shell",
                        "Remove privilege escalation and download-and-execute commands.",
                    );
                    item.path = path.clone();
                    findings.push(item);
                }
            }
            // Scan every regular file, including SKILL.md; never echo secret values.
            for (offset, line) in text.lines().enumerate() {
                if SECRET.is_match(line) {
                    let mut item = finding(
                        skill,
                        "O133",
                        false,
                        "Likely secret detected (value redacted)",
                        "Remove the credential, rotate it, and use environment-based credentials.",
                    );
                    item.path = path.clone();
                    item.line = Some(offset + 1);
                    findings.push(item);
                }
            }
        }
        Ok(findings)
    }
}

static INTERACTIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bread\s+(?:-[^\s]*p\b|-[^\s]*r\b)|\binput\s*\(").unwrap());
static DANGEROUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\bsudo\b|\b(?:curl|wget)\b[^|\r\n]*\|\s*(?:(?:/usr)?/bin/)?(?:sh|bash|zsh)\b")
        .unwrap()
});
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----|\b(?:AKIA|ASIA)[A-Z0-9]{16}\b|\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}|xox[baprs]-[A-Za-z0-9-]{20,}|sk-(?:proj-)?[A-Za-z0-9_-]{20,})\b").unwrap()
});

fn walk(
    fs: &dyn Fs,
    dir: &Path,
    files: &mut Vec<PathBuf>,
    findings: &mut Vec<Finding>,
) -> Result<(), Error> {
    let mut entries = fs.read_dir(dir).map_err(|error| io_error(dir, error))?;
    entries.sort();
    for path in entries {
        let info = fs
            .symlink_metadata(&path)
            .map_err(|error| io_error(&path, error))?;
        if info.is_symlink() {
            findings.push(Finding::error(
                "O122",
                path,
                "Symlinks are not allowed inside a skill",
                "Replace the link with a regular file inside the skill.",
            ));
        } else if info.is_dir() {
            walk(fs, &path, files, findings)?;
        } else if info.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

/// CommonMark handles inline, reference-style and image destinations; code paths
/// are restricted to the three bundled-resource roots. External URLs/anchors skip.
fn references(body: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for event in Parser::new(body) {
        let candidate = match event {
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                Some(dest_url.into_string())
            }
            Event::Code(code)
                if ["scripts/", "references/", "assets/"]
                    .iter()
                    .any(|prefix| code.starts_with(prefix)) =>
            {
                Some(code.into_string())
            }
            _ => None,
        };
        if let Some(candidate) = candidate {
            let path = candidate.split(['#', '?']).next().unwrap_or("");
            if path.is_empty() || path.starts_with('/') || path.contains(':') {
                continue;
            }
            let decoded = percent_decode(path);
            if !Path::new(&decoded).is_absolute() {
                paths.insert(decoded);
            }
        }
    }
    paths
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(pair) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(byte) = u8::from_str_radix(pair, 16) {
                    decoded.push(byte);
                    index += 3;
                    continue;
                }
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}
