//! Instruction-file rules I3xx for a project folder: `AGENTS.md`, `CLAUDE.md`,
//! `.claude/CLAUDE.md` and `CLAUDE.local.md`. These files are team-owned and
//! edited only by people, so every rule is read-only and ends in a suggested
//! fix. This module ships I301–I305, I308 and I309; I306 arrives with the work
//! item that needs it.
use super::{Finding, Severity};
use crate::{error::Error, services::fs::Fs};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

/// Budgets from the optimisation strategy: instruction text is paid for on
/// every turn, so each file that always loads gets a line budget.
pub const AGENTS_MAX_LINES: usize = 200;
pub const CURSOR_RULE_MAX_LINES: usize = 500;
pub const ALWAYS_APPLY_MAX_LINES: usize = 100;

/// Heading words that usually introduce a procedure rather than a fact.
const PROCEDURE_WORDS: [&str; 11] = [
    "how to",
    "process",
    "procedure",
    "runbook",
    "checklist",
    "steps",
    "workflow",
    "release",
    "deploy",
    "deployment",
    "migration",
];

/// Import targets whose content changes between sessions, which breaks the
/// cacheable prefix every time they do.
const VOLATILE_IMPORT: [&str; 10] = [
    "log",
    "status",
    "changelog",
    "todo",
    "notes",
    "journal",
    "scratch",
    ".jsonl",
    ".tmp",
    ".git/",
];
/// Words that mark a date on the same line as a generated timestamp rather
/// than a date mentioned in prose.
const STAMP_WORDS: [&str; 8] = [
    "updated",
    "generated",
    "as of",
    "last ",
    "snapshot",
    "timestamp",
    "refreshed",
    "synced",
];

/// The two places Claude Code accepts a project `CLAUDE.md`, with the import
/// spellings that reach `./AGENTS.md` from each (imports resolve relative to the
/// importing file).
const CLAUDE_MD: [(&str, &[&str]); 2] = [
    ("CLAUDE.md", &["@AGENTS.md", "@./AGENTS.md"]),
    (".claude/CLAUDE.md", &["@../AGENTS.md"]),
];

/// Which instruction files a folder has and whether `CLAUDE.md` imports
/// `AGENTS.md`; the input to every rule.
#[derive(Debug, Default)]
pub struct Layout {
    pub agents: Option<PathBuf>,
    pub agents_text: String,
    /// Each existing `CLAUDE.md`, its text, and whether it imports `AGENTS.md`.
    pub claude: Vec<(PathBuf, String, bool)>,
    pub local: Option<PathBuf>,
}

impl Layout {
    pub fn imports_agents(&self) -> bool {
        self.claude.iter().any(|(_, _, imports)| *imports)
    }
}

fn read_if_file(fs: &dyn Fs, path: &Path) -> Result<Option<String>, Error> {
    match fs.symlink_metadata(path) {
        Ok(info) if info.is_file() || info.file_type().is_symlink() => {
            let bytes = fs.read(path).map_err(|source| io_error(path, source))?;
            Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_error(path, source)),
    }
}

/// An `@path` import token on a line, as Claude Code reads it: whitespace
/// delimited, anywhere on the line, not inside quotes.
fn has_import(text: &str, spellings: &[&str]) -> bool {
    text.lines().any(|line| {
        line.split_whitespace().any(|token| {
            let token = token.trim_end_matches([',', ';', ')', ']']);
            spellings.contains(&token)
        })
    })
}

pub fn layout(fs: &dyn Fs, folder: &Path) -> Result<Layout, Error> {
    let mut layout = Layout::default();
    let agents = folder.join("AGENTS.md");
    if let Some(text) = read_if_file(fs, &agents)? {
        layout.agents = Some(agents);
        layout.agents_text = text;
    }
    for (relative, spellings) in CLAUDE_MD {
        let path = folder.join(relative);
        if let Some(text) = read_if_file(fs, &path)? {
            let imports = has_import(&text, spellings);
            layout.claude.push((path, text, imports));
        }
    }
    let local = folder.join("CLAUDE.local.md");
    if read_if_file(fs, &local)?.is_some() {
        layout.local = Some(local);
    }
    Ok(layout)
}

/// Files in `folder`'s ancestors (up to and excluding `stop`, or the filesystem
/// root) that make Claude Code ignore `AGENTS.md`: any `CLAUDE.md`,
/// `.claude/CLAUDE.md` or `CLAUDE.local.md` above the working directory.
pub fn shadowing_ancestors(
    fs: &dyn Fs,
    folder: &Path,
    stop: Option<&Path>,
) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    let mut current = folder.parent();
    while let Some(dir) = current {
        // Never look above the repository: a CLAUDE.md in a parent folder
        // outside it is someone else's instruction file.
        if stop.is_some_and(|stop| !dir.starts_with(stop)) {
            break;
        }
        for name in ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"] {
            let path = dir.join(name);
            if read_if_file(fs, &path)?.is_some() {
                found.push(path);
            }
        }
        if stop.is_some_and(|stop| dir == stop) {
            break;
        }
        current = dir.parent();
    }
    Ok(found)
}

/// Lines worth comparing between the two files: non-blank, not an import,
/// not a bare heading marker, and long enough not to collide by accident.
fn comparable(line: &str) -> Option<&str> {
    let text = line.trim();
    if text.len() < 24 || text.starts_with('@') || text.chars().all(|c| !c.is_alphanumeric()) {
        return None;
    }
    Some(text)
}

fn finding(
    id: &str,
    warning: bool,
    path: PathBuf,
    message: impl Into<String>,
    hint: &str,
) -> Finding {
    let mut finding = Finding::error(id, path, message, hint);
    if warning {
        finding.severity = Severity::Warning;
    }
    finding
}

/// Run the instruction-file rules. `repo_root` bounds the ancestor search for
/// I309; `None` means "not in a repository", which searches to the root.
pub fn check(fs: &dyn Fs, folder: &Path, repo_root: Option<&Path>) -> Result<Vec<Finding>, Error> {
    let layout = layout(fs, folder)?;
    let mut findings = Vec::new();

    // I304: a CLAUDE.md must hand off to AGENTS.md, and AGENTS.md must exist.
    for (path, _, imports) in &layout.claude {
        if !imports {
            findings.push(finding(
                "I304",
                false,
                path.clone(),
                "CLAUDE.md does not import AGENTS.md, so Claude Code and Cursor read different instructions",
                "Move the team-shared instructions into AGENTS.md (Cursor reads it natively) and reduce this file to a line containing @AGENTS.md plus any Claude-only notes.",
            ));
        } else if layout.agents.is_none() {
            findings.push(finding(
                "I304",
                false,
                path.clone(),
                "CLAUDE.md imports AGENTS.md, but AGENTS.md does not exist next to it",
                "Create AGENTS.md with the shared instructions, or remove the import.",
            ));
        }
    }

    // I308: content present in both files is paid for twice and drifts.
    if layout.agents.is_some() {
        let shared: BTreeSet<&str> = layout.agents_text.lines().filter_map(comparable).collect();
        for (path, text, _) in &layout.claude {
            for (offset, line) in text.lines().enumerate() {
                if let Some(text) = comparable(line) {
                    if shared.contains(text) {
                        let mut item = finding(
                            "I308",
                            true,
                            path.clone(),
                            "Line also appears in AGENTS.md; both tools already read it from there",
                            "Delete the duplicate from CLAUDE.md and keep only Claude-only notes beside the @AGENTS.md import.",
                        );
                        item.line = Some(offset + 1);
                        findings.push(item);
                    }
                }
            }
        }
    }

    // I309: AGENTS.md is the source of truth but Claude Code will not read it.
    if let Some(agents) = &layout.agents {
        if !layout.imports_agents() {
            let mut shadows: Vec<PathBuf> = layout.local.iter().cloned().collect();
            shadows.extend(shadowing_ancestors(fs, folder, repo_root)?);
            // A CLAUDE.md in this folder without the import is already I304.
            if !shadows.is_empty() && layout.claude.is_empty() {
                let list = shadows
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                findings.push(finding(
                    "I309",
                    false,
                    agents.clone(),
                    format!(
                        "AGENTS.md is shadowed: Claude Code skips it whenever a CLAUDE.md or CLAUDE.local.md exists in the folder or above it, and found {list}"
                    ),
                    "Add a CLAUDE.md beside AGENTS.md containing the line @AGENTS.md, or set Claude Code's Project instructions to claude-md-and-agents-md.",
                ));
            }
        }
    }

    // I301: AGENTS.md is loaded every turn in both tools; keep it to facts.
    if let Some(agents) = &layout.agents {
        let lines = layout.agents_text.lines().count();
        if lines > AGENTS_MAX_LINES {
            findings.push(finding(
                "I301",
                true,
                agents.clone(),
                format!("AGENTS.md is {lines} lines; the budget is {AGENTS_MAX_LINES} because every line is paid for on every turn in both tools"),
                "Move procedures into skills (loaded only when relevant), path-specific guidance into .claude/rules/ or .cursor/rules/, and reference material into files the agent reads on demand.",
            ));
        }
        // I303: a section that reads like a procedure belongs in a skill.
        for section in sections(&layout.agents_text) {
            if let Some(reason) = procedure(&section) {
                let mut item = finding(
                    "I303",
                    true,
                    agents.clone(),
                    format!(
                        "Section {:?} reads like a procedure ({reason}); a skill loads it only when the task needs it",
                        section.title
                    ),
                    "Create a skill from this section (skillctl new <name>) and leave a one-line pointer to it here.",
                );
                item.line = Some(section.line);
                findings.push(item);
            }
        }
    }

    // I305: anything that changes between sessions in an always-loaded file
    // invalidates the prompt cache for every turn after it.
    let mut always_loaded: Vec<(PathBuf, String)> = Vec::new();
    if let Some(agents) = &layout.agents {
        always_loaded.push((agents.clone(), layout.agents_text.clone()));
    }
    for (path, text, _) in &layout.claude {
        always_loaded.push((path.clone(), text.clone()));
    }
    if let Some(local) = &layout.local {
        if let Some(text) = read_if_file(fs, local)? {
            always_loaded.push((local.clone(), text));
        }
    }
    always_loaded.extend(claude_rules(fs, folder)?);
    always_loaded.extend(cursor_rules(fs, folder)?);
    for (path, text) in &always_loaded {
        for (offset, reason) in volatile_lines(text) {
            let mut item = finding(
                "I305",
                true,
                path.clone(),
                format!(
                    "{reason}; the file is loaded on every turn, so this breaks the cacheable prefix whenever it changes"
                ),
                "Keep always-loaded instruction files byte-identical between sessions: drop timestamps and dynamic content, and import only files that rarely change.",
            );
            item.line = Some(offset + 1);
            findings.push(item);
        }
    }

    // I302: Cursor rules that always apply are AGENTS.md by another name.
    for (path, text) in cursor_rules(fs, folder)? {
        let lines = text.lines().count();
        if lines > CURSOR_RULE_MAX_LINES {
            findings.push(finding(
                "I302",
                true,
                path.clone(),
                format!("Cursor rule is {lines} lines; the budget is {CURSOR_RULE_MAX_LINES}"),
                "Split the rule by topic or move procedures into skills.",
            ));
        }
        if always_apply(&text) && lines > ALWAYS_APPLY_MAX_LINES {
            findings.push(finding(
                "I302",
                true,
                path,
                format!("Always-apply Cursor rule is {lines} lines; it is paid for on every turn, like AGENTS.md (budget {ALWAYS_APPLY_MAX_LINES})"),
                "Scope the rule with globs or a description so it loads on demand, or shorten it.",
            ));
        }
    }
    Ok(findings)
}

/// A heading-delimited part of a Markdown file. The text before the first
/// heading is a section with an empty title at line 1.
struct Section {
    title: String,
    line: usize,
    body: Vec<String>,
}

fn sections(text: &str) -> Vec<Section> {
    let mut sections = vec![Section {
        title: String::new(),
        line: 1,
        body: Vec::new(),
    }];
    let mut in_fence = false;
    for (offset, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if !in_fence && trimmed.starts_with('#') {
            let title = trimmed.trim_start_matches('#').trim();
            if !title.is_empty() {
                sections.push(Section {
                    title: title.to_owned(),
                    line: offset + 1,
                    body: Vec::new(),
                });
                continue;
            }
        }
        sections
            .last_mut()
            .expect("preamble")
            .body
            .push(line.to_owned());
    }
    sections
}

/// Why a section counts as a procedure, or `None`. Strong signals on their
/// own: five or more ordered steps, or three or more code blocks. A heading
/// that names a procedure needs only a few steps or one block to confirm it.
fn procedure(section: &Section) -> Option<String> {
    // Ordered steps count only outside fenced code; a numbered list quoted in
    // a code sample is content, not a procedure.
    let mut steps = 0;
    let mut fences = 0;
    let mut in_fence = false;
    for line in &section.body {
        let text = line.trim_start();
        if text.starts_with("```") || text.starts_with("~~~") {
            fences += 1;
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let digits = text.chars().take_while(char::is_ascii_digit).count();
        if digits > 0
            && text[digits..].starts_with(['.', ')'])
            && text[digits + 1..].starts_with(' ')
        {
            steps += 1;
        }
    }
    let blocks = fences / 2;
    let title = section.title.to_lowercase();
    let named = PROCEDURE_WORDS.iter().any(|word| title.contains(word));
    if steps >= 5 {
        Some(format!("{steps} ordered steps"))
    } else if blocks >= 3 {
        Some(format!("{blocks} code blocks"))
    } else if named && (steps >= 3 || blocks >= 1) {
        Some(format!(
            "heading names a procedure and it has {steps} ordered steps and {blocks} code blocks"
        ))
    } else {
        None
    }
}

/// Lines of an instruction file whose content varies between sessions, with
/// the reason: an ISO timestamp, a date beside a "last updated" style word, a
/// shell expansion or `!`command`` injection, or an import of a volatile file.
/// Fenced code is skipped: a shell example is content, not an instruction.
fn volatile_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (offset, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let lower = line.to_lowercase();
        if iso_timestamp(line) {
            out.push((offset, "line carries a timestamp".to_owned()));
        } else if iso_date(line) && STAMP_WORDS.iter().any(|word| lower.contains(word)) {
            out.push((offset, "line carries a generated date stamp".to_owned()));
        }
        if line.contains("!`") || line.contains("$(") || line.contains("${") {
            out.push((
                offset,
                "line contains a shell expansion or command injection whose output varies"
                    .to_owned(),
            ));
        }
        for token in line.split_whitespace() {
            if let Some(target) = token.strip_prefix('@') {
                let target = target.trim_end_matches([',', ';', ')', ']']).to_lowercase();
                if target.len() > 1 && VOLATILE_IMPORT.iter().any(|needle| target.contains(needle))
                {
                    out.push((
                        offset,
                        format!("import @{target} points at a file that changes often"),
                    ));
                }
            }
        }
    }
    out
}

fn iso_date(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.windows(10).any(|w| {
        w[4] == b'-'
            && w[7] == b'-'
            && w[..4].iter().all(u8::is_ascii_digit)
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[8..].iter().all(u8::is_ascii_digit)
    })
}

fn iso_timestamp(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.windows(16).any(|w| {
        w[4] == b'-'
            && w[7] == b'-'
            && (w[10] == b'T' || w[10] == b' ')
            && w[13] == b':'
            && w[..4].iter().all(u8::is_ascii_digit)
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[8..10].iter().all(u8::is_ascii_digit)
            && w[11..13].iter().all(u8::is_ascii_digit)
            && w[14..16].iter().all(u8::is_ascii_digit)
    })
}

/// `.claude/rules/*.md` files, sorted, with their text (loaded every session
/// unless path-scoped).
fn claude_rules(fs: &dyn Fs, folder: &Path) -> Result<Vec<(PathBuf, String)>, Error> {
    let dir = folder.join(".claude/rules");
    let entries = match fs.read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(io_error(&dir, source)),
    };
    let mut rules = Vec::new();
    for path in entries {
        if path.extension().is_some_and(|ext| ext == "md") {
            if let Some(text) = read_if_file(fs, &path)? {
                rules.push((path, text));
            }
        }
    }
    Ok(rules)
}

/// `.cursor/rules/*.mdc` files, sorted, with their text.
fn cursor_rules(fs: &dyn Fs, folder: &Path) -> Result<Vec<(PathBuf, String)>, Error> {
    let dir = folder.join(".cursor/rules");
    let entries = match fs.read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(io_error(&dir, source)),
    };
    let mut rules = Vec::new();
    for path in entries {
        if path.extension().is_some_and(|ext| ext == "mdc") {
            if let Some(text) = read_if_file(fs, &path)? {
                rules.push((path, text));
            }
        }
    }
    Ok(rules)
}

/// Whether an `.mdc` rule's frontmatter sets `alwaysApply: true`.
fn always_apply(text: &str) -> bool {
    let Ok((yaml, _)) = crate::skill::frontmatter::split(text) else {
        return false;
    };
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml)
        .ok()
        .and_then(|value| value.get("alwaysApply")?.as_bool())
        .unwrap_or(false)
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Read {}", path.display()),
        source,
        hint: "Check the instruction file and its read permissions.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fs::RealFs;
    use std::fs;

    fn ids(folder: &Path, root: Option<&Path>) -> Vec<String> {
        check(&RealFs, folder, root)
            .unwrap()
            .into_iter()
            .map(|f| f.rule)
            .collect()
    }

    #[test]
    fn import_spellings_follow_the_importing_file() {
        assert!(has_import("Notes\n@AGENTS.md\n", &["@AGENTS.md"]));
        assert!(has_import(
            "- Shared rules: @AGENTS.md, then more",
            &["@AGENTS.md"]
        ));
        assert!(!has_import("see \"@AGENTS.md\"", &["@AGENTS.md"]));
        assert!(!has_import("@AGENTS.md.bak\n", &["@AGENTS.md"]));
        assert!(has_import("@../AGENTS.md", &["@../AGENTS.md"]));
    }

    #[test]
    fn clean_layouts_have_no_findings() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert!(ids(dir, None).is_empty());
        fs::write(
            dir.join("AGENTS.md"),
            "# Project\n\nRun cargo test before every commit.\n",
        )
        .unwrap();
        assert!(ids(dir, None).is_empty());
        fs::write(
            dir.join("CLAUDE.md"),
            "@AGENTS.md\n\nUse /compact focus on the task.\n",
        )
        .unwrap();
        assert!(ids(dir, None).is_empty());
        fs::write(dir.join("CLAUDE.local.md"), "my sandbox url\n").unwrap();
        assert!(ids(dir, None).is_empty(), "import makes local harmless");
    }

    #[test]
    fn i301_i302_and_i303_watch_the_budgets() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        // Facts only, within budget: clean.
        let facts =
            "# Project\n\nRun `cargo test` before every commit.\nUse British spelling in docs.\n";
        fs::write(dir.join("AGENTS.md"), facts).unwrap();
        assert!(ids(dir, None).is_empty());
        // 201 lines: I301.
        fs::write(dir.join("AGENTS.md"), "line\n".repeat(201)).unwrap();
        assert_eq!(ids(dir, None), ["I301"]);
        // A five-step list, a three-block section, and a named procedure with
        // one block: three I303 findings at the heading lines. A "Release
        // notes" heading with plain prose is not one.
        let text = format!(
            "{facts}\n## Deploy\n\n1. build\n2. tag\n3. push\n4. wait\n5. verify\n\n## Debugging tips\n\n```\na\n```\n```\nb\n```\n```\nc\n```\n\n## Release notes\n\nWe cut a release every two weeks; see the changelog.\n\n## How to rotate keys\n\n```\nrotate --all\n```\n"
        );
        fs::write(dir.join("AGENTS.md"), &text).unwrap();
        let findings = check(&RealFs, dir, None).unwrap();
        let hits: Vec<(String, Option<usize>)> =
            findings.iter().map(|f| (f.rule.clone(), f.line)).collect();
        assert_eq!(
            hits,
            [
                ("I303".to_owned(), Some(6)),
                ("I303".to_owned(), Some(14)),
                ("I303".to_owned(), Some(30)),
            ],
            "{findings:#?}"
        );
        assert!(findings[0].message.contains("5 ordered steps"));
        assert!(findings[1].message.contains("3 code blocks"));
        // Headings inside fenced code are not sections.
        fs::write(
            dir.join("AGENTS.md"),
            "# Project\n\n```\n# Deploy\n1. a\n2. b\n3. c\n4. d\n5. e\n```\n",
        )
        .unwrap();
        assert!(
            ids(dir, None).is_empty(),
            "fenced headings and steps are not procedures... except the preamble counts steps"
        );
        // Cursor rules: size and always-apply budgets.
        fs::write(dir.join("AGENTS.md"), facts).unwrap();
        let rules = dir.join(".cursor/rules");
        fs::create_dir_all(&rules).unwrap();
        fs::write(
            rules.join("big.mdc"),
            format!("---\ndescription: big\n---\n{}", "x\n".repeat(500)),
        )
        .unwrap();
        fs::write(
            rules.join("always.mdc"),
            format!("---\nalwaysApply: true\n---\n{}", "x\n".repeat(100)),
        )
        .unwrap();
        fs::write(
            rules.join("scoped.mdc"),
            format!(
                "---\nglobs: [\"*.rs\"]\nalwaysApply: false\n---\n{}",
                "x\n".repeat(100)
            ),
        )
        .unwrap();
        fs::write(rules.join("notes.md"), "x\n".repeat(600)).unwrap();
        let findings = check(&RealFs, dir, None).unwrap();
        let hits: Vec<(String, String)> = findings
            .iter()
            .map(|f| {
                (
                    f.rule.clone(),
                    f.path.file_name().unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert_eq!(
            hits,
            [
                ("I302".to_owned(), "always.mdc".to_owned()),
                ("I302".to_owned(), "big.mdc".to_owned()),
            ]
        );
    }

    #[test]
    fn i305_flags_timestamps_dynamic_content_and_volatile_imports_outside_fences() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let text = "# Project\n\nLast updated: 2026-10-19\nGenerated 2026-10-19T13:18:38Z by a script\nThe project started in 2024-03-01 and ships monthly.\nBranch: !`git branch --show-current`\nUse $(pwd) as the root\n@notes/daily.md\n@docs/architecture.md\n\n```sh\necho $(date) 2026-01-01T00:00:00Z @status.log\n```\n";
        fs::write(dir.join("AGENTS.md"), text).unwrap();
        fs::write(dir.join("CLAUDE.md"), "@AGENTS.md\n").unwrap();
        let findings = check(&RealFs, dir, None).unwrap();
        let hits: Vec<(String, Option<usize>)> = findings
            .iter()
            .filter(|f| f.rule == "I305")
            .map(|f| (f.message.split(';').next().unwrap().to_owned(), f.line))
            .collect();
        assert_eq!(
            hits,
            [
                ("line carries a generated date stamp".to_owned(), Some(3)),
                ("line carries a timestamp".to_owned(), Some(4)),
                (
                    "line contains a shell expansion or command injection whose output varies"
                        .to_owned(),
                    Some(6)
                ),
                (
                    "line contains a shell expansion or command injection whose output varies"
                        .to_owned(),
                    Some(7)
                ),
                (
                    "import @notes/daily.md points at a file that changes often".to_owned(),
                    Some(8)
                ),
            ],
            "{findings:#?}"
        );
        // Rules folders are always loaded too.
        fs::write(dir.join("AGENTS.md"), "# Project\n").unwrap();
        fs::create_dir_all(dir.join(".claude/rules")).unwrap();
        fs::write(
            dir.join(".claude/rules/style.md"),
            "Snapshot taken 2026-10-19.\n",
        )
        .unwrap();
        let rules: Vec<String> = check(&RealFs, dir, None)
            .unwrap()
            .into_iter()
            .map(|f| f.rule)
            .collect();
        assert_eq!(rules, ["I305"]);
    }

    #[test]
    fn i304_i308_and_i309_fire_on_their_shapes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        fs::write(
            dir.join("CLAUDE.md"),
            "# Project\n\nRun cargo test before every commit.\n",
        )
        .unwrap();
        assert_eq!(ids(dir, None), ["I304"]);
        fs::write(
            dir.join("AGENTS.md"),
            "# Project\n\nRun cargo test before every commit.\n",
        )
        .unwrap();
        fs::write(
            dir.join("CLAUDE.md"),
            "@AGENTS.md\n\nRun cargo test before every commit.\n",
        )
        .unwrap();
        let findings = check(&RealFs, dir, None).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "I308");
        assert_eq!(findings[0].line, Some(3));
        fs::remove_file(dir.join("AGENTS.md")).unwrap();
        assert_eq!(ids(dir, None), ["I304"]);
        fs::write(dir.join("AGENTS.md"), "# Project\n").unwrap();
        fs::remove_file(dir.join("CLAUDE.md")).unwrap();
        fs::write(dir.join("CLAUDE.local.md"), "x\n").unwrap();
        assert_eq!(ids(dir, None), ["I309"]);
        // A parent CLAUDE.md inside the repository shadows too; outside it does not.
        fs::remove_file(dir.join("CLAUDE.local.md")).unwrap();
        let nested = dir.join("services/api");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("AGENTS.md"), "# api\n").unwrap();
        fs::write(dir.join("CLAUDE.md"), "root notes\n").unwrap();
        assert_eq!(ids(&nested, Some(dir)), ["I309"]);
        assert_eq!(ids(&nested, Some(&nested)), Vec::<String>::new());
    }
}
