//! Script-bundle rules K501–K507. A bundle is `SCRIPT.md` describing the
//! wrappers beside it, linked as the folder `.claude/scripts/<name>` so that
//! `AGENTS.md` can tell agents to run `.claude/scripts/<name>/<wrapper>`
//! instead of a raw command whose output floods the context. The wrappers are
//! held to the same hygiene bar as skill scripts.
use super::{Finding, Rule, Severity, io_error, org};
use crate::{error::Error, names::SkillName, services::fs::Fs, skill::Skill};

/// Rules an allow-list can never suppress (K501 is the runner's gate).
pub const STRUCTURAL: [&str; 5] = ["K501", "K502", "K503", "K504", "K505"];

pub struct ScriptRules;

fn finding(
    skill: &Skill,
    id: &str,
    warning: bool,
    message: impl Into<String>,
    hint: &str,
) -> Finding {
    let mut finding = Finding::error(id, skill.document(), message, hint);
    if warning {
        finding.severity = Severity::Warning;
    }
    finding
}

impl Rule for ScriptRules {
    fn check(&self, skill: &Skill) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fields = &skill.frontmatter;
        match fields.name.value() {
            Some(name) if SkillName::new(name.clone()).is_err() => findings.push(finding(
                skill,
                "K502",
                false,
                "name must match the artifact-name pattern and contain 1–64 characters",
                "Use lowercase ASCII letters and digits separated by single hyphens.",
            )),
            Some(name) if skill.dir.file_name().and_then(|v| v.to_str()) != Some(name.as_str()) => {
                findings.push(finding(
                    skill,
                    "K502",
                    false,
                    "name does not equal the script folder's name",
                    "Make the frontmatter name and folder name identical; the folder is linked as .claude/scripts/<name>.",
                ))
            }
            Some(_) => {}
            None => findings.push(finding(
                skill,
                "K502",
                false,
                "name is required and must be a string",
                "Set name to the bundle's name.",
            )),
        }
        if !fields
            .description
            .value()
            .is_some_and(|value| !value.trim().is_empty() && value.chars().count() <= 1024)
        {
            findings.push(finding(
                skill,
                "K503",
                false,
                "description must be a non-empty string of at most 1,024 characters",
                "Say which commands the wrappers replace and what they print.",
            ));
        }
        if skill.version().ok().flatten().is_none() {
            findings.push(finding(
                skill,
                "K504",
                false,
                "metadata.version must be present and parse as semver",
                "Set metadata.version to a quoted semver string under a metadata mapping.",
            ));
        }
        findings
    }

    fn check_fs(&self, fs: &dyn Fs, skill: &Skill) -> Result<Vec<Finding>, Error> {
        let mut findings = self.check(skill);
        let mut files = Vec::new();
        org::walk(
            fs,
            &skill.dir,
            &mut files,
            &mut findings,
            "K507",
            "Symlinks are not allowed inside a script bundle",
            "Replace the link with a regular file inside the bundle.",
        )?;
        let document = skill.document();
        let mut wrappers = 0;
        for path in files.iter().filter(|path| **path != document) {
            let bytes = fs.read(path).map_err(|error| io_error(path, error))?;
            let text = String::from_utf8_lossy(&bytes);
            let info = fs
                .symlink_metadata(path)
                .map_err(|error| io_error(path, error))?;
            let is_wrapper = bytes.starts_with(b"#!") || org::executable(&info);
            if is_wrapper {
                wrappers += 1;
                if !bytes.starts_with(b"#!") || !org::executable(&info) {
                    let mut item = finding(
                        skill,
                        "K505",
                        false,
                        "wrapper needs a shebang and executable permission",
                        "Add an interpreter shebang and chmod +x the wrapper.",
                    );
                    item.path = path.clone();
                    findings.push(item);
                }
                for (offset, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with('#') {
                        continue;
                    }
                    if org::interactive(line) {
                        let mut item = finding(
                            skill,
                            "K506",
                            true,
                            "wrapper may request interactive terminal input; agents run it unattended",
                            "Take everything from arguments and never prompt.",
                        );
                        item.path = path.clone();
                        item.line = Some(offset + 1);
                        findings.push(item);
                    }
                }
                if org::dangerous(&text) {
                    let mut item = finding(
                        skill,
                        "K506",
                        false,
                        "wrapper uses sudo or pipes a download into a shell",
                        "Remove privilege escalation and download-and-execute commands.",
                    );
                    item.path = path.clone();
                    findings.push(item);
                }
            }
            for (offset, line) in text.lines().enumerate() {
                if org::likely_secret(line) {
                    let mut item = finding(
                        skill,
                        "K506",
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
        if wrappers == 0 {
            findings.push(finding(
                skill,
                "K505",
                false,
                "the bundle has no wrapper: no executable file with a shebang beside SCRIPT.md",
                "Add at least one wrapper script, for example test-quiet.sh.",
            ));
        }
        Ok(findings)
    }
}
