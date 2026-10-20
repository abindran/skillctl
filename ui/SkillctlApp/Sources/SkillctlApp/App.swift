import SwiftUI

@main
struct SkillctlApp: App {
    @State private var state = AppState()

    var body: some Scene {
        WindowGroup("skillctl") {
            ContentView()
                .environment(state)
                .frame(minWidth: 960, minHeight: 600)
                .task { await state.refreshAll() }
        }
        .commands {
            CommandGroup(after: .toolbar) {
                Button("Refresh") { Task { await state.refreshAll() } }
                    .keyboardShortcut("r", modifiers: .command)
                Button(state.showConsole ? "Hide Console" : "Show Console") {
                    state.showConsole.toggle()
                }
                .keyboardShortcut("`", modifiers: .command)
            }
        }
        Settings {
            SettingsView()
                .environment(state)
                .groupBoxStyle(CardGroupBoxStyle())
        }
    }
}
