import Foundation
import XCTest

@testable import SkillctlApp

/// Drives the real `skillctl` binary (the checkout's debug build) through the
/// app's state layer against a temporary home: the Rust JSON and the Swift
/// models are tested together, end to end. Skipped when the CLI is not built.
@MainActor
final class EndToEndTests: XCTestCase {
    private var temp: URL!
    private var home: URL!
    private var userHome: URL!
    private var repo: URL!
    private var state: AppState!

    /// `target/debug/skillctl`, found by walking up from this file.
    static func debugBinary() -> URL? {
        var folder = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        for _ in 0..<8 {
            let candidate = folder.appendingPathComponent("target/debug/skillctl")
            if FileManager.default.isExecutableFile(atPath: candidate.path) { return candidate }
            folder = folder.deletingLastPathComponent()
        }
        return nil
    }

    override func setUp() async throws {
        guard let binary = Self.debugBinary() else {
            throw XCTSkip("target/debug/skillctl is not built; run `cargo build` at the repository root first")
        }
        temp = FileManager.default.temporaryDirectory
            .appendingPathComponent("skillctl-ui-\(UUID().uuidString)")
            .resolvingSymlinksInPath()
        home = temp.appendingPathComponent("skills")
        userHome = temp.appendingPathComponent("userhome")
        repo = temp.appendingPathComponent("repo")
        for folder in [userHome!, repo!] {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        }
        try Self.shell("/usr/bin/git", ["init", "-q", repo.path])
        let defaults = UserDefaults(suiteName: "dev.abindran.skillctl.tests.\(UUID().uuidString)")!
        defaults.set(binary.path, forKey: "binaryPath")
        defaults.set(home.path, forKey: "homeOverride")
        // HOME is redirected so `hook add` writes a throwaway settings.json.
        state = AppState(defaults: defaults, environment: ["HOME": userHome.path])
    }

    override func tearDown() async throws {
        if let temp { try? FileManager.default.removeItem(at: temp) }
    }

    private static func shell(_ program: String, _ args: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: program)
        process.arguments = args
        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "\(program) \(args)")
    }

    private func cli() throws -> CLI {
        try XCTUnwrap(state.cli, "binary not resolved; searched \(state.searched)")
    }

    func testWholeWorkflowThroughTheAppStateAgainstTheRealBinary() async throws {
        let cli = try cli()
        await state.checkTool()
        XCTAssertNil(state.toolProblem, state.toolProblem ?? "")
        XCTAssertTrue(state.version?.hasPrefix("skillctl ") == true, state.version ?? "no version")

        // Author a skill and a hook through the app's own actions.
        try await cli.perform(["init"])
        await state.loadDrafts()
        XCTAssertTrue(state.drafts.isEmpty)
        await state.newDraft(kind: "skill", name: "review")
        await state.newDraft(kind: "hook", name: "logger")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertEqual(state.drafts.map(\.name), ["logger", "review"])
        XCTAssertEqual(state.drafts.map(\.kind), ["hook", "skill"])
        XCTAssertTrue(state.drafts[1].path.hasSuffix("/drafts/review"), state.drafts[1].path)
        let draftFindings = await state.validateDraft("review")
        XCTAssertNotNil(draftFindings)
        await state.seal("review", bump: nil)
        await state.seal("logger", bump: nil)
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertTrue(state.drafts.isEmpty, "sealing removes the drafts")
        XCTAssertEqual(state.artifact(named: "review", version: "0.1.0")?.kind, "skill")

        // Edit copies a version back into a draft; sealing with a bump stores the next version.
        await state.edit("review@0.1.0")
        XCTAssertEqual(state.drafts.map(\.name), ["review"])
        await state.seal("review", bump: "minor")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertNotNil(state.artifact(named: "review", version: "0.2.0"), "\(state.artifacts.map(\.reference))")

        // Garbage collection is previewed before it runs, and keeps the newest version.
        let preview = await state.previewGarbage(keep: 1)
        XCTAssertEqual(preview?.map(\.id), ["review@0.1.0"], "\(String(describing: preview))")
        XCTAssertNotNil(state.artifact(named: "review", version: "0.1.0"), "a preview deletes nothing")
        await state.collectGarbage(keep: 1)
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertNil(state.artifact(named: "review", version: "0.1.0"))
        XCTAssertNotNil(state.artifact(named: "review", version: "0.2.0"))

        // The root document of a version or a draft is readable through the app.
        let shownValue = await state.show("review@0.2.0")
        let shownDocument = try XCTUnwrap(shownValue)
        XCTAssertEqual(shownDocument.document, "SKILL.md")
        XCTAssertEqual(shownDocument.version, "0.2.0")
        XCTAssertTrue(shownDocument.content.contains("name: review"), shownDocument.content)
        XCTAssertEqual(shownDocument.files.map(\.path), ["SKILL.md"])
        XCTAssertTrue(shownDocument.documentURL.path.hasSuffix("/review/SKILL.md"))

        // A specific version can be deleted from the store; pinned ones are refused.
        await state.edit("review@0.2.0")  // edit itself bumps the draft to 0.2.1
        await state.seal("review", bump: nil)
        XCTAssertNotNil(state.artifact(named: "review", version: "0.2.1"), "\(state.artifacts.map(\.reference))")
        await state.uninstall("review@0.2.1")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertNil(state.artifact(named: "review", version: "0.2.1"), "the version is gone")
        XCTAssertNotNil(state.artifact(named: "review", version: "0.2.0"), "the other one stays")
        await state.newDraft(kind: "command", name: "checkpoint")
        await state.seal("checkpoint", bump: nil)
        await state.edit("checkpoint@0.1.0")
        await state.seal("checkpoint", bump: nil)
        await state.makeGlobal("checkpoint@0.1.1")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertEqual(state.artifact(named: "checkpoint", version: "0.1.1")?.isGlobal, true)
        XCTAssertEqual(Scope.of(state.artifacts.filter { $0.name == "checkpoint" }), .global)
        XCTAssertTrue(FileManager.default.fileExists(atPath: userHome.appendingPathComponent(".claude/commands/checkpoint.md").path),
                      "a global command is linked into the redirected HOME")
        await state.removeGlobal("checkpoint")
        XCTAssertEqual(state.artifact(named: "checkpoint", version: "0.1.1")?.isGlobal, false)
        await state.uninstallMany(["checkpoint@0.1.0", "checkpoint@0.1.1"])
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertTrue(state.artifacts.allSatisfy { $0.name != "checkpoint" }, "bulk delete removed both versions")

        // Agents get a Cursor override through derive.
        await state.newDraft(kind: "agent", name: "explore")
        await state.derive(agent: "explore", model: "gpt-5")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        let agentDraft = try XCTUnwrap(state.drafts.first { $0.name == "explore" })
        XCTAssertEqual(agentDraft.kind, "agent")
        XCTAssertTrue(FileManager.default.fileExists(atPath: agentDraft.url.appendingPathComponent("cursor.md").path))
        await state.seal("explore", bump: nil)
        XCTAssertEqual(state.artifact(named: "explore", version: "1.0.0")?.kind ?? state.artifacts.first { $0.name == "explore" }?.kind, "agent")

        // Attach a folder to a new project with the skill pinned.
        await state.attach(folder: repo, newProject: "web", existing: nil, skills: ["review"])
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertEqual(state.projects.map(\.name), ["web"])
        let project = try XCTUnwrap(state.projects.first)
        XCTAssertEqual(project.skills, ["review": "0.2.0"], "attach pins the highest stable version")
        let instance = try XCTUnwrap(project.instances.first)
        XCTAssertEqual(instance.url.resolvingSymlinksInPath(), repo.resolvingSymlinksInPath())
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: repo.appendingPathComponent(".claude/skills/review/SKILL.md").path),
            "the pin is linked into the project")

        // The store review sees every version with its pins and kind.
        let review = try XCTUnwrap(state.artifact(named: "review", version: "0.2.0"))
        XCTAssertEqual(review.kind, "skill")
        XCTAssertEqual(review.pinned_by, ["web"])
        XCTAssertFalse(review.unused)
        let logger = try XCTUnwrap(state.artifact(named: "logger", version: "0.1.0"))
        XCTAssertEqual(logger.kind, "hook")
        XCTAssertTrue(logger.unused, "an unregistered hook is unpinned")
        XCTAssertNil(logger.uses, "the log cannot see hooks")

        // Register the hook: link, settings merge, pin.
        await state.registerHook("logger")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertEqual(state.hooks.map(\.name), ["logger"])
        let hook = try XCTUnwrap(state.hooks.first)
        XCTAssertEqual(hook.pin, "0.1.0")
        XCTAssertEqual(hook.link.status, "ok")
        XCTAssertTrue(hook.missing.isEmpty)
        XCTAssertTrue(hook.healthy)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: userHome.appendingPathComponent(".claude/settings.json").path),
            "registration lands in the redirected HOME, never the real one")
        XCTAssertEqual(state.artifact(named: "logger", version: "0.1.0")?.pinned_by, ["hook"])

        // A log written by the logger feeds the summary.
        let yesterday = ISO8601DateFormatter().string(from: Date().addingTimeInterval(-86_400))
        var lines = (0..<3).map {
            #"{"ts":"\#(yesterday)","tool":"claude-code","event":"UserPromptSubmit","session":"s\#($0)"}"#
        }
        lines.append(#"{"ts":"\#(yesterday)","tool":"claude-code","event":"PostToolUse","session":"s0","tool_name":"Skill","skill":"review"}"#)
        lines.append(#"{"ts":"\#(yesterday)","tool":"cursor","event":"Stop","session":"c1","harness_version":"3.25.9"}"#)
        try FileManager.default.createDirectory(at: home.appendingPathComponent("log"), withIntermediateDirectories: true)
        try lines.joined(separator: "\n").write(to: home.appendingPathComponent("log/events.jsonl"), atomically: true, encoding: .utf8)
        await state.loadSummary()
        let summary = try XCTUnwrap(state.summary)
        XCTAssertEqual(summary.days, 7)
        XCTAssertEqual(summary.current.prompts, 3)
        XCTAssertEqual(summary.current.activations, 1)
        XCTAssertEqual(summary.current.skills, ["review": 1])
        XCTAssertEqual(summary.current.by_tool, ["claude-code": 4, "cursor": 1])
        XCTAssertEqual(summary.current.sessions, 4)
        XCTAssertEqual(summary.previous.events, 0)
        XCTAssertTrue(summary.alerts.isEmpty, "\(summary.alerts)")
        await state.loadArtifacts()
        XCTAssertEqual(state.artifact(named: "review", version: "0.2.0")?.uses, 1)

        // Usage decodes whether or not ccusage is installed on this machine.
        await state.loadUsage()
        let usage = try XCTUnwrap(state.usage)
        if usage.available {
            XCTAssertFalse(usage.from.isEmpty)
        } else {
            XCTAssertTrue(usage.note.contains("ccusage"), usage.note)
        }
        XCTAssertTrue((7...8).contains(summary.current.daily.count), "a window ending now touches 7 or 8 calendar days: \(summary.current.daily.count)")
        XCTAssertEqual(summary.current.hook_shortlists, 0)

        // Doctor: clean, with its advisory sections decoded.
        await state.runDoctor(fix: false)
        let doctor = try XCTUnwrap(state.doctor)
        XCTAssertEqual(doctor.remaining, 0, "\(doctor.problems)")
        XCTAssertTrue(doctor.problems.isEmpty)
        XCTAssertNotNil(doctor.telemetry)
        let harnesses = try XCTUnwrap(doctor.harnesses)
        XCTAssertEqual(harnesses.detected["cursor"] ?? nil, "3.25.9", "Cursor's version comes from the log")
        XCTAssertEqual(doctor.advisory.map(\.title), ["Telemetry", "Harness versions", "MCP servers"])

        // Status of the attached folder, validation and lint all decode.
        let statusValue = await state.status(of: instance)
        let status = try XCTUnwrap(statusValue)
        XCTAssertTrue(status.attached)
        XCTAssertEqual(status.links?.map(\.status), ["ok", "ok"], "\(String(describing: status.links))")
        let findingsValue = await state.validate("review@0.2.0")
        let findings = try XCTUnwrap(findingsValue)
        XCTAssertTrue(findings.allSatisfy { ["error", "warning"].contains($0.severity) }, "\(findings)")
        let lintValue = await state.lint(folder: repo)
        let lint = try XCTUnwrap(lintValue)
        XCTAssertTrue(lint.isEmpty, "a folder without instruction files lints clean: \(lint)")

        // A failing command surfaces the CLI's own message and nothing changes.
        await state.addPin("does-not-exist", project: "web")
        XCTAssertNotNil(state.lastError)
        XCTAssertEqual(state.console.last?.succeeded, false)
        XCTAssertEqual(state.projects.first?.skills, ["review": "0.2.0"])
        state.lastError = nil
        await state.uninstall("review@0.2.0")
        XCTAssertTrue(state.lastError?.contains("pinned by web") == true, state.lastError ?? "no error")
        XCTAssertNotNil(state.artifact(named: "review", version: "0.2.0"), "a pinned version is never deleted")
        state.lastError = nil

        // Unpin, unregister, detach: back to empty.
        await state.removePin("review", project: "web")
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertEqual(state.projects.first?.skills, [:])
        XCTAssertTrue(state.artifact(named: "review", version: "0.2.0")?.unused == true)
        await state.removeHook("logger")
        XCTAssertTrue(state.hooks.isEmpty)
        await state.detach(instance: instance, deleteProject: true)
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertTrue(state.projects.isEmpty)
        XCTAssertFalse(FileManager.default.fileExists(atPath: repo.appendingPathComponent(".claude/skills/review").path))

        // Every command the app ran went through --json.
        XCTAssertFalse(state.console.isEmpty)
        XCTAssertTrue(state.console.allSatisfy { $0.args.first == "--json" }, "\(state.console.map(\.args))")
    }

    /// The Log screen's collector card decodes `telemetry status` and
    /// `telemetry report`; turning it off removes only skillctl's variables.
    /// (Turning it on would load a real LaunchAgent, so it is tested in Rust
    /// with --no-service instead.)
    func testTelemetryStatusReportAndTurningOff() async throws {
        let formatter = ISO8601DateFormatter()
        let now = formatter.string(from: Date())
        let day = String(now.prefix(10))
        let records = home.appendingPathComponent("telemetry")
        try FileManager.default.createDirectory(at: records, withIntermediateDirectories: true)
        try """
            {"ts":"\(now)","signal":"log","name":"api_request","attrs":{"model":"claude-sonnet-5","cost_usd":0.75,"output_tokens":900,"skill.name":"review","query_source":"main"}}
            {"ts":"\(now)","signal":"log","name":"tool_result","attrs":{"tool_name":"Bash","success":"false"}}

            """.write(to: records.appendingPathComponent("\(day).jsonl"), atomically: true, encoding: .utf8)
        let settings = userHome.appendingPathComponent(".claude/settings.json")
        try FileManager.default.createDirectory(at: settings.deletingLastPathComponent(), withIntermediateDirectories: true)
        try #"{"env": {"CLAUDE_CODE_ENABLE_TELEMETRY": "1", "OTEL_LOGS_EXPORTER": "otlp", "KEEP": "me"}}"#
            .write(to: settings, atomically: true, encoding: .utf8)

        await state.loadTelemetry()
        XCTAssertNil(state.lastError, state.lastError ?? "")
        let status = try XCTUnwrap(state.telemetry)
        XCTAssertFalse(status.settings.configured)
        XCTAssertTrue(status.settings.missing.contains("OTEL_EXPORTER_OTLP_ENDPOINT"))
        XCTAssertEqual(status.records_today, 2)
        let report = try XCTUnwrap(state.telemetryReport)
        XCTAssertEqual(report.source, "logs")
        XCTAssertEqual(report.total.cost, 0.75)
        XCTAssertEqual(report.by_skill.first?.key, "review")
        XCTAssertEqual(report.tools.first?.failures, 1)

        await state.setTelemetry(enabled: false)
        XCTAssertNil(state.lastError, state.lastError ?? "")
        let written = try JSONSerialization.jsonObject(with: Data(contentsOf: settings)) as? [String: Any]
        XCTAssertEqual(written?["env"] as? [String: String], ["KEEP": "me"])
    }

    /// The Quality screen's "What is linted" card decodes `lint --files`.
    func testLintFilesListsWhatLintReads() async throws {
        let listed = await state.lintFiles(folder: repo)
        let empty = try XCTUnwrap(listed)
        XCTAssertFalse(empty.contains(where: \.present))
        try "# Facts\nOne.\n".write(to: repo.appendingPathComponent("AGENTS.md"), atomically: true, encoding: .utf8)
        let relisted = await state.lintFiles(folder: repo)
        let files = try XCTUnwrap(relisted)
        let agents = try XCTUnwrap(files.first { $0.name == "AGENTS.md" })
        XCTAssertTrue(agents.present)
        XCTAssertEqual(agents.lines, 2)
        XCTAssertTrue(agents.rules.contains("I301"))

        // The Rules reference tab decodes the catalog.
        await state.loadRules()
        XCTAssertNil(state.lastError, state.lastError ?? "")
        XCTAssertTrue(state.rules.contains { $0.id == "S001" && !$0.allow_listable })
        XCTAssertTrue(state.rules.contains { $0.id == "I309" && $0.reported_by == "lint" })
    }

    func testAnOutdatedBinaryIsReportedInsteadOfUsed() async throws {
        let fake = temp.appendingPathComponent("old-skillctl")
        try """
        #!/bin/sh
        case "$1" in
          --version) echo "skillctl 0.1.0 (commit abc, registry schema 1)";;
          --help) printf 'Usage: skillctl [OPTIONS] <COMMAND>\\n\\nCommands:\\n  init     Initialize\\n  doctor   Diagnose\\n  validate Validate\\n';;
          *) echo '{"error":"usage","message":"unrecognized subcommand","hint":"","exit_code":2}'; exit 2;;
        esac
        """.write(to: fake, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: fake.path)
        let old = CLI(binary: fake, home: home.path, environment: [:])
        let missing = await old.missingCommands()
        XCTAssertEqual(missing, ["artifacts", "drafts", "hook", "lint", "log", "projects", "rules", "show", "telemetry", "uninstall"])

        let defaults = UserDefaults(suiteName: "dev.abindran.skillctl.tests.\(UUID().uuidString)")!
        defaults.set(fake.path, forKey: "binaryPath")
        defaults.set(home.path, forKey: "homeOverride")
        let state = AppState(defaults: defaults, environment: [:])
        await state.refreshAll()
        let problem = try XCTUnwrap(state.toolProblem)
        XCTAssertTrue(problem.contains("older than this app"), problem)
        XCTAssertTrue(problem.contains("log"), problem)
        XCTAssertTrue(state.projects.isEmpty && state.console.allSatisfy { $0.args == ["--version"] || $0.args == ["--help"] },
                      "nothing else is run against an outdated binary: \(state.console.map(\.args))")

        // The current binary passes the same check.
        let currentMissing = await (try cli()).missingCommands()
        XCTAssertEqual(currentMissing, [])
    }

    func testBinaryDiscoveryPrefersAnExplicitPathThenTheBundledCopyThenACheckoutsDebugBuild() throws {
        let candidates = CLI.candidates(preferred: "~/custom/skillctl").map(\.path)
        XCTAssertEqual(candidates.first, FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("custom/skillctl").path)
        let contents = try XCTUnwrap(Bundle.main.executableURL?.deletingLastPathComponent().deletingLastPathComponent().path)
        XCTAssertEqual(candidates[1], contents + "/Helpers/skillctl", "the copy bundled with the app comes second")
        let debugIndex = try XCTUnwrap(candidates.firstIndex { $0.hasSuffix("target/debug/skillctl") })
        XCTAssertGreaterThan(debugIndex, 1)
        let cargoIndex = try XCTUnwrap(candidates.firstIndex { $0.hasSuffix(".cargo/bin/skillctl") })
        XCTAssertLessThan(debugIndex, cargoIndex, "a checkout's debug build beats an installed, possibly older, copy")
        XCTAssertNil(CLI.locate(preferred: temp.appendingPathComponent("missing").path).flatMap { $0.path.hasSuffix("missing") ? $0 : nil })
    }

    func testLinkingTheCommandLineToolReplacesOnlySymlinks() throws {
        let binary = try XCTUnwrap(Self.debugBinary())
        let bin = temp.appendingPathComponent("local/bin")
        let link = bin.appendingPathComponent("skillctl")
        let first = state.linkCommandLineTool(binary, into: bin)
        XCTAssertTrue(first.hasPrefix("Linked "), first)
        XCTAssertEqual(try FileManager.default.destinationOfSymbolicLink(atPath: link.path), binary.path)
        // A second run replaces the symlink.
        let second = state.linkCommandLineTool(binary, into: bin)
        XCTAssertTrue(second.hasPrefix("Linked "), second)
        // A regular file in the way is never touched.
        try FileManager.default.removeItem(at: link)
        try "#!/bin/sh\n".write(to: link, atomically: true, encoding: .utf8)
        let refused = state.linkCommandLineTool(binary, into: bin)
        XCTAssertTrue(refused.contains("is not a symlink"), refused)
        XCTAssertEqual(try String(contentsOf: link, encoding: .utf8), "#!/bin/sh\n")
    }

    func testTheCommandLineToolIsNotInstalledFromATranslocatedOrUnbundledApp() async throws {
        XCTAssertTrue(AppState.isTranslocated(URL(fileURLWithPath: "/private/var/folders/x1/T/AppTranslocation/3F2A-9C/d/Skillctl.app/Contents/Helpers/skillctl")))
        XCTAssertFalse(AppState.isTranslocated(URL(fileURLWithPath: "/Applications/Skillctl.app/Contents/Helpers/skillctl")))
        // The test process is not the app bundle, so nothing is linked and the
        // redirected home stays untouched.
        await state.installCommandLineTool(force: true)
        XCTAssertEqual(state.commandLine, .notBundled)
        XCTAssertFalse(FileManager.default.fileExists(atPath: userHome.appendingPathComponent(".local/bin/skillctl").path))
    }

    func testAddingLocalBinToPathWritesTheShellsStartupFileOnce() throws {
        XCTAssertEqual(AppState.profileFile(shell: "/bin/zsh", home: userHome).lastPathComponent, ".zprofile")
        XCTAssertEqual(AppState.profileFile(shell: "/opt/homebrew/bin/bash", home: userHome).lastPathComponent, ".bash_profile")
        XCTAssertEqual(AppState.profileFile(shell: "/opt/homebrew/bin/fish", home: userHome).path,
                       userHome.appendingPathComponent(".config/fish/conf.d/skillctl.fish").path)
        XCTAssertEqual(AppState.profileFile(shell: "/bin/sh", home: userHome).lastPathComponent, ".profile")

        // An existing file without a trailing newline keeps its content and gains the block once.
        let profile = userHome.appendingPathComponent(".zprofile")
        try "export EDITOR=vim".write(to: profile, atomically: true, encoding: .utf8)
        XCTAssertTrue(try AppState.addToPath(profile: profile, shell: "/bin/zsh"))
        let written = try String(contentsOf: profile, encoding: .utf8)
        XCTAssertTrue(written.hasPrefix("export EDITOR=vim\n"), written)
        XCTAssertTrue(written.contains(AppState.pathMarker), written)
        XCTAssertTrue(written.contains("export PATH=\"$HOME/.local/bin:$PATH\""), written)
        XCTAssertFalse(try AppState.addToPath(profile: profile, shell: "/bin/zsh"), "a second run writes nothing")
        XCTAssertEqual(try String(contentsOf: profile, encoding: .utf8), written)

        // A missing fish conf.d folder is created, with fish's own syntax.
        let fish = AppState.profileFile(shell: "/opt/homebrew/bin/fish", home: userHome)
        XCTAssertTrue(try AppState.addToPath(profile: fish, shell: "/opt/homebrew/bin/fish"))
        XCTAssertTrue(try String(contentsOf: fish, encoding: .utf8).contains("fish_add_path --global --prepend $HOME/.local/bin"))
    }

    func testTheLoginShellReportsItsPath() async throws {
        // zsh with a redirected HOME that has no startup files: PATH still comes from path_helper.
        let path = await LoginShell.path(shell: "/bin/zsh", environment: ["HOME": userHome.path])
        let reported = try XCTUnwrap(path)
        XCTAssertTrue(reported.split(separator: ":").contains("/usr/bin"), reported)
        // A startup file that puts ~/.local/bin on PATH is seen through the same probe.
        XCTAssertTrue(try AppState.addToPath(profile: userHome.appendingPathComponent(".zprofile"), shell: "/bin/zsh"))
        let state = AppState(defaults: UserDefaults(suiteName: "dev.abindran.skillctl.tests.\(UUID().uuidString)")!,
                             environment: ["HOME": userHome.path, "SHELL": "/bin/zsh"])
        let onPath = await state.pathIncludes(userHome.appendingPathComponent(".local/bin"))
        XCTAssertEqual(onPath, true)
        // A shell that does not exist yields "unknown", never a wrong answer.
        let none = await LoginShell.path(shell: temp.appendingPathComponent("nosuchshell").path, environment: [:])
        XCTAssertNil(none)
    }
}
