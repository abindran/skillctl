import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var state
    @State private var linkMessage: String?

    var body: some View {
        @Bindable var state = state
        Form {
            Section("skillctl binary") {
                HStack {
                    TextField("Path (empty = search the usual places)", text: $state.binaryPath)
                    Button("Browse…") {
                        if let url = Panels.chooseFile(prompt: "Use") { state.binaryPath = url.path }
                    }
                }
                LabeledContent("Resolved to") {
                    Text(state.cli?.binary.path ?? "not found")
                        .foregroundStyle(state.cli == nil ? Color.red : Color.secondary)
                        .textSelection(.enabled)
                }
                LabeledContent("Version") {
                    Text(state.version ?? "—").foregroundStyle(.secondary)
                }
                DisclosureGroup("Searched locations") {
                    ForEach(state.searched, id: \.self) { path in
                        Text(path).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            Section("Command line") {
                LabeledContent("Bundled CLI") {
                    Text(state.bundledBinary?.path ?? "not in this build")
                        .foregroundStyle(.secondary).textSelection(.enabled)
                        .lineLimit(1).truncationMode(.middle)
                }
                if state.bundledBinary != nil {
                    LabeledContent("Terminal command") {
                        CommandLineStatusText(status: state.commandLine)
                    }
                    HStack {
                        Button("Link again") {
                            Task {
                                await state.installCommandLineTool(force: true)
                                linkMessage = nil
                            }
                        }
                        if case .linked(_, let onPath) = state.commandLine, onPath != true {
                            Button("Add ~/.local/bin to PATH") {
                                Task { linkMessage = await state.addLocalBinToPath() }
                            }
                        }
                        Text("The app links its own skillctl into ~/.local/bin on every launch, so the terminal and the app always run the same binary.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if let linkMessage {
                        Text(linkMessage).font(.caption).textSelection(.enabled)
                    }
                }
            }
            Section("skillctl home") {
                TextField("SKILLCTL_HOME override (empty = ~/.skills)", text: $state.homeOverride)
                Text("Point this at another home to inspect a different store and registry, for example a test home used by the CLI's own tests.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 560, height: 360)
    }
}

/// One line for the state of the terminal command.
struct CommandLineStatusText: View {
    let status: AppState.CommandLineStatus

    var body: some View {
        switch status {
        case .notBundled:
            Text("not in this build").foregroundStyle(.secondary)
        case .translocated:
            Text("not linked: move the app to Applications and open it again").foregroundStyle(Color.orange)
        case .linked(let link, let onPath):
            switch onPath {
            case .some(true):
                Text("\(link), on PATH").foregroundStyle(Color.secondary).textSelection(.enabled)
            case .some(false):
                Text("\(link), but ~/.local/bin is not on PATH").foregroundStyle(Color.orange).textSelection(.enabled)
            case .none:
                Text("\(link); could not tell whether ~/.local/bin is on PATH").foregroundStyle(Color.secondary).textSelection(.enabled)
            }
        case .failed(let message):
            Text(message).foregroundStyle(Color.red).textSelection(.enabled)
        }
    }
}
