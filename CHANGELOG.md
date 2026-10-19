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

- `doctor` audits MCP servers (strategy work item W10): every server the
  harnesses connect to from the current folder (`~/.claude.json` user and
  per-project entries with `disabledMcpjsonServers`, `.mcp.json`,
  `~/.cursor/mcp.json`, `.cursor/mcp.json`), its command or URL, and how many
  tool calls the event log records for it. After seven days of log history a
  server with no recorded call is `mcp-unused` (warning, naming a CLI
  alternative such as `gh` for GitHub where one exists); before that it is
  `mcp-unmeasured` (info). Advisory, like the telemetry section, under `mcp`
  in `--json`.
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
