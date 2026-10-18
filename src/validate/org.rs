//! Organization quality and safety lints. All inspection is read-only; scripts are never run.
use super::{Finding, Rule, Severity, io_error};
use crate::{
    error::Error,
    services::fs::Fs,
    skill::{Skill, frontmatter::Field},
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
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
        // A non-mapping metadata block is already S007; do not cascade into O101.
        let metadata_invalid = matches!(skill.frontmatter.metadata, Field::Invalid(_));
        if !metadata_invalid && skill.version().ok().flatten().is_none() {
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
        let mut referenced_dirs = BTreeSet::new();
        for reference in refs {
            let path = Path::new(&reference);
            if path.is_absolute()
                || path.components().any(|part| part == Component::ParentDir)
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
            let mut is_dir = false;
            for part in path.components() {
                prefix.push(part);
                match fs.symlink_metadata(&prefix) {
                    Ok(info) if info.is_symlink() => {
                        exists = false;
                        break;
                    }
                    Ok(info) => is_dir = info.is_dir(),
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
            } else if is_dir {
                // Referencing a folder (for example `references/`) covers its files.
                referenced_dirs.insert(full);
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
                && !referenced_dirs.iter().any(|dir| path.starts_with(dir))
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
                    if interactive(line) {
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
                if likely_secret(line) {
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

static INPUT_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\binput\s*\(").unwrap());
// `read -p`, `read -s`, `read -rp`: prompting or silent reads always target a terminal.
static READ_PROMPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bread\s+(?:-\S*\s+)*-[A-Za-z]*[ps]\b").unwrap());
static READ_BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bread\b").unwrap());
// Any number of intermediate pipes, `env`, and absolute interpreter paths.
static DANGEROUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)\bsudo\b|\b(?:curl|wget)\b[^\r\n]*\|\s*(?:(?:/usr)?/bin/)?(?:env\s+)?(?:sh|bash|zsh)\b",
    )
    .unwrap()
});
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----|\b(?:AKIA|ASIA)[A-Z0-9]{16}\b|\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}|xox[baprs]-[A-Za-z0-9-]{20,})\b").unwrap()
});
// OpenAI-style keys are mixed-case alphanumerics; hyphenated prose like
// `sk-learn-based-pipeline` must not match, so the tail is checked in code.
static OPENAI_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk-(?:proj-)?[A-Za-z0-9_-]{20,}\b").unwrap());

fn likely_secret(line: &str) -> bool {
    SECRET.is_match(line)
        || OPENAI_KEY.find_iter(line).any(|found| {
            let tail = found
                .as_str()
                .trim_start_matches("sk-")
                .trim_start_matches("proj-");
            tail.chars().any(|c| c.is_ascii_digit())
                && tail.chars().any(|c| c.is_ascii_uppercase())
                && tail.chars().any(|c| c.is_ascii_lowercase())
        })
}

/// Terminal reads only: `read` fed by a redirect, a pipe, or a loop header
/// (`while read -r line; do … done < file`) is ordinary data processing.
fn interactive(line: &str) -> bool {
    if INPUT_CALL.is_match(line) || READ_PROMPT.is_match(line) {
        return true;
    }
    let Some(found) = READ_BARE.find(line) else {
        return false;
    };
    if line.contains("/dev/tty") {
        return true;
    }
    let before = &line[..found.start()];
    let header = line.trim_start();
    let loop_header = ["while ", "until ", "for "]
        .iter()
        .any(|keyword| header.starts_with(keyword));
    !line.contains('<') && !before.contains('|') && !loop_header
}

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

const RESOURCE_ROOTS: [&str; 3] = ["scripts/", "references/", "assets/"];

/// A whitespace-delimited token naming a bundled resource, with trailing
/// punctuation such as `,` or `)` removed.
fn resource_token(token: &str) -> Option<String> {
    let token = token.trim_end_matches(['`', '\'', '"', ')', ',', ';', ':']);
    RESOURCE_ROOTS
        .iter()
        .any(|prefix| token.starts_with(prefix))
        .then(|| token.to_owned())
}

/// CommonMark handles inline, reference-style and image destinations. Inline code
/// counts its first token (`scripts/run.sh --help` references the script) and
/// fenced blocks count every resource token. External URLs and anchors skip;
/// absolute paths are kept so O123 can reject them.
fn references(body: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let mut in_code_block = false;
    for event in Parser::new(body) {
        let candidates: Vec<String> = match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                Vec::new()
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                Vec::new()
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                vec![dest_url.into_string()]
            }
            Event::Code(code) => code
                .split_whitespace()
                .next()
                .and_then(resource_token)
                .into_iter()
                .collect(),
            Event::Text(text) if in_code_block => {
                text.split_whitespace().filter_map(resource_token).collect()
            }
            _ => Vec::new(),
        };
        for candidate in candidates {
            let path = candidate.split(['#', '?']).next().unwrap_or("");
            if path.is_empty() || path.contains(':') {
                continue;
            }
            paths.insert(percent_decode(path));
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
