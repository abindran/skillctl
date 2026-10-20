import SwiftUI

struct HooksView: View {
    @Environment(AppState.self) private var state
    @State private var showRegister = false
    @State private var confirmRemove: HookEntry?

    private var unregistered: [Artifact] {
        state.storeHooks.filter { !state.registeredHookNames.contains($0.name) }
    }

    var body: some View {
        VStack(spacing: 0) {
            if state.hooks.isEmpty {
                ContentUnavailableView(
                    "No hooks registered", systemImage: "link",
                    description: Text("Register a stored hook to link it under ~/.claude/hooks and merge its events into ~/.claude/settings.json, which Cursor reads too."))
            } else {
                Table(state.hooks) {
                    TableColumn("Hook") { hook in
                        VStack(alignment: .leading, spacing: 2) {
                            Text("\(hook.name)@\(hook.pin)").bold()
                            Text(hook.link.path).font(.caption2).foregroundStyle(.tertiary)
                                .lineLimit(1).truncationMode(.middle)
                        }
                    }
                    TableColumn("Link") { hook in SeverityBadge(text: hook.link.status) }.width(70)
                    TableColumn("Events") { hook in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(hook.events.joined(separator: ", ")).font(.caption)
                            if !hook.missing.isEmpty {
                                Text("not registered: \(hook.missing.joined(separator: ", "))")
                                    .font(.caption).foregroundStyle(.orange)
                            }
                        }
                    }
                    TableColumn("") { hook in
                        HStack {
                            if !hook.healthy {
                                Button("Repair") { Task { await state.registerHook("\(hook.name)@\(hook.pin)") } }
                                    .help("hook add registers only what is missing")
                            }
                            Button("Remove…", role: .destructive) { confirmRemove = hook }
                        }
                        .buttonStyle(.borderless)
                    }
                    .width(150)
                }
            }
            Divider()
            HStack {
                Image(systemName: "info.circle").foregroundStyle(.secondary)
                Text("Hooks registered here run in Claude Code and, through Third-Party Imports, in Cursor. Never duplicate them into ~/.cursor/hooks.json.")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
            }
            .padding(10)
        }
        .navigationTitle("Hooks")
        .toolbar {
            ToolbarItem {
                Button { showRegister = true } label: { Label("Register…", systemImage: "plus") }
                    .disabled(unregistered.isEmpty)
                    .help(unregistered.isEmpty ? "Every stored hook is registered; install one first" : "Register a stored hook")
            }
        }
        .sheet(isPresented: $showRegister) {
            RegisterHookSheet(candidates: unregistered)
        }
        .confirmationDialog(
            "Remove \(confirmRemove?.name ?? "")?",
            isPresented: Binding(get: { confirmRemove != nil }, set: { if !$0 { confirmRemove = nil } }),
            presenting: confirmRemove
        ) { hook in
            Button("Remove registration", role: .destructive) { Task { await state.removeHook(hook.name) } }
            Button("Cancel", role: .cancel) {}
        } message: { hook in
            Text("Unpins \(hook.name), removes its link and its own settings entries (other hooks' entries stay). The stored version is kept; a backup of settings.json is written first.")
        }
    }
}

struct RegisterHookSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    let candidates: [Artifact]
    @State private var selection: Artifact.ID?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Register a hook").font(.headline)
            Table(candidates, selection: $selection) {
                TableColumn("Hook", value: \.name)
                TableColumn("Version", value: \.version).width(80)
            }
            .frame(minHeight: 200)
            Text("hook add links the version under ~/.claude/hooks, merges its events into ~/.claude/settings.json (backed up, idempotent) and pins any companion rule into every project.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Register") {
                    if let selection { Task { await state.registerHook(selection) } }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(selection == nil)
            }
        }
        .padding(20)
        .frame(width: 480)
    }
}
