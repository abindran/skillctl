//! Deterministic reports. Machine-readable failures remain a single document.
use super::{Finding, Severity};
use std::{io, path::PathBuf};

pub struct SkillReport {
    pub dir: PathBuf,
    pub findings: Vec<Finding>,
}

pub fn json(findings: &[Finding]) -> io::Result<String> {
    serde_json::to_string_pretty(findings).map_err(io::Error::other)
}

/// One testcase per skill, one failure element per error. Warnings stay in
/// system-out. Escape attributes/text and strip invalid XML controls/ANSI.
pub fn junit(reports: &[SkillReport]) -> String {
    fn escape(text: &str) -> String {
        quick_junit::XmlString::new(text)
            .as_str()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    let failures = reports
        .iter()
        .filter(|report| {
            report
                .findings
                .iter()
                .any(|finding| finding.severity == Severity::Error)
        })
        .count();
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites tests=\"{}\" failures=\"{failures}\"><testsuite name=\"skillctl\" tests=\"{}\" failures=\"{failures}\">\n",
        reports.len(),
        reports.len()
    );
    for report in reports {
        xml.push_str(&format!(
            "<testcase classname=\"skillctl.validate\" name=\"{}\">\n",
            escape(&report.dir.display().to_string())
        ));
        for finding in &report.findings {
            if finding.severity == Severity::Error {
                xml.push_str(&format!(
                    "<failure type=\"{}\" message=\"{}\">{}</failure>\n",
                    escape(&finding.rule),
                    escape(&finding.message),
                    escape(&text(std::slice::from_ref(finding)))
                ));
            }
        }
        xml.push_str(&format!(
            "<system-out>{}</system-out></testcase>\n",
            escape(&text(&report.findings))
        ));
    }
    xml.push_str("</testsuite></testsuites>");
    xml
}

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
