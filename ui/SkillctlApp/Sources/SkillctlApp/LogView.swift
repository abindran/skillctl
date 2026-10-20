import Charts
import SwiftUI

/// What the event log and ccusage say about the last N days: activity by
/// day, activations (and how many came through the forced-eval hook), cost
/// and tokens per prompt, alerts, and the per-skill, per-agent and per-server
/// breakdowns. Everything here is `log summary` and `log usage`.
struct LogView: View {
    @Environment(AppState.self) private var state

    private struct Point: Identifiable {
        let day: String
        let series: String
        let value: Int
        var id: String { "\(day)|\(series)" }
    }

    var body: some View {
        @Bindable var state = state
        ScrollView {
            if let summary = state.summary {
                VStack(alignment: .leading, spacing: Metrics.section) {
                    Text("\(summary.current.day.from) → \(summary.current.day.to), compared with \(summary.previous.day.from) → \(summary.previous.day.to)")
                        .foregroundStyle(.secondary)

                    activity(summary)
                    metrics(summary)
                    alerts(summary)
                    usageSection
                    HStack(alignment: .top, spacing: Metrics.gap) {
                        chart("Skill activations", Count.from(summary.current.skills), empty: "No skill activations in this window.")
                        chart("Subagent runs", Count.from(summary.current.agents), empty: "No subagent runs in this window.")
                        chart("MCP tool calls", Count.from(summary.current.mcp_calls), empty: "No MCP tool calls in this window.")
                    }
                    HStack(alignment: .top, spacing: Metrics.gap) {
                        chart("Slash commands", Count.from(summary.current.commands), empty: "No slash commands recorded (skillctl-logger 1.2.2 records them).")
                        Spacer().frame(maxWidth: .infinity)
                        Spacer().frame(maxWidth: .infinity)
                    }
                    Text(summary.note).font(.caption).foregroundStyle(.tertiary)
                }
                .pageLayout()
            } else {
                ContentUnavailableView(
                    "No summary", systemImage: "chart.bar",
                    description: Text("The event log is written by the skillctl-logger hook. Register it on the Hooks screen and use Claude Code or Cursor once."))
                .padding(Metrics.page)
            }
        }
        .navigationTitle("Log")
        .toolbar {
            ToolbarItem {
                Picker("Window", selection: $state.summaryDays) {
                    Text("7 days").tag(7)
                    Text("14 days").tag(14)
                    Text("30 days").tag(30)
                }
                .onChange(of: state.summaryDays) {
                    Task {
                        await state.loadSummary()
                        await state.loadUsage()
                    }
                }
            }
        }
        .task { if state.usage == nil { await state.loadUsage() } }
    }

    // MARK: Activity by day

    private func activity(_ summary: LogSummary) -> some View {
        let points = summary.current.daily.flatMap { day in
            [
                Point(day: String(day.day.suffix(5)), series: "prompts", value: day.prompts),
                Point(day: String(day.day.suffix(5)), series: "skill activations", value: day.activations),
                Point(day: String(day.day.suffix(5)), series: "subagent runs", value: day.subagent_runs),
                Point(day: String(day.day.suffix(5)), series: "compactions", value: day.compactions),
            ]
        }
        return GroupBox {
            if points.allSatisfy({ $0.value == 0 }) {
                Text("No events in this window.").font(.caption).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 80, alignment: .center)
            } else {
                Chart(points) { point in
                    BarMark(x: .value("Day", point.day), y: .value("Count", point.value))
                        .foregroundStyle(by: .value("Series", point.series))
                        .position(by: .value("Series", point.series))
                }
                .chartLegend(position: .top, alignment: .leading)
                .frame(height: 180)
            }
        } label: {
            Label("Activity by day", systemImage: "calendar")
        }
    }

    // MARK: Metrics

    private func metrics(_ summary: LogSummary) -> some View {
        let cur = summary.current
        let prev = summary.previous
        return LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: Metrics.gap)], spacing: Metrics.gap) {
            StatTile(title: "Sessions", value: "\(cur.sessions)", detail: "prev \(prev.sessions)")
            StatTile(title: "Prompts", value: "\(cur.prompts)",
                     detail: "prev \(prev.prompts) · " + cur.by_tool.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value)" }.joined(separator: ", "))
            StatTile(title: "Skill activations", value: "\(cur.activations)",
                     detail: "\(cur.activations_per_100_prompts?.compact ?? "—") per 100 prompts (prev \(prev.activations_per_100_prompts?.compact ?? "—"))")
            StatTile(title: "Activation through the hook",
                     value: cur.hook_shortlists == 0 ? "—" : "\(cur.hook_followed) of \(cur.hook_shortlists)",
                     detail: cur.hook_shortlists == 0
                        ? "no shortlist injected yet (skill-forced-eval 1.0.2 records them)"
                        : "\(cur.hook_follow_ratio?.compact ?? "—")% of forced-eval shortlists were followed by an activation",
                     tint: cur.hook_shortlists == 0 ? .secondary : .accentColor)
            StatTile(title: "Subagent runs", value: "\(cur.subagent_runs)", detail: "prev \(prev.subagent_runs)")
            StatTile(title: "Compactions", value: "\(cur.compactions)",
                     detail: "\(cur.compactions_per_session?.compact ?? "—") per session (prev \(prev.compactions_per_session?.compact ?? "—"))")
            StatTile(title: "Tool calls", value: "\(cur.tool_calls)", detail: "prev \(prev.tool_calls)")
            StatTile(title: "Test commands",
                     value: "raw \(cur.test_commands_raw), wrapped \(cur.test_commands_wrapped)",
                     detail: cur.test_commands_raw > 0 ? "raw runs waste output tokens; register quiet-commands" : "prev raw \(prev.test_commands_raw), wrapped \(prev.test_commands_wrapped)",
                     tint: cur.test_commands_raw > 0 ? .orange : .accentColor)
        }
    }

    // MARK: Alerts

    private func alerts(_ summary: LogSummary) -> some View {
        GroupBox {
            if summary.alerts.isEmpty {
                Label("None", systemImage: "checkmark.circle").foregroundStyle(.green)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(summary.alerts) { alert in
                        HStack(alignment: .top, spacing: 8) {
                            SeverityBadge(text: alert.severity).frame(width: 64, alignment: .leading)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(alert.message)
                                Text("fix: \(alert.fix)").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
        } label: {
            Label("Alerts", systemImage: "bell")
        }
    }

    // MARK: Cost and tokens (ccusage)

    @ViewBuilder
    private var usageSection: some View {
        GroupBox {
            if let usage = state.usage {
                if usage.available {
                    usageBody(usage)
                } else {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(usage.note).font(.callout)
                        Text("ccusage reads Claude Code's local session files, so cost, output tokens and the cache hit ratio per prompt can sit next to the activations above.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            } else if state.busy.contains("usage") {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Reading ccusage… it parses every Claude Code session file, which can take a while.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
            } else {
                Text("Not loaded.").font(.caption).foregroundStyle(.secondary)
            }
        } label: {
            HStack {
                Label("Cost and tokens (Claude Code, via ccusage)", systemImage: "dollarsign.circle")
                Spacer()
                Button("Reload") { Task { await state.loadUsage() } }
                    .controlSize(.small)
                    .disabled(state.busy.contains("usage"))
            }
        }
    }

    private func usageBody(_ usage: UsageReport) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: Metrics.gap)], spacing: Metrics.gap) {
                StatTile(title: "Cost", value: dollars(usage.total_cost), detail: "\(usage.days.count) day(s) with activity")
                StatTile(title: "Cost per prompt", value: usage.cost_per_prompt.map(dollars) ?? "—",
                         detail: "\(usage.prompts) Claude Code prompt(s) in the log")
                StatTile(title: "Output tokens per prompt",
                         value: usage.output_tokens_per_prompt.map { tokens(Int($0)) } ?? "—",
                         detail: "terse-output skills show up here as this falling")
                StatTile(title: "Cache hit ratio",
                         value: usage.cache_hit_ratio.map { "\(Int(($0 * 100).rounded()))%" } ?? "—",
                         detail: "cache reads over everything sent; a drop after an upgrade or a bigger AGENTS.md costs real money",
                         tint: (usage.cache_hit_ratio ?? 1) < 0.8 ? .orange : .green)
            }
            if usage.days.count > 1 {
                HStack(alignment: .top, spacing: Metrics.gap) {
                    GroupBox("Cost per day") {
                        Chart(usage.days) { day in
                            BarMark(x: .value("Day", String(day.date.suffix(5))), y: .value("USD", day.cost))
                        }
                        .frame(height: 140)
                    }
                    GroupBox("Output tokens per prompt") {
                        Chart(usage.days.filter { $0.output_tokens_per_prompt != nil }) { day in
                            LineMark(x: .value("Day", String(day.date.suffix(5))), y: .value("Tokens", day.output_tokens_per_prompt ?? 0))
                            PointMark(x: .value("Day", String(day.date.suffix(5))), y: .value("Tokens", day.output_tokens_per_prompt ?? 0))
                        }
                        .frame(height: 140)
                    }
                }
            }
            let models = modelTotals(usage)
            if !models.isEmpty {
                GroupBox("Cost by model") {
                    Chart(models) { model in
                        BarMark(x: .value("USD", model.cost), y: .value("Model", model.model))
                            .annotation(position: .trailing) { Text(dollars(model.cost)).font(.caption2) }
                    }
                    .chartXAxis(.hidden)
                    .frame(height: CGFloat(models.count) * 26 + 20)
                }
            }
            Table(usage.days.reversed()) {
                TableColumn("Date", value: \.date).width(100)
                TableColumn("Prompts") { Text("\($0.prompts)") }.width(60)
                TableColumn("Output tokens") { Text(tokens($0.output_tokens)) }.width(100)
                TableColumn("Cache hit") { Text($0.cache_hit_ratio.map { "\(Int(($0 * 100).rounded()))%" } ?? "—") }.width(80)
                TableColumn("Cost") { Text(dollars($0.cost)) }.width(80)
                TableColumn("Cost / prompt") { Text($0.cost_per_prompt.map(dollars) ?? "—") }.width(100)
                TableColumn("Models") { Text($0.models.map(\.model).joined(separator: ", ")).foregroundStyle(.secondary).lineLimit(1) }
            }
            .frame(height: CGFloat(usage.days.count) * 26 + 32)
            Text(usage.note).font(.caption2).foregroundStyle(.tertiary)
        }
    }

    private struct ModelTotal: Identifiable {
        let model: String
        let cost: Double
        var id: String { model }
    }

    private func modelTotals(_ usage: UsageReport) -> [ModelTotal] {
        var totals: [String: Double] = [:]
        for day in usage.days {
            for model in day.models {
                totals[model.model, default: 0] += model.cost
            }
        }
        return totals.map { ModelTotal(model: $0.key, cost: $0.value) }.sorted { $0.cost > $1.cost }
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

    @ViewBuilder
    private func chart(_ title: String, _ counts: [Count], empty: String) -> some View {
        GroupBox(title) {
            if counts.isEmpty {
                Text(empty).font(.caption).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 120, alignment: .center)
            } else {
                Chart(counts.prefix(8)) { count in
                    BarMark(x: .value("Count", count.value), y: .value("Name", count.name))
                        .annotation(position: .trailing) { Text("\(count.value)").font(.caption2) }
                }
                .chartXAxis(.hidden)
                .frame(height: CGFloat(min(counts.count, 8)) * 26 + 20)
            }
        }
        .frame(maxWidth: .infinity)
    }
}
