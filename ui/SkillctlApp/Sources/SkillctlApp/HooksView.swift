import SwiftUI

/// `hook list` as a table: link state, declared against registered events,
/// repair and removal. Shown at the top of the Hooks library screen.
struct HookRegistrationTable: View {
    @Environment(AppState.self) private var state
    @Binding var confirmRemove: HookEntry?

    var body: some View {
        if state.hooks.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text("No hooks registered.")
                Text("Register a stored version below: it is linked under ~/.claude/hooks and its events merged into ~/.claude/settings.json, which Cursor reads too. Never duplicate hooks into ~/.cursor/hooks.json.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
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
    }
}
