import Charts
import SwiftUI

struct LogView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        @Bindable var state = state
        ScrollView {
            if let summary = state.summary {
                VStack(alignment: .leading, spacing: 20) {
                    Text("\(summary.current.day.from) → \(summary.current.day.to), compared with \(summary.previous.day.from) → \(summary.previous.day.to)")
                        .foregroundStyle(.secondary)

                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 170), spacing: 12)], spacing: 12) {
                        metric("Events", summary.current.events, summary.previous.events,
                               detail: summary.current.by_tool.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value)" }.joined(separator: ", "))
                        metric("Sessions", summary.current.sessions, summary.previous.sessions)
                        metric("Prompts", summary.current.prompts, summary.previous.prompts)
                        metric("Tool calls", summary.current.tool_calls, summary.previous.tool_calls)
                        StatTile(title: "Skill activations", value: "\(summary.current.activations)",
                                 detail: "\(summary.current.activations_per_100_prompts?.compact ?? "—") per 100 prompts (prev \(summary.previous.activations_per_100_prompts?.compact ?? "—"))")
                        metric("Subagent runs", summary.current.subagent_runs, summary.previous.subagent_runs)
                        StatTile(title: "Compactions", value: "\(summary.current.compactions)",
                                 detail: "\(summary.current.compactions_per_session?.compact ?? "—") per session (prev \(summary.previous.compactions_per_session?.compact ?? "—"))")
                        StatTile(title: "Test commands",
                                 value: "raw \(summary.current.test_commands_raw), wrapped \(summary.current.test_commands_wrapped)",
                                 detail: "prev raw \(summary.previous.test_commands_raw), wrapped \(summary.previous.test_commands_wrapped)",
                                 tint: summary.current.test_commands_raw > 0 ? .orange : .accentColor)
                    }

                    GroupBox("Alerts") {
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
                    }

                    HStack(alignment: .top, spacing: 16) {
                        chart("Skill activations", Count.from(summary.current.skills), empty: "No skill activations in this window.")
                        chart("Subagent runs", Count.from(summary.current.agents), empty: "No subagent runs in this window.")
                        chart("MCP tool calls", Count.from(summary.current.mcp_calls), empty: "No MCP tool calls in this window.")
                    }

                    Text(summary.note).font(.caption).foregroundStyle(.secondary)
                }
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                ContentUnavailableView(
                    "No summary", systemImage: "chart.bar",
                    description: Text("The event log is written by the skillctl-logger hook. Register it on the Hooks screen and use Claude Code or Cursor once."))
                .padding(40)
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
                .onChange(of: state.summaryDays) { Task { await state.loadSummary() } }
            }
        }
    }

    private func metric(_ title: String, _ now: Int, _ before: Int, detail: String? = nil) -> some View {
        StatTile(title: title, value: "\(now)", detail: detail ?? "prev \(before)")
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
