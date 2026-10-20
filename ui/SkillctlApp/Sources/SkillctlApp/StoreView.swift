import SwiftUI

/// The whole store at once, for inventory and the periodic bloat review:
/// one expandable row per artifact with its versions underneath, kind chips
/// with counts, an unused filter, an inspector for the selected artifact and
/// a cleanup flow that previews what `gc` would delete before it runs.
struct StoreView: View {
    @Environment(AppState.self) private var state
    @State private var search = ""
    @State private var kind = "all"
    @State private var unusedOnly = false
    @State private var selection: Set<StoreRow.ID> = []
    @State private var showBulkDelete = false
    @State private var expanded: Set<String> = []
    @State private var showInspector = false
    @State private var showInstall = false
    @State private var showCleanup = false

    /// A table row: an artifact (with `children`) or one of its versions.
    struct StoreRow: Identifiable, Hashable {
        let id: String
        let name: String
        let version: String?
        let kind: String
        let pinnedBy: [String]
        let uses: Int?
        let lastUsed: String?
        let unused: Bool
        let reason: String?
        let children: [StoreRow]?

        var isArtifact: Bool { version == nil }
    }

    private var filtered: [Artifact] {
        state.artifacts.filter { artifact in
            (kind == "all" || artifact.kind == kind)
                && (!unusedOnly || artifact.unused)
                && (search.isEmpty || artifact.name.localizedCaseInsensitiveContains(search))
        }
    }

    private var rows: [StoreRow] {
        let names = Array(Set(filtered.map(\.name))).sorted()
        return names.map { name in
            let versions = filtered.filter { $0.name == name }
            let children = versions.reversed().map { artifact in
                StoreRow(id: artifact.id, name: artifact.name, version: artifact.version, kind: artifact.kind,
                         pinnedBy: artifact.pinned_by, uses: artifact.uses, lastUsed: artifact.last_used,
                         unused: artifact.unused, reason: artifact.reason, children: nil)
            }
            let pinned = Array(Set(versions.flatMap(\.pinned_by))).sorted()
            let uses = versions.compactMap(\.uses)
            return StoreRow(
                id: "artifact:\(name)", name: name, version: nil, kind: versions.first?.kind ?? "skill",
                pinnedBy: pinned, uses: uses.isEmpty ? nil : uses.max(),
                lastUsed: versions.compactMap(\.last_used).max(),
                unused: versions.allSatisfy(\.unused), reason: nil, children: children)
        }
    }

    private var kindCounts: [(id: String, title: String, count: Int)] {
        let perKind: [(id: String, title: String, count: Int)] = ArtifactKind.all.map { kind in
            (id: kind.id, title: kind.title, count: state.artifacts.filter { $0.kind == kind.id }.count)
        }
        return [(id: "all", title: "All", count: state.artifacts.count)] + perKind.filter { $0.count > 0 }
    }

    private var selectedName: String? {
        guard let first = selection.sorted().first else { return nil }
        if first.hasPrefix("artifact:") { return String(first.dropFirst("artifact:".count)) }
        return state.artifacts.first { $0.id == first }?.name
    }

    /// The versions the current selection stands for: a selected artifact row
    /// means all of its (filtered) versions.
    private var selectedVersions: [Artifact] {
        var out: [Artifact] = []
        for id in selection.sorted() {
            if id.hasPrefix("artifact:") {
                let name = String(id.dropFirst("artifact:".count))
                out += filtered.filter { $0.name == name }
            } else if let artifact = state.artifacts.first(where: { $0.id == id }) {
                out.append(artifact)
            }
        }
        var seen = Set<String>()
        return out.filter { seen.insert($0.id).inserted }
    }

    var body: some View {
        VStack(spacing: 0) {
                filterBar
                Divider()
                Table(rows, children: \.children, selection: $selection) {
                    TableColumn("Artifact") { row in
                        if row.isArtifact {
                            HStack(spacing: 6) {
                                Image(systemName: ArtifactKind.named(row.kind).symbol).foregroundStyle(.secondary)
                                Text(row.name).fontWeight(.medium)
                                if let count = row.children?.count, count > 1 {
                                    Text("\(count) versions").font(.caption).foregroundStyle(.secondary)
                                }
                                if row.pinnedBy.contains("global") {
                                    SeverityBadge(text: "global").help(Scope.global.help)
                                } else if row.kind != "hook", row.pinnedBy.contains(where: { $0 != "hook" }) {
                                    SeverityBadge(text: "project").help(Scope.project.help)
                                }
                            }
                        } else {
                            Text("v\(row.version ?? "")").foregroundStyle(.secondary)
                        }
                    }
                    .width(min: 260, ideal: 340)
                    TableColumn("Kind") { row in
                        if row.isArtifact { SeverityBadge(text: row.kind) }
                    }
                    .width(70)
                    TableColumn("Pinned by") { row in
                        Text(row.pinnedBy.isEmpty ? "—" : row.pinnedBy.joined(separator: ", "))
                            .foregroundStyle(row.pinnedBy.isEmpty ? .secondary : .primary)
                    }
                    .width(min: 110, ideal: 140)
                    TableColumn("Uses") { row in
                        Text(row.uses.map(String.init) ?? "n/a").foregroundStyle(.secondary)
                    }
                    .width(50)
                    TableColumn("Review") { row in
                        if row.unused {
                            SeverityBadge(text: row.isArtifact ? "all unused" : "unused")
                                .help(row.reason ?? "no project, global or hook pin")
                        }
                    }
                    .width(90)
                    TableColumn("Last used") { row in
                        Text(row.lastUsed.map { String($0.prefix(10)) } ?? "—").foregroundStyle(.secondary)
                    }
                    .width(90)
                }
                .onChange(of: selection) { if !selection.isEmpty { showInspector = true } }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                Divider()
                summaryBar
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .inspector(isPresented: $showInspector) {
            inspector
                .inspectorColumnWidth(min: 300, ideal: 380, max: 520)
        }
        .navigationTitle("Store")
        .toolbar {
            ToolbarItemGroup {
                Button { showInstall = true } label: { Label("Install…", systemImage: "square.and.arrow.down") }
                    .help("Install artifacts from a folder or a Git URL")
                Button { showCleanup = true } label: { Label("Clean up…", systemImage: "trash") }
                    .help("Preview and delete unpinned versions beyond the newest ones")
                Button { showBulkDelete = true } label: { Label("Delete selected…", systemImage: "trash.slash") }
                    .disabled(selectedVersions.isEmpty)
                    .help("Delete the selected versions (unpinned ones; pinned ones are skipped). Tip: Unused only, then ⌘A.")
                Toggle(isOn: $showInspector) { Label("Inspector", systemImage: "sidebar.right") }
                    .help("Show the selected artifact's details")
            }
        }
        .sheet(isPresented: $showInstall) { InstallSheet() }
        .sheet(isPresented: $showCleanup) { CleanupSheet() }
        .sheet(isPresented: $showBulkDelete) {
            BulkDeleteSheet(versions: selectedVersions, registered: state.registeredHookNames) { selection = [] }
        }
    }

    private var filterBar: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            HStack(spacing: Metrics.tight) {
                ForEach(kindCounts, id: \.id) { item in
                    Button {
                        kind = item.id
                    } label: {
                        Text("\(item.title) \(item.count)")
                            .font(.callout)
                            .padding(.horizontal, 10).padding(.vertical, 4)
                            .background(kind == item.id ? Color.accentColor.opacity(0.2) : Color.secondary.opacity(0.12), in: Capsule())
                            .foregroundStyle(kind == item.id ? Color.accentColor : Color.primary)
                    }
                    .buttonStyle(.plain)
                }
                Spacer()
            }
            HStack(spacing: Metrics.gap) {
                TextField("Search by name", text: $search)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: Metrics.listWidth)
                Toggle("Unused only", isOn: $unusedOnly)
                Spacer()
            }
        }
        .barLayout()
    }

    private var summaryBar: some View {
        let unused = state.artifacts.filter(\.unused).count
        let names = Set(state.artifacts.map(\.name)).count
        return HStack {
            Text("\(state.artifacts.count) version(s) of \(names) artifact(s)")
            if unused > 0 {
                Text("·")
                Text("\(unused) unused").foregroundStyle(.orange)
            }
            Spacer()
            Text("Unused = no project, global or hook pin, or a pinned skill or agent with no recorded use after a week of logging.")
                .foregroundStyle(.tertiary).lineLimit(1).truncationMode(.tail)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .barLayout(vertical: Metrics.header)
    }

    @ViewBuilder
    private var inspector: some View {
        if let name = selectedName, let kindId = state.artifacts.first(where: { $0.name == name })?.kind {
            ArtifactDetail(
                kind: ArtifactKind.named(kindId),
                entry: KindView.Entry(
                    name: name,
                    draft: state.drafts.first { $0.name == name },
                    versions: state.artifacts.filter { $0.name == name }))
            .id(name)
        } else {
            ContentUnavailableView("Select an artifact", systemImage: "shippingbox",
                                   description: Text("Its description, versions and actions appear here. Expand a row to see every version."))
        }
    }
}

/// Preview what `gc` would delete, then delete it.
struct CleanupSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State private var keep = 1
    @State private var victims: [GcReport.Victim]?

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("Clean up the store").font(.headline)
            Stepper("Keep the newest \(keep) version(s) of every artifact", value: $keep, in: 1...10)
                .onChange(of: keep) { Task { await preview() } }
            Text("Deletes store versions that no project, global or hook pin uses and that are not among each artifact's newest \(keep). Pinned versions are never touched; reinstall brings a deleted version back.")
                .font(.caption).foregroundStyle(.secondary)
            GroupBox("Would delete") {
                if let victims {
                    if victims.isEmpty {
                        Label("Nothing: every unpinned version is among the newest \(keep).", systemImage: "checkmark.circle")
                            .foregroundStyle(.green)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 2) {
                                ForEach(victims) { victim in
                                    Text("\(victim.skill)@\(victim.version)").font(.system(.body, design: .monospaced))
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .frame(minHeight: 120, maxHeight: 260)
                    }
                } else {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 120)
                }
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Delete \(victims?.count ?? 0) version(s)", role: .destructive) {
                    Task { await state.collectGarbage(keep: keep) }
                    dismiss()
                }
                .disabled((victims?.count ?? 0) == 0)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(Metrics.page)
        .frame(width: 560)
        .task { await preview() }
    }

    private func preview() async {
        victims = nil
        victims = await state.previewGarbage(keep: keep) ?? []
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
        VStack(alignment: .leading, spacing: Metrics.gap) {
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
        .padding(Metrics.page)
        .frame(width: 560)
    }
}

/// Delete the selected versions after showing which ones skillctl will accept.
struct BulkDeleteSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    let versions: [Artifact]
    let registered: Set<String>
    let done: () -> Void

    private var deletable: [Artifact] { versions.filter { $0.pinned_by.isEmpty } }
    private var pinned: [Artifact] { versions.filter { !$0.pinned_by.isEmpty } }

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text("Delete \(deletable.count) version(s) from the store").font(.headline)
            if !deletable.isEmpty {
                GroupBox("Will be deleted") {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(deletable) { artifact in
                                HStack {
                                    Text(artifact.reference).font(.system(.body, design: .monospaced))
                                    SeverityBadge(text: artifact.kind)
                                    if artifact.unused { SeverityBadge(text: "unused") }
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(minHeight: 80, maxHeight: 240)
                }
            }
            if !pinned.isEmpty {
                GroupBox("Skipped: pinned or registered") {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(pinned) { artifact in
                            Text("\(artifact.reference) — pinned by \(artifact.pinned_by.joined(separator: ", "))")
                                .font(.caption)
                        }
                        Text("Unpin them on the Projects screen, or remove the hook registration, and delete again.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            Text("Each version is removed with skillctl uninstall <name>@<version>; reinstall brings it back. Drafts are untouched.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Delete \(deletable.count)", role: .destructive) {
                    let targets = deletable.map(\.reference)
                    Task {
                        await state.uninstallMany(targets)
                        done()
                    }
                    dismiss()
                }
                .disabled(deletable.isEmpty)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(Metrics.page)
        .frame(width: 560)
    }
}
