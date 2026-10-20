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
        XCTAssertEqual(missing, ["artifacts", "drafts", "hook", "lint", "log", "projects"])

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
}
