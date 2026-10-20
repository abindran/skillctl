import SwiftUI

/// Validation of store versions and lint of a project's instruction files.
struct QualityView: View {
    @Environment(AppState.self) private var state
    @State private var tab = 0
    @State private var reference: Artifact.ID?
    @State private var validateFindings: [Finding]?
    @State private var lintFolder: URL?
    @State private var lintFindings: [Finding]?
    @State private var running = false

    private var instanceFolders: [URL] {
        state.projects.flatMap(\.instances).map(\.url)
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $tab) {
                Text("Validate an artifact").tag(0)
                Text("Lint instruction files").tag(1)
            }
            .pickerStyle(.segmented)
            .padding(12)
            Divider()
            if tab == 0 { validate } else { lint }
        }
        .navigationTitle("Quality")
    }

    private var validate: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Picker("Artifact", selection: $reference) {
                    Text("Choose…").tag(Artifact.ID?.none)
                    ForEach(state.artifacts) { Text("\($0.reference) (\($0.kind))").tag(Artifact.ID?.some($0.id)) }
                }
                .frame(maxWidth: 420)
                Button("Validate") {
                    guard let reference else { return }
                    running = true
                    Task {
                        validateFindings = await state.validate(reference) ?? []
                        running = false
                    }
                }
                .disabled(reference == nil || running)
                if running { ProgressView().controlSize(.small) }
                Spacer()
            }
            .padding(.horizontal, 12)
            Text("Skills get S and O rules (including the directive-description rules O140–O145), hooks K1xx, rules K2xx, commands K3xx, agents H22x, scripts K5xx. Nothing is written.")
                .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 12)
            if let validateFindings {
                FindingsTable(findings: validateFindings, emptyText: "0 error(s), 0 warning(s)")
            } else {
                Spacer()
            }
        }
        .padding(.top, 12)
    }

    private var lint: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Menu(lintFolder?.path ?? "Choose a folder…") {
                    ForEach(instanceFolders, id: \.self) { url in
                        Button(url.path) { lintFolder = url }
                    }
                    Divider()
                    Button("Other folder…") { lintFolder = Panels.chooseFolder(prompt: "Lint") ?? lintFolder }
                }
                .frame(maxWidth: 520)
                Button("Lint") {
                    guard let lintFolder else { return }
                    running = true
                    Task {
                        lintFindings = await state.lint(folder: lintFolder) ?? []
                        running = false
                    }
                }
                .disabled(lintFolder == nil || running)
                if running { ProgressView().controlSize(.small) }
                Spacer()
            }
            .padding(.horizontal, 12)
            Text("Read-only checks I301–I306, I308 and I309 on AGENTS.md, CLAUDE.md, CLAUDE.local.md and .cursor/rules; each finding carries the suggested fix. The files are team-owned and changed through review, never by skillctl.")
                .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 12)
            if let lintFindings {
                FindingsTable(findings: lintFindings, emptyText: "No instruction-file findings.")
            } else {
                Spacer()
            }
        }
        .padding(.top, 12)
    }
}
