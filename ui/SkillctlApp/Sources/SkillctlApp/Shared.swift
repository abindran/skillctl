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

/// Validation and lint findings as a table with the hint under each row.
struct FindingsTable: View {
    let findings: [Finding]
    var emptyText = "No findings."

    var body: some View {
        if findings.isEmpty {
            ContentUnavailableView(emptyText, systemImage: "checkmark.circle")
        } else {
            Table(findings) {
                TableColumn("Rule") { finding in
                    HStack(spacing: 6) {
                        SeverityBadge(text: finding.severity)
                        Text(finding.rule).font(.system(.body, design: .monospaced))
                    }
                }
                .width(min: 110, ideal: 130)
                TableColumn("Finding") { finding in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(finding.message)
                        if let hint = finding.hint {
                            Text(hint).font(.caption).foregroundStyle(.secondary)
                        }
                        if let path = finding.path {
                            Text(finding.line.map { "\(path):\($0)" } ?? path)
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
        }
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
