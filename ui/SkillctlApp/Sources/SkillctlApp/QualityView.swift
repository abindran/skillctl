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
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Picker("", selection: $tab) {
                    Text("Validate an artifact").tag(0)
                    Text("Lint instruction files").tag(1)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                Spacer()
            }
            .barLayout()
            Divider()
            ScrollView {
                Group {
                    if tab == 0 { validate } else { lint }
                }
                .pageLayout()
            }
        }
        .navigationTitle("Quality")
        .task {
            // Snapshot mode only: show a real validation so the layout can be reviewed.
            guard ContentView.snapshotFolder != nil, ProcessInfo.processInfo.environment["SKILLCTL_APP_SNAPSHOT_VALIDATE"] == "1", validateFindings == nil else { return }
            let candidates = state.artifacts.filter { $0.kind == "skill" && $0.version.hasSuffix(".0") } + state.artifacts
            if let artifact = candidates.first {
                reference = artifact.id
                validateFindings = await state.validate(artifact.id) ?? []
            }
        }
    }

    private var validate: some View {
        VStack(alignment: .leading, spacing: Metrics.section) {
            GroupBox {
                VStack(alignment: .leading, spacing: Metrics.gap) {
                    HStack(spacing: Metrics.gap) {
                        Menu(reference ?? "Choose an artifact…") {
                            ForEach(ArtifactKind.all) { kind in
                                let items = state.artifacts.filter { $0.kind == kind.id }
                                if !items.isEmpty {
                                    SwiftUI.Section(kind.title) {
                                        ForEach(items) { artifact in
                                            Button(artifact.reference) { reference = artifact.id; validateFindings = nil }
                                        }
                                    }
                                }
                            }
                        }
                        .fixedSize()
                        Button("Validate") {
                            guard let reference else { return }
                            running = true
                            Task {
                                validateFindings = await state.validate(reference) ?? []
                                running = false
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(reference == nil || running)
                        if running { ProgressView().controlSize(.small) }
                        Spacer()
                    }
                    Text("Skills get S and O rules (including the directive-description rules O140–O145), hooks K1xx, rules K2xx, commands K3xx, agents H22x, scripts K5xx. Nothing is written.")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } label: {
                Label("Artifact", systemImage: "checkmark.seal")
            }
            results(validateFindings, emptyText: "0 error(s), 0 warning(s)", prompt: "Choose an artifact and press Validate.")
        }
    }

    private var lint: some View {
        VStack(alignment: .leading, spacing: Metrics.section) {
            GroupBox {
                VStack(alignment: .leading, spacing: Metrics.gap) {
                    HStack(spacing: Metrics.gap) {
                        Menu(lintFolder?.path ?? "Choose a folder…") {
                            ForEach(instanceFolders, id: \.self) { url in
                                Button(url.path) { lintFolder = url }
                            }
                            Divider()
                            Button("Other folder…") { lintFolder = Panels.chooseFolder(prompt: "Lint") ?? lintFolder }
                        }
                        .fixedSize()
                        Button("Lint") {
                            guard let lintFolder else { return }
                            running = true
                            Task {
                                lintFindings = await state.lint(folder: lintFolder) ?? []
                                running = false
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(lintFolder == nil || running)
                        if running { ProgressView().controlSize(.small) }
                        Spacer()
                    }
                    Text("Read-only checks I301–I306, I308 and I309 on AGENTS.md, CLAUDE.md, CLAUDE.local.md and .cursor/rules; each finding carries the suggested fix. The files are team-owned and changed through review, never by skillctl.")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } label: {
                Label("Folder", systemImage: "folder")
            }
            results(lintFindings, emptyText: "No instruction-file findings.", prompt: "Choose a folder and press Lint.")
        }
    }

    /// The findings, filling the rest of the screen on the same card surface.
    @ViewBuilder
    private func results(_ findings: [Finding]?, emptyText: String, prompt: String) -> some View {
        VStack(alignment: .leading, spacing: Metrics.header) {
            Label("Findings", systemImage: "list.bullet.rectangle").fontWeight(.semibold)
            Group {
                if let findings {
                    FindingsTable(findings: findings, emptyText: emptyText)
                } else {
                    Text(prompt)
                        .foregroundStyle(.secondary)
                        .padding(Metrics.card)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: Metrics.radius).fill(Color.secondary.opacity(0.08)))
            .overlay(RoundedRectangle(cornerRadius: Metrics.radius).strokeBorder(Color.secondary.opacity(0.12)))
            .clipShape(RoundedRectangle(cornerRadius: Metrics.radius))
        }
    }
}
