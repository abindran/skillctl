//! Record the Git commit for `skillctl --version`.
//!
//! Release builds pass `SKILLCTL_COMMIT` explicitly; local builds ask Git and
//! fall back to `unknown` when the source is not a checkout.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SKILLCTL_COMMIT");
    println!("cargo:rerun-if-changed=.git/HEAD");
    let commit = std::env::var("SKILLCTL_COMMIT")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let commit: String = commit.chars().take(12).collect();
    println!("cargo:rustc-env=SKILLCTL_COMMIT={commit}");
}
