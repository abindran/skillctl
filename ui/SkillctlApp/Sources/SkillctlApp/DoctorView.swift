import SwiftUI

struct DoctorView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let report = state.doctor {
                    GroupBox {
                        if report.problems.isEmpty {
                            Label("No problems found.", systemImage: "checkmark.circle.fill")
                                .foregroundStyle(.green)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        } else {
                            VStack(alignment: .leading, spacing: 4) {
                                ProblemsList(problems: report.problems)
                                Divider()
                                Text("\(report.problems.count) problem(s), \(report.fixed) fixed, \(report.remaining) remaining")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    } label: {
                        Label("Problems", systemImage: "exclamationmark.triangle")
                    }

                    ForEach(report.advisory, id: \.title) { section in
                        GroupBox {
                            if section.findings.isEmpty {
                                Text("Nothing to report.").foregroundStyle(.secondary)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            } else {
                                ProblemsList(problems: section.findings)
                            }
                            if section.title == "Harness versions", let harnesses = report.harnesses {
                                Divider()
                                HStack(spacing: 16) {
                                    ForEach(harnesses.detected.keys.sorted(), id: \.self) { key in
                                        Text("\(key.replacingOccurrences(of: "_", with: " ")): \(harnesses.detected[key].flatMap { $0 } ?? "unknown")")
                                            .font(.caption)
                                    }
                                }
                                .foregroundStyle(.secondary)
                            }
                        } label: {
                            Label("\(section.title) (advisory)", systemImage: "info.circle")
                        }
                    }
                } else {
                    ContentUnavailableView(
                        "Doctor has not run yet", systemImage: "stethoscope",
                        description: Text("Run it to check links, pins, store integrity, hook registration, telemetry, harness versions and MCP usage."))
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .navigationTitle("Doctor")
        .toolbar {
            ToolbarItemGroup {
                Button { Task { await state.runDoctor(fix: false) } } label: {
                    Label("Run", systemImage: "play")
                }
                .disabled(state.busy.contains("doctor") || state.busy.contains("doctor-fix"))
                Button { Task { await state.runDoctor(fix: true) } } label: {
                    Label("Fix", systemImage: "wrench.and.screwdriver")
                }
                .disabled(state.doctor?.remaining ?? 0 == 0 || state.busy.contains("doctor-fix"))
                .help("doctor --fix: drop vanished instances, relink, restore read-only modes")
            }
        }
        .task { if state.doctor == nil { await state.runDoctor(fix: false) } }
    }
}
