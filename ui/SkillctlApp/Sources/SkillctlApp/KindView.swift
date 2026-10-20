import SwiftUI

/// One artifact kind, as a list of artifacts (one row per name, drafts first)
/// and a detail pane for the selected one: its description, its draft, its
/// store versions and, for hooks, its registration. Every action is `new`,
/// `edit`, `seal`, `validate`, `add`, `hook add` or `agent derive` with the
/// kind filled in.
struct KindView: View {
    @Environment(AppState.self) private var state
    let kind: ArtifactKind

    @State private var selectedName: String?
    @State private var search = ""
    @State private var showNew = false
    @State private var newName = ""
    @State private var showInstall = false

    /// One artifact of this kind: its draft (if any) and its store versions.
    struct Entry: Identifiable, Hashable {
        let name: String
        let draft: Draft?
        let versions: [Artifact]
        var id: String { name }
        var latest: Artifact? { versions.last }
        var description: String? { draft?.description ?? latest?.description }
    }

    private var entries: [Entry] {
        var names = Set(state.drafts.filter { $0.kind == kind.id }.map(\.name))
        names.formUnion(state.artifacts.filter { $0.kind == kind.id }.map(\.name))
        return names.sorted().map { name in
            Entry(
                name: name,
                draft: state.drafts.first { $0.name == name && $0.kind == kind.id },
                versions: state.artifacts.filter { $0.name == name && $0.kind == kind.id })
        }
        .filter { search.isEmpty || $0.name.localizedCaseInsensitiveContains(search) }
    }

    private var selected: Entry? {
        entries.first { $0.name == selectedName } ?? entries.first
    }

    var body: some View {
        HStack(spacing: 0) {
            list
                .frame(width: Metrics.listWidth)
            Divider()
            if let entry = selected {
                ArtifactDetail(kind: kind, entry: entry)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ContentUnavailableView {
                    Label("No \(kind.title.lowercased()) yet", systemImage: kind.symbol)
                } description: {
                    Text(kind.summary)
                } actions: {
                    Button("New…") { newName = ""; showNew = true }
                    Button("Install…") { showInstall = true }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle(kind.title)
        .toolbar {
            ToolbarItemGroup {
                Button { newName = ""; showNew = true } label: { Label("New draft…", systemImage: "plus") }
                    .help("skillctl new --kind \(kind.id) <name>")
                Button { showInstall = true } label: { Label("Install…", systemImage: "square.and.arrow.down") }
                    .help("Install from a folder or a Git URL")
            }
        }
        .sheet(isPresented: $showNew) { newSheet }
        .sheet(isPresented: $showInstall) { InstallSheet() }
    }

    private var list: some View {
        VStack(spacing: 0) {
            TextField("Search \(kind.title.lowercased())", text: $search)
                .textFieldStyle(.roundedBorder)
                .padding(Metrics.gap)
            List(selection: $selectedName) {
                let drafts = entries.filter { $0.draft != nil }
                let installed = entries.filter { $0.draft == nil }
                if !drafts.isEmpty {
                    SwiftUI.Section("Drafts") {
                        ForEach(drafts) { row($0) }
                    }
                }
                SwiftUI.Section(installed.isEmpty ? "" : "Installed") {
                    ForEach(installed) { row($0) }
                }
            }
            .listStyle(.inset)
            Divider()
            Text("\(entries.count) \(kind.title.lowercased()), \(entries.flatMap(\.versions).count) version(s)")
                .font(.caption).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Metrics.gap)
                .padding(.vertical, Metrics.header)
        }
    }

    private func row(_ entry: Entry) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(entry.name).fontWeight(.medium)
                if entry.draft != nil { SeverityBadge(text: "draft") }
            }
            Text(subtitle(entry)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
        .padding(.vertical, 4)
        .tag(entry.name)
    }

    private func subtitle(_ entry: Entry) -> String {
        guard let latest = entry.latest else { return "not sealed yet" }
        var parts = ["v\(latest.version)"]
        if entry.versions.count > 1 { parts[0] += " (+\(entry.versions.count - 1) older)" }
        if kind.id == "hook" {
            parts.append(state.registeredHookNames.contains(entry.name) ? "registered" : "not registered")
        } else if latest.pinned_by.isEmpty {
            parts.append("unpinned")
        } else {
            parts.append("pinned by \(latest.pinned_by.joined(separator: ", "))")
        }
        return parts.joined(separator: " · ")
    }

    private var newSheet: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("New \(kind.singular) draft").font(.headline)
            TextField("Name (lowercase letters, digits, hyphens)", text: $newName)
            Text("Runs skillctl new --kind \(kind.id) <name>: a writable folder under ~/.skills/drafts with a \(kind.document) scaffold. Edit it in your editor, validate, then seal.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { showNew = false }
                Button("Create") {
                    let name = newName.trimmingCharacters(in: .whitespaces)
                    Task {
                        await state.newDraft(kind: kind.id, name: name)
                        selectedName = name
                    }
                    showNew = false
                }
                .keyboardShortcut(.defaultAction)
                .disabled(newName.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Metrics.page)
        .frame(width: 460)
    }
}

extension ArtifactKind {
    var singular: String { String(title.lowercased().dropLast()) }
}

/// The right-hand pane of a kind screen.
struct ArtifactDetail: View {
    @Environment(AppState.self) private var state
    let kind: ArtifactKind
    let entry: KindView.Entry

    @State private var selectedVersion: Artifact.ID?
    @State private var sealBump = "patch"
    @State private var showSeal = false
    @State private var deriveModel = ""
    @State private var showDerive = false
    @State private var pinProject: String?
    @State private var showPin = false
    @State private var validating: String?
    @State private var findings: [Finding]?
    @State private var confirmRemoveHook: HookEntry?
    @State private var confirmDelete: String?
    /// Which document the lower pane shows: a version reference or `<name>@draft`.
    @State private var shownReference: String?
    @State private var shown: ShownDocument?
    @State private var loadingDocument = false

    private var isHook: Bool { kind.id == "hook" }
    private var pinnedVersions: [Artifact] { entry.versions.filter { !$0.pinned_by.isEmpty } }
    private var version: Artifact? {
        entry.versions.first { $0.id == selectedVersion } ?? entry.latest
    }
    private var registration: HookEntry? {
        state.hooks.first { $0.name == entry.name }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Metrics.section) {
                header
                if let draft = entry.draft { draftCard(draft) }
                if isHook { registrationCard }
                if !entry.versions.isEmpty { versionsCard }
                documentCard
                Text("Root document \(kind.document). A pin links to \(kind.linksTo).")
                    .font(.caption).foregroundStyle(.tertiary)
            }
            .pageLayout()
        }
        .task(id: "\(entry.name)|\(selectedVersion ?? "")|\(entry.draft?.path ?? "")") {
            // The selected version's document, or the draft's when there is no version.
            let reference = version?.reference ?? entry.draft.map { "\($0.name)@draft" }
            await showDocument(reference)
        }
        .sheet(isPresented: $showSeal) { sealSheet }
        .sheet(isPresented: $showDerive) { deriveSheet }
        .sheet(isPresented: $showPin) { pinSheet }
        .sheet(isPresented: Binding(get: { validating != nil }, set: { if !$0 { validating = nil } })) {
            VStack(alignment: .leading, spacing: Metrics.gap) {
                Text("Validation of \(validating ?? "")").font(.headline)
                if let findings {
                    ScrollView {
                        FindingsTable(findings: findings, emptyText: "0 error(s), 0 warning(s)")
                    }
                    .frame(minHeight: 160, maxHeight: 480)
                    .background(RoundedRectangle(cornerRadius: Metrics.radius).fill(Color.secondary.opacity(0.08)))
                } else {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 240)
                }
                HStack { Spacer(); Button("Close") { validating = nil }.keyboardShortcut(.defaultAction) }
            }
            .padding(Metrics.page)
            .frame(width: 720)
        }
        .confirmationDialog(
            "Remove the registration of \(confirmRemoveHook?.name ?? "")?",
            isPresented: Binding(get: { confirmRemoveHook != nil }, set: { if !$0 { confirmRemoveHook = nil } }),
            presenting: confirmRemoveHook
        ) { hook in
            Button("Remove registration", role: .destructive) { Task { await state.removeHook(hook.name) } }
            Button("Cancel", role: .cancel) {}
        } message: { hook in
            Text("Unpins \(hook.name), removes its link and its own settings entries. The stored version is kept; settings.json is backed up first.")
        }
        .confirmationDialog(
            "Delete \(confirmDelete ?? "") from the store?",
            isPresented: Binding(get: { confirmDelete != nil }, set: { if !$0 { confirmDelete = nil } }),
            presenting: confirmDelete
        ) { target in
            Button("Delete", role: .destructive) { Task { await state.uninstall(target) } }
            Button("Cancel", role: .cancel) {}
        } message: { target in
            Text(target.contains("@")
                 ? "Removes this version's folder from ~/.skills/store. Pinned versions are refused by skillctl; reinstall brings it back. Drafts are untouched."
                 : "Removes every stored version of \(target). skillctl refuses while any version is pinned; reinstall brings them back. A draft, if any, is untouched.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: kind.symbol).foregroundStyle(.secondary)
                Text(entry.name).font(.title2.bold())
                SeverityBadge(text: kind.id)
                if entry.draft != nil { SeverityBadge(text: "draft") }
                Spacer()
                if !entry.versions.isEmpty {
                    Menu {
                        Button("Delete all \(entry.versions.count) version(s)…", role: .destructive) { confirmDelete = entry.name }
                            .disabled(!pinnedVersions.isEmpty)
                        if !pinnedVersions.isEmpty {
                            Text("Pinned: \(pinnedVersions.map { "v\($0.version) by \($0.pinned_by.joined(separator: ", "))" }.joined(separator: "; ")). Unpin first.")
                        }
                    } label: {
                        Label("More", systemImage: "ellipsis.circle")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                }
            }
            Text(entry.description ?? "No description in \(kind.document) yet.")
                .font(.callout)
                .foregroundStyle(entry.description == nil ? .tertiary : .secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func draftCard(_ draft: Draft) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text(draft.path).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer()
                }
                HStack {
                    Button("Reveal in Finder") { Panels.reveal(draft.path) }
                    Button("Validate") { validate("\(draft.name)@draft") }
                    if kind.id == "agent" {
                        Button("Derive for Cursor…") { deriveModel = ""; showDerive = true }
                    }
                    Spacer()
                    Button("Seal…") { sealBump = "patch"; showSeal = true }
                        .buttonStyle(.borderedProminent)
                }
                .controlSize(.small)
            }
        } label: {
            Label("Draft (writable)", systemImage: "pencil")
        }
    }

    private var registrationCard: some View {
        GroupBox {
            if let hook = registration {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        SeverityBadge(text: hook.link.status)
                        Text("v\(hook.pin) linked at \(hook.link.path)")
                            .font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    }
                    Text("Events: \(hook.events.joined(separator: ", "))").font(.caption)
                    if !hook.missing.isEmpty {
                        Text("Not registered: \(hook.missing.joined(separator: ", "))").font(.caption).foregroundStyle(.orange)
                    }
                    HStack {
                        if !hook.healthy {
                            Button("Repair") { Task { await state.registerHook("\(hook.name)@\(hook.pin)") } }
                        }
                        Spacer()
                        Button("Remove registration…", role: .destructive) { confirmRemoveHook = hook }
                    }
                    .controlSize(.small)
                }
            } else {
                HStack {
                    Text("Not registered. Registering links the version under ~/.claude/hooks and merges its events into ~/.claude/settings.json, which Cursor reads too.")
                        .font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    if let latest = entry.latest {
                        Button("Register v\(latest.version)") { Task { await state.registerHook(latest.reference) } }
                            .buttonStyle(.borderedProminent).controlSize(.small)
                    }
                }
            }
        } label: {
            Label("Registration", systemImage: "link")
        }
    }

    private var versionsCard: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                Table(entry.versions.reversed(), selection: $selectedVersion) {
                    TableColumn("Version", value: \.version).width(80)
                    TableColumn(isHook ? "Registered" : "Pinned by") { artifact in
                        if isHook {
                            Text(registration?.pin == artifact.version ? "yes" : "—").foregroundStyle(.secondary)
                        } else {
                            Text(artifact.pinned_by.isEmpty ? "—" : artifact.pinned_by.joined(separator: ", "))
                                .foregroundStyle(artifact.pinned_by.isEmpty ? .secondary : .primary)
                        }
                    }
                    TableColumn("Uses") { artifact in
                        Text(artifact.uses.map(String.init) ?? "n/a").foregroundStyle(.secondary)
                    }
                    .width(50)
                    TableColumn("Last used") { artifact in
                        Text(artifact.last_used.map { String($0.prefix(10)) } ?? "—").foregroundStyle(.secondary)
                    }
                    .width(90)
                    TableColumn("Review") { artifact in
                        if artifact.unused { SeverityBadge(text: "unused") }
                    }
                    .width(70)
                }
                .frame(height: CGFloat(entry.versions.count) * 26 + 32)
                if let version {
                    HStack {
                        Text("v\(version.version):").font(.caption).foregroundStyle(.secondary)
                        Button("Validate") { validate(version.reference) }
                        Button("Edit as draft") { Task { await state.edit(version.reference) } }
                            .help("skillctl edit \(version.reference): a writable copy with a patch bump")
                            .disabled(entry.draft != nil)
                        if isHook {
                            if registration?.pin != version.version {
                                Button("Register this version") { Task { await state.registerHook(version.reference) } }
                            }
                        } else {
                            Button("Pin into project…") { pinProject = state.projects.first?.name; showPin = true }
                                .disabled(state.projects.isEmpty)
                        }
                        Spacer()
                        Button("Delete version…", role: .destructive) { confirmDelete = version.reference }
                            .disabled(!version.pinned_by.isEmpty)
                            .help(version.pinned_by.isEmpty
                                  ? "skillctl uninstall \(version.reference)"
                                  : "Pinned by \(version.pinned_by.joined(separator: ", ")); unpin first")
                    }
                    .controlSize(.small)
                }
            }
        } label: {
            Label("Store versions (read-only)", systemImage: "shippingbox")
        }
    }

    private func validate(_ reference: String) {
        findings = nil
        validating = reference
        Task { findings = await state.validate(reference) ?? [] }
    }

    @MainActor
    private func showDocument(_ reference: String?) async {
        shownReference = reference
        guard let reference else { shown = nil; return }
        loadingDocument = true
        let document = await state.show(reference)
        if shownReference == reference { shown = document }
        loadingDocument = false
    }

    private var documentCard: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    if let draft = entry.draft {
                        Picker("", selection: Binding(
                            get: { shownReference ?? "" },
                            set: { reference in Task { await showDocument(reference) } }
                        )) {
                            if let version { Text("v\(version.version)").tag(version.reference) }
                            Text("draft").tag("\(draft.name)@draft")
                        }
                        .pickerStyle(.segmented)
                        .fixedSize()
                    } else if let version {
                        Text("v\(version.version)").font(.callout).foregroundStyle(.secondary)
                    }
                    if let shown {
                        Text(shown.document).font(.system(.callout, design: .monospaced))
                        Text("\(shown.files.count) file(s)").font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if let shown {
                        Button("Open") { NSWorkspace.shared.open(shown.documentURL) }
                            .help("Open \(shown.document) in the default editor" + (shown.version == "draft" ? "" : "; store files are read-only"))
                        Button("Reveal") { Panels.reveal(shown.dir) }
                    }
                }
                .controlSize(.small)
                if loadingDocument && shown == nil {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, minHeight: 80)
                } else if let shown {
                    if shown.files.count > 1 {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 6) {
                                ForEach(shown.files) { file in
                                    Text(file.path)
                                        .font(.system(.caption2, design: .monospaced))
                                        .padding(.horizontal, 6).padding(.vertical, 2)
                                        .background(Color.secondary.opacity(file.path == shown.document ? 0.25 : 0.12), in: Capsule())
                                }
                            }
                        }
                    }
                    ScrollView {
                        Text(shown.content)
                            .font(.system(.body, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(Metrics.gap)
                    }
                    .frame(minHeight: 160, idealHeight: 420, maxHeight: 560)
                    .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
                } else {
                    Text("Select a version above to read its \(kind.document).")
                        .font(.caption).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, minHeight: 60, alignment: .center)
                }
            }
        } label: {
            Label("Document", systemImage: "doc.text")
        }
    }

    private var sealSheet: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("Seal \(entry.name)").font(.headline)
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
                Button("Cancel") { showSeal = false }
                Button("Seal") {
                    Task { await state.seal(entry.name, bump: sealBump == "keep" ? nil : sealBump) }
                    showSeal = false
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(Metrics.page)
        .frame(width: 460)
    }

    private var deriveSheet: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("Cursor override for \(entry.name)").font(.headline)
            TextField("Cursor model id, for example gpt-5 or claude-4.5-sonnet[effort=high]", text: $deriveModel)
            Text("Writes cursor.md into the draft: the same prompt with this model and a read-the-skill line per preloaded skill. Once sealed and pinned it links as .cursor/agents/\(entry.name).md, and rule H222 stops firing.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { showDerive = false }
                Button("Derive") {
                    Task { await state.derive(agent: entry.name, model: deriveModel) }
                    showDerive = false
                }
                .keyboardShortcut(.defaultAction)
                .disabled(deriveModel.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Metrics.page)
        .frame(width: 520)
    }

    private var pinSheet: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("Pin \(version?.reference ?? entry.name)").font(.headline)
            Picker("Project", selection: $pinProject) {
                ForEach(state.projects) { Text($0.name).tag(String?.some($0.name)) }
            }
            Text("Runs skillctl add <name@version> --project <project>: the registry pin changes and every attached folder of the project gets the link.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { showPin = false }
                Button("Pin") {
                    if let pinProject, let version {
                        Task { await state.addPin(version.reference, project: pinProject) }
                    }
                    showPin = false
                }
                .keyboardShortcut(.defaultAction)
                .disabled(pinProject == nil || version == nil)
            }
        }
        .padding(Metrics.page)
        .frame(width: 460)
    }
}
