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
    /// The root document's description, when it parses.
    let description: String?
    let pinned_by: [String]
    let uses: Int?
    let last_used: String?
    let unused: Bool
    let reason: String?

    var id: String { "\(name)@\(version)" }
    var reference: String { id }
    /// Linked into the user-level folders, so every project sees it.
    var isGlobal: Bool { pinned_by.contains("global") }
    /// The projects that pin this version.
    var projects: [String] { pinned_by.filter { $0 != "global" && $0 != "hook" } }
}

/// Where an artifact is in effect: user-level (global), in specific
/// projects, or nowhere yet.
enum Scope: String {
    case global, project, unpinned

    static func of(_ versions: [Artifact]) -> Scope {
        if versions.contains(where: \.isGlobal) { return .global }
        if versions.contains(where: { !$0.projects.isEmpty }) { return .project }
        return .unpinned
    }

    var help: String {
        switch self {
        case .global: return "Global: linked into ~/.claude and ~/.cursor, so every project sees it"
        case .project: return "Project: linked only into the projects that pin it"
        case .unpinned: return "Not pinned anywhere"
        }
    }
}

/// The root document of a version or draft, with the files beside it (`skillctl show`).
/// `rules`: one entry of the rule catalog.
struct RuleEntry: Decodable, Hashable, Identifiable {
    let id: String
    /// `skill`, `hook`, `rule`, `command`, `agent`, `script` or `instructions`.
    let applies_to: String
    let group: String
    let severity: String
    let summary: String
    let allow_listable: Bool
    /// `validate`, `lint` or `advisory`.
    let reported_by: String
}

/// `lint --files`: one instruction file lint reads, present or not.
struct InspectedFile: Decodable, Hashable, Identifiable {
    let name: String
    let path: String
    let present: Bool
    let lines: Int?
    let link: Bool
    let loads: String
    let rules: [String]
    var id: String { path }
}

struct ShownDocument: Decodable, Hashable {
    struct Entry: Decodable, Hashable, Identifiable {
        let path: String
        let bytes: Int
        var id: String { path }
    }
    let name: String
    /// A semantic version, or `draft`.
    let version: String
    let kind: String
    let dir: String
    /// The root document's file name, for example `SKILL.md`.
    let document: String
    let content: String
    let files: [Entry]

    var documentURL: URL { URL(fileURLWithPath: dir).appendingPathComponent(document) }
}

/// What `gc` would delete (`--dry-run`) or did delete.
struct GcReport: Decodable, Hashable {
    struct Victim: Decodable, Hashable, Identifiable {
        let skill: String
        let version: String
        let path: String
        var id: String { "\(skill)@\(version)" }
    }
    let deleted: [Victim]
}

/// A writable draft under `~/.skills/drafts/<name>` (`skillctl drafts`).
struct Draft: Decodable, Hashable, Identifiable {
    let name: String
    /// Detected from the root document; nil while the folder has none.
    let kind: String?
    let description: String?
    let path: String

    var id: String { name }
    var url: URL { URL(fileURLWithPath: path) }
}

/// What each artifact kind is and where a pin links it; the CLI's `Kind`.
struct ArtifactKind: Identifiable, Hashable {
    let id: String
    let title: String
    let document: String
    let linksTo: String
    let summary: String
    let symbol: String

    static let all: [ArtifactKind] = [
        ArtifactKind(id: "skill", title: "Skills", document: "SKILL.md",
                     linksTo: ".claude/skills/<name>, .agents/skills/<name> (and ~/.claude/skills for global pins)",
                     summary: "Agent Skills: a SKILL.md with an activating description plus optional scripts and references. Validated with the S, O and directive-description rules.",
                     symbol: "sparkles"),
        ArtifactKind(id: "command", title: "Commands", document: "COMMAND.md",
                     linksTo: ".claude/commands/<name>.md and .cursor/commands/<name>.md",
                     summary: "Slash commands: one prompt file both tools load, such as /checkpoint. Rules K301–K305.",
                     symbol: "terminal"),
        ArtifactKind(id: "agent", title: "Agents", document: "AGENT.md",
                     linksTo: ".claude/agents/<name>.md; a cursor.md override links as .cursor/agents/<name>.md",
                     summary: "Subagents with their own model, tools and prompt; schema.json makes the subagent-schema hook check their output. Rules H220–H227; `derive` writes the Cursor override.",
                     symbol: "person.2"),
        ArtifactKind(id: "rule", title: "Rules", document: "RULE.mdc",
                     linksTo: ".cursor/rules/<name>.mdc",
                     summary: "Cursor rules with description, globs and alwaysApply. Rules K201–K207.",
                     symbol: "text.badge.checkmark"),
        ArtifactKind(id: "script", title: "Scripts", document: "SCRIPT.md",
                     linksTo: ".claude/scripts/<name>/ (the whole folder)",
                     summary: "Script bundles such as the quiet test and log wrappers that AGENTS.md points agents at. Rules K501–K507; lint I306 checks they are referenced.",
                     symbol: "scroll"),
        ArtifactKind(id: "hook", title: "Hooks", document: "HOOK.md",
                     linksTo: "~/.claude/hooks/<name> plus entries in ~/.claude/settings.json, which Cursor imports",
                     summary: "Lifecycle hooks (events, matchers, timeout, a script). Registered once for both tools with `hook add`, never pinned into projects. Rules K101–K111.",
                     symbol: "link"),
    ]

    static func named(_ id: String) -> ArtifactKind {
        all.first { $0.id == id } ?? all[0]
    }
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
    /// One configured server per harness scope, with its logged use.
    struct Server: Decodable, Hashable, Identifiable {
        let name: String
        let scope: String
        let transport: String?
        let disabled: Bool
        let uses: Int
        let last_used: String?
        var id: String { "\(name)|\(scope)" }
    }
    /// A server reached only through a gateway such as mcpgw (logger 1.2.4+).
    struct Behind: Decodable, Hashable, Identifiable {
        let name: String
        let gateway: String
        let uses: Int
        let last_used: String
        var id: String { "\(gateway)/\(name)" }
    }
    let servers: [Server]?
    let behind_gateways: [Behind]?
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
    /// Servers reached through a gateway, with the gateway: ["zrepo": "mcpgw"].
    let mcp_via: [String: String]?
    /// Failed tool calls from `PostToolUseFailure` (logger 1.2.5+).
    let tool_failures: Int?
    /// Successes and failures by tool, MCP calls by server.
    let tool_results: [String: ToolOutcome]?
    /// Slash commands run, by name.
    let commands: [String: Int]
    let test_commands_raw: Int
    let test_commands_wrapped: Int
    /// Prompts for which the forced-eval hook injected a shortlist, and how
    /// many of those were followed by an activation of a candidate.
    let hook_shortlists: Int
    let hook_followed: Int
    let hook_follow_ratio: Double?
    /// One entry per day of the window, oldest first.
    let daily: [DayCounts]

    var day: (from: String, to: String) {
        (String(from.prefix(10)), String(to.prefix(10)))
    }
}

struct DayCounts: Decodable, Hashable, Identifiable {
    let day: String
    let sessions: Int
    let prompts: Int
    let activations: Int
    let subagent_runs: Int
    let compactions: Int
    let shortlists: Int
    var id: String { day }
}

/// `log usage`: ccusage's daily cost and tokens joined with the log's prompts.
/// `telemetry status`: whether Claude Code exports to the local collector.
struct TelemetryStatus: Decodable, Hashable {
    struct Settings: Decodable, Hashable {
        let path: String
        let configured: Bool
        let missing: [String]
        let endpoint: String?
    }
    struct Service: Decodable, Hashable {
        let supported: Bool
        let path: String?
        let installed: Bool
        let program: String?
        let program_exists: Bool
        let loaded: Bool
    }
    let endpoint: String
    let port: Int
    let settings: Settings
    let service: Service
    /// `running`, `port-taken` or `stopped`.
    let collector: String
    let data_dir: String
    let records_today: Int
    let last_record: String?

    var running: Bool { collector == "running" }
    var healthy: Bool { running && settings.configured }
}

/// `telemetry report`: cost and tokens from the collector's records.
struct TelemetryReport: Decodable, Hashable {
    struct Bucket: Decodable, Hashable, Identifiable {
        let key: String
        let requests: Int
        let cost: Double
        let input_tokens: Int
        let output_tokens: Int
        let cache_read_tokens: Int
        let cache_creation_tokens: Int
        var id: String { key }
    }
    struct Tally: Decodable, Hashable, Identifiable {
        let key: String
        let count: Int
        let failures: Int
        var id: String { key }
    }
    let from: String
    let to: String
    /// `logs`, `metrics` or `none`.
    let source: String
    let records: Int
    let last_record: String?
    let total: Bucket
    let by_model: [Bucket]
    let by_skill: [Bucket]
    let by_agent: [Bucket]
    let by_source: [Bucket]
    let daily: [Bucket]
    /// One tool's results; MCP calls are keyed `<server>/<tool>`.
    struct ToolStat: Decodable, Hashable, Identifiable {
        let key: String
        let tool: String
        let server: String?
        let count: Int
        let failures: Int
        let failure_rate: Double?
        let avg_ms: Double?
        /// Failures by error type (`ShellError`, `McpToolCallError`, …).
        let errors: [String: Int]
        var id: String { key }
    }
    let tools: [ToolStat]
    /// MCP results per server.
    let mcp_servers: [ToolStat]?
    let commands: [Tally]
    let api_errors: Int
    let active_seconds: [String: Double]
}

struct UsageReport: Decodable, Hashable {
    struct ModelCost: Decodable, Hashable, Identifiable {
        let model: String
        let cost: Double
        let output_tokens: Int
        var id: String { model }
    }
    struct Day: Decodable, Hashable, Identifiable {
        let date: String
        let prompts: Int
        let sessions: Int
        let input_tokens: Int
        let output_tokens: Int
        let cache_creation_tokens: Int
        let cache_read_tokens: Int
        let total_tokens: Int
        let cost: Double
        let cache_hit_ratio: Double?
        let cost_per_prompt: Double?
        let output_tokens_per_prompt: Double?
        let models: [ModelCost]
        var id: String { date }
    }
    let available: Bool
    let note: String
    let from: String
    let to: String
    let days: [Day]
    let total_cost: Double
    let total_output_tokens: Int
    let prompts: Int
    let cache_hit_ratio: Double?
    let cost_per_prompt: Double?
    let output_tokens_per_prompt: Double?
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

    /// `via` names the gateway a server was reached through: "zrepo (via mcpgw)".
    static func from(_ map: [String: Int], via: [String: String] = [:]) -> [Count] {
        map.map { entry -> Count in
            let name = via[entry.key].map { gateway in "\(entry.key) (via \(gateway))" } ?? entry.key
            return Count(name: name, value: entry.value)
        }
            .sorted { $0.value == $1.value ? $0.name < $1.name : $0.value > $1.value }
    }
}

/// One tool's logged results (`log summary`'s `tool_results`).
struct ToolOutcome: Decodable, Hashable {
    let succeeded: Int
    let failed: Int
    let interrupted: Int
}
