import Foundation

// Codable mirrors of skillctl's `--json` output. Field names follow the CLI
// exactly so a change there fails loudly here.

struct Instance: Decodable, Hashable, Identifiable {
    let path: String
    let attached_at: String?
    let created_dirs: [String]?

    var id: String { path }
    var url: URL { URL(fileURLWithPath: path) }
    var folderName: String { url.lastPathComponent }
}

struct Project: Decodable, Hashable, Identifiable {
    let name: String
    let skills: [String: String]
    let instances: [Instance]

    var id: String { name }
    var pins: [(skill: String, version: String)] {
        skills.sorted { $0.key < $1.key }.map { (skill: $0.key, version: $0.value) }
    }
}

struct Artifact: Decodable, Hashable, Identifiable {
    let name: String
    let version: String
    let kind: String
    let pinned_by: [String]
    let uses: Int?
    let last_used: String?
    let unused: Bool
    let reason: String?

    var id: String { "\(name)@\(version)" }
    var reference: String { id }
}

struct SkillListing: Decodable, Hashable, Identifiable {
    struct Version: Decodable, Hashable {
        let version: String
        let projects: [String]
        let global: Bool
    }
    let name: String
    let kind: String
    let versions: [Version]

    var id: String { name }
}

struct HookEntry: Decodable, Hashable, Identifiable {
    struct Link: Decodable, Hashable {
        let path: String
        let status: String
    }
    let name: String
    let pin: String
    let events: [String]
    let registered: [String]
    let missing: [String]
    let link: Link

    var id: String { name }
    var healthy: Bool { missing.isEmpty && link.status == "ok" }
}

struct Problem: Decodable, Hashable, Identifiable {
    let check: String
    let severity: String
    let message: String
    let fix: String
    let path: String?
    let fixed: Bool?

    var id: String { "\(check)|\(message)|\(path ?? "")" }
}

struct Telemetry: Decodable, Hashable {
    let ccusage_on_path: Bool?
    let findings: [Problem]
}

struct Harnesses: Decodable, Hashable {
    struct Sighting: Decodable, Hashable {
        let version: String
        let seen: String
    }
    let detected: [String: String?]
    let history: [String: [Sighting]]
    let findings: [Problem]
}

struct MCPAudit: Decodable, Hashable {
    let findings: [Problem]
}

struct DoctorReport: Decodable, Hashable {
    let problems: [Problem]
    let fixed: Int
    let remaining: Int
    let telemetry: Telemetry?
    let harnesses: Harnesses?
    let mcp: MCPAudit?
    let dry_run: Bool?

    var advisory: [(title: String, findings: [Problem])] {
        [
            ("Telemetry", telemetry?.findings ?? []),
            ("Harness versions", harnesses?.findings ?? []),
            ("MCP servers", mcp?.findings ?? []),
        ]
    }
}

struct LogWindow: Decodable, Hashable {
    let from: String
    let to: String
    let events: Int
    let by_tool: [String: Int]
    let sessions: Int
    let prompts: Int
    let tool_calls: Int
    let activations: Int
    let activations_per_100_prompts: Double?
    let skills: [String: Int]
    let subagent_runs: Int
    let agents: [String: Int]
    let compactions: Int
    let compactions_per_session: Double?
    let mcp_calls: [String: Int]
    let test_commands_raw: Int
    let test_commands_wrapped: Int

    var day: (from: String, to: String) {
        (String(from.prefix(10)), String(to.prefix(10)))
    }
}

struct LogAlert: Decodable, Hashable, Identifiable {
    let check: String
    let severity: String
    let message: String
    let fix: String

    var id: String { "\(check)|\(message)" }
}

struct LogSummary: Decodable, Hashable {
    let days: Int
    let current: LogWindow
    let previous: LogWindow
    let alerts: [LogAlert]
    let note: String
}

struct Finding: Decodable, Hashable, Identifiable {
    let rule: String
    let severity: String
    let path: String?
    let line: Int?
    let message: String
    let hint: String?

    var id: String { "\(rule)|\(path ?? "")|\(line ?? 0)|\(message)" }
}

struct StatusLink: Decodable, Hashable, Identifiable {
    let path: String
    let pin: String?
    let skill: String?
    let status: String
    let target: String?

    var id: String { path }
}

struct StatusReport: Decodable, Hashable {
    let attached: Bool
    let instance: String?
    let links: [StatusLink]?
}

/// Named-count pairs for charts, sorted by count.
struct Count: Identifiable, Hashable {
    let name: String
    let value: Int
    var id: String { name }

    static func from(_ map: [String: Int]) -> [Count] {
        map.map { Count(name: $0.key, value: $0.value) }
            .sorted { $0.value == $1.value ? $0.name < $1.name : $0.value > $1.value }
    }
}
