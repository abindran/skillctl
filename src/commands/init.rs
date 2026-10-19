//! Home initialization. Existing files, including user-edited templates, are never replaced.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::context::Context;
use crate::error::Error;
use crate::registry::{Registry, lock::RegistryLock};
use crate::services::fs::Fs;

/// `new` substitutes the artifact name for `{{name}}` when scaffolding a draft.
pub const DEFAULT_TEMPLATE: &str = "---\nname: {{name}}\ndescription: ALWAYS invoke this skill when the user asks to [describe the task], including indirect phrasing like \"[example request]\" or \"[another example]\". Do not [do the task] directly without this skill.\nmetadata:\n  version: \"0.1.0\"\n---\n\n# {{name}}\n\n## Instructions\n\nDescribe the steps to follow when using this skill.\n";

/// The hook scaffold: a `HOOK.md` declaring one event and an entry script.
pub const DEFAULT_HOOK_TEMPLATE: &str = "---\nname: {{name}}\ndescription: Describe which lifecycle events this hook handles and what it records or enforces when they fire.\nmetadata:\n  version: \"0.1.0\"\nevents: [SessionEnd]\nentry: hook.py\n---\n\n# {{name}}\n\nRuns on the events listed in the frontmatter. The harness passes the event as\nJSON on stdin; exit 0 to continue, or 2 to block where the event allows it.\n";

/// The hook entry script scaffold. It must never fail the harness, so malformed\n/// input is swallowed and every path exits 0.
pub const DEFAULT_HOOK_SCRIPT: &str = "#!/usr/bin/env python3\n\"\"\"{{name}}: a skillctl hook. Reads one lifecycle event from stdin and exits 0.\"\"\"\nimport json\nimport sys\n\n\ndef main() -> int:\n    try:\n        event = json.load(sys.stdin)\n    except ValueError:\n        return 0  # never fail the harness over malformed input\n    _ = event.get(\"hook_event_name\")\n    return 0\n\n\nif __name__ == \"__main__\":\n    sys.exit(main())\n";

/// The Cursor rule scaffold: `RULE.mdc` with Cursor's frontmatter plus name
/// and version, linked as `.cursor/rules/<name>.mdc`.
pub const DEFAULT_RULE_TEMPLATE: &str = "---\nname: {{name}}\ndescription: Describe when Cursor should attach this rule.\nglobs: \"**/*\"\nalwaysApply: false\nmetadata:\n  version: \"0.1.0\"\n---\n\nWrite the rule text here. Keep it to facts and constraints; put procedures in a skill.\n";

/// The slash-command scaffold: `COMMAND.md` whose body is the prompt, linked as
/// `.claude/commands/<name>.md` and `.cursor/commands/<name>.md`.
pub const DEFAULT_COMMAND_TEMPLATE: &str = "---\nname: {{name}}\ndescription: Describe what this slash command does.\nargument-hint: \"[what to pass]\"\nmetadata:\n  version: \"0.1.0\"\n---\n\nWrite the prompt the command expands to. $ARGUMENTS is replaced by what the user typed after /{{name}}.\n";

/// The subagent scaffold: `AGENT.md` with Claude Code's frontmatter plus
/// Cursor's `readonly`, linked as `.claude/agents/<name>.md`.
pub const DEFAULT_AGENT_TEMPLATE: &str = "---\nname: {{name}}\ndescription: Describe when the main agent should delegate to this subagent. Use proactively for [the kind of task].\ntools: Read, Grep, Glob\nmodel: haiku\nreadonly: true\nmaxTurns: 15\nmetadata:\n  version: \"0.1.0\"\n---\n\nYou investigate; you never edit. Write full findings to .claude/scratch/<task-id>.md.\nReturn ONLY this JSON: {\"status\":\"ok|partial|failed\",\"answer\":\"<=120 words\",\"files\":[{\"path\":\"\",\"lines\":\"\",\"why\":\"\"}],\"open_questions\":[],\"confidence\":0.0,\"artifact\":\".claude/scratch/<task-id>.md\"}\n";

#[derive(Debug, Serialize)]
pub struct InitReport {
    pub home: PathBuf,
    pub dry_run: bool,
    /// Paths created, or that would be created, in dependency order.
    pub created: Vec<PathBuf>,
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Initialize {}", path.display()),
        source,
        hint: "Check the skillctl home path and its permissions, then run skillctl init again."
            .into(),
    }
}

fn exists(fs: &dyn Fs, path: &Path) -> Result<bool, Error> {
    match fs.symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(path, error)),
    }
}

fn ensure_dir(fs: &dyn Fs, path: &Path, created: &mut Vec<PathBuf>) -> Result<(), Error> {
    if created.iter().any(|p| p == path) {
        return Ok(());
    }
    match fs.symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        // Allow directory aliases (notably /tmp on macOS), but never replace them.
        Ok(metadata) if metadata.is_symlink() => {
            fs.read_dir(path).map_err(|error| io_error(path, error))?;
            return Ok(());
        }
        Ok(_) => {
            return Err(io_error(
                path,
                io::Error::new(io::ErrorKind::AlreadyExists, "expected a directory"),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        ensure_dir(fs, parent, created)?;
    }
    match fs.create_dir(path) {
        Ok(()) => created.push(path.to_owned()),
        // Another initializer may create the home before either can open its lock.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            ensure_dir(fs, path, created)?;
        }
        Err(error) => return Err(io_error(path, error)),
    }
    Ok(())
}

/// Initialize missing layout entries under an exclusive registry lock.
/// Dry runs use the injected write-recording filesystem and never open a lock file.
pub fn initialize(context: &Context<'_>) -> Result<InitReport, Error> {
    let fs = context.fs.as_ref();
    let mut report = InitReport {
        home: context.home.home().to_owned(),
        dry_run: context.flags.dry_run,
        created: Vec::new(),
    };
    // Bootstrap the lock's parent; all remaining checks/writes happen while locked.
    ensure_dir(fs, &report.home, &mut report.created)?;
    let mut populate = || {
        for dir in [
            context.home.store(),
            context.home.drafts(),
            context.home.templates().join("default"),
            context.home.templates().join("hook"),
            context.home.templates().join("rule"),
            context.home.templates().join("command"),
            context.home.templates().join("agent"),
        ] {
            ensure_dir(fs, &dir, &mut report.created)?;
        }
        let registry = context.home.registry();
        if !exists(fs, &registry)? {
            Registry::default().save(fs, &registry)?;
            report.created.push(registry);
        }
        for (relative, contents, mode) in [
            ("default/SKILL.md", DEFAULT_TEMPLATE, 0o644),
            ("hook/HOOK.md", DEFAULT_HOOK_TEMPLATE, 0o644),
            ("hook/hook.py", DEFAULT_HOOK_SCRIPT, 0o755),
            ("rule/RULE.mdc", DEFAULT_RULE_TEMPLATE, 0o644),
            ("command/COMMAND.md", DEFAULT_COMMAND_TEMPLATE, 0o644),
            ("agent/AGENT.md", DEFAULT_AGENT_TEMPLATE, 0o644),
        ] {
            let template = context.home.templates().join(relative);
            if !exists(fs, &template)? {
                fs.write_atomic(&template, contents.as_bytes())
                    .map_err(|error| io_error(&template, error))?;
                fs.set_mode(&template, mode)
                    .map_err(|error| io_error(&template, error))?;
                report.created.push(template);
            }
        }
        Ok(())
    };
    if context.flags.dry_run {
        populate()?;
    } else {
        RegistryLock::open(&context.home.registry_lock())?.with_exclusive(populate)?;
    }
    Ok(report)
}

/// Other commands initialize only a missing home, without emitting a second result.
pub fn ensure_home(context: &Context<'_>) -> Result<(), Error> {
    if !exists(context.fs.as_ref(), context.home.home())? {
        initialize(context)?;
    }
    Ok(())
}

pub fn run(context: &mut Context<'_>) -> Result<(), Error> {
    let report = initialize(context)?;
    let message = if report.dry_run {
        let paths = report
            .created
            .iter()
            .map(|path| format!("  create {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Would initialize {} ({} paths to create)\n{paths}",
            report.home.display(),
            report.created.len()
        )
    } else if report.created.is_empty() {
        format!("Already initialized {}", report.home.display())
    } else {
        format!("Initialized {}", report.home.display())
    };
    context
        .output
        .result(&report, &message)
        .map_err(|error| io_error(&report.home, error))
}
