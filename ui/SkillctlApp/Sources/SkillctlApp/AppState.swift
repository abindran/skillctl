import Foundation
import SwiftUI

enum ToolbarAction: Equatable {
    case newDraft, install, cleanUp, deleteSelected, attach
}

struct ToolbarRequest: Equatable {
    let action: ToolbarAction
    let screen: Screen
    let id = UUID()
}

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
    var drafts: [Draft] = []
    var hooks: [HookEntry] = []
    var doctor: DoctorReport?
    var lastDoctorRun: Date?
    var summary: LogSummary?
    var usage: UsageReport?
    var telemetry: TelemetryStatus?
    var telemetryReport: TelemetryReport?
    var summaryDays = 7

    // Window toolbar: one toolbar for the whole window (per-screen toolbars
    // inside the split view were left behind when switching screens), which
    // asks the current screen to act; the screen owns its sheets.
    var toolbarRequest: ToolbarRequest?
    /// The Store's selected version count and inspector, read by the toolbar.
    var storeSelectionCount = 0
    var storeInspector = false

    func request(_ action: ToolbarAction, on screen: Screen) {
        toolbarRequest = ToolbarRequest(action: action, screen: screen)
    }

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
    /// `0.1.0` from `skillctl 0.1.0 (commit …, registry schema 1)`.
    var shortVersion: String? {
        version?.split(separator: " ").dropFirst().first.map(String.init)
    }
    /// The CLI inside this app bundle, if the build carries one.
    var bundledBinary: URL? { CLI.bundled }
    var usesBundledBinary: Bool { cli != nil && cli?.binary == CLI.bundled }

    // MARK: Command line tool

    /// What became of the CLI shipped inside the app on this launch.
    enum CommandLineStatus: Equatable {
        /// This build carries no CLI (a `swift run` or test process).
        case notBundled
        /// macOS runs the app from a temporary read-only copy because it was
        /// opened straight from a download; a link into the bundle would die
        /// with that copy, so nothing was linked.
        case translocated
        /// `~/.local/bin/skillctl` points at the bundled CLI. `onPath` says
        /// whether a new terminal will find it; nil when the shell could not
        /// be asked.
        case linked(link: String, onPath: Bool?)
        /// Linking failed; the message says why.
        case failed(String)
    }
    private(set) var commandLine: CommandLineStatus = .notBundled
    private var commandLineChecked = false

    /// `HOME` as the CLI sees it (tests redirect it), else the user's home.
    var userHome: URL {
        if let home = environment["HOME"], !home.isEmpty { return URL(fileURLWithPath: home) }
        return FileManager.default.homeDirectoryForCurrentUser
    }
    /// Where the command line tool is linked: `~/.local/bin`, which needs no
    /// administrator rights and is the conventional user-level bin folder.
    var localBin: URL { userHome.appendingPathComponent(".local/bin") }
    private var loginShell: String {
        environment["SHELL"] ?? ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
    }

    /// Gatekeeper's app translocation runs a quarantined app from a random
    /// folder under `/private/var/folders/…/AppTranslocation/`. The user's
    /// move of the app into Applications (through Finder) ends it.
    static func isTranslocated(_ url: URL) -> Bool {
        url.path.contains("/AppTranslocation/")
    }

    /// Install the bundled CLI for the terminal: link it into `~/.local/bin`
    /// and find out whether that folder is on the login shell's `PATH`. Runs
    /// once per launch (every launch, so a moved app is relinked); `force`
    /// repeats it. Does nothing in builds without a bundled CLI.
    func installCommandLineTool(force: Bool = false) async {
        guard force || !commandLineChecked else { return }
        commandLineChecked = true
        guard let bundled = bundledBinary else {
            commandLine = .notBundled
            return
        }
        if Self.isTranslocated(bundled) {
            commandLine = .translocated
            return
        }
        let message = linkCommandLineTool(bundled, into: localBin)
        guard message.hasPrefix("Linked ") else {
            commandLine = .failed(message)
            return
        }
        let onPath = await pathIncludes(localBin)
        commandLine = .linked(link: localBin.appendingPathComponent("skillctl").path, onPath: onPath)
    }

    /// Whether `directory` is on `PATH`: in this process's environment, or
    /// in what the user's login shell reports. nil when the shell did not
    /// answer within the timeout.
    func pathIncludes(_ directory: URL) async -> Bool? {
        let target = directory.standardizedFileURL.path
        func contains(_ path: String) -> Bool {
            path.split(separator: ":").contains { entry in
                URL(fileURLWithPath: (String(entry) as NSString).expandingTildeInPath).standardizedFileURL.path == target
            }
        }
        if let path = ProcessInfo.processInfo.environment["PATH"], contains(path) {
            return true
        }
        guard let loginPath = await LoginShell.path(shell: loginShell, environment: environment) else {
            return nil
        }
        return contains(loginPath)
    }

    static let pathMarker = "# Added by Skillctl.app: the skillctl command line tool"

    /// The startup file a new terminal of this shell reads. Login shells
    /// (Terminal, iTerm and editors' terminals open login shells on macOS)
    /// read `.zprofile` and `.bash_profile`; fish reads every `conf.d` file.
    static func profileFile(shell: String, home: URL) -> URL {
        switch (shell as NSString).lastPathComponent {
        case "zsh": return home.appendingPathComponent(".zprofile")
        case "bash": return home.appendingPathComponent(".bash_profile")
        case "fish": return home.appendingPathComponent(".config/fish/conf.d/skillctl.fish")
        default: return home.appendingPathComponent(".profile")
        }
    }

    /// The lines that put `~/.local/bin` on `PATH` for this shell.
    static func pathLines(shell: String) -> String {
        let export = (shell as NSString).lastPathComponent == "fish"
            ? "fish_add_path --global --prepend $HOME/.local/bin"
            : "export PATH=\"$HOME/.local/bin:$PATH\""
        return "\n\(pathMarker)\n\(export)\n"
    }

    /// Append the `PATH` lines to `profile` unless the marker is already in
    /// it. Returns whether anything was written. The file is created when
    /// missing; existing content is never changed.
    static func addToPath(profile: URL, shell: String) throws -> Bool {
        let existing = (try? String(contentsOf: profile, encoding: .utf8)) ?? ""
        if existing.contains(pathMarker) { return false }
        try FileManager.default.createDirectory(at: profile.deletingLastPathComponent(), withIntermediateDirectories: true)
        let separator = existing.isEmpty || existing.hasSuffix("\n") ? "" : "\n"
        try (existing + separator + pathLines(shell: shell)).write(to: profile, atomically: true, encoding: .utf8)
        return true
    }

    /// Put `~/.local/bin` on `PATH` through the login shell's startup file,
    /// then re-check. Returns the message to show.
    func addLocalBinToPath() async -> String {
        let profile = Self.profileFile(shell: loginShell, home: userHome)
        let shown = profile.path.replacingOccurrences(of: userHome.path, with: "~")
        do {
            let appended = try Self.addToPath(profile: profile, shell: loginShell)
            await installCommandLineTool(force: true)
            return appended
                ? "Added ~/.local/bin to PATH in \(shown). Open a new terminal window for it to take effect."
                : "\(shown) already has the Skillctl PATH lines. Open a new terminal window."
        } catch {
            return "Could not write \(shown): \(error.localizedDescription)"
        }
    }

    /// Make a CLI usable from a terminal as `<directory>/skillctl` by linking
    /// to it. Replaces an existing symlink, never a regular file. Returns the
    /// message to show.
    func linkCommandLineTool(_ binary: URL, into directory: URL) -> String {
        let link = directory.appendingPathComponent("skillctl")
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            if let attributes = try? FileManager.default.attributesOfItem(atPath: link.path) {
                guard attributes[.type] as? FileAttributeType == .typeSymbolicLink else {
                    return "\(link.path) exists and is not a symlink; move it away first."
                }
                try FileManager.default.removeItem(at: link)
            }
            try FileManager.default.createSymbolicLink(at: link, withDestinationURL: binary)
            return "Linked \(link.path) → \(binary.path). Add \(directory.path) to PATH if it is not there yet."
        } catch {
            return "Could not link: \(error.localizedDescription)"
        }
    }
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
        await installCommandLineTool()
        if toolProblem != nil { return }
        async let a: () = loadProjects()
        async let b: () = loadArtifacts()
        async let c: () = loadHooks()
        async let d: () = loadSummary()
        async let e: () = loadDrafts()
        _ = await (a, b, c, d, e)
    }

    func loadDrafts() async {
        drafts = await load("drafts", [Draft].self, ["drafts"]) ?? drafts
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

    /// ccusage's cost and tokens for the summary window; slow, so loaded on demand.
    func loadUsage() async {
        usage = await load("usage", UsageReport.self, ["log", "usage", "--days", String(summaryDays)]) ?? usage
    }

    /// The local collector's state and what it recorded over the summary window.
    func loadTelemetry() async {
        telemetry = await load("telemetry", TelemetryStatus.self, ["telemetry", "status"]) ?? telemetry
        telemetryReport = await load(
            "telemetry-report", TelemetryReport.self, ["telemetry", "report", "--days", String(summaryDays)]) ?? telemetryReport
    }

    /// `telemetry enable` points Claude Code at the collector and runs this
    /// app's bundled CLI as a login service; `disable` undoes both.
    func setTelemetry(enabled: Bool) async {
        await perform(["telemetry", enabled ? "enable" : "disable"]) { await self.loadTelemetry() }
        if doctor != nil { await runDoctor(fix: false) }
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
            lastDoctorRun = Date()
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

    var rules: [RuleEntry] = []

    /// The rule catalog, loaded once.
    func loadRules() async {
        if rules.isEmpty { rules = await load("rules", [RuleEntry].self, ["rules"]) ?? [] }
    }

    /// The instruction files `lint` reads in `folder`, present or absent.
    func lintFiles(folder: URL) async -> [InspectedFile]? {
        await load("lint-files", [InspectedFile].self, ["lint", "--files", folder.path])
    }

    func lint(folder: URL) async -> [Finding]? {
        await load("lint", [Finding].self, ["lint", folder.path], allowExit: 3)
    }

    // MARK: Authoring

    func newDraft(kind: String, name: String) async {
        await perform(["new", "--kind", kind, name]) { await self.loadDrafts() }
    }

    /// Copy a stored version into a draft (`edit`), replacing an existing draft.
    func edit(_ reference: String) async {
        await perform(["edit", reference, "--force"]) { await self.loadDrafts() }
    }

    func seal(_ name: String, bump: String?) async {
        var args = ["seal", name]
        if let bump { args += ["--bump", bump] }
        await perform(args) { await self.loadDrafts(); await self.loadArtifacts() }
    }

    /// Make a skill, command or agent global (user-level links in ~/.claude
    /// and ~/.cursor). The CLI refuses project-pinned ones and other kinds.
    func makeGlobal(_ reference: String) async {
        await perform(["global", "add", reference]) { await self.loadArtifacts() }
    }

    func removeGlobal(_ name: String) async {
        await perform(["global", "remove", name]) { await self.loadArtifacts() }
    }

    func derive(agent: String, model: String) async {
        await perform(["agent", "derive", agent, "--model", model]) { await self.loadDrafts() }
    }

    /// The root document and file list of `name@version` or `name@draft`.
    func show(_ reference: String) async -> ShownDocument? {
        await load("show", ShownDocument.self, ["show", reference])
    }

    func validateDraft(_ name: String) async -> [Finding]? {
        await validate("\(name)@draft")
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

    /// What `gc --keep n` would delete, without deleting anything.
    func previewGarbage(keep: Int) async -> [GcReport.Victim]? {
        guard let cli else { reportMissingBinary(); return nil }
        busy.insert("gc-preview")
        defer { busy.remove("gc-preview") }
        let run = await cli.run(["--dry-run", "--yes", "gc", "--keep", String(keep)])
        console.append(run)
        guard run.succeeded else {
            lastError = CLI.failure(of: run).localizedDescription
            return nil
        }
        return (try? JSONDecoder().decode(GcReport.self, from: Data(run.stdout.utf8)))?.deleted
    }

    /// Delete one store version (`name@version`) or every version of an
    /// artifact (`name`); the CLI refuses pinned ones and names the pins.
    func uninstall(_ target: String) async {
        await perform(["uninstall", target]) { await self.loadArtifacts(); await self.loadDrafts(); await self.loadHooks() }
    }

    /// Delete several store versions; stops at the first refusal and reports it.
    func uninstallMany(_ targets: [String]) async {
        guard let cli else { return reportMissingBinary() }
        busy.insert("uninstall-many")
        defer { busy.remove("uninstall-many") }
        var failures: [String] = []
        for target in targets {
            let run = await cli.run(["--yes", "uninstall", target])
            console.append(run)
            if !run.succeeded { failures.append(CLI.failure(of: run).localizedDescription) }
        }
        await loadArtifacts()
        await loadDrafts()
        await loadHooks()
        if !failures.isEmpty { lastError = failures.joined(separator: "\n") }
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
