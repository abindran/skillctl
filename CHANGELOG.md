# Changelog

All notable changes to skillctl are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

Every release tag `vX.Y.Z` must have a matching `## [X.Y.Z]` section below; the
release workflow refuses to publish without one and uses that section as the
GitHub release notes. Add entries under **Unreleased** as you go and rename the
heading when cutting a release.

## [Unreleased]

### Added

- `skillctl telemetry serve|enable|disable|status|report`: a local
  OpenTelemetry collector for Claude Code. `serve` receives OTLP over
  HTTP/JSON on 127.0.0.1 (default port 4318) and writes one JSON line per
  event under `~/.skills/telemetry/<date>.jsonl`, dropping email, account
  and organisation ids, prompts, responses, tool inputs and error text
  first. `enable` writes the six export variables into the `env` block of
  `~/.claude/settings.json` (merged, backed up) and on macOS runs the
  collector as a login service (LaunchAgent
  `dev.abindran.skillctl.collector`); `disable` removes only the values it
  wrote. `report [--days <n>]` breaks cost and tokens down by model,
  skill, agent and source from the API request events, with tool runs and
  failures, slash commands and active time.
- The app's Document card shows every bundled file (references, scripts,
  schemas) from its chips, with line numbers, and Preview, Open and Reveal
  links sit beside every file the app names: findings, doctor problems and
  the Quality scope cards.
- `skillctl rules [--for <kind>]` prints the catalog of every validate and
  lint rule: ID, kind, group, default severity, a one-line summary and
  whether `validate.allow` can silence it. A test keeps it complete. The
  app's Quality screen has a Rules reference tab with search and a kind
  filter, opened pre-filtered from "N rules apply" on the scope cards.
- `lint --files` lists the instruction files lint reads in a folder, each
  present or absent with its line count, whether it is a link, when the
  harnesses load it and the rules on it, so a folder with nothing to lint
  is told apart from a clean one. The app's Quality screen shows it under
  the lint findings as "What is linted", and the chosen version's kind,
  rule families and files as "What is validated".
- The app's sidebar footer shows only the skillctl version, and a warning
  that opens Settings when the binary is missing or outdated; the binary's
  path and build commit stay in Settings.
- The app's Log screen has a Cost by skill and agent card: a switch that
  runs `telemetry enable` with the bundled CLI (so the login service runs
  the copy inside the app), the setup checklist, and cost by skill, agent
  and model once data arrives.

- `global add` accepts commands and agents: they link into
  `~/.claude/commands`, `~/.cursor/commands`, `~/.claude/agents` and (with
  a `cursor.md` override) `~/.cursor/agents`, so every project sees them.
  `global list`, `doctor` and `doctor --fix` cover those links; rules and
  scripts stay project-level.
- `skillctl-logger` 1.2.2 records the name of a slash command a prompt
  starts with, or that the model runs through `SlashCommand` (`command`),
  so commands get recorded uses in `artifacts` and a count in `log
  summary`; pinned commands with no use after a week now show as unused.
- The app badges every skill, command and agent `global`, `project` or
  `unused`, offers Make global and Remove from global, and has a per-kind
  Clean up unused action.
- `log usage [--days <n>]`: Claude Code cost and tokens per day from
  `ccusage daily --json`, joined with the log's prompts for cost per prompt,
  output tokens per prompt and cache hit ratio; clear report when ccusage
  is absent.
- `log summary` adds a per-day series (`daily`) and the forced-eval hook's
  follow-through: `hook_shortlists`, `hook_followed`, `hook_follow_ratio`.
- `skill-forced-eval` 1.0.2 writes each injected shortlist to the event log
  (`SkillShortlist` with `candidates`), which is what the follow-through is
  computed from.
- `show <name>[@<version>|@draft]`: prints the root document of a stored
  version or a draft; `--json` adds kind, folder and the file list. The
  macOS app renders it under the selected version.
- `uninstall <name>[@<version>]`: deletes one store version or every version
  of an artifact, refusing pinned ones with the pins named; `--dry-run`
  previews. The macOS app's delete actions use it.
- `drafts`: lists the writable drafts with their detected kind, description
  and path, read-only; the macOS app's authoring screens use it. `artifacts`
  rows carry the root document's `description` too.
- A native macOS app in `ui/SkillctlApp/` (SwiftUI, macOS 14+): dashboard,
  a library screen per artifact kind (skills, commands, agents, rules,
  scripts, hooks) laid out as a searchable list of artifacts and a detail
  pane (description, draft, versions, hook registration) with the authoring
  actions new, validate, seal, edit, pin, register and agent derive, a
  one spacing scale (`Metrics`) and a shared card style applied to every
  screen, card, bar and sheet, fixed-width list columns, a
  needs-attention section on the dashboard, a Log screen with activity by
  day, activation through the hook and ccusage cost and tokens, a Doctor
  status card with copyable fixes, bulk deletion of selected (for example
  unused) versions on the Store screen, the selected version's root
  document shown under the versions table, delete actions for a version or
  a whole artifact (through `uninstall`), findings shown as a summary plus
  cards grouped by severity, a Store screen with one
  expandable row per artifact, kind chips, an inspector and a cleanup flow
  that previews `gc --dry-run` before deleting, and a snapshot mode
  (`SKILLCTL_APP_SNAPSHOT=<folder>`) that renders every screen to PNG,
  projects (attach, pin, unpin, relink, detach), store (install, validate,
  garbage collection, unused review), hooks (register, repair, remove),
  doctor (run, fix, advisory sections), log summary with charts, and
  validate/lint. Every screen runs the CLI with `--json`; a console pane
  shows each command and its output. `Scripts/bundle.sh` produces
  `Skillctl.app`; `.github/workflows/swift.yml` builds it in CI. The app
  bundles the CLI at `Contents/Helpers/skillctl` (`Scripts/bundle.sh` builds
  it; `UNIVERSAL=1` for both architectures), prefers that copy, then a
  checkout's `target/debug/skillctl`, over installed copies, offers to link
  the bundled binary into `~/.local/bin` for terminal use, and
  reports a binary that lacks the subcommands it needs instead of using
  it. `swift test` runs end-to-end tests that drive the real binary through
  the app's state layer against a temporary home.
- `log summary [--days <n>] [--until <time>] [--strict]` (strategy work
  item W29): the event log over two windows with per-skill, per-agent,
  per-server and test-command counts, and the alerts `log-silent`,
  `activation-drop`, `compaction-rise` and `raw-test-commands`; `--strict`
  exits 1 on a warning-level alert. Cache-hit and cost alerts are pointed at
  `ccusage` and the OpenTelemetry backend, which hold that data.
- `doctor` records the Claude Code and Cursor versions it sees in
  `~/.skills/harness-versions.json` and reports `harness-upgraded` once
  after each upgrade (strategy work item W28), the cue to re-run the
  regression eval; `harness-recorded` and `harness-missing` are info.
- `eventlog` module: one reader for `log/events.jsonl` shared by
  `artifacts`, `doctor` and `log summary`.
- `skillctl-logger` 1.2.1 maps Cursor's own event names
  (`beforeSubmitPrompt`, `afterShellExecution`, …) onto Claude Code's,
  reads the command of Cursor's native shell events for `test_command`, and
  records `harness_version` from `cursor_version`.
- `hooks/quiet-commands` (strategy work item W24): a `PreToolUse` hook on
  Claude Code's Bash tool that blocks bare test and build commands in
  projects linking the `test-quiet` wrapper and hands the agent the wrapped
  command; `SKILLCTL_QUIET_REWRITE=1` rewrites in place instead (which
  Claude Code only allows together with auto-approval). Silent in Cursor and
  wherever there is nothing to redirect.
- `skillctl-logger` 1.2.0 (now 1.2.1) records `test_command: raw|wrapped` for Bash
  calls that run a recognised test or build command (the matcher now
  includes `Bash`); the command text is still never logged. Raw next to
  wrapped is the signal that the `quiet-commands` hook is needed.
- `artifacts [--unused]` (strategy work item W30): every store version with
  its kind, pins and recorded use from the event log (skill activations,
  subagent runs); `--unused` keeps unpinned versions and pinned skills or
  agents with no recorded use after seven days of log.
- `doctor` reports the model defaults in effect (strategy work item W25):
  the main `model` from `~/.claude/settings.json` and whether subagents
  inherit it or run on `CLAUDE_CODE_SUBAGENT_MODEL`.
- README "Model and effort defaults" section (W25) and a note that subagent
  telemetry (strategy work item W21) is already covered by the logger's
  `SubagentStart`/`SubagentStop` events with the agent type.
- Script bundles (strategy work item W23). A folder with `SCRIPT.md` and
  one or more executable wrappers is linked as the folder
  `.claude/scripts/<name>`; rules K501–K507 check the document, that every
  wrapper has a shebang and the executable bit, the script hygiene shared
  with skills, and the absence of symlinks. `lint` rule I306 warns when a
  bundle pinned in the folder's project is not named in `AGENTS.md` or
  `CLAUDE.md`. `new --kind script` scaffolds a bundle with a quiet `run.sh`.
  Two bundles ship under `wrappers/`: `test-quiet` (run a command, print one
  line on success and the failing lines on failure, keep the exit code) and
  `log-errors` (the matching lines of a log with context, duplicates
  collapsed, capped at 200 lines).
- The bundled `hooks/subagent-schema/` hook (strategy work item W20): on
  `SubagentStop` it validates the agent's `last_assistant_message` against
  the `schema.json` beside the agent definition the project links, exits 2
  with the problems (so the subagent is sent back for the required JSON) at
  most three times per agent run, then reports a `systemMessage`. Silent for
  agents without a schema and for Claude Code's auxiliary agents.
- Agent artifacts (strategy work items W17, W18 and W19). A folder with
  `AGENT.md` is a subagent, linked as `.claude/agents/<name>.md` and, when
  the folder carries a `cursor.md` override, `.cursor/agents/<name>.md`. Rules
  H220–H227 check the name (a built-in name such as `Explore` is allowed and
  overrides the built-in agent), description, version and body, field types,
  a Claude-only model alias without an override (H222), tool scope without
  `readonly: true`, which Cursor would run unscoped (H224), a stale override
  (H226) and an invalid `schema.json` (H227). `skillctl agent derive <agent>
  --model <id>` writes the override into a draft: the same prompt, a Cursor
  model id, `readonly` kept, and a read-this-skill line per preloaded skill.
  `new --kind agent` scaffolds a read-only Haiku explorer. Four agents ship
  under `agents/`: `explore` (overrides Claude Code's built-in on Haiku),
  `repo-explorer`, `log-digester` and `reviewer`, each with the `schema.json`
  its final message must satisfy.
- Rule and command artifacts (strategy work items W15 and W31). A folder with
  `RULE.mdc` is a Cursor rule, linked as `.cursor/rules/<name>.mdc`; one with
  `COMMAND.md` is a slash command, linked as `.claude/commands/<name>.md` and
  `.cursor/commands/<name>.md`. Both go through `new --kind rule|command`
  (templates created by `init`), `validate` (K201–K207, K301–K305), `seal`,
  `install`, `edit` and `add`; links are hidden through `.git/info/exclude`
  like skill links; `status`, `doctor` and `relink` cover them. The linker now
  plans links by artifact kind (`Layout::Kinds` for projects, `Layout::Folders`
  for user-level folders) and sweeps the kind folders so an unpinned rule or
  command loses its links. `global add` refuses rules and commands, which are
  project-level.
- `HOOK.md` `companions`: artifacts `hook add` pins into every project and
  `hook remove` unpins (checked by K111). `skill-forced-eval` 1.0.1 names
  the new bundled `rules/skill-check/` always-apply rule as its companion, so
  Cursor gets the same skill check as a rule.
- The bundled `hooks/skill-forced-eval/` hook (strategy work items W13 and
  W14): on `UserPromptSubmit` in Claude Code it shortlists the skills the
  agent can see (`.claude/skills/`, `.agents/skills/`, `~/.claude/skills/`)
  whose quoted trigger phrasings, path components or keywords match the
  prompt, at most three, and injects a mandatory YES/NO check for them.
  Silent in Cursor, without visible skills, and for prompts that match
  nothing; always exits 0.
- `lint` rule I305 (strategy work item W11), a warning for content in an
  always-loaded instruction file (`AGENTS.md`, `CLAUDE.md`, `CLAUDE.local.md`,
  `.claude/rules/*.md`, `.cursor/rules/*.mdc`) that changes between sessions
  and so invalidates the cacheable prompt prefix: timestamps and "last
  updated" dates, `$(…)`/`${…}` expansions and `` !`command` `` injection,
  and `@imports` of files that change often. Fenced code is skipped.
- `doctor` reports whether Cursor runs the registered hooks (strategy work
  item W12): with a hook registered and no `"tool": "cursor"` event in the
  log it warns (`cursor-imports`) with the Third-Party Imports setting to
  confirm; once Cursor events exist it reports the count and last time.
- `doctor` audits MCP servers (strategy work item W10): every server the
  harnesses connect to from the current folder (`~/.claude.json` user and
  per-project entries with `disabledMcpjsonServers`, `.mcp.json`,
  `~/.cursor/mcp.json`, `.cursor/mcp.json`), its command or URL, and how many
  tool calls the event log records for it. After seven days of log history a
  server with no recorded call is `mcp-unused` (warning, naming a CLI
  alternative such as `gh` for GitHub where one exists); before that it is
  `mcp-unmeasured` (info). Advisory, like the telemetry section, under `mcp`
  in `--json`.
- `hook add` updates an existing `settings.json` entry whose matcher, timeout
  or entry script no longer matches the hook (reported as `updated`), so a
  hook upgrade that changes its matcher takes effect.
- `skillctl-logger` 1.1.0 records MCP tool calls: the `PostToolUse` matcher
  is now `Skill|mcp__.*` and each MCP event carries `server`.
- Directive-description rules O140–O145 (strategy work item W09), all
  warnings, in `validate`, `seal` and `install`: O140 a trigger verb
  addressing "this skill"; O141 a negative constraint; O142 concrete trigger
  phrasings (quoted, or introduced with "including"/"such as"); O144 no
  hedging language and no scaffold `[placeholder]` left in; O145 no
  `keywords` field. O103 now accepts any "when" clause rather than the literal
  "use when", so the directive form passes it.
- The skill index budget (O143): `status --budget` sizes the listing the
  agent sees (project pins plus global skills) against 1% of a 200,000-token
  context window and flags descriptions over 1,536 characters, which
  harnesses truncate. `add`, `global add` and `attach --skills` print the
  description warnings for the pinned skill and the budget line on stderr,
  and carry both under `advisory` in `--json`. `hook add` for a hook on
  `UserPromptSubmit` warns when any pinned skill fails O140–O145.
- `new` scaffolds the skill description in directive form with
  `[placeholders]`; existing templates are left untouched.
- `lint` rules I301–I303 (strategy work item W07), all warnings: I301 for an
  `AGENTS.md` over 200 lines; I303 for a section of `AGENTS.md` that reads
  like a procedure (five or more ordered steps, three or more code blocks, or
  a procedure-named heading with a few steps or a code block), reported at
  the heading with the suggestion to make it a skill; I302 for a
  `.cursor/rules/*.mdc` file over 500 lines or an `alwaysApply: true` rule
  over 100 lines.
- `lint [<folder>]` (strategy work item W06): read-only checks on a project's
  instruction files with the same formats, allow-list and exit codes as
  `validate`. I304 (error): a `CLAUDE.md` or `.claude/CLAUDE.md` without an
  `@AGENTS.md` import, or importing a missing `AGENTS.md`. I308 (warning): a
  `CLAUDE.md` line that also appears in `AGENTS.md`, with its line number.
  I309 (error): an `AGENTS.md` that Claude Code will skip because a
  `CLAUDE.md` or `CLAUDE.local.md` exists in the folder or a parent up to the
  repository root and nothing imports it. Every finding carries the suggested
  fix; skillctl never edits these team-owned files.
- Hook registration (strategy work item W02b). `hook add <hook>[@version]`
  pins a stored hook in the registry's new `global.hooks` section (omitted
  while empty, so existing registries are unchanged), links
  `~/.claude/hooks/<hook>` to the store version, and merges one entry per
  declared event into `~/.claude/settings.json`: other settings and
  hand-written hooks are kept, the previous bytes go to `settings.json.bak`,
  the write is atomic, a second run changes nothing, and an unparsable file is
  refused rather than overwritten. Cursor reads that file through its
  third-party imports, so one registration serves both tools. `hook remove`
  reverses all of it; `hook list` shows pin, link health and unregistered
  events. `relink --all` and `doctor --fix` restore hook links; `doctor` adds
  `hook-unregistered`, `hook-duplicate` (twice in settings, or also in
  `~/.cursor/hooks.json`) and `settings-invalid`; `gc` treats pinned hooks as
  pinned. `HOOK.md` gains optional `matchers` (per-event tool pattern) and
  `timeout` (1–600 s, default 10), validated by K111.
- The bundled `hooks/skillctl-logger/` hook: one JSON line per lifecycle
  event in `$SKILLCTL_HOME/log/events.jsonl`, tagged `claude-code` or
  `cursor`, accepting both tools' payload shapes, recording prompt length but
  never prompt text, and always exiting 0.
- Hook artifacts (strategy work item W02a). A folder with `HOOK.md` (frontmatter
  `name`, `description`, `metadata.version`, `events`, `entry`) and its entry
  script is a hook, stored, versioned, hashed and installed exactly like a
  skill in the same flat store namespace. `new --kind hook` scaffolds one from
  the new `templates/hook/` (`HOOK.md` and an executable Python `hook.py`,
  created by `init`); `seal`, `edit`, `install` and `validate` detect the kind
  from the folder; `skills` shows `(hook)` and a `kind` field; `meta.json`
  records `kind` (absent means `skill`, so existing stores need no migration).
  Hook rules K101–K110 replace the S/O/H groups for hooks; K101–K105 and K107
  cannot be allow-listed. `add`, `attach --skills` and `global add` refuse
  hooks and point at `hook add`.
- `new` copies every regular file beside the template document into the draft
  (previously only `SKILL.md` was read), so templates can ship scripts.
- `doctor` reports an observability baseline (strategy work item W01): whether
  Claude Code's OpenTelemetry export is enabled and complete
  (`CLAUDE_CODE_ENABLE_TELEMETRY`, the metrics and logs exporters, the OTLP
  protocol and endpoint, and `OTEL_LOG_TOOL_DETAILS`, without which
  `agent.name` is redacted), whether `ccusage` is on `PATH`, and a note that
  Cursor's numbers come from its usage dashboard and hook logs. The findings
  are advisory: they appear in the text report and under `telemetry` in
  `--json`, and never change the exit status.

### Fixed

- I305 no longer flags `$(…)` or `${…}` inside inline code: a rule that
  documents `"${...}"` placeholders does not expand anything. The
  onezoho-automation YAML conventions rule was a false positive.
- `subagent-schema` 1.0.1 finds an agent's `schema.json` by its frontmatter
  `name` as well as its file name (Claude Code identifies agents by `name`),
  and looks in `~/.claude/agents` for global agents after the project.
- `skill-forced-eval` 1.0.3 no longer states an unsourced "50% to 100%"
  activation figure; it points at `log summary`'s activation through the
  hook as the measurement. `skillctl-logger` 1.2.3 corrects the comment on
  Cursor's native event names, and the `skill-check` rule 1.0.1 lists
  `.cursor/skills/`, Cursor's own skills folder.

### Changed

- `doctor` reads Claude Code's export variables from the `env` block of
  `~/.claude/settings.json` as well as the environment, probes a local
  endpoint and warns `telemetry-collector-down` when nothing listens there
  (every export would be dropped), or notes `telemetry-collector` with the
  last record when skillctl's collector answers. The `telemetry-off` fix now
  suggests `skillctl telemetry enable`.

## [0.1.0] - 2026-10-18

First release. Published as the crate `skillctl-cli` (the binary is `skillctl`)
with a prebuilt universal macOS tarball attached to the GitHub release.

### Added

- Store and registry: one read-only copy of each skill version under
  `~/.skills/store/`, a YAML registry of projects, instances and pins with
  advisory locking, atomic writes and a backup on every save, and a canonical
  SHA-256 content hash per version.
- Linking: `attach`, `detach`, `status`, `add`, `remove`, `relink` and
  `global add/remove/list`. Links are symlinks into the store, created only
  where skillctl owns the path, repaired idempotently by `reconcile`, hidden
  through per-instance blocks in `.git/info/exclude`, and resolved to their
  project by device and inode rather than by path text.
- Authoring: `new` scaffolds a draft from a template, `edit` copies a stored
  version into a writable draft with a patch bump, `add --draft` links a draft
  for trying out, and `seal` validates, hashes and publishes it read-only.
- Installation: `install` from a Git URL (shallow clone, your own credentials)
  or a local folder, with skill discovery, `--path`, `--all`, `--ref`,
  `--version`, duplicate detection by content hash, and provenance in
  `meta.json`.
- Validation: `validate` with the Agent Skills spec rules S001–S008,
  organisation rules O101–O133 and harness-compatibility warnings H201–H205;
  text, JSON and JUnit reports; per-skill and registry-wide allow-lists;
  `--strict`; optional `--with-reference` against `skills-ref`.
- Maintenance: `doctor [--fix]` for drift between registry, links, store and
  environment; `gc [--keep n]` for unused versions; `config get/set` with
  automatic relinking after link-target changes; `completions <shell>`.
- Every mutating command supports `--dry-run`, every command supports `--json`,
  and every prompt has a flag so commands run non-interactively.
- `--version` prints the version, build commit and supported registry schema.
- Release pipeline on GitHub Actions: checks, universal macOS binary, optional
  signing and notarization, clean-runner end-to-end verification, GitHub
  release with tarball and checksum, crates.io publish and a verification
  install from crates.io.

### Known gaps

- The manual harness compatibility matrix (Claude Code, Cursor, ZohoCode) has
  not been run; the default link targets are provisional.
- `-v/--verbose` has no effect yet, and `--dry-run` prints a summary rather than
  the full recorded action plan.

[Unreleased]: https://github.com/abindran/skillctl/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/abindran/skillctl/releases/tag/v0.1.0
