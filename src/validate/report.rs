//! Stable plain-text reports; machine-readable command formats arrive with full validation.
use super::{Finding, Severity};

pub fn text(findings: &[Finding]) -> String {
    let mut output = String::new();
    for finding in findings {
        let location = match finding.line {
            Some(line) => format!("{}:{line}", finding.path.display()),
            None => finding.path.display().to_string(),
        };
        output.push_str(&format!(
            "{location} {} {} {}\n  Hint: {}\n",
            finding.rule, finding.severity, finding.message, finding.hint
        ));
    }
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let warnings = findings.len() - errors;
    output.push_str(&format!("{errors} error(s), {warnings} warning(s)"));
    output
}
