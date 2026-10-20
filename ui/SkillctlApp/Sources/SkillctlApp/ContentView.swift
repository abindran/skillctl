import SwiftUI

/// The sidebar entries. Library screens are one per artifact kind.
enum Screen: Hashable, Identifiable {
    case dashboard
    case kind(String)
    case store
    case projects
    case doctor
    case log
    case quality

    var id: String {
        switch self {
        case .kind(let kind): return "kind:\(kind)"
        default: return title
        }
    }

    var title: String {
        switch self {
        case .dashboard: return "Dashboard"
        case .kind(let kind): return ArtifactKind.named(kind).title
        case .store: return "Store"
        case .projects: return "Projects"
        case .doctor: return "Doctor"
        case .log: return "Log"
        case .quality: return "Quality"
        }
    }

    var symbol: String {
        switch self {
        case .dashboard: return "square.grid.2x2"
        case .kind(let kind): return ArtifactKind.named(kind).symbol
        case .store: return "shippingbox"
        case .projects: return "folder"
        case .doctor: return "stethoscope"
        case .log: return "chart.bar"
        case .quality: return "checkmark.seal"
        }
    }

    static let library: [Screen] = ArtifactKind.all.map { .kind($0.id) } + [.store]
    static let health: [Screen] = [.doctor, .log, .quality]
}

struct ContentView: View {
    @Environment(AppState.self) private var state
    @State private var screen: Screen? = .dashboard

    var body: some View {
        @Bindable var state = state
        NavigationSplitView {
            List(selection: $screen) {
                row(.dashboard)
                SwiftUI.Section("Library") {
                    ForEach(Screen.library) { row($0) }
                }
                SwiftUI.Section("Workspace") {
                    row(.projects)
                }
                SwiftUI.Section("Health") {
                    ForEach(Screen.health) { row($0) }
                }
            }
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(min: 190, ideal: 210)
            .safeAreaInset(edge: .bottom) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(state.toolProblem != nil ? "\(state.version ?? "skillctl") is outdated" : state.version ?? "skillctl not found")
                        .font(.caption)
                        .foregroundStyle(state.cli == nil || state.toolProblem != nil ? Color.red : Color.secondary)
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

    /// One sidebar row. The explicit tag is what makes selection work: the
    /// list's selection is a `Screen`, so each row must be tagged with one.
    private func row(_ item: Screen) -> some View {
        Label(item.title, systemImage: item.symbol)
            .badge(badge(for: item))
            .tag(item)
    }

    @ViewBuilder
    private var detail: some View {
        switch screen ?? .dashboard {
        case .dashboard: DashboardView(select: { screen = $0 })
        case .kind(let kind): KindView(kind: ArtifactKind.named(kind))
        case .store: StoreView()
        case .projects: ProjectsView()
        case .doctor: DoctorView()
        case .log: LogView()
        case .quality: QualityView()
        }
    }

    private func badge(for item: Screen) -> Int {
        switch item {
        case .kind("hook"): return state.hooks.filter { !$0.healthy }.count
        case .kind(let kind): return state.drafts.filter { $0.kind == kind }.count
        case .store: return state.artifacts.filter(\.unused).count
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
