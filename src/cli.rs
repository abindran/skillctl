use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

use crate::{
    artifact::Kind,
    names::{ProjectName, SkillName},
};

/// `--version` output: version, build commit and the registry schema this
/// binary reads and writes. `-V` prints the short form.
pub static LONG_VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "{} (commit {}, registry schema {})",
        env!("CARGO_PKG_VERSION"),
        env!("SKILLCTL_COMMIT"),
        crate::registry::SCHEMA_VERSION
    )
});

/// Manage and install reusable skills
#[derive(Debug, Parser)]
#[command(name = "skillctl", version, long_version = LONG_VERSION.as_str(), about)]
pub struct Cli {
    /// Set the skillctl home directory.
    #[arg(long, env = "SKILLCTL_HOME", global = true)]
    pub home: Option<PathBuf>,

    /// Automatically confirm prompts.
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Output results as JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Preview changes without applying them.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Suppress non-error output.
    #[arg(short, long, conflicts_with = "verbose", global = true)]
    pub quiet: bool,

    /// Enable verbose output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// The command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Available top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize skillctl.
    Init,

    /// Attach skills to a project.
    Attach {
        /// Attach to an existing project.
        #[arg(long, conflicts_with = "new")]
        project: Option<ProjectName>,

        /// Create and attach to a new project.
        #[arg(long, conflicts_with = "project")]
        new: Option<ProjectName>,

        /// Comma-separated list of skills.
        #[arg(long, value_delimiter = ',')]
        skills: Vec<String>,

        /// Inside a Git subfolder, attach the repository top level.
        #[arg(long, conflicts_with = "here")]
        top_level: bool,

        /// Inside a Git subfolder, attach the current folder itself.
        #[arg(long, conflicts_with = "top_level")]
        here: bool,
    },

    /// Detach from a project.
    Detach {
        /// Keep project links.
        #[arg(long)]
        keep_links: bool,

        /// Delete the project.
        #[arg(long)]
        delete_project: bool,
    },

    /// Show the current status.
    Status {
        /// Also size the skill listing the agent sees here against the index
        /// budget (1% of the context window; descriptions over 1,536 characters).
        #[arg(long)]
        budget: bool,
    },

    /// Add a skill to a project.
    Add {
        /// Skill name, optionally followed by @version.
        skill: String,

        /// Target project.
        #[arg(long)]
        project: Option<ProjectName>,

        /// Link the writable draft instead of a store version.
        #[arg(long)]
        draft: bool,
    },

    /// Remove a skill from a project.
    Remove {
        /// Name of the skill to remove.
        skill: SkillName,

        /// Target project.
        #[arg(long)]
        project: Option<ProjectName>,
    },

    /// Manage globally installed skills.
    Global {
        /// The global operation to execute.
        #[command(subcommand)]
        command: GlobalCommand,
    },

    /// Subagent helpers.
    Agent {
        /// The agent operation to execute.
        #[command(subcommand)]
        command: AgentCommand,
    },

    /// Register hooks with the harnesses (Claude Code settings, read by Cursor too).
    Hook {
        /// The hook operation to execute.
        #[command(subcommand)]
        command: HookCommand,
    },

    /// Read the event log written by the skillctl-logger hook.
    Log {
        /// The log operation to execute.
        #[command(subcommand)]
        command: LogCommand,
    },

    /// A local OpenTelemetry collector for Claude Code: tokens and cost per
    /// model, skill and agent, recorded under the skillctl home.
    Telemetry {
        /// The telemetry operation to execute.
        #[command(subcommand)]
        command: TelemetryCommand,
    },

    /// List projects.
    Projects,

    /// List available skills.
    Skills,

    /// List writable drafts with their kind and path.
    Drafts,

    /// List every store version with its kind, pins and recorded use.
    Artifacts {
        /// Only versions nothing pins, or pinned skills and agents with no
        /// recorded use after a week of logging.
        #[arg(long)]
        unused: bool,
    },

    /// Relink project skills.
    Relink {
        /// Relink all projects.
        #[arg(long, conflicts_with = "project")]
        all: bool,

        /// Relink a specific project.
        #[arg(long, conflicts_with = "all")]
        project: Option<ProjectName>,
    },

    /// Create a new skill or hook draft.
    New {
        /// Name of the skill or hook to create.
        skill: SkillName,

        /// Create from a template (default: "default" for skills, "hook" for hooks).
        #[arg(long)]
        from_template: Option<String>,

        /// Artifact kind to scaffold.
        #[arg(long, value_enum, default_value_t = Kind::Skill)]
        kind: Kind,
    },

    /// Copy a stored version into a writable draft with a patch bump.
    Edit {
        /// Skill name, optionally followed by @version (default: highest stable).
        skill: String,

        /// Replace an existing draft.
        #[arg(long)]
        force: bool,
    },

    /// Seal a skill as a versioned release.
    Seal {
        /// Name of the skill to seal.
        skill: SkillName,

        /// Version increment.
        #[arg(long, value_enum)]
        bump: Option<Bump>,
    },

    /// Install skills from a Git URL or a local folder into the store.
    Install {
        /// Git clone URL (git@…, https://…, ssh://…, file://…) or an existing folder.
        source: String,

        /// Git tag or branch to clone (default: the remote's default branch).
        #[arg(long = "ref")]
        reference: Option<String>,

        /// Only this subfolder of the source (relative), instead of searching for SKILL.md.
        #[arg(long)]
        path: Option<PathBuf>,

        /// Install every discovered skill without asking.
        #[arg(long)]
        all: bool,

        /// Version to record when SKILL.md has no metadata.version.
        #[arg(long = "version")]
        version: Option<String>,
    },

    /// Print the root document of a stored version (default: newest stable) or a draft.
    Show {
        /// Artifact name, optionally followed by @version or @draft.
        target: String,
    },

    /// Delete a store version, or every version of an artifact, that nothing pins.
    Uninstall {
        /// Artifact name, optionally followed by @version (default: every version).
        target: String,
    },

    /// Validate a skill or project.
    Validate {
        /// Target to validate.
        target: Option<String>,

        /// Output format.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,

        /// Treat warnings as failures.
        #[arg(long)]
        strict: bool,

        /// Validate against a reference.
        #[arg(long)]
        with_reference: bool,
    },

    /// Check a project's instruction files (AGENTS.md, CLAUDE.md, CLAUDE.local.md)
    /// and report problems with suggested fixes. Never writes.
    Lint {
        /// Project folder to check (default: the current folder).
        folder: Option<PathBuf>,

        /// Output format.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,

        /// Treat warnings as failures.
        #[arg(long)]
        strict: bool,

        /// List the instruction files lint reads here instead: present or
        /// absent, size, when the harnesses load each, and the rules on it.
        #[arg(long, conflicts_with_all = ["format", "strict"])]
        files: bool,
    },

    /// Diagnose the skillctl environment.
    Doctor {
        /// Attempt to fix detected problems.
        #[arg(long)]
        fix: bool,
    },

    /// Clean up unused resources.
    Gc {
        /// Number of versions to retain.
        #[arg(long, default_value_t = 1)]
        keep: u32,
    },

    /// Read or update configuration.
    Config {
        /// The configuration operation.
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Generate shell completion scripts.
    Completions {
        /// Target shell.
        shell: Shell,
    },
}

/// Operations on globally installed skills.
#[derive(Debug, Subcommand)]
pub enum GlobalCommand {
    /// Make a skill, command or agent global: linked into the user-level
    /// folders (~/.claude/skills, ~/.claude/commands, ~/.claude/agents and the
    /// Cursor equivalents) so every project sees it.
    Add {
        /// Artifact name, optionally followed by @version.
        skill: String,
    },

    /// Remove an artifact from the global set and its user-level links.
    Remove {
        /// Name of the artifact to remove.
        skill: SkillName,
    },

    /// List global artifacts with their links.
    List,
}
/// Subagent operations.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Write cursor.md into an agent draft: the same prompt with a Cursor model
    /// id and a read-the-skill line per preloaded skill, linked as
    /// .cursor/agents/<name>.md once sealed and pinned.
    Derive {
        /// Name of the agent draft (run skillctl edit <name> first for a sealed agent).
        agent: String,

        /// Cursor model id to run the agent on, optionally with options such as [effort=high].
        #[arg(long)]
        model: String,
    },
}

/// Hook registration operations.
#[derive(Debug, Subcommand)]
pub enum HookCommand {
    /// Pin a stored hook, link it under ~/.claude/hooks and register its events
    /// in ~/.claude/settings.json (merged, backed up, idempotent).
    Add {
        /// Hook name, optionally followed by @version (default: highest stable).
        hook: String,
    },

    /// Unpin a hook, remove its link and its settings entries.
    Remove {
        /// Name of the hook to remove.
        hook: SkillName,
    },

    /// List pinned hooks with their link and registration state.
    List,
}

/// Event log operations.
#[derive(Debug, Subcommand)]
pub enum LogCommand {
    /// Sessions, prompts, skill activations, subagent runs, compactions, MCP
    /// calls and test commands for the last N days against the N days before,
    /// with alerts for activation drops, compaction rises and a silent log.
    Summary {
        /// Length of each window in days.
        #[arg(long, default_value_t = 7)]
        days: u32,

        /// End of the current window (RFC 3339 or YYYY-MM-DD; default: now).
        #[arg(long)]
        until: Option<String>,

        /// Exit 1 when a warning-level alert fires.
        #[arg(long)]
        strict: bool,
    },

    /// Claude Code tokens and cost per day from `ccusage daily --json`, joined
    /// with the event log's prompts: cost and output tokens per prompt, cache
    /// hit ratio. Reports when ccusage is not installed.
    Usage {
        /// Days to cover, ending today.
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
}

/// Local collector operations.
#[derive(Debug, Subcommand)]
pub enum TelemetryCommand {
    /// Run the collector in the foreground: OTLP over HTTP/JSON on 127.0.0.1,
    /// one JSON line per event under $SKILLCTL_HOME/telemetry, with identity
    /// and content attributes (email, ids, prompts, tool inputs) dropped.
    Serve {
        /// Port to listen on (0 picks a free one).
        #[arg(long, default_value_t = crate::commands::collector::DEFAULT_PORT)]
        port: u16,
    },

    /// Point Claude Code at the collector (the env block of
    /// ~/.claude/settings.json, merged and backed up) and, on macOS, run the
    /// collector as a login service (a LaunchAgent).
    Enable {
        /// Port the collector listens on.
        #[arg(long, default_value_t = crate::commands::collector::DEFAULT_PORT)]
        port: u16,

        /// Only write the settings; run skillctl telemetry serve yourself.
        #[arg(long)]
        no_service: bool,
    },

    /// Remove the variables enable wrote (others are kept) and stop and remove
    /// the service. Recorded data stays.
    Disable {
        /// Port enable was run with.
        #[arg(long, default_value_t = crate::commands::collector::DEFAULT_PORT)]
        port: u16,
    },

    /// Whether Claude Code exports to the collector, the service, the
    /// collector itself and today's records.
    Status {
        /// Port the collector listens on.
        #[arg(long, default_value_t = crate::commands::collector::DEFAULT_PORT)]
        port: u16,
    },

    /// Cost and tokens by model, skill, agent and source, tool runs and
    /// slash commands, from the recorded events.
    Report {
        /// Days to cover, ending today (UTC).
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
}

/// Configuration operations.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Get a configuration value.
    Get {
        /// Configuration key.
        key: String,
    },

    /// Set a configuration value.
    Set {
        /// Configuration key.
        key: String,

        /// Value to assign.
        value: String,
    },
}

/// Version increment options.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Bump {
    /// Increment the major version.
    Major,

    /// Increment the minor version.
    Minor,

    /// Increment the patch version.
    Patch,
}

/// Validation output formats.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text.
    Text,

    /// JSON output.
    Json,

    /// JUnit XML output.
    Junit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn attach_parses_comma_separated_skills() {
        let cli = Cli::try_parse_from(["skillctl", "attach", "--skills", "a,b@1.2.0"]).unwrap();

        match cli.command {
            Command::Attach { skills, .. } => {
                assert_eq!(skills, vec!["a", "b@1.2.0"]);
            }
            _ => panic!("Expected Attach command"),
        }
    }

    #[test]
    fn attach_rejects_project_and_new_together() {
        let result = Cli::try_parse_from(["skillctl", "attach", "--project", "x", "--new", "y"]);

        assert!(result.is_err());
    }

    #[test]
    fn quiet_and_verbose_cannot_be_used_together() {
        let result = Cli::try_parse_from(["skillctl", "-q", "-v", "status"]);

        assert!(result.is_err());
    }

    #[test]
    fn global_json_flag_works_after_subcommand() {
        let cli = Cli::try_parse_from(["skillctl", "status", "--json"]).unwrap();

        assert!(cli.json);
        assert!(matches!(cli.command, Command::Status { budget: false }));
    }
}
