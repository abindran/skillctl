use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}")]
    Usage { message: String, hint: String },

    #[error("Missing required input: {missing}")]
    MissingAnswer { missing: String, hint: String },

    #[error("Validation failed: {summary}")]
    Validation { summary: String, hint: String },

    /// A complete validation report has already been written to stdout.
    #[error("Validation found errors (see report)")]
    ReportedValidation,

    #[error("Conflicts detected: {summary}")]
    Conflicts { summary: String, hint: String },

    #[error("{operation}: {source}")]
    Io {
        operation: String,
        #[source]
        source: io::Error,
        hint: String,
    },

    #[error("Invalid registry: {message}")]
    Registry { message: String, hint: String },

    #[error("Command '{command}' is not implemented")]
    NotImplemented { command: String, hint: String },
}

impl Error {
    pub fn hint(&self) -> &str {
        match self {
            Error::ReportedValidation => "Fix the findings in the validation report.",
            Error::Usage { hint, .. }
            | Error::MissingAnswer { hint, .. }
            | Error::Validation { hint, .. }
            | Error::Conflicts { hint, .. }
            | Error::Io { hint, .. }
            | Error::NotImplemented { hint, .. }
            | Error::Registry { hint, .. } => hint,
        }
    }
    /// Stable machine-readable category, independent of the display message.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Usage { .. } => "usage",
            Self::MissingAnswer { .. } => "missing_answer",
            Self::Validation { .. } | Self::ReportedValidation => "validation",
            Self::Conflicts { .. } => "conflicts",
            Self::Io { .. } => "io",
            Self::NotImplemented { .. } => "not_implemented",
            Self::Registry { .. } => "registry",
        }
    }

    /// Numeric status for the binary to convert into a process exit code.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage { .. } => 2,
            Error::MissingAnswer { .. } => 2,
            Error::Validation { .. } | Error::ReportedValidation => 3,
            Error::Conflicts { .. } => 4,
            Error::Io { .. } => 1,
            Error::NotImplemented { .. } => 1,
            Error::Registry { .. } => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn every_variant_has_its_documented_exit_code_category_and_hint() {
        let cases = [
            (
                Error::Usage {
                    message: "bad argument".into(),
                    hint: "next step".into(),
                },
                2,
                "usage",
                "bad argument",
            ),
            (
                Error::MissingAnswer {
                    missing: "--project".into(),
                    hint: "next step".into(),
                },
                2,
                "missing_answer",
                "--project",
            ),
            (
                Error::Validation {
                    summary: "invalid skill".into(),
                    hint: "next step".into(),
                },
                3,
                "validation",
                "invalid skill",
            ),
            (
                Error::Conflicts {
                    summary: "existing folder".into(),
                    hint: "next step".into(),
                },
                4,
                "conflicts",
                "existing folder",
            ),
            (
                Error::Io {
                    operation: "read registry".into(),
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
                    hint: "next step".into(),
                },
                1,
                "io",
                "read registry: denied",
            ),
            (
                Error::NotImplemented {
                    command: "global add".into(),
                    hint: "next step".into(),
                },
                1,
                "not_implemented",
                "global add",
            ),
        ];
        for (error, code, kind, message) in cases {
            assert_eq!(error.exit_code(), code, "{error:?}");
            assert_eq!(error.kind(), kind);
            assert_eq!(error.hint(), "next step");
            assert!(error.to_string().contains(message));
            let report = crate::output::ErrorReport::from(&error);
            assert_eq!(report.exit_code, code);
            assert_eq!(report.error, kind);
            assert_eq!(report.hint, "next step");
        }
    }

    #[test]
    fn io_preserves_the_original_source() {
        let error = Error::Io {
            operation: "read registry".into(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            hint: "Check registry permissions".into(),
        };
        let source = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
        assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(source.to_string(), "denied");
    }
}
