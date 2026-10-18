use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

use crate::names::{ProjectName, SkillName};

/// Manage and install reusable skills
#[derive(Debug, Parser)]
#[command(name = "skillctl", version, about)]
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
    Status,

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

    /// List projects.
    Projects,

    /// List available skills.
    Skills,

    /// Relink project skills.
    Relink {
        /// Relink all projects.
        #[arg(long, conflicts_with = "project")]
        all: bool,

        /// Relink a specific project.
        #[arg(long, conflicts_with = "all")]
        project: Option<ProjectName>,
    },

    /// Create a new skill.
    New {
        /// Name of the skill to create.
        skill: SkillName,

        /// Create from a template.
        #[arg(long)]
        from_template: Option<String>,
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
    /// Install a skill globally.
    Add {
        /// Name of the skill to install.
        skill: String,
    },

    /// Remove a globally installed skill.
    Remove {
        /// Name of the skill to remove.
        skill: SkillName,
    },

    /// List globally installed skills.
    List,
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
        assert!(matches!(cli.command, Command::Status));
    }
}
