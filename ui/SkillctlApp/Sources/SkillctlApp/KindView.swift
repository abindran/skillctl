import SwiftUI

/// One artifact kind end to end: its drafts (author, validate, seal), its
/// store versions (validate, edit, pin or register) and, for hooks, the
/// registration state. Everything here is `new`, `edit`, `seal`, `validate`,
/// `add`, `hook add` and `agent derive` with the kind filled in.
struct KindView: View {
    @Environment(AppState.self) private var state
    let kind: ArtifactKind

    @State private var showNew = false
    @State private var newName = ""
    @State private var showInstall = false
    @State private var sealing: Draft?
    @State private var sealBump = "patch"
    @State private var deriving: Draft?
    @State private var deriveModel = ""
    @State private var pinning: Artifact?
    @State private var pinProject: String?
    @State private var validating: String?
    @State private var findings: [Finding]?
    @State private var confirmRemoveHook: HookEntry?

    private var drafts: [Draft] { state.drafts.filter { $0.kind == kind.id } }
    private var versions: [Artifact] { state.artifacts.filter { $0.kind == kind.id } }
    private var isHook: Bool { kind.id == "hook" }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                GroupBox {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(kind.summary)
                        Text("Root document \(kind.document). A pin links to \(kind.linksTo).")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                if isHook {
                    GroupBox("Registered") {
                        HookRegistrationTable(confirmRemove: $confirmRemoveHook)
                            .frame(minHeight: state.hooks.isEmpty ? 80 : CGFloat(state.hooks.count) * 48 + 40)
                    }
                }

                GroupBox {
                    if drafts.isEmpty {
                        Text("No \(kind.title.lowercased()) drafts. New… scaffolds one under ~/.skills/drafts; Edit copies a stored version there.")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        Table(drafts) {
                            TableColumn("Draft", value: \.name)
                            TableColumn("Folder") { draft in
                                Text(draft.path).font(.caption).foregroundStyle(.secondary)
                                    .lineLimit(1).truncationMode(.middle)
                            }
                            TableColumn("") { draft in
                                HStack {
                                    Button("Reveal") { Panels.reveal(draft.path) }
                                    Button("Validate") { validate("\(draft.name)@draft") }
                                    if kind.id == "agent" {
                                        Button("Derive for Cursor…") { deriveModel = ""; deriving = draft }
                                    }
                                    Button("Seal…") { sealBump = "patch"; sealing = draft }
                                }
                                .buttonStyle(.borderless)
                            }
                            .width(min: 260, ideal: 320)
                        }
                        .frame(minHeight: CGFloat(drafts.count) * 28 + 40)
                    }
                } label: {
                    HStack {
                        Text("Drafts (writable)")
                        Spacer()
                        Button("New…") { newName = ""; showNew = true }
                    }
                }

                GroupBox {
                    if versions.isEmpty {
                        Text("Nothing of this kind in the store yet.")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        Table(versions) {
                            TableColumn("Name", value: \.name)
                            TableColumn("Version", value: \.version).width(70)
                            TableColumn(isHook ? "Registered" : "Pinned by") { artifact in
                                if isHook {
                                    Text(state.registeredHookNames.contains(artifact.name) && artifact.pinned_by.contains("hook") ? "yes" : "no")
                                        .foregroundStyle(.secondary)
                                } else {
                                    Text(artifact.pinned_by.isEmpty ? "—" : artifact.pinned_by.joined(separator: ", "))
                                        .foregroundStyle(artifact.pinned_by.isEmpty ? .secondary : .primary)
                                }
                            }
                            TableColumn("Uses") { artifact in
                                Text(artifact.uses.map(String.init) ?? "n/a").foregroundStyle(.secondary)
                            }
                            .width(50)
                            TableColumn("") { artifact in
                                HStack {
                                    Button("Validate") { validate(artifact.reference) }
                                    Button("Edit") { Task { await state.edit(artifact.reference) } }
                                        .help("Copy this version into a writable draft with a patch bump")
                                    if isHook {
                                        if !state.registeredHookNames.contains(artifact.name) {
                                            Button("Register") { Task { await state.registerHook(artifact.reference) } }
                                        }
                                    } else {
                                        Button("Pin…") { pinProject = state.projects.first?.name; pinning = artifact }
                                            .disabled(state.projects.isEmpty)
                                    }
                                }
                                .buttonStyle(.borderless)
                            }
                            .width(min: 220, ideal: 260)
                        }
                        .frame(minHeight: CGFloat(versions.count) * 28 + 40)
                    }
                } label: {
                    HStack {
                        Text("Store versions (read-only)")
                        Spacer()
                        Button("Install…") { showInstall = true }
                    }
                }
            }
            .padding(20)
        }
        .navigationTitle(kind.title)
        .sheet(isPresented: $showNew) { newSheet }
        .sheet(isPresented: $showInstall) { InstallSheet() }
        .sheet(item: $sealing) { draft in sealSheet(draft) }
        .sheet(item: $deriving) { draft in deriveSheet(draft) }
        .sheet(item: $pinning) { artifact in pinSheet(artifact) }
        .sheet(isPresented: Binding(get: { validating != nil }, set: { if !$0 { validating = nil } })) {
            VStack(alignment: .leading, spacing: 12) {
                Text("Validation of \(validating ?? "")").font(.headline)
                if let findings {
                    FindingsTable(findings: findings, emptyText: "0 error(s), 0 warning(s)").frame(minHeight: 240)
                } else {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 240)
                }
                HStack { Spacer(); Button("Close") { validating = nil }.keyboardShortcut(.defaultAction) }
            }
            .padding(20)
            .frame(width: 720)
        }
        .confirmationDialog(
            "Remove \(confirmRemoveHook?.name ?? "")?",
            isPresented: Binding(get: { confirmRemoveHook != nil }, set: { if !$0 { confirmRemoveHook = nil } }),
            presenting: confirmRemoveHook
        ) { hook in
            Button("Remove registration", role: .destructive) { Task { await state.removeHook(hook.name) } }
            Button("Cancel", role: .cancel) {}
        } message: { hook in
            Text("Unpins \(hook.name), removes its link and its own settings entries. The stored version is kept; settings.json is backed up first.")
        }
    }

    private func validate(_ reference: String) {
        findings = nil
        validating = reference
        Task { findings = await state.validate(reference) ?? [] }
    }

    private var newSheet: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("New \(kind.title.lowercased().dropLast()) draft").font(.headline)
            TextField("Name (lowercase letters, digits, hyphens)", text: $newName)
            Text("Runs skillctl new --kind \(kind.id) <name>: a writable folder under ~/.skills/drafts with a \(kind.document) scaffold. Edit it in your editor, validate, then seal.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { showNew = false }
                Button("Create") {
                    Task { await state.newDraft(kind: kind.id, name: newName) }
                    showNew = false
                }
                .keyboardShortcut(.defaultAction)
                .disabled(newName.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func sealSheet(_ draft: Draft) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Seal \(draft.name)").font(.headline)
            Picker("Version bump", selection: $sealBump) {
                Text("as written in \(kind.document)").tag("keep")
                Text("patch").tag("patch")
                Text("minor").tag("minor")
                Text("major").tag("major")
            }
            Text("Validates the draft (errors refuse), copies it into the read-only store as a new version and removes the draft. Pins are not changed; pin the new version where you want it.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { sealing = nil }
                Button("Seal") {
                    Task { await state.seal(draft.name, bump: sealBump == "keep" ? nil : sealBump) }
                    sealing = nil
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func deriveSheet(_ draft: Draft) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Cursor override for \(draft.name)").font(.headline)
            TextField("Cursor model id, for example gpt-5 or claude-4.5-sonnet[effort=high]", text: $deriveModel)
            Text("Writes cursor.md into the draft: the same prompt with this model and a read-the-skill line per preloaded skill. Once sealed and pinned it links as .cursor/agents/\(draft.name).md, and rule H222 stops firing.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { deriving = nil }
                Button("Derive") {
                    Task { await state.derive(agent: draft.name, model: deriveModel) }
                    deriving = nil
                }
                .keyboardShortcut(.defaultAction)
                .disabled(deriveModel.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 520)
    }

    private func pinSheet(_ artifact: Artifact) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Pin \(artifact.reference)").font(.headline)
            Picker("Project", selection: $pinProject) {
                ForEach(state.projects) { Text($0.name).tag(String?.some($0.name)) }
            }
            Text("Runs skillctl add \(artifact.reference) --project <project>: the registry pin changes and every attached folder of the project gets the link.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { pinning = nil }
                Button("Pin") {
                    if let pinProject { Task { await state.addPin(artifact.reference, project: pinProject) } }
                    pinning = nil
                }
                .keyboardShortcut(.defaultAction)
                .disabled(pinProject == nil)
            }
        }
        .padding(20)
        .frame(width: 460)
    }
}
