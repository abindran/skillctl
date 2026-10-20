import SwiftUI

enum Screen: String, CaseIterable, Identifiable {
    case dashboard = "Dashboard"
    case projects = "Projects"
    case store = "Store"
    case hooks = "Hooks"
    case doctor = "Doctor"
    case log = "Log"
    case quality = "Quality"

    var id: String { rawValue }
    var symbol: String {
        switch self {
        case .dashboard: return "square.grid.2x2"
        case .projects: return "folder"
        case .store: return "shippingbox"
        case .hooks: return "link"
        case .doctor: return "stethoscope"
        case .log: return "chart.bar"
        case .quality: return "checkmark.seal"
        }
    }
}

struct ContentView: View {
    @Environment(AppState.self) private var state
    @State private var section: Screen? = .dashboard

    var body: some View {
        @Bindable var state = state
        NavigationSplitView {
            List(Screen.allCases, selection: $section) { item in
                Label(item.rawValue, systemImage: item.symbol)
                    .badge(badge(for: item))
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 200)
            .safeAreaInset(edge: .bottom) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(state.version ?? "skillctl not found")
                        .font(.caption)
                        .foregroundStyle(state.cli == nil ? Color.red : Color.secondary)
                    if let cli = state.cli {
                        Text(cli.binary.path)
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(8)
            }
        } detail: {
            VSplitView {
                detail
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if state.showConsole {
                    ConsoleView()
                        .frame(minHeight: 120, idealHeight: 180)
                }
            }
        }
        .toolbar {
            ToolbarItemGroup {
                if state.isBusy { ProgressView().controlSize(.small) }
                Button {
                    Task { await state.refreshAll() }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .help("Reload everything from skillctl (⌘R)")
                Toggle(isOn: $state.showConsole) {
                    Label("Console", systemImage: "terminal")
                }
                .help("Show the commands the app ran and their output (⌘`)")
            }
        }
        .alert("skillctl reported a problem", isPresented: Binding(
            get: { state.lastError != nil },
            set: { if !$0 { state.lastError = nil } }
        )) {
            Button("OK", role: .cancel) { state.lastError = nil }
        } message: {
            Text(state.lastError ?? "")
        }
    }

    @ViewBuilder
    private var detail: some View {
        switch section ?? .dashboard {
        case .dashboard: DashboardView(select: { section = $0 })
        case .projects: ProjectsView()
        case .store: StoreView()
        case .hooks: HooksView()
        case .doctor: DoctorView()
        case .log: LogView()
        case .quality: QualityView()
        }
    }

    private func badge(for item: Screen) -> Int {
        switch item {
        case .store: return state.artifacts.filter(\.unused).count
        case .hooks: return state.hooks.filter { !$0.healthy }.count
        case .doctor: return state.doctor?.remaining ?? 0
        case .log: return state.summary?.alerts.filter { $0.severity == "warning" }.count ?? 0
        default: return 0
        }
    }
}

/// The commands the app has run, newest last, with their output.
struct ConsoleView: View {
    @Environment(AppState.self) private var state
    @State private var selected: CommandRun.ID?

    var body: some View {
        HSplitView {
            List(state.console, selection: $selected) { run in
                HStack {
                    Image(systemName: run.succeeded ? "checkmark.circle" : "xmark.octagon")
                        .foregroundStyle(run.succeeded ? .green : .red)
                    Text(run.commandLine)
                        .font(.system(.caption, design: .monospaced))
                        .lineLimit(1)
                    Spacer()
                    Text(String(format: "%.1fs", run.duration))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .tag(run.id)
            }
            .frame(minWidth: 260, idealWidth: 360)
            ScrollView {
                if let run = state.console.first(where: { $0.id == selected }) ?? state.console.last {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(run.commandLine).bold()
                        if let cwd = run.cwd { Text("in \(cwd)").foregroundStyle(.secondary) }
                        Text("exit \(run.exitCode)")
                            .foregroundStyle(run.succeeded ? Color.secondary : Color.red)
                        if !run.stdout.isEmpty { Text(run.stdout) }
                        if !run.stderr.isEmpty { Text(run.stderr).foregroundStyle(.orange) }
                    }
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                } else {
                    Text("No commands run yet.")
                        .foregroundStyle(.secondary)
                        .padding()
                }
            }
        }
    }
}
