# The macOS app

`ui/SkillctlApp/` is a SwiftUI application that fronts the CLI. It is deliberately thin: the app never reads the registry, the store or the event log itself. Each screen runs `skillctl --json <command>` through `Process`, decodes the JSON with `Codable` types that mirror the CLI's output field for field (`Models.swift`), and renders it. Mutating actions run `skillctl --json --yes <command>` and then reload the data they changed. This keeps one implementation of every rule and makes every click reproducible in a terminal: the Console pane lists each invocation with its arguments, working folder, exit code and output.

## Layout

| File | Role |
|---|---|
| `Package.swift` | SwiftPM executable target, macOS 14 deployment target, Swift 5 language mode (tools version 5.10) so strict concurrency does not block a small app |
| `CLI.swift` | `CLI`: binary discovery (Settings path, then `target/debug/skillctl` found by walking up from the executable so a checkout's debug build beats an installed, possibly older, copy, then `~/.cargo/bin`, Homebrew, `PATH`), `missingCommands` (the `requiredCommands` that `--help` does not list; an outdated binary is reported, not used), `run` (background queue, both pipes drained concurrently, stdin closed, optional `SKILLCTL_HOME` and extra environment), `decode`, `perform`, and `failure(of:)`, which turns the CLI's `{"error","message","hint","exit_code"}` object into a `CLIError`. `CommandRun` is the console record. |
| `Models.swift` | Codable mirrors: `Project`, `Instance`, `Artifact` (the `artifacts` row), `Draft` (the `drafts` row), `HookEntry`, `DoctorReport` with `Problem`, `Telemetry`, `Harnesses`, `MCPAudit`, `LogSummary` with `LogWindow` and `LogAlert`, `Finding` (validate and lint), `StatusReport` |
| `AppState.swift` | `@Observable @MainActor` store: settings persisted in `UserDefaults`, loaded data, a `busy` set, the console, `lastError`, and one method per CLI action. `doctor` is decoded regardless of exit status because it exits 1 while problems remain; `validate` and `lint` accept exit 3 for the same reason. |
| `ContentView.swift` | `NavigationSplitView` with the `Screen` sidebar in sections (Dashboard; Library: one `.kind(id)` row per `ArtifactKind` plus Store; Workspace: Projects; Health: Doctor, Log, Quality), badges (drafts per kind, unhealthy hooks, unused versions, doctor problems, warning alerts), the toolbar, the error alert, the `VSplitView` console. Every row carries an explicit `.tag(Screen)`: the list's selection is a `Screen`, and without the tag SwiftUI tags rows with their `id` and selection silently never matches (the first version's sidebar did nothing for that reason). |
| `KindView.swift` | The library screen for one `ArtifactKind` (`Models.swift` carries the kind table: title, root document, where a pin links, summary): the kind's drafts from `skillctl drafts` with Reveal, Validate (`<name>@draft`), Seal (bump picker → `seal --bump`), and for agents Derive (`agent derive --model`); its store versions with Validate, Edit (`edit --force` into a draft), Pin (`add --project`) or, for hooks, Register; New (`new --kind`) and Install sheets. The Hooks screen also embeds `HookRegistrationTable` (`hook list`, repair, remove). |
| `DashboardView`, `ProjectsView`, `StoreView`, `DoctorView`, `LogView`, `QualityView`, `SettingsView`; `HooksView.swift` (the registration table) | One file per screen; sheets for attach, add pin, install, new, seal, derive and pin; confirmation dialogs before detach, garbage collection and hook removal |
| `Shared.swift` | `SeverityBadge`, `FindingsTable`, `ProblemsList`, `StatTile`, `Panels` (NSOpenPanel wrappers, reveal in Finder) |
| `Scripts/bundle.sh`, `Resources/Info.plist` | `swift build` plus a hand-made `Skillctl.app` bundle in `dist/`; unsigned. The script quits a running copy first because `open` only activates an already running app. |
| `Tests/SkillctlAppTests/EndToEndTests.swift` | XCTest, `@MainActor`. Finds `target/debug/skillctl` by walking up from the test file (skips if the CLI is not built), builds an `AppState` with a private `UserDefaults` suite, a temporary `SKILLCTL_HOME` and `HOME` redirected into the temp folder, then runs the whole workflow against the real binary: init, `new` a skill and a hook through the drafts listing, validate the draft, `seal`, `edit` a version back into a draft and seal it with a minor bump, `new` an agent and `derive` its Cursor override, attach a Git folder to a new project with a pin, register the hook (settings land in the redirected `HOME`), write a log and read the summary, doctor with its advisory sections (Cursor's version from the log), status, validate, lint, a failing `add`, unpin, unregister, detach. Two more tests cover the outdated-binary report (a fake `skillctl` whose `--help` lacks `log`) and the discovery order. |

## Decisions

1. **CLI, not a library.** Linking the Rust crate through a C ABI would have meant a second public surface and a build that needs both toolchains. The JSON output already exists for scripts, is covered by the CLI's tests, and is versioned with the binary the app finds, so the app follows whatever `skillctl` is installed.
2. **No prompts, ever.** Every call passes `--yes` where a prompt is possible and closes stdin, so a command can never hang on a question; anything that deserves a confirmation is confirmed in the app instead (detach, gc, hook remove).
3. **Working folder is a parameter.** Commands that act on the current folder (`attach`, `detach`, `status`, `lint`) receive the chosen folder as the process's `currentDirectoryURL`; the app itself has no notion of a current project.
4. **Failures are the CLI's words.** The error alert shows the CLI's `message` (and `hint` as the recovery suggestion), never an app-side paraphrase.
5. **Not in the Rust CI.** `ci.yml` is untouched; `swift.yml` runs only when `ui/**` changes: it builds the CLI, builds the app, runs `swift test` and uploads the bundle as an artifact.
6. **Development builds win.** The user tests with the debug build until a release is cut, and an installed `~/.cargo/bin/skillctl` from an earlier release produced `unrecognized subcommand 'log'` in the first version of the app. Hence the discovery order and the `--help` capability check.

## Running

```
ui/SkillctlApp/Scripts/bundle.sh [debug] && open ui/SkillctlApp/dist/Skillctl.app
```

or `swift run` inside `ui/SkillctlApp/`. Settings (⌘,) take an explicit binary path and a `SKILLCTL_HOME` override, which points the whole app at another home, for example one of the CLI test suite's temporary homes while debugging.
