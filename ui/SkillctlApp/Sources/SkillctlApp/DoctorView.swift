import AppKit
import SwiftUI

/// `doctor` as a status card, the problems with their fixes, and the three
/// advisory sections. Every fix can be copied; every path revealed.
struct DoctorView: View {
    @Environment(AppState.self) private var state
    @State private var copied: String?

    private var running: Bool { state.busy.contains("doctor") || state.busy.contains("doctor-fix") }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let report = state.doctor {
                    statusCard(report)
                    if !report.problems.isEmpty {
                        section(
                            title: "Problems", symbol: "exclamationmark.triangle", problems: report.problems,
                            footer: "\(report.problems.count) problem(s), \(report.fixed) fixed, \(report.remaining) remaining. Fix runs doctor --fix: it drops vanished instances from the registry, relinks everything and restores read-only modes.")
                    }
                    ForEach(report.advisory, id: \.title) { advisory in
                        if advisory.title == "Harness versions", let harnesses = report.harnesses {
                            harnessCard(advisory.findings, harnesses)
                        } else {
                            section(title: advisory.title, symbol: advisory.title == "Telemetry" ? "antenna.radiowaves.left.and.right" : "server.rack",
                                    problems: advisory.findings, footer: nil, advisory: true)
                        }
                    }
                } else if running {
                    ProgressView("Running doctor…").frame(maxWidth: .infinity, minHeight: 200)
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
                Button { Task { await state.runDoctor(fix: false) } } label: { Label("Run", systemImage: "play") }
                    .disabled(running)
                Button { Task { await state.runDoctor(fix: true) } } label: { Label("Fix", systemImage: "wrench.and.screwdriver") }
                    .disabled(state.doctor?.remaining ?? 0 == 0 || running)
                    .help("doctor --fix: drop vanished instances, relink, restore read-only modes")
            }
        }
        .task { if state.doctor == nil { await state.runDoctor(fix: false) } }
    }

    // MARK: Status

    private func statusCard(_ report: DoctorReport) -> some View {
        let advisoryWarnings = report.advisory.flatMap(\.findings).filter { $0.severity == "warning" }.count
        let advisoryInfos = report.advisory.flatMap(\.findings).filter { $0.severity == "info" }.count
        return GroupBox {
            HStack(alignment: .center, spacing: 16) {
                Image(systemName: report.remaining == 0 ? "checkmark.seal.fill" : "exclamationmark.triangle.fill")
                    .font(.system(size: 34))
                    .foregroundStyle(report.remaining == 0 ? .green : .red)
                VStack(alignment: .leading, spacing: 4) {
                    Text(report.remaining == 0 ? "Healthy" : "\(report.remaining) problem(s) remaining")
                        .font(.title2.bold())
                    Text(report.remaining == 0
                         ? "Links, pins, store integrity and hook registration all check out."
                         : "Each problem below names the command that fixes it; Fix applies what can be applied automatically.")
                        .font(.callout).foregroundStyle(.secondary)
                    HStack(spacing: 10) {
                        if let lastRun = state.lastDoctorRun {
                            Text("checked \(lastRun.formatted(date: .omitted, time: .shortened))")
                        }
                        Text("·")
                        Text("\(advisoryWarnings) advisory warning(s), \(advisoryInfos) note(s)")
                        if report.dry_run == true { Text("· dry run") }
                    }
                    .font(.caption).foregroundStyle(.tertiary)
                }
                Spacer()
                VStack(spacing: 6) {
                    Button("Run again") { Task { await state.runDoctor(fix: false) } }
                        .disabled(running)
                    if report.remaining > 0 {
                        Button("Fix") { Task { await state.runDoctor(fix: true) } }
                            .buttonStyle(.borderedProminent)
                            .disabled(running)
                    }
                }
                .controlSize(.small)
            }
            .padding(4)
        }
    }

    // MARK: Sections

    private func section(title: String, symbol: String, problems: [Problem], footer: String?, advisory: Bool = false) -> some View {
        GroupBox {
            if problems.isEmpty {
                Label("Nothing to report.", systemImage: "checkmark.circle")
                    .font(.callout).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(problems.enumerated()), id: \.element.id) { index, problem in
                        problemRow(problem)
                        if index < problems.count - 1 { Divider() }
                    }
                    if let footer {
                        Divider()
                        Text(footer).font(.caption).foregroundStyle(.secondary).padding(.top, 6)
                    }
                }
            }
        } label: {
            HStack(spacing: 8) {
                Label(title, systemImage: symbol)
                if advisory {
                    Text("advisory").font(.caption2).foregroundStyle(.tertiary)
                }
                Spacer()
                if !problems.isEmpty {
                    let errors = problems.filter { $0.severity == "error" }.count
                    let warnings = problems.filter { $0.severity == "warning" }.count
                    let infos = problems.count - errors - warnings
                    if errors > 0 { SeverityBadge(text: "\(errors) error") }
                    if warnings > 0 { SeverityBadge(text: "\(warnings) warning") }
                    if infos > 0 { SeverityBadge(text: "\(infos) info") }
                }
            }
        }
    }

    private func problemRow(_ problem: Problem) -> some View {
        HStack(alignment: .top, spacing: 10) {
            SeverityBadge(text: problem.fixed == true ? "fixed" : problem.severity)
                .frame(width: 64, alignment: .leading)
                .padding(.top, 2)
            VStack(alignment: .leading, spacing: 4) {
                Text(problem.message).fixedSize(horizontal: false, vertical: true)
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Image(systemName: "wrench.adjustable").font(.caption).foregroundStyle(.secondary)
                    Text(problem.fix).font(.callout).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
                if let path = problem.path {
                    HStack(spacing: 6) {
                        Image(systemName: "doc.text").font(.caption2)
                        Text(path).font(.system(.caption2, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                        Button("Reveal") { Panels.reveal(path) }.buttonStyle(.link).font(.caption2)
                    }
                    .foregroundStyle(.tertiary)
                }
            }
            Spacer(minLength: 0)
            Button(copied == problem.id ? "Copied" : "Copy fix") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(problem.fix, forType: .string)
                copied = problem.id
            }
            .controlSize(.small)
            .help("Copy the suggested fix to the clipboard")
        }
        .padding(.vertical, 8)
    }

    private func harnessCard(_ findings: [Problem], _ harnesses: Harnesses) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 18) {
                    ForEach(harnesses.detected.keys.sorted(), id: \.self) { key in
                        let name = key == "claude_code" ? "Claude Code" : key.capitalized
                        let version = harnesses.detected[key].flatMap { $0 }
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name).font(.caption).foregroundStyle(.secondary)
                            Text(version ?? "unknown").font(.title3.weight(.semibold))
                                .foregroundStyle(version == nil ? .secondary : .primary)
                            if let history = harnesses.history[key], history.count > 1 {
                                Text("previously " + history.dropLast().map { "\($0.version) (\($0.seen))" }.joined(separator: ", "))
                                    .font(.caption2).foregroundStyle(.tertiary)
                            } else if let first = harnesses.history[key]?.first {
                                Text("since \(first.seen)").font(.caption2).foregroundStyle(.tertiary)
                            }
                        }
                    }
                    Spacer()
                }
                if !findings.isEmpty {
                    Divider()
                    ForEach(findings) { problemRow($0) }
                }
            }
        } label: {
            HStack {
                Label("Harness versions", systemImage: "app.badge")
                Text("advisory").font(.caption2).foregroundStyle(.tertiary)
                Spacer()
                if findings.contains(where: { $0.check == "harness-upgraded" }) { SeverityBadge(text: "upgraded") }
            }
        }
    }
}
