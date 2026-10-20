import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var state

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
