//! Interactive prompts and deterministic queued answers for command tests.
//!
//! Callers supply flag answers before calling this service. Configure the terminal
//! prompter with the flag that answers the current question so non-interactive
//! failures are actionable. Prompts use stderr, leaving stdout for command output.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, IsTerminal};

use dialoguer::{Confirm, Input, MultiSelect, Select};

use crate::error::Error;

type Result<T> = std::result::Result<T, Error>;

/// Object-safe prompts shared by commands and their test doubles.
pub trait Prompter {
    fn select(&self, question: &str, items: &[String], default: usize) -> Result<usize>;
    fn multi_select(
        &self,
        question: &str,
        items: &[String],
        preselected: &[bool],
    ) -> Result<Vec<usize>>;
    fn input(
        &self,
        question: &str,
        validate: &dyn Fn(&str) -> std::result::Result<(), String>,
    ) -> Result<String>;
    fn confirm(&self, question: &str, default: bool) -> Result<bool>;
}

/// Terminal UI. `yes` accepts confirmations, never inventing other answers.
#[derive(Debug, Clone)]
pub struct TermPrompter {
    answer_flag: String,
    yes: bool,
}

impl TermPrompter {
    /// Name the CLI flag that supplies the answer to this prompt.
    pub fn new(answer_flag: impl Into<String>) -> Self {
        Self {
            answer_flag: answer_flag.into(),
            yes: false,
        }
    }

    /// Configure automatic acceptance of confirmation prompts (`--yes`).
    pub fn with_yes(mut self, yes: bool) -> Self {
        self.yes = yes;
        self
    }

    /// Set the answer flag before asking a different command-specific question.
    pub fn with_answer_flag(mut self, answer_flag: impl Into<String>) -> Self {
        self.answer_flag = answer_flag.into();
        self
    }

    fn require_terminal(&self, question: &str) -> Result<()> {
        if io::stdin().is_terminal() {
            Ok(())
        } else {
            Err(Error::MissingAnswer {
                missing: format!("{question} ({})", self.answer_flag),
                hint: format!(
                    "Pass {} to answer without a terminal; see skillctl <command> --help.",
                    self.answer_flag
                ),
            })
        }
    }
}

impl Prompter for TermPrompter {
    fn select(&self, question: &str, items: &[String], default: usize) -> Result<usize> {
        self.require_terminal(question)?;
        check_select(items, default)?;
        Select::new()
            .with_prompt(question)
            .items(items)
            .default(default)
            .interact_on(&dialoguer::console::Term::stderr())
            .map_err(prompt_error)
    }

    fn multi_select(
        &self,
        question: &str,
        items: &[String],
        preselected: &[bool],
    ) -> Result<Vec<usize>> {
        self.require_terminal(question)?;
        check_preselected(items, preselected)?;
        MultiSelect::new()
            .with_prompt(question)
            .items(items)
            .defaults(preselected)
            .interact_on(&dialoguer::console::Term::stderr())
            .map_err(prompt_error)
    }

    fn input(
        &self,
        question: &str,
        validate: &dyn Fn(&str) -> std::result::Result<(), String>,
    ) -> Result<String> {
        self.require_terminal(question)?;
        Input::<String>::new()
            .with_prompt(question)
            .allow_empty(true)
            .validate_with(|value: &String| validate(value))
            .interact_text_on(&dialoguer::console::Term::stderr())
            .map_err(prompt_error)
    }

    fn confirm(&self, question: &str, default: bool) -> Result<bool> {
        if self.yes {
            return Ok(true);
        }
        self.require_terminal(question)?;
        Confirm::new()
            .with_prompt(question)
            .default(default)
            .interact_on(&dialoguer::console::Term::stderr())
            .map_err(prompt_error)
    }
}

fn prompt_error(error: dialoguer::Error) -> Error {
    let dialoguer::Error::IO(source) = error;
    Error::Io {
        operation: "read terminal prompt".into(),
        source,
        hint: "Retry in an interactive terminal or supply the command's answer flags.".into(),
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Usage {
        message: message.into(),
        hint: "Check the prompt's choices, defaults, and scripted answers.".into(),
    }
}

fn check_select(items: &[String], default: usize) -> Result<()> {
    if default >= items.len() {
        return Err(invalid(
            "Selection requires choices and an in-range default",
        ));
    }
    Ok(())
}

fn check_preselected(items: &[String], preselected: &[bool]) -> Result<()> {
    if preselected.len() != items.len() {
        return Err(invalid(
            "Preselected values must match the number of choices",
        ));
    }
    Ok(())
}

/// A typed answer to the next prompt in a command test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Select(usize),
    MultiSelect(Vec<usize>),
    Input(String),
    Confirm(bool),
}

/// Queued answers. Unexpected prompts or answer kinds panic to fail tests loudly.
/// Call `assert_finished` to also detect expected prompts that never occurred.
#[derive(Debug, Default)]
pub struct ScriptedPrompter {
    answers: RefCell<VecDeque<Answer>>,
}

impl ScriptedPrompter {
    pub fn new(answers: impl IntoIterator<Item = Answer>) -> Self {
        Self {
            answers: RefCell::new(answers.into_iter().collect()),
        }
    }

    pub fn assert_finished(&self) {
        assert!(
            self.answers.borrow().is_empty(),
            "Unused scripted prompt answers: {:?}",
            self.answers.borrow()
        );
    }

    fn next(&self, question: &str) -> Answer {
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("Unexpected prompt: {question}"))
    }
}

impl Prompter for ScriptedPrompter {
    fn select(&self, question: &str, items: &[String], default: usize) -> Result<usize> {
        check_select(items, default)?;
        let Answer::Select(index) = self.next(question) else {
            panic!("Expected a Select answer for: {question}");
        };
        if index >= items.len() {
            return Err(invalid("Scripted selection is outside the choices"));
        }
        Ok(index)
    }

    fn multi_select(
        &self,
        question: &str,
        items: &[String],
        preselected: &[bool],
    ) -> Result<Vec<usize>> {
        check_preselected(items, preselected)?;
        let Answer::MultiSelect(indices) = self.next(question) else {
            panic!("Expected a MultiSelect answer for: {question}");
        };
        let mut seen = vec![false; items.len()];
        for &index in &indices {
            if index >= items.len() || seen[index] {
                return Err(invalid("Scripted selections must be in range and unique"));
            }
            seen[index] = true;
        }
        Ok(indices)
    }

    fn input(
        &self,
        question: &str,
        validate: &dyn Fn(&str) -> std::result::Result<(), String>,
    ) -> Result<String> {
        let Answer::Input(value) = self.next(question) else {
            panic!("Expected an Input answer for: {question}");
        };
        validate(&value).map_err(invalid)?;
        Ok(value)
    }

    fn confirm(&self, question: &str, _default: bool) -> Result<bool> {
        let Answer::Confirm(value) = self.next(question) else {
            panic!("Expected a Confirm answer for: {question}");
        };
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn items() -> Vec<String> {
        vec!["web".into(), "api".into()]
    }

    #[test]
    fn scripted_answers_drive_all_prompts_through_trait_object() {
        let scripted = ScriptedPrompter::new([
            Answer::Select(1),
            Answer::MultiSelect(vec![0, 1]),
            Answer::Input("web".into()),
            Answer::Confirm(false),
        ]);
        let prompt: &dyn Prompter = &scripted;
        assert_eq!(prompt.select("Project?", &items(), 0).unwrap(), 1);
        assert_eq!(
            prompt
                .multi_select("Skills?", &items(), &[true, false])
                .unwrap(),
            vec![0, 1]
        );
        assert_eq!(prompt.input("Name?", &|_| Ok(())).unwrap(), "web");
        assert!(!prompt.confirm("Continue?", true).unwrap());
        scripted.assert_finished();
    }

    #[test]
    fn input_validator_is_enforced() {
        let prompt = ScriptedPrompter::new([Answer::Input("INVALID".into())]);
        let error = prompt
            .input("Name?", &|_| Err("Use lowercase".into()))
            .unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("Use lowercase"));
        prompt.assert_finished();
    }

    #[test]
    fn invalid_choices_defaults_and_selections_are_rejected() {
        let prompt = ScriptedPrompter::default();
        assert!(prompt.select("Empty?", &[], 0).is_err());
        assert!(prompt.select("Default?", &items(), 2).is_err());
        assert!(prompt.multi_select("Defaults?", &items(), &[true]).is_err());
        for answer in [
            Answer::Select(2),
            Answer::MultiSelect(vec![2]),
            Answer::MultiSelect(vec![0, 0]),
        ] {
            let prompt = ScriptedPrompter::new([answer.clone()]);
            let result = match answer {
                Answer::Select(_) => prompt.select("Pick?", &items(), 0).map(|_| ()),
                _ => prompt
                    .multi_select("Pick?", &items(), &[false, false])
                    .map(|_| ()),
            };
            assert!(result.is_err());
            prompt.assert_finished();
        }
    }

    #[test]
    fn empty_multi_selection_is_valid() {
        let prompt = ScriptedPrompter::new([Answer::MultiSelect(vec![])]);
        assert!(prompt.multi_select("None?", &[], &[]).unwrap().is_empty());
        prompt.assert_finished();
    }

    #[test]
    #[should_panic(expected = "Unexpected prompt")]
    fn unexpected_prompt_fails_test() {
        ScriptedPrompter::default()
            .confirm("Unexpected?", false)
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "Expected a Select answer")]
    fn mismatched_answer_fails_test() {
        ScriptedPrompter::new([Answer::Confirm(true)])
            .select("Project?", &items(), 0)
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "Unused scripted prompt answers")]
    fn unconsumed_answers_fail_explicit_check() {
        ScriptedPrompter::new([Answer::Confirm(true)]).assert_finished();
    }

    #[test]
    fn non_terminal_stdin_is_checked_in_subprocess() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "services::prompt::tests::non_terminal_child",
                "--ignored",
                "--nocapture",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "subprocess helper: requires stdin redirected to null"]
    fn non_terminal_child() {
        assert!(!io::stdin().is_terminal());
        let prompt = TermPrompter::new("--project");
        let results = [
            prompt.select("Project?", &items(), 0).map(|_| ()),
            prompt
                .multi_select("Skills?", &items(), &[false, false])
                .map(|_| ()),
            prompt
                .input("Name?", &|_| panic!("must not request input"))
                .map(|_| ()),
            prompt.confirm("Continue?", false).map(|_| ()),
        ];
        for result in results {
            let error = result.unwrap_err();
            assert!(matches!(error, Error::MissingAnswer { .. }));
            assert_eq!(error.exit_code(), 2);
            assert!(error.to_string().contains("--project"));
            assert!(error.hint().contains("--project"));
        }
        let prompt = prompt.with_yes(true).with_answer_flag("--skills");
        assert!(prompt.confirm("Delete?", false).unwrap());
        let error = prompt.select("Skills?", &items(), 0).unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(error.hint().contains("--skills"));
    }
}
