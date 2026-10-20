import AppKit
import SwiftUI

/// A coloured pill for `error`, `warning`, `info` and link states.
struct SeverityBadge: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.18), in: Capsule())
            .foregroundStyle(color)
    }

    private var color: Color {
        switch text {
        case "error", "missing", "wrong", "dangling", "conflict": return .red
        case "warning": return .orange
        case "info": return .blue
        case "ok", "fixed": return .green
        default: return .secondary
        }
    }
}

/// Validation and lint findings: a summary line, then the findings grouped
/// by severity, each with its rule, message, suggested fix and location.
struct FindingsTable: View {
    let findings: [Finding]
    var emptyText = "No findings."

    private var errors: [Finding] { findings.filter { $0.severity == "error" } }
    private var warnings: [Finding] { findings.filter { $0.severity == "warning" } }
    private var others: [Finding] { findings.filter { $0.severity != "error" && $0.severity != "warning" } }

    /// The folder every finding shares, so each row shows only what differs.
    private var commonFolder: String? {
        let paths = findings.compactMap(\.path)
        guard let first = paths.first else { return nil }
        let folder = (first as NSString).deletingLastPathComponent
        return paths.allSatisfy { ($0 as NSString).deletingLastPathComponent == folder } ? folder : nil
    }

    var body: some View {
        if findings.isEmpty {
            ContentUnavailableView(emptyText, systemImage: "checkmark.circle")
        } else {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 10) {
                    if !errors.isEmpty { Label("\(errors.count) error(s)", systemImage: "xmark.octagon.fill").foregroundStyle(.red) }
                    if !warnings.isEmpty { Label("\(warnings.count) warning(s)", systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange) }
                    if !others.isEmpty { Label("\(others.count) note(s)", systemImage: "info.circle.fill").foregroundStyle(.blue) }
                    Spacer()
                    if let commonFolder {
                        Text(commonFolder).font(.caption).foregroundStyle(.tertiary)
                            .lineLimit(1).truncationMode(.middle)
                    }
                }
                .font(.callout)
                .padding(.horizontal, 12).padding(.vertical, 8)
                Divider()
                List {
                    group("Errors", errors)
                    group("Warnings", warnings)
                    group("Notes", others)
                }
                .listStyle(.inset)
            }
        }
    }

    @ViewBuilder
    private func group(_ title: String, _ items: [Finding]) -> some View {
        if !items.isEmpty {
            SwiftUI.Section(title) {
                ForEach(items) { finding in row(finding) }
            }
        }
    }

    private func row(_ finding: Finding) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(finding.rule)
                    .font(.system(.caption, design: .monospaced).weight(.semibold))
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.15), in: RoundedRectangle(cornerRadius: 4))
                Text(finding.message)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }
            if let hint = finding.hint, !hint.isEmpty {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Image(systemName: "wrench.adjustable").font(.caption).foregroundStyle(.secondary)
                    Text(hint).font(.callout).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.leading, 2)
            }
            if let path = finding.path {
                HStack(spacing: 6) {
                    Image(systemName: "doc.text").font(.caption2)
                    Text(location(path, finding.line))
                        .font(.system(.caption2, design: .monospaced))
                        .lineLimit(1).truncationMode(.middle)
                    Button("Reveal") { Panels.reveal(path) }
                        .buttonStyle(.link).font(.caption2)
                }
                .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 4)
    }

    private func location(_ path: String, _ line: Int?) -> String {
        let shown = commonFolder == nil ? path : (path as NSString).lastPathComponent
        return line.map { "\(shown):\($0)" } ?? shown
    }
}

/// Doctor-style problems (check, severity, message, fix).
struct ProblemsList: View {
    let problems: [Problem]

    var body: some View {
        ForEach(problems) { problem in
            HStack(alignment: .top, spacing: 8) {
                SeverityBadge(text: problem.fixed == true ? "fixed" : problem.severity)
                    .frame(width: 64, alignment: .leading)
                VStack(alignment: .leading, spacing: 2) {
                    Text(problem.message)
                    Text("fix: \(problem.fix)").font(.caption).foregroundStyle(.secondary)
                    if let path = problem.path {
                        Text(path).font(.caption2).foregroundStyle(.tertiary)
                            .lineLimit(1).truncationMode(.middle)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.vertical, 3)
        }
    }
}

/// A labelled number for the dashboard and the log view.
struct StatTile: View {
    let title: String
    let value: String
    var detail: String? = nil
    var tint: Color = .accentColor

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.title2.weight(.semibold)).foregroundStyle(tint)
            if let detail {
                Text(detail).font(.caption2).foregroundStyle(.tertiary).lineLimit(2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
    }
}

enum Panels {
    /// A folder chooser; nil when cancelled.
    @MainActor
    static func chooseFolder(prompt: String) -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = prompt
        return panel.runModal() == .OK ? panel.url : nil
    }

    /// A file chooser; nil when cancelled.
    @MainActor
    static func chooseFile(prompt: String) -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.treatsFilePackagesAsDirectories = true
        panel.prompt = prompt
        return panel.runModal() == .OK ? panel.url : nil
    }

    static func reveal(_ path: String) {
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }
}

extension Double {
    /// `12`, `8.3`, `0.03`: as skillctl prints it.
    var compact: String {
        self == rounded() ? String(Int(self)) : String(self)
    }
}
