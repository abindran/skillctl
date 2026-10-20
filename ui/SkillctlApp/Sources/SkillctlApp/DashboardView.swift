import SwiftUI

struct DashboardView: View {
    @Environment(AppState.self) private var state
    let select: (Screen) -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Metrics.section) {
                if state.cli == nil || state.toolProblem != nil {
                    GroupBox {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(state.cli == nil ? "The skillctl binary was not found." : "The skillctl binary is older than this app.").bold()
                            Text(state.toolProblem ?? "Open Settings (⌘,) and point the app at it, for example the checkout's target/debug/skillctl or ~/.cargo/bin/skillctl.")
                                .foregroundStyle(.secondary)
                            SettingsLink { Text("Open Settings…") }
                        }
                        
                    }
                }

                Text("Needs attention").font(.title2.bold())
                GroupBox {
                    let items = attention
                    if items.isEmpty {
                        Label("Nothing needs attention.", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(items, id: \.text) { item in
                                HStack {
                                    SeverityBadge(text: item.severity)
                                    Text(item.text)
                                    Spacer()
                                    Button(item.action) { select(item.screen) }
                                }
                            }
                        }
                    }
                }

                Text("Overview").font(.title2.bold())
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: Metrics.gap)], spacing: Metrics.gap) {
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

                Text("Library").font(.title2.bold())
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: Metrics.gap)], spacing: Metrics.gap) {
                    ForEach(ArtifactKind.all) { kind in
                        let names = Set(state.artifacts.filter { $0.kind == kind.id }.map(\.name))
                        let drafts = state.drafts.filter { $0.kind == kind.id }.count
                        Button { select(.kind(kind.id)) } label: {
                            StatTile(title: kind.title, value: "\(names.count)",
                                     detail: drafts > 0 ? "\(drafts) draft(s) in progress" : kind.id == "hook" ? "\(state.hooks.count) registered" : "\(state.artifacts.filter { $0.kind == kind.id && !$0.pinned_by.isEmpty }.count) version(s) pinned",
                                     tint: names.isEmpty && drafts == 0 ? .secondary : .accentColor)
                        }
                    }
                }
                .buttonStyle(.plain)

                if let summary = state.summary {
                    Text("Last \(summary.days) days").font(.title2.bold())
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: Metrics.gap)], spacing: Metrics.gap) {
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
                            
                        }
                    }
                }
            }
            .pageLayout()
        }
        .navigationTitle("Dashboard")
    }

    private struct Attention {
        let severity: String
        let text: String
        let action: String
        let screen: Screen
    }

    private var attention: [Attention] {
        var items: [Attention] = []
        if let remaining = state.doctor?.remaining, remaining > 0 {
            items.append(Attention(severity: "error", text: "Doctor reports \(remaining) problem(s)", action: "Open Doctor", screen: .doctor))
        }
        let unhealthy = state.hooks.filter { !$0.healthy }
        if !unhealthy.isEmpty {
            items.append(Attention(severity: "error", text: "\(unhealthy.count) hook(s) with a broken link or unregistered events", action: "Open Hooks", screen: .kind("hook")))
        }
        let warnings = state.summary?.alerts.filter { $0.severity == "warning" } ?? []
        if !warnings.isEmpty {
            items.append(Attention(severity: "warning", text: warnings.map(\.message).joined(separator: "; "), action: "Open Log", screen: .log))
        }
        let unused = state.artifacts.filter(\.unused).count
        if unused > 0 {
            items.append(Attention(severity: "info", text: "\(unused) store version(s) nothing pins or uses; garbage collection would remove the superseded ones", action: "Review in Store", screen: .store))
        }
        if !state.drafts.isEmpty {
            let kind = state.drafts.first?.kind ?? "skill"
            items.append(Attention(severity: "info", text: "\(state.drafts.count) draft(s) waiting to be sealed: \(state.drafts.map(\.name).joined(separator: ", "))", action: "Open \(ArtifactKind.named(kind).title)", screen: .kind(kind)))
        }
        return items
    }
}
