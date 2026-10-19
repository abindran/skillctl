//! Instruction-file rules I3xx for a project folder: `AGENTS.md`, `CLAUDE.md`,
//! `.claude/CLAUDE.md` and `CLAUDE.local.md`. These files are team-owned and
//! edited only by people, so every rule is read-only and ends in a suggested
//! fix. This module ships I304, I308 and I309; the other I3xx rules arrive with
//! the work items that need them.
use super::{Finding, Severity};
use crate::{error::Error, services::fs::Fs};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

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
    Ok(findings)
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
