//! Text and JSON output. Results go to stdout; human diagnostics go to stderr.
//!
//! JSON errors use a stable category plus a message, hint, and numeric exit code.
//! Quiet mode hides optional messages, never errors or explicit JSON results.

use std::io::{self, Write};

use serde::Serialize;

use crate::error::Error;

#[derive(Debug, Serialize)]
pub struct ErrorReport<'a> {
    pub error: &'static str,
    pub message: String,
    pub hint: &'a str,
    pub exit_code: u8,
}

impl<'a> From<&'a Error> for ErrorReport<'a> {
    fn from(error: &'a Error) -> Self {
        Self {
            error: error.kind(),
            message: error.to_string(),
            hint: error.hint(),
            exit_code: error.exit_code(),
        }
    }
}

/// An output destination that can use real streams or in-memory test buffers.
pub struct Output<Out, Err> {
    stdout: Out,
    stderr: Err,
    json: bool,
    quiet: bool,
}

impl<Out: Write, Err: Write> Output<Out, Err> {
    pub fn new(stdout: Out, stderr: Err, json: bool, quiet: bool) -> Self {
        Self {
            stdout,
            stderr,
            json,
            quiet,
        }
    }

    /// Write one command result. JSON remains available even in quiet mode.
    pub fn result<T: Serialize>(&mut self, value: &T, human: &str) -> io::Result<()> {
        if self.json {
            self.write_json(value)
        } else if !self.quiet {
            writeln!(self.stdout, "{human}")?;
            self.stdout.flush()
        } else {
            Ok(())
        }
    }

    /// Write a pre-rendered validation document without wrapping XML/JSON in JSON.
    pub fn document(&mut self, document: &str, machine_readable: bool) -> io::Result<()> {
        if machine_readable || !self.quiet {
            writeln!(self.stdout, "{document}")?;
            self.stdout.flush()?;
        }
        Ok(())
    }

    /// Optional progress or diagnostic information never contaminates JSON stdout.
    pub fn message(&mut self, message: &str) -> io::Result<()> {
        if !self.quiet {
            writeln!(self.stderr, "{message}")?;
            self.stderr.flush()?;
        }
        Ok(())
    }

    pub fn error(&mut self, error: &Error) -> io::Result<()> {
        if self.json {
            self.write_json(&ErrorReport::from(error))
        } else {
            writeln!(self.stderr, "Error: {error}")?;
            writeln!(self.stderr, "Hint: {}", error.hint())?;
            self.stderr.flush()
        }
    }

    fn write_json<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        // Serialize before writing so serialization failures emit no partial document.
        let document = serde_json::to_vec(value).map_err(io::Error::other)?;
        self.stdout.write_all(&document)?;
        writeln!(self.stdout)?;
        self.stdout.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn error() -> Error {
        Error::Usage {
            message: "Invalid project".into(),
            hint: "Run skillctl attach --help".into(),
        }
    }

    #[test]
    fn human_errors_and_hints_use_stderr_even_when_quiet() {
        for quiet in [false, true] {
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            Output::new(&mut stdout, &mut stderr, false, quiet)
                .error(&error())
                .unwrap();
            assert!(stdout.is_empty());
            assert_eq!(
                stderr,
                b"Error: Invalid project\nHint: Run skillctl attach --help\n"
            );
        }
    }

    #[test]
    fn json_errors_use_stdout_even_when_quiet() {
        for quiet in [false, true] {
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            Output::new(&mut stdout, &mut stderr, true, quiet)
                .error(&error())
                .unwrap();
            let report: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            assert_eq!(
                report,
                json!({
                    "error": "usage",
                    "message": "Invalid project",
                    "hint": "Run skillctl attach --help",
                    "exit_code": 2
                })
            );
            assert!(stdout.ends_with(b"\n"));
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn human_results_use_stdout_and_obey_quiet() {
        for quiet in [false, true] {
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            Output::new(&mut stdout, &mut stderr, false, quiet)
                .result(&json!({"ok": true}), "Done")
                .unwrap();
            assert_eq!(
                stdout,
                if quiet {
                    b"".as_slice()
                } else {
                    b"Done\n".as_slice()
                }
            );
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn messages_stay_on_stderr_and_json_results_survive_quiet() {
        for quiet in [false, true] {
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            let mut output = Output::new(&mut stdout, &mut stderr, true, quiet);
            output.message("Working").unwrap();
            output.result(&json!({"ok": true}), "Done").unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&stdout).unwrap(),
                json!({"ok": true})
            );
            assert_eq!(
                stderr,
                if quiet {
                    b"".as_slice()
                } else {
                    b"Working\n".as_slice()
                }
            );
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
    }

    #[test]
    fn write_errors_are_propagated() {
        for json in [false, true] {
            let err = Output::new(FailingWriter, FailingWriter, json, false)
                .error(&error())
                .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
            assert!(
                Output::new(FailingWriter, FailingWriter, json, false)
                    .result(&true, "Done")
                    .is_err()
            );
        }
    }

    #[test]
    fn flush_errors_are_propagated() {
        struct FlushFailure;
        impl Write for FlushFailure {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("flush failed"))
            }
        }
        for json in [false, true] {
            assert!(
                Output::new(FlushFailure, FlushFailure, json, false)
                    .error(&error())
                    .is_err()
            );
        }
    }
}
