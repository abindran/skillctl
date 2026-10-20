import Charts
import SwiftUI

/// The local OpenTelemetry collector bundled with the app (`skillctl
/// telemetry`): a switch that points Claude Code at it and runs it as a login
/// service, its state, and what it recorded: cost and tokens by skill, agent,
/// model and source, and tool runs. ccusage above has the daily totals; this
/// is the only place cost is broken down per skill and per subagent.
struct TelemetrySection: View {
    @Environment(AppState.self) private var state

    private var switching: Bool {
        state.busy.contains("telemetry enable") || state.busy.contains("telemetry disable")
    }

    var body: some View {
        GroupBox {
            if let status = state.telemetry {
                if status.healthy {
                    recorded(status)
                } else {
                    setup(status)
                }
            } else if state.busy.contains("telemetry") {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, minHeight: 60)
            } else {
                Text("Not loaded.").font(.caption).foregroundStyle(.secondary)
            }
        } label: {
            HStack(spacing: Metrics.header) {
                Label("Cost by skill and agent (Claude Code, via the local collector)", systemImage: "antenna.radiowaves.left.and.right")
                if let status = state.telemetry { SeverityBadge(text: status.collector) }
                Spacer()
                if let status = state.telemetry, status.healthy || status.service.installed {
                    Menu {
                        Button("Reload") { Task { await state.loadTelemetry() } }
                        Button("Reveal recorded data") { Panels.reveal(status.data_dir) }
                        Divider()
                        Button("Turn off collector…", role: .destructive) { Task { await state.setTelemetry(enabled: false) } }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .disabled(switching)
                }
            }
        }
    }

    // MARK: Off or broken

    private func setup(_ status: TelemetryStatus) -> some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("Claude Code can export the cost and tokens of every API request, tagged with the skill and subagent that made it. Turning this on updates your Claude Code settings (with a backup) and runs the collector bundled with this app in the background.")
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
            Text("Prompts, responses, tool inputs, error text and your account identity are never stored.")
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: Metrics.tight) {
                check(status.settings.configured, "Claude Code sends its telemetry here", missing: nil)
                if status.service.supported {
                    check(status.service.loaded && status.service.program_exists, "The collector starts at login",
                          missing: serviceProblem(status.service))
                }
                check(status.running, "The collector is running",
                      missing: status.collector == "port-taken" ? "another program uses this port" : nil)
            }
            HStack {
                Button {
                    Task { await state.setTelemetry(enabled: true) }
                } label: {
                    Label(status.service.installed || status.settings.configured ? "Repair and turn on" : "Turn on collector", systemImage: "power")
                }
                .buttonStyle(.borderedProminent)
                .disabled(switching || state.cli == nil)
                if switching { ProgressView().controlSize(.small) }
                Spacer()
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func serviceProblem(_ service: TelemetryStatus.Service) -> String? {
        if !service.installed { return "not installed" }
        if !service.program_exists { return "its copy of skillctl was moved or deleted" }
        if !service.loaded { return "installed but not loaded" }
        return nil
    }

    private func check(_ ok: Bool, _ text: String, missing: String?) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Metrics.tight) {
            Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle")
                .foregroundStyle(ok ? Color.green : Color.secondary)
            Text(text).font(.callout)
            if !ok, let missing {
                Text(missing).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
        }
    }

    // MARK: Running

    @ViewBuilder
    private func recorded(_ status: TelemetryStatus) -> some View {
        VStack(alignment: .leading, spacing: Metrics.card) {
            if let program = status.service.program, let current = state.cli?.binary.path, program != current {
                HStack(spacing: Metrics.tight) {
                    Image(systemName: "info.circle").foregroundStyle(.secondary)
                    Text("The collector runs a different copy of skillctl than this app.")
                        .font(.caption).foregroundStyle(.secondary)
                        .help(program)
                    Button("Use this app's") { Task { await state.setTelemetry(enabled: true) } }
                        .controlSize(.small)
                        .disabled(switching)
                }
            }
            if let report = state.telemetryReport, report.source != "none" {
                summary(report)
            } else {
                VStack(alignment: .leading, spacing: Metrics.tight) {
                    Text("The collector is running; no API request recorded in this window yet.")
                        .font(.callout)
                    Text("Only Claude Code sessions started after telemetry was turned on export, so restart open sessions. Events arrive every 5 seconds, counters every 60.")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func summary(_ report: TelemetryReport) -> some View {
        VStack(alignment: .leading, spacing: Metrics.card) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: Metrics.gap)], spacing: Metrics.gap) {
                StatTile(title: "Cost", value: dollars(report.total.cost),
                         detail: "\(report.total.requests) API request(s), \(report.from) → \(report.to)")
                StatTile(title: "Inside skills", value: share(report.by_skill.filter { $0.key != "(none)" }, of: report.total),
                         detail: "cost of requests made while a skill was active")
                StatTile(title: "Subagents", value: share(report.by_source.filter { $0.key == "subagent" }, of: report.total),
                         detail: "cost share of subagent requests; CLAUDE_CODE_SUBAGENT_MODEL sets their default model")
                StatTile(title: "Output tokens", value: tokens(report.total.output_tokens),
                         detail: "\(tokens(report.total.cache_read_tokens)) cache-read tokens")
            }
            HStack(alignment: .top, spacing: Metrics.gap) {
                costChart("By skill", report.by_skill)
                costChart("By agent", report.by_agent)
                costChart("By model", report.by_model)
            }
            if !report.tools.isEmpty {
                GroupBox("Tool runs") {
                    Chart(report.tools.prefix(10)) { tool in
                        BarMark(x: .value("Runs", tool.count), y: .value("Tool", tool.key))
                            .annotation(position: .trailing) {
                                Text(tool.failures > 0 ? "\(tool.count) (\(tool.failures) failed)" : "\(tool.count)").font(.caption2)
                            }
                    }
                    .chartXAxis(.hidden)
                    .frame(height: CGFloat(min(report.tools.count, 10)) * 24 + 20)
                }
            }
            Text(footer(report)).font(.caption2).foregroundStyle(.tertiary)
        }
    }

    @ViewBuilder
    private func costChart(_ title: String, _ buckets: [TelemetryReport.Bucket]) -> some View {
        GroupBox(title) {
            Chart(buckets.prefix(8)) { bucket in
                BarMark(x: .value("USD", bucket.cost), y: .value("Name", bucket.key))
                    .annotation(position: .trailing) { Text(dollars(bucket.cost)).font(.caption2) }
            }
            .chartXAxis(.hidden)
            .frame(height: CGFloat(min(max(buckets.count, 1), 8)) * 26 + 20)
        }
        .frame(maxWidth: .infinity)
    }

    private func footer(_ report: TelemetryReport) -> String {
        var parts = [report.source == "logs" ? "From API request events" : "From cost and token counters (the logs exporter is off)"]
        if let last = report.last_record { parts.append("last record \(last)") }
        if report.api_errors > 0 { parts.append("\(report.api_errors) API error(s)") }
        if let active = report.active_seconds["cli"] { parts.append("Claude Code active \(Int(active / 60)) min") }
        return parts.joined(separator: " · ")
    }

    private func share(_ buckets: [TelemetryReport.Bucket], of total: TelemetryReport.Bucket) -> String {
        guard total.cost > 0 else { return "—" }
        let part = buckets.reduce(0) { $0 + $1.cost }
        return "\(Int((part / total.cost * 100).rounded()))%"
    }

    private func dollars(_ value: Double) -> String {
        String(format: "$%.2f", value)
    }

    private func tokens(_ value: Int) -> String {
        switch value {
        case 1_000_000...: return String(format: "%.2fM", Double(value) / 1e6)
        case 1_000...: return String(format: "%.1fK", Double(value) / 1e3)
        default: return "\(value)"
        }
    }
}
