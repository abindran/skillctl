import SwiftUI

struct StoreView: View {
    @Environment(AppState.self) private var state
    @State private var filter = ""
    @State private var kind = "all"
    @State private var unusedOnly = false
    @State private var selection: Set<Artifact.ID> = []
    @State private var showInstall = false
    @State private var showGC = false
    @State private var gcKeep = 1
    @State private var validating: Artifact?
    @State private var findings: [Finding]?

    private var kinds: [String] {
        ["all"] + Array(Set(state.artifacts.map(\.kind))).sorted()
    }

    private var rows: [Artifact] {
        state.artifacts.filter { artifact in
            (kind == "all" || artifact.kind == kind)
                && (!unusedOnly || artifact.unused)
                && (filter.isEmpty || artifact.name.localizedCaseInsensitiveContains(filter))
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("Filter by name", text: $filter).frame(maxWidth: 240)
                Picker("Kind", selection: $kind) {
                    ForEach(kinds, id: \.self) { Text($0.capitalized).tag($0) }
                }
                .frame(maxWidth: 160)
                Toggle("Unused only", isOn: $unusedOnly)
                Spacer()
                Text("\(rows.count) of \(state.artifacts.count)").foregroundStyle(.secondary)
            }
            .padding(12)
            Table(rows, selection: $selection) {
                TableColumn("Name", value: \.name)
                TableColumn("Version", value: \.version).width(80)
                TableColumn("Kind", value: \.kind).width(70)
                TableColumn("Pinned by") { artifact in
                    Text(artifact.pinned_by.isEmpty ? "—" : artifact.pinned_by.joined(separator: ", "))
                        .foregroundStyle(artifact.pinned_by.isEmpty ? .secondary : .primary)
                }
                TableColumn("Uses") { artifact in
                    Text(artifact.uses.map(String.init) ?? "n/a")
                        .foregroundStyle(artifact.uses == nil ? .secondary : .primary)
                }
                .width(50)
                TableColumn("Last used") { artifact in
                    Text(artifact.last_used.map { String($0.prefix(10)) } ?? "—").foregroundStyle(.secondary)
                }
                .width(90)
                TableColumn("Review") { artifact in
                    if artifact.unused {
                        HStack(spacing: 6) {
                            SeverityBadge(text: "unused")
                            Text(artifact.reason ?? "").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                }
            }
            .contextMenu(forSelectionType: Artifact.ID.self) { ids in
                if ids.count == 1, let artifact = state.artifacts.first(where: { $0.id == ids.first }) {
                    Button("Validate \(artifact.reference)") { validate(artifact) }
                    if artifact.kind == "hook", !state.registeredHookNames.contains(artifact.name) {
                        Button("Register hook") { Task { await state.registerHook(artifact.reference) } }
                    }
                }
            } primaryAction: { ids in
                if let artifact = state.artifacts.first(where: { $0.id == ids.first }) { validate(artifact) }
            }
        }
        .navigationTitle("Store")
        .toolbar {
            ToolbarItemGroup {
                Button { showInstall = true } label: { Label("Install…", systemImage: "square.and.arrow.down") }
                    .help("Install artifacts from a folder or a Git URL")
                Button { showGC = true } label: { Label("Collect garbage…", systemImage: "trash") }
                    .help("Delete unpinned versions beyond the newest ones")
                Button {
                    if let artifact = state.artifacts.first(where: { $0.id == selection.first }) { validate(artifact) }
                } label: { Label("Validate", systemImage: "checkmark.seal") }
                .disabled(selection.count != 1)
            }
        }
        .sheet(isPresented: $showInstall) { InstallSheet() }
        .sheet(item: $validating) { artifact in
            VStack(alignment: .leading, spacing: 12) {
                Text("Validation of \(artifact.reference)").font(.headline)
                if let findings {
                    FindingsTable(findings: findings, emptyText: "0 error(s), 0 warning(s)")
                        .frame(minHeight: 240)
                } else {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 240)
                }
                HStack { Spacer(); Button("Close") { validating = nil } .keyboardShortcut(.defaultAction) }
            }
            .padding(20)
            .frame(width: 720)
        }
        .confirmationDialog("Collect garbage?", isPresented: $showGC) {
            Button("Delete unpinned versions, keeping the newest \(gcKeep)", role: .destructive) {
                Task { await state.collectGarbage(keep: gcKeep) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Removes store versions that no project, global or hook pin uses and that are not among each artifact's newest \(gcKeep) version(s). This cannot be undone; reinstall to get a version back.")
        }
    }

    private func validate(_ artifact: Artifact) {
        findings = nil
        validating = artifact
        Task { findings = await state.validate(artifact.reference) ?? [] }
    }
}

struct InstallSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State private var source = ""
    @State private var reference = ""
    @State private var subpath = ""
    @State private var all = true

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Install into the store").font(.headline)
            HStack {
                TextField("Folder path or Git URL (git@…, https://…, file://…)", text: $source)
                Button("Choose folder…") {
                    if let url = Panels.chooseFolder(prompt: "Install") { source = url.path }
                }
            }
            TextField("Git tag or branch (optional)", text: $reference)
            TextField("Only this subfolder of the source (optional)", text: $subpath)
            Toggle("Install every discovered artifact (otherwise only a single one is accepted)", isOn: $all)
            Text("Each artifact is validated with the registry allow-list before it is stored. The same version with different content is refused.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Install") {
                    Task { await state.install(source: source, all: all, reference: reference, subpath: subpath) }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(source.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 560)
    }
}
