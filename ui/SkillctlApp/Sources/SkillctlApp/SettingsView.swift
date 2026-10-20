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
                if let bundled = state.bundledBinary {
                    HStack {
                        Button("Link skillctl into ~/.local/bin") {
                            let bin = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin")
                            linkMessage = state.linkCommandLineTool(bundled, into: bin)
                        }
                        Text("so the same binary the app uses works in a terminal; no separate install needed")
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
