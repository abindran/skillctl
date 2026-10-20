import SwiftUI

struct ProjectsView: View {
    @Environment(AppState.self) private var state
    @State private var selected: Project.ID?
    @State private var showAttach = false

    private var project: Project? {
        state.projects.first { $0.id == selected } ?? state.projects.first
    }

    var body: some View {
        HSplitView {
            List(state.projects, selection: $selected) { project in
                VStack(alignment: .leading) {
                    Text(project.name).bold()
                    Text("\(project.skills.count) pin(s), \(project.instances.count) folder(s)")
                        .font(.caption).foregroundStyle(.secondary)
                }
                .tag(project.id)
            }
            .frame(minWidth: 200, idealWidth: 240, maxWidth: 280)
            .listStyle(.inset)
            .overlay {
                if state.projects.isEmpty {
                    ContentUnavailableView("No projects", systemImage: "folder",
                                           description: Text("Attach a folder to create one."))
                }
            }
            if let project {
                ProjectDetail(project: project)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Spacer()
            }
        }
        .navigationTitle("Projects")
        .toolbar {
            ToolbarItem {
                Button { showAttach = true } label: { Label("Attach folder…", systemImage: "plus") }
                    .help("Attach a folder to a new or existing project")
            }
        }
        .sheet(isPresented: $showAttach) {
            AttachSheet()
        }
    }
}

struct ProjectDetail: View {
    @Environment(AppState.self) private var state
    let project: Project
    @State private var showAddPin = false
    @State private var statuses: [String: StatusReport] = [:]
    @State private var confirmDetach: Instance?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(project.name).font(.title2.bold())
                    Text("\(project.skills.count) pin(s) linked into \(project.instances.count) folder(s). Pins are per project; every attached folder of the project receives the same links.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                GroupBox("Attached folders") {
                    if project.instances.isEmpty {
                        Text("None. Attach a folder to this project to link its pins.")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    ForEach(project.instances) { instance in
                        HStack(alignment: .firstTextBaseline) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(instance.path).font(.system(.body, design: .monospaced))
                                if let status = statuses[instance.path], let links = status.links {
                                    let bad = links.filter { $0.status != "ok" }
                                    Text(bad.isEmpty
                                         ? "\(links.count) link(s) ok"
                                         : "\(bad.count) of \(links.count) link(s) need attention: \(bad.map { $0.status }.joined(separator: ", "))")
                                        .font(.caption)
                                        .foregroundStyle(bad.isEmpty ? .green : .orange)
                                } else if let attached = instance.attached_at {
                                    Text("attached \(attached.prefix(10))").font(.caption).foregroundStyle(.secondary)
                                }
                            }
                            Spacer()
                            Button("Check") { Task { statuses[instance.path] = await state.status(of: instance) } }
                            Button("Reveal") { Panels.reveal(instance.path) }
                            Button("Detach…", role: .destructive) { confirmDetach = instance }
                        }
                        .padding(.vertical, 2)
                    }
                }

                GroupBox {
                    if project.pins.isEmpty {
                        Text("No pins yet.").foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        Table(project.pins.map { PinRow(skill: $0.skill, version: $0.version, kind: state.kind(of: $0.skill) ?? "?") }) {
                            TableColumn("Artifact", value: \.skill)
                            TableColumn("Version", value: \.version).width(80)
                            TableColumn("Kind", value: \.kind).width(80)
                            TableColumn("") { row in
                                Button("Unpin", role: .destructive) {
                                    Task { await state.removePin(row.skill, project: project.name) }
                                }
                                .buttonStyle(.borderless)
                            }
                            .width(60)
                        }
                        .frame(minHeight: CGFloat(project.pins.count) * 28 + 40)
                    }
                } label: {
                    HStack {
                        Text("Pins")
                        Spacer()
                        Button("Relink") { Task { await state.relink(project: project.name) } }
                        Button("Add pin…") { showAddPin = true }
                    }
                }
            }
            .padding(20)
        }
        .sheet(isPresented: $showAddPin) {
            AddPinSheet(project: project)
        }
        .confirmationDialog(
            "Detach \(confirmDetach?.folderName ?? "")?",
            isPresented: Binding(get: { confirmDetach != nil }, set: { if !$0 { confirmDetach = nil } }),
            presenting: confirmDetach
        ) { instance in
            Button("Detach, keep the project") { Task { await state.detach(instance: instance, deleteProject: false) } }
            Button("Detach and delete the project", role: .destructive) {
                Task { await state.detach(instance: instance, deleteProject: true) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { instance in
            Text("Removes the managed links under \(instance.path). The store is untouched.")
        }
    }
}

private struct PinRow: Identifiable {
    let skill: String
    let version: String
    let kind: String
    var id: String { skill }
}

struct AddPinSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    let project: Project
    @State private var selection: Artifact.ID?
    @State private var filter = ""

    private var candidates: [Artifact] {
        state.artifacts
            .filter { $0.kind != "hook" && project.skills[$0.name] == nil }
            .filter { filter.isEmpty || $0.name.localizedCaseInsensitiveContains(filter) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Pin into \(project.name)").font(.headline)
            TextField("Filter", text: $filter)
            Table(candidates, selection: $selection) {
                TableColumn("Artifact", value: \.name)
                TableColumn("Version", value: \.version).width(80)
                TableColumn("Kind", value: \.kind).width(80)
                TableColumn("Pinned by") { Text($0.pinned_by.joined(separator: ", ")) }
            }
            .frame(minHeight: 260)
            HStack {
                Text("Hooks are registered from the Hooks screen, not pinned.").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Pin") {
                    if let selection {
                        Task { await state.addPin(selection, project: project.name) }
                    }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(selection == nil)
            }
        }
        .padding(20)
        .frame(width: 620)
    }
}

struct AttachSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State private var folder: URL?
    @State private var mode = 0
    @State private var newName = ""
    @State private var existing: String?
    @State private var skills: Set<Artifact.ID> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Attach a folder").font(.headline)
            HStack {
                Text(folder?.path ?? "No folder chosen").lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("Choose…") { folder = Panels.chooseFolder(prompt: "Attach") }
            }
            Picker("Project", selection: $mode) {
                Text("New project").tag(0)
                Text("Existing project").tag(1)
            }
            .pickerStyle(.segmented)
            if mode == 0 {
                TextField("Project name (lowercase, hyphens)", text: $newName)
                Text("Pin initial artifacts (optional):").font(.caption).foregroundStyle(.secondary)
                List(state.artifacts.filter { $0.kind != "hook" }, selection: $skills) { artifact in
                    Text(artifact.reference).tag(artifact.id)
                }
                .frame(minHeight: 160)
            } else {
                Picker("Existing", selection: $existing) {
                    Text("Choose…").tag(String?.none)
                    ForEach(state.projects) { Text($0.name).tag(String?.some($0.name)) }
                }
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Attach") {
                    if let folder {
                        Task {
                            await state.attach(folder: folder,
                                               newProject: mode == 0 ? newName : nil,
                                               existing: mode == 1 ? existing : nil,
                                               skills: mode == 0 ? skills.sorted() : [])
                        }
                    }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(folder == nil || (mode == 0 ? newName.isEmpty : existing == nil))
            }
        }
        .padding(20)
        .frame(width: 520)
    }
}
