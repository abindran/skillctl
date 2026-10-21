import AppKit
import SwiftUI

/// The app's one spacing scale. Every screen, card, bar and sheet uses these
/// values, so edges line up across screens and nothing is spaced by eye.
enum Metrics {
    /// Outer margin of every screen and sheet.
    static let page: CGFloat = 24
    /// Vertical gap between the sections of a screen.
    static let section: CGFloat = 20
    /// Inner padding of a card.
    static let card: CGFloat = 14
    /// Gap between items inside a card, grid or bar.
    static let gap: CGFloat = 12
    /// Gap between a label and what it labels.
    static let tight: CGFloat = 6
    /// Gap between a card's title row and its body.
    static let header: CGFloat = 8
    /// Width of the list column of a list-plus-detail screen.
    static let listWidth: CGFloat = 280
    /// Corner radius of cards and tiles.
    static let radius: CGFloat = 10
}

/// Every `GroupBox` in the app: a title row aligned with the card's left
/// edge, then the body on a rounded surface with the same inner padding
/// everywhere. Applied once at the root so call sites stay plain GroupBoxes.
struct CardGroupBoxStyle: GroupBoxStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: Metrics.header) {
            configuration.label
                .fontWeight(.semibold)
                .frame(maxWidth: .infinity, alignment: .leading)
            // One stack around the body: a GroupBox body with several children
            // must become one card, not one card per child.
            VStack(alignment: .leading, spacing: Metrics.header) {
                configuration.content
            }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Metrics.card)
                .background(
                    RoundedRectangle(cornerRadius: Metrics.radius)
                        .fill(Color.secondary.opacity(0.08))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: Metrics.radius)
                        .strokeBorder(Color.secondary.opacity(0.12))
                )
        }
    }
}

/// The frame of every sheet: a title, the body, then the buttons right-aligned
/// at the bottom, all inside the page margin.
struct SheetLayout<Content: View, Buttons: View>: View {
    let title: String
    let width: CGFloat
    @ViewBuilder let content: Content
    @ViewBuilder let buttons: Buttons

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            Text(title).font(.title3.bold())
            content
            HStack(spacing: Metrics.gap) {
                Spacer()
                buttons
            }
            .padding(.top, Metrics.tight)
        }
        .padding(Metrics.page)
        .frame(width: width)
    }
}

extension View {
    /// The outer margin of a scrolling screen, pinned to the top-left.
    func pageLayout() -> some View {
        padding(Metrics.page)
            .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    /// A full-width bar (filters, summaries) aligned to the page margin.
    func barLayout(vertical: CGFloat = Metrics.gap) -> some View {
        padding(.horizontal, Metrics.page)
            .padding(.vertical, vertical)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

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
        case "error", "missing", "wrong", "dangling", "conflict", "stopped": return .red
        case "warning", "port-taken": return .orange
        case "info": return .blue
        case "ok", "fixed", "running": return .green
        case "global": return .purple
        case "project": return .teal
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
            Label(emptyText, systemImage: "checkmark.circle.fill")
                .foregroundStyle(.green)
                .padding(Metrics.card)
                .frame(maxWidth: .infinity, alignment: .leading)
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
                .padding(.horizontal, Metrics.card).padding(.vertical, Metrics.header)
                Divider()
                VStack(alignment: .leading, spacing: Metrics.gap) {
                    group("Errors", errors)
                    group("Warnings", warnings)
                    group("Notes", others)
                }
                .padding(Metrics.card)
            }
        }
    }

    @ViewBuilder
    private func group(_ title: String, _ items: [Finding]) -> some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                Text(title.uppercased())
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .padding(.bottom, Metrics.tight)
                ForEach(Array(items.enumerated()), id: \.element.id) { index, finding in
                    if index > 0 { Divider() }
                    row(finding)
                }
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
                    FileLinks(path: path, line: finding.line)
                }
                .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, Metrics.tight)
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
            .padding(.vertical, Metrics.tight)
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
        .padding(Metrics.card)
        .background(RoundedRectangle(cornerRadius: Metrics.radius).fill(Color.secondary.opacity(0.08)))
        .overlay(RoundedRectangle(cornerRadius: Metrics.radius).strokeBorder(Color.secondary.opacity(0.12)))
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

    /// Open a file in its default app (an editor for Markdown and scripts).
    static func open(_ path: String) {
        NSWorkspace.shared.open(URL(fileURLWithPath: path))
    }
}

/// A file's text for display, or why it cannot be shown. Store files are
/// read-only and small, so reading them directly is safe.
enum FileText {
    static let limit = 1_000_000

    static func read(_ path: String) -> Result<String, FileTextError> {
        let url = URL(fileURLWithPath: path)
        guard let data = try? Data(contentsOf: url) else { return .failure(.unreadable) }
        if data.count > limit { return .failure(.tooLarge(data.count)) }
        guard let text = String(data: data, encoding: .utf8) else { return .failure(.binary(data.count)) }
        return .success(text)
    }
}

enum FileTextError: Error {
    case unreadable
    case tooLarge(Int)
    case binary(Int)

    var message: String {
        switch self {
        case .unreadable: return "The file could not be read."
        case .tooLarge(let bytes): return "The file is \(ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)); open it in an editor instead."
        case .binary(let bytes): return "Binary file (\(ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)))."
        }
    }
}

/// Monospaced text with line numbers; `highlight` marks one line and scrolls to it.
struct NumberedText: View {
    let text: String
    var highlight: Int?

    var body: some View {
        let lines = text.components(separatedBy: "\n")
        let width = CGFloat(String(lines.count).count) * 9 + 8
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(lines.enumerated()), id: \.offset) { index, line in
                        HStack(alignment: .top, spacing: Metrics.gap) {
                            Text("\(index + 1)")
                                .foregroundStyle(.tertiary)
                                .frame(width: width, alignment: .trailing)
                            Text(line.isEmpty ? " " : line)
                                .textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .font(.system(.callout, design: .monospaced))
                        .padding(.vertical, 1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(index + 1 == highlight ? Color.yellow.opacity(0.18) : Color.clear)
                        .id(index + 1)
                    }
                }
                .padding(Metrics.gap)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .onAppear { if let highlight { proxy.scrollTo(highlight, anchor: .center) } }
        }
        .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
    }
}

/// A sheet showing one file so it can be checked without leaving the app.
struct FilePreviewSheet: View {
    let path: String
    var line: Int?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gap) {
            HStack(spacing: Metrics.tight) {
                Image(systemName: "doc.text").foregroundStyle(.secondary)
                Text((path as NSString).lastPathComponent).font(.headline)
                if let line { Text("line \(line)").font(.caption).foregroundStyle(.secondary) }
                Spacer()
                Button("Open in Editor") { Panels.open(path) }
                Button("Reveal") { Panels.reveal(path) }
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
            .controlSize(.small)
            Text(path).font(.system(.caption2, design: .monospaced)).foregroundStyle(.tertiary)
                .lineLimit(1).truncationMode(.middle)
            switch FileText.read(path) {
            case .success(let text): NumberedText(text: text, highlight: line)
            case .failure(let error):
                Text(error.message).foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(Metrics.page)
        .frame(minWidth: 720, idealWidth: 860, minHeight: 480, idealHeight: 640)
    }
}

/// Preview, Open and Reveal links for a file, shown next to its name.
struct FileLinks: View {
    let path: String
    var line: Int?
    @State private var previewing = false

    var body: some View {
        HStack(spacing: Metrics.header) {
            Button("Preview") { previewing = true }
                .help("Show the file here")
            Button("Open") { Panels.open(path) }
                .help("Open in the default editor")
            Button("Reveal") { Panels.reveal(path) }
                .help("Show in Finder")
        }
        .buttonStyle(.link)
        .font(.caption)
        .sheet(isPresented: $previewing) { FilePreviewSheet(path: path, line: line) }
    }
}

extension Double {
    /// `12`, `8.3`, `0.03`: as skillctl prints it.
    var compact: String {
        self == rounded() ? String(Int(self)) : String(self)
    }
}

/// One row of a tool results table.
struct ToolResultRow: Identifiable, Hashable {
    let name: String
    var detail: String? = nil
    let calls: Int
    let failed: Int
    var avgMs: Double? = nil
    var errors: [String: Int] = [:]
    var id: String { name }
    var rate: Double { calls > 0 ? Double(failed) / Double(calls) : 0 }
}

/// Calls, failures, failure rate and (when known) average time and error
/// types per tool, worst first. Rates of 10% or more are shown in red.
struct ToolResultsTable: View {
    let rows: [ToolResultRow]
    var limit = 15
    var showTiming = false

    private var sorted: [ToolResultRow] {
        rows.sorted { ($0.failed, $0.calls, $1.name) > ($1.failed, $1.calls, $0.name) }
    }

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: Metrics.gap, verticalSpacing: Metrics.tight) {
            GridRow {
                Text("Tool")
                Text("Calls").gridColumnAlignment(.trailing)
                Text("Failed").gridColumnAlignment(.trailing)
                Text("Rate").gridColumnAlignment(.trailing)
                if showTiming { Text("Avg time").gridColumnAlignment(.trailing) }
                Text("Errors")
            }
            .font(.caption).foregroundStyle(.secondary)
            Divider()
            ForEach(sorted.prefix(limit)) { row in
                GridRow {
                    HStack(spacing: Metrics.tight) {
                        Text(row.name).font(.system(.callout, design: .monospaced)).lineLimit(1)
                        if let detail = row.detail { Text(detail).font(.caption2).foregroundStyle(.tertiary) }
                    }
                    Text("\(row.calls)").monospacedDigit()
                    Text("\(row.failed)").monospacedDigit()
                        .foregroundStyle(row.failed > 0 ? Color.primary : Color.secondary)
                    Text(String(format: "%.1f%%", row.rate * 100)).monospacedDigit()
                        .foregroundStyle(row.rate >= 0.1 ? Color.red : (row.failed > 0 ? Color.orange : Color.secondary))
                    if showTiming {
                        Text(row.avgMs.map { $0 >= 1000 ? String(format: "%.1f s", $0 / 1000) : "\(Int($0)) ms" } ?? "—")
                            .monospacedDigit().foregroundStyle(.secondary)
                    }
                    Text(row.errors.sorted { $0.value > $1.value }.map { "\($0.key) \($0.value)" }.joined(separator: ", "))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                .font(.callout)
            }
            if rows.count > limit {
                Text("\(rows.count - limit) more").font(.caption).foregroundStyle(.tertiary)
            }
        }
    }
}
