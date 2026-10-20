import Foundation

/// The JSON error object skillctl prints with `--json` when a command fails.
struct CLIError: Decodable, Error, LocalizedError {
    let error: String
    let message: String
    let hint: String?
    let exit_code: Int?

    var errorDescription: String? { message }
    var recoverySuggestion: String? { hint }
}

enum CLIFailure: Error, LocalizedError {
    case binaryNotFound(searched: [String])
    case failed(args: [String], code: Int32, stdout: String, stderr: String)
    case decoding(args: [String], detail: String)

    var errorDescription: String? {
        switch self {
        case .binaryNotFound(let searched):
            return "The skillctl binary was not found. Searched: \(searched.joined(separator: ", ")). Set its path in Settings."
        case .failed(let args, let code, let stdout, let stderr):
            let text = stderr.isEmpty ? stdout : stderr
            return "skillctl \(args.joined(separator: " ")) exited with \(code): \(text.trimmingCharacters(in: .whitespacesAndNewlines))"
        case .decoding(let args, let detail):
            return "Could not read the output of skillctl \(args.joined(separator: " ")): \(detail)"
        }
    }
}

/// One finished invocation, kept for the console pane.
struct CommandRun: Identifiable {
    let id = UUID()
    let started: Date
    let args: [String]
    let cwd: String?
    let exitCode: Int32
    let stdout: String
    let stderr: String
    let duration: TimeInterval

    var succeeded: Bool { exitCode == 0 }
    var commandLine: String { (["skillctl"] + args).joined(separator: " ") }
}

/// Runs the skillctl binary. Every call adds the global `--json` flag and,
/// for commands that would otherwise prompt, `--yes`.
struct CLI {
    let binary: URL
    /// `SKILLCTL_HOME` override; nil keeps the user's default `~/.skills`.
    let home: String?
    /// Extra variables for the child process (tests redirect `HOME`).
    var environment: [String: String] = [:]

    /// Where the binary is looked for, in order: the path from Settings; a
    /// debug build of the skillctl checkout the app itself lives in
    /// (`target/debug/skillctl`, found by walking up from the executable),
    /// because the app is developed against the CLI it ships with and an
    /// installed copy may predate it; then the Cargo and Homebrew locations
    /// and every `PATH` entry.
    static func candidates(preferred: String?) -> [URL] {
        var urls: [URL] = []
        if let preferred, !preferred.isEmpty {
            urls.append(URL(fileURLWithPath: (preferred as NSString).expandingTildeInPath))
        }
        var folder = Bundle.main.executableURL?.deletingLastPathComponent()
        for _ in 0..<8 {
            guard let current = folder else { break }
            urls.append(current.appendingPathComponent("target/debug/skillctl"))
            folder = current.deletingLastPathComponent()
        }
        let homeDir = FileManager.default.homeDirectoryForCurrentUser
        urls.append(homeDir.appendingPathComponent(".cargo/bin/skillctl"))
        urls.append(URL(fileURLWithPath: "/opt/homebrew/bin/skillctl"))
        urls.append(URL(fileURLWithPath: "/usr/local/bin/skillctl"))
        if let path = ProcessInfo.processInfo.environment["PATH"] {
            for entry in path.split(separator: ":") where !entry.isEmpty {
                urls.append(URL(fileURLWithPath: String(entry)).appendingPathComponent("skillctl"))
            }
        }
        return urls
    }

    /// Subcommands the app depends on. An installed skillctl without one of
    /// them is older than the app and is reported instead of used.
    static let requiredCommands = ["artifacts", "doctor", "hook", "lint", "log", "projects", "validate"]

    /// The required subcommands this binary's `--help` does not list; empty
    /// when the binary supports everything the app uses.
    func missingCommands() async -> [String] {
        let run = await run(["--help"], json: false)
        guard run.succeeded else { return Self.requiredCommands }
        let listed = Set(
            run.stdout.split(separator: "\n")
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .compactMap { $0.split(separator: " ", maxSplits: 1).first.map(String.init) }
        )
        return Self.requiredCommands.filter { !listed.contains($0) }
    }

    static func locate(preferred: String?) -> URL? {
        candidates(preferred: preferred).first { url in
            FileManager.default.isExecutableFile(atPath: url.path)
        }
    }

    /// Run `skillctl --json <args>` and return the finished run. Never throws
    /// for a non-zero exit; callers decide what a failure means.
    func run(_ args: [String], cwd: URL? = nil, json: Bool = true) async -> CommandRun {
        let fullArgs = (json ? ["--json"] : []) + args
        return await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(returning: self.runBlocking(fullArgs, cwd: cwd))
            }
        }
    }

    /// The synchronous part of `run`, on a background queue: launch, drain both
    /// pipes concurrently so neither can fill up and stall, wait.
    private func runBlocking(_ fullArgs: [String], cwd: URL?) -> CommandRun {
        let started = Date()
        do {
            let process = Process()
            process.executableURL = binary
            process.arguments = fullArgs
            process.currentDirectoryURL = cwd
            var environment = ProcessInfo.processInfo.environment
            if let home, !home.isEmpty {
                environment["SKILLCTL_HOME"] = (home as NSString).expandingTildeInPath
            }
            environment.merge(self.environment) { _, override in override }
            // Never let a prompt block: stdin is closed, and --yes answers.
            environment["CI"] = environment["CI"] ?? "1"
            process.environment = environment
            let out = Pipe()
            let err = Pipe()
            process.standardOutput = out
            process.standardError = err
            process.standardInput = FileHandle.nullDevice
            try process.run()
            let group = DispatchGroup()
            var stdoutData = Data()
            var stderrData = Data()
            group.enter()
            DispatchQueue.global().async {
                stdoutData = out.fileHandleForReading.readDataToEndOfFile()
                group.leave()
            }
            group.enter()
            DispatchQueue.global().async {
                stderrData = err.fileHandleForReading.readDataToEndOfFile()
                group.leave()
            }
            group.wait()
            process.waitUntilExit()
            return CommandRun(
                started: started, args: fullArgs, cwd: cwd?.path,
                exitCode: process.terminationStatus,
                stdout: String(decoding: stdoutData, as: UTF8.self),
                stderr: String(decoding: stderrData, as: UTF8.self),
                duration: Date().timeIntervalSince(started))
        } catch {
            return CommandRun(
                started: started, args: fullArgs, cwd: cwd?.path, exitCode: -1,
                stdout: "", stderr: error.localizedDescription,
                duration: Date().timeIntervalSince(started))
        }
    }

    /// Run and decode the JSON result, turning skillctl's JSON error object
    /// into a `CLIError` when the command fails.
    func decode<T: Decodable>(_ type: T.Type, _ args: [String], cwd: URL? = nil) async throws -> (T, CommandRun) {
        let run = await run(args, cwd: cwd)
        if !run.succeeded {
            throw Self.failure(of: run)
        }
        do {
            let value = try JSONDecoder().decode(T.self, from: Data(run.stdout.utf8))
            return (value, run)
        } catch {
            throw CLIFailure.decoding(args: args, detail: "\(error)")
        }
    }

    /// Run a command for its side effect; throws on failure.
    @discardableResult
    func perform(_ args: [String], cwd: URL? = nil) async throws -> CommandRun {
        let run = await run(["--yes"] + args, cwd: cwd)
        if !run.succeeded {
            throw Self.failure(of: run)
        }
        return run
    }

    static func failure(of run: CommandRun) -> Error {
        for text in [run.stdout, run.stderr] {
            for line in text.split(separator: "\n") where line.hasPrefix("{\"error\"") {
                if let error = try? JSONDecoder().decode(CLIError.self, from: Data(line.utf8)) {
                    return error
                }
            }
        }
        return CLIFailure.failed(args: run.args, code: run.exitCode, stdout: run.stdout, stderr: run.stderr)
    }

    /// `skillctl --version`, first line.
    func version() async -> String? {
        let run = await run(["--version"], json: false)
        guard run.succeeded else { return nil }
        return run.stdout.split(separator: "\n").first.map(String.init)
    }
}
