//! Central compatibility lists. These are lint knowledge, not harness guarantees.
pub const SPEC: &[&str] = &[
    "name",
    "description",
    "license",
    "compatibility",
    "allowed-tools",
    "metadata",
];
pub const CLAUDE_ONLY: &[&str] = &[
    "context",
    "agent",
    "hooks",
    "model",
    "effort",
    "shell",
    "argument-hint",
    "arguments",
];
pub const SHARED_EXTENSIONS: &[&str] = &["disable-model-invocation", "user-invocable", "paths"];

pub fn known(field: &str) -> bool {
    SPEC.contains(&field) || CLAUDE_ONLY.contains(&field) || SHARED_EXTENSIONS.contains(&field)
}
