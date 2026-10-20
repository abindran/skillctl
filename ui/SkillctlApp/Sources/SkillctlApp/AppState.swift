import Foundation
import SwiftUI

/// Everything the windows show, loaded from the CLI and refreshed after each
/// action. One instance per app, injected through the environment.
@MainActor
@Observable
final class AppState {
    // Settings, persisted.
    private let defaults: UserDefaults
    var binaryPath: String {
        didSet { defaults.set(binaryPath, forKey: "binaryPath"); relocate() }
    }
    var homeOverride: String {
        didSet { defaults.set(homeOverride, forKey: "homeOverride"); relocate() }
    }

    // Resolved tool.
    private(set) var cli: CLI?
    private(set) var version: String?
    private(set) var searched: [String] = []
    /// Why the resolved binary cannot be used (older than the app), or nil.
    private(set) var toolProblem: String?

    // Loaded data.
    var projects: [Project] = []
    var artifacts: [Artifact] = []
    var hooks: [HookEntry] = []
    var doctor: DoctorReport?
    var summary: LogSummary?
    var summaryDays = 7

    // Activity.
    var busy: Set<String> = []
    var lastError: String?
    var console: [CommandRun] = []
    var showConsole = false

    /// Extra environment for every CLI call (tests redirect `HOME`).
    private let environment: [String: String]

    init(defaults: UserDefaults = .standard, environment: [String: String] = [:]) {
        self.defaults = defaults
        self.environment = environment
        binaryPath = defaults.string(forKey: "binaryPath") ?? ""
        homeOverride = defaults.string(forKey: "homeOverride") ?? ""
        relocate()
    }

    var isBusy: Bool { !busy.isEmpty }
    var storeHooks: [Artifact] { artifacts.filter { $0.kind == "hook" } }
    var registeredHookNames: Set<String> { Set(hooks.map(\.name)) }

    func artifact(named name: String, version: String) -> Artifact? {
        artifacts.first { $0.name == name && $0.version == version }
    }

    func kind(of skill: String) -> String? {
        artifacts.first { $0.name == skill }?.kind
    }

    private func relocate() {
        searched = CLI.candidates(preferred: binaryPath).map(\.path)
        toolProblem = nil
        if let url = CLI.locate(preferred: binaryPath) {
            cli = CLI(binary: url, home: homeOverride, environment: environment)
            Task { await checkTool() }
        } else {
            cli = nil
            version = nil
        }
    }

    /// Record the binary's version and whether it is current enough for the app.
    func checkTool() async {
        guard let cli else { return }
        version = await cli.version()
        let missing = await cli.missingCommands()
        toolProblem = missing.isEmpty ? nil : """
            skillctl at \(cli.binary.path) (\(version ?? "unknown version")) has no \(missing.joined(separator: ", ")) command(s), so it is older than this app. \
            Point Settings (⌘,) at a current build, for example the checkout's target/debug/skillctl, or reinstall skillctl.
            """
    }

    // MARK: Loading

    func refreshAll() async {
        await checkTool()
        if toolProblem != nil { return }
        async let a: () = loadProjects()
        async let b: () = loadArtifacts()
        async let c: () = loadHooks()
        async let d: () = loadSummary()
        _ = await (a, b, c, d)
    }

    func loadProjects() async {
        projects = await load("projects", [Project].self, ["projects"]) ?? projects
    }

    func loadArtifacts() async {
        artifacts = await load("artifacts", [Artifact].self, ["artifacts"]) ?? artifacts
    }

    func loadHooks() async {
        hooks = await load("hooks", [HookEntry].self, ["hook", "list"]) ?? hooks
    }

    func loadSummary() async {
        summary = await load("summary", LogSummary.self, ["log", "summary", "--days", String(summaryDays)]) ?? summary
    }

    func runDoctor(fix: Bool) async {
        // doctor exits 1 while problems remain, so decode regardless of status.
        guard let cli else { return reportMissingBinary() }
        let key = fix ? "doctor-fix" : "doctor"
        busy.insert(key)
        defer { busy.remove(key) }
        var args = ["doctor"]
        if fix { args.append("--fix") }
        let run = await cli.run(fix ? ["--yes"] + args : args)
        console.append(run)
        if let report = try? JSONDecoder().decode(DoctorReport.self, from: Data(run.stdout.utf8)) {
            doctor = report
        } else {
            lastError = CLI.failure(of: run).localizedDescription
        }
        if fix { await refreshAll() }
    }

    func status(of instance: Instance) async -> StatusReport? {
        guard let cli else { return nil }
        let result = try? await cli.decode(StatusReport.self, ["status"], cwd: instance.url)
        if let result { console.append(result.1) }
        return result?.0
    }

    func validate(_ reference: String) async -> [Finding]? {
        await load("validate", [Finding].self, ["validate", reference], allowExit: 3)
    }

    func lint(folder: URL) async -> [Finding]? {
        await load("lint", [Finding].self, ["lint", folder.path], allowExit: 3)
    }

    // MARK: Actions (each refreshes what it changed)

    func addPin(_ reference: String, project: String) async {
        await perform(["add", reference, "--project", project]) { await self.loadProjects(); await self.loadArtifacts() }
    }

    func removePin(_ skill: String, project: String) async {
        await perform(["remove", skill, "--project", project]) { await self.loadProjects(); await self.loadArtifacts() }
    }

    func attach(folder: URL, newProject: String?, existing: String?, skills: [String]) async {
        var args = ["attach"]
        if let newProject, !newProject.isEmpty {
            args += ["--new", newProject]
        } else if let existing {
            args += ["--project", existing]
        }
        if !skills.isEmpty { args += ["--skills", skills.joined(separator: ",")] }
        await perform(args, cwd: folder) { await self.refreshAll() }
    }

    func detach(instance: Instance, deleteProject: Bool) async {
        var args = ["detach"]
        if deleteProject { args.append("--delete-project") }
        await perform(args, cwd: instance.url) { await self.refreshAll() }
    }

    func relink(project: String?) async {
        let args = project.map { ["relink", "--project", $0] } ?? ["relink", "--all"]
        await perform(args) { await self.loadProjects() }
    }

    func registerHook(_ reference: String) async {
        await perform(["hook", "add", reference]) { await self.loadHooks(); await self.loadArtifacts() }
    }

    func removeHook(_ name: String) async {
        await perform(["hook", "remove", name]) { await self.loadHooks(); await self.loadArtifacts() }
    }

    func install(source: String, all: Bool, reference: String?, subpath: String?) async {
        var args = ["install", source]
        if all { args.append("--all") }
        if let reference, !reference.isEmpty { args += ["--ref", reference] }
        if let subpath, !subpath.isEmpty { args += ["--path", subpath] }
        await perform(args) { await self.loadArtifacts() }
    }

    func collectGarbage(keep: Int) async {
        await perform(["gc", "--keep", String(keep)]) { await self.loadArtifacts() }
    }

    // MARK: Plumbing

    private func reportMissingBinary() {
        lastError = CLIFailure.binaryNotFound(searched: searched).localizedDescription
    }

    private func load<T: Decodable>(_ key: String, _ type: T.Type, _ args: [String], allowExit: Int32? = nil) async -> T? {
        guard let cli else { reportMissingBinary(); return nil }
        busy.insert(key)
        defer { busy.remove(key) }
        let run = await cli.run(args)
        console.append(run)
        if run.succeeded || run.exitCode == allowExit {
            do {
                return try JSONDecoder().decode(T.self, from: Data(run.stdout.utf8))
            } catch {
                lastError = CLIFailure.decoding(args: args, detail: "\(error)").localizedDescription
                return nil
            }
        }
        lastError = CLI.failure(of: run).localizedDescription
        return nil
    }

    private func perform(_ args: [String], cwd: URL? = nil, then reload: () async -> Void) async {
        guard let cli else { return reportMissingBinary() }
        let key = args.joined(separator: " ")
        busy.insert(key)
        defer { busy.remove(key) }
        let run = await cli.run(["--yes"] + args, cwd: cwd)
        console.append(run)
        if run.succeeded {
            await reload()
        } else {
            lastError = CLI.failure(of: run).localizedDescription
        }
    }
}
