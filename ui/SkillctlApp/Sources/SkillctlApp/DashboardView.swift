import SwiftUI

struct DashboardView: View {
    @Environment(AppState.self) private var state
    let select: (Screen) -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                if state.cli == nil || state.toolProblem != nil {
                    GroupBox {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(state.cli == nil ? "The skillctl binary was not found." : "The skillctl binary is older than this app.").bold()
                            Text(state.toolProblem ?? "Open Settings (⌘,) and point the app at it, for example the checkout's target/debug/skillctl or ~/.cargo/bin/skillctl.")
                                .foregroundStyle(.secondary)
                            SettingsLink { Text("Open Settings…") }
                        }
                        .padding(4)
                    }
                }

                Text("Overview").font(.title2.bold())
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: 12)], spacing: 12) {
                    Button { select(.projects) } label: {
                        StatTile(title: "Projects", value: "\(state.projects.count)",
                                 detail: "\(state.projects.flatMap(\.instances).count) attached folder(s)")
                    }
                    Button { select(.store) } label: {
                        StatTile(title: "Store versions", value: "\(state.artifacts.count)",
                                 detail: "\(state.artifacts.filter(\.unused).count) unused",
                                 tint: state.artifacts.contains(where: \.unused) ? .orange : .accentColor)
                    }
                    Button { select(.kind("hook")) } label: {
                        StatTile(title: "Hooks registered", value: "\(state.hooks.count)",
                                 detail: state.hooks.allSatisfy(\.healthy) ? "all links and events in place" : "some events missing",
                                 tint: state.hooks.allSatisfy(\.healthy) ? .green : .orange)
                    }
                    Button { select(.doctor) } label: {
                        StatTile(title: "Doctor",
                                 value: state.doctor.map { $0.remaining == 0 ? "OK" : "\($0.remaining)" } ?? "—",
                                 detail: state.doctor == nil ? "not run yet" : "problem(s) remaining",
                                 tint: (state.doctor?.remaining ?? 0) == 0 ? .green : .red)
                    }
                }
                .buttonStyle(.plain)

                if let summary = state.summary {
                    Text("Last \(summary.days) days").font(.title2.bold())
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: 12)], spacing: 12) {
                        StatTile(title: "Prompts", value: "\(summary.current.prompts)",
                                 detail: "prev \(summary.previous.prompts)")
                        StatTile(title: "Skill activations / 100 prompts",
                                 value: summary.current.activations_per_100_prompts?.compact ?? "—",
                                 detail: "prev \(summary.previous.activations_per_100_prompts?.compact ?? "—")")
                        StatTile(title: "Subagent runs", value: "\(summary.current.subagent_runs)",
                                 detail: "prev \(summary.previous.subagent_runs)")
                        StatTile(title: "Compactions / session",
                                 value: summary.current.compactions_per_session?.compact ?? "—",
                                 detail: "prev \(summary.previous.compactions_per_session?.compact ?? "—")")
                    }
                    if !summary.alerts.isEmpty {
                        GroupBox("Alerts") {
                            VStack(alignment: .leading, spacing: 6) {
                                ForEach(summary.alerts) { alert in
                                    HStack(alignment: .top, spacing: 8) {
                                        SeverityBadge(text: alert.severity)
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(alert.message)
                                            Text(alert.fix).font(.caption).foregroundStyle(.secondary)
                                        }
                                    }
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(4)
                        }
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .navigationTitle("Dashboard")
    }
}
