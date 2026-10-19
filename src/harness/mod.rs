//! Harness-side registration of stored artifacts. Skills are linked into
//! `skills/` folders by the linker; hooks are linked into `~/.claude/hooks/` and
//! registered in Claude Code's user settings, which Cursor also reads through
//! its third-party imports, so one registration serves both tools.
pub mod claude;

use crate::paths::HomePath;

/// Where hook versions are linked so settings entries survive upgrades.
pub const HOOK_LINK_TARGET: &str = "~/.claude/hooks";
/// Claude Code's user-level settings, read by Cursor as well.
pub const CLAUDE_SETTINGS: &str = "~/.claude/settings.json";
/// Cursor's own user-level hooks file; a hook registered in both runs twice.
pub const CURSOR_HOOKS: &str = "~/.cursor/hooks.json";

pub fn hook_link_target() -> HomePath {
    HomePath::new(HOOK_LINK_TARGET).expect("constant home path")
}
pub fn claude_settings() -> HomePath {
    HomePath::new(CLAUDE_SETTINGS).expect("constant home path")
}
pub fn cursor_hooks() -> HomePath {
    HomePath::new(CURSOR_HOOKS).expect("constant home path")
}
