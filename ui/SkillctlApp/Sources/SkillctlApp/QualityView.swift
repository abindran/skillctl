import SwiftUI

/// Validation of store versions and lint of a project's instruction files.
struct QualityView: View {
    @Environment(AppState.self) private var state
    @State private var tab = 0
    @State private var reference: Artifact.ID?
    @State private var validateFindings: [Finding]?
    @State private var lintFolder: URL?
    @State private var lintFindings: [Finding]?
    @State private var lintFiles: [InspectedFile]?
    @State private var shown: ShownDocument?
    @State private var running = false
    @State private var ruleFilter = "all"
    @State private var ruleSearch = ""

    private var instanceFolders: [URL] {
        state.projects.flatMap(\.instances).map(\.url)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Picker("", selection: $tab) {
                    Text("Validate an artifact").tag(0)
                    Text("Lint instruction files").tag(1)
                    Text("Rules reference").tag(2)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                Spacer()
            }
            .barLayout()
            Divider()
            ScrollView {
                Group {
                    switch tab {
                    case 0: validate
                    case 1: lint
                    default: rulesReference
                    }
                }
                .pageLayout()
            }
        }
        .navigationTitle("Quality")
        .task { await state.loadRules() }
        .task {
            // Snapshot mode only: open a tab (0 validate, 1 lint, 2 rules) for review.
            if ContentView.snapshotFolder != nil,
               let value = ProcessInfo.processInfo.environment["SKILLCTL_APP_SNAPSHOT_QUALITY_TAB"], let index = Int(value) {
                tab = index
            }
        }
        .task {
            // Snapshot mode only: show a real validation so the layout can be reviewed.
            guard ContentView.snapshotFolder != nil, ProcessInfo.processInfo.environment["SKILLCTL_APP_SNAPSHOT_VALIDATE"] == "1", validateFindings == nil else { return }
            let candidates = state.artifacts.filter { $0.kind == "skill" && $0.version.hasSuffix(".0") } + state.artifacts
            if let artifact = candidates.first {
                reference = artifact.id
                validateFindings = await state.validate(artifact.id) ?? []
                shown = await state.show(artifact.id)
            }
        }
    }

    private var validate: some View {
        VStack(alignment: .leading, spacing: Metrics.section) {
            GroupBox {
                VStack(alignment: .leading, spacing: Metrics.gap) {
                    HStack(spacing: Metrics.gap) {
                        Menu(reference ?? "Choose an artifact…") {
                            ForEach(ArtifactKind.all) { kind in
                                let items = state.artifacts.filter { $0.kind == kind.id }
                                if !items.isEmpty {
                                    SwiftUI.Section(kind.title) {
                                        ForEach(items) { artifact in
                                            Button(artifact.reference) { choose(artifact.id) }
                                        }
                                    }
                                }
                            }
                        }
                        .fixedSize()
                        Button("Validate") {
                            guard let reference else { return }
                            running = true
                            Task {
                                validateFindings = await state.validate(reference) ?? []
                                running = false
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(reference == nil || running)
                        if running { ProgressView().controlSize(.small) }
                        Spacer()
                    }
                    Text("Skills get S and O rules (including the directive-description rules O140–O145), hooks K1xx, rules K2xx, commands K3xx, agents H22x, scripts K5xx. Nothing is written.")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } label: {
                Label("Artifact", systemImage: "checkmark.seal")
            }
            results(validateFindings, emptyText: "0 error(s), 0 warning(s)", prompt: "Choose an artifact and press Validate.")
            if let shown { validatedScope(shown) }
        }
    }

    private func choose(_ id: String) {
        reference = id
        validateFindings = nil
        shown = nil
        Task { shown = await state.show(id) }
    }

    private func chooseFolder(_ url: URL?) {
        guard let url else { return }
        lintFolder = url
        lintFindings = nil
        lintFiles = nil
        Task { lintFiles = await state.lintFiles(folder: url) }
    }

    // MARK: What is checked

    /// The version being validated: its kind, the rules that apply and every file in it.
    private func validatedScope(_ shown: ShownDocument) -> some View {
        let kind = ArtifactKind.named(shown.kind)
        return scopeCard(title: "What is validated", symbol: "doc.text.magnifyingglass") {
            VStack(alignment: .leading, spacing: Metrics.gap) {
                HStack(spacing: Metrics.tight) {
                    Image(systemName: kind.symbol).foregroundStyle(.secondary)
                    Text("\(shown.name) \(shown.version == "draft" ? "draft" : "v\(shown.version)")").fontWeight(.semibold)
                    SeverityBadge(text: shown.kind)
                    Spacer()
                    Button("Reveal") { Panels.reveal(shown.documentURL.path) }.controlSize(.small)
                }
                HStack(alignment: .firstTextBaseline) {
                    Text(kind.summary).font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer()
                    rulesLink(for: shown.kind)
                }
                Divider()
                ForEach(shown.files) { file in
                    HStack(spacing: Metrics.tight) {
                        Image(systemName: file.path == shown.document ? "doc.richtext" : "doc")
                            .foregroundStyle(.secondary).frame(width: 18)
                        Text(file.path).font(.system(.callout, design: .monospaced))
                        if file.path == shown.document {
                            Text("root document").font(.caption2).foregroundStyle(.tertiary)
                        }
                        Spacer()
                        Text(ByteCountFormatter.string(fromByteCount: Int64(file.bytes), countStyle: .file))
                            .font(.caption).foregroundStyle(.secondary).monospacedDigit()
                    }
                }
            }
        }
    }

    /// Every instruction file lint reads in the folder, with when it loads.
    private func lintedScope(_ files: [InspectedFile]) -> some View {
        let present = files.filter(\.present).count
        return scopeCard(title: "What is linted", symbol: "doc.text.magnifyingglass",
                         trailing: "\(present) of \(files.count) present") {
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Spacer()
                    rulesLink(for: "instructions")
                }
                ForEach(Array(files.enumerated()), id: \.element.id) { index, file in
                    HStack(alignment: .firstTextBaseline, spacing: Metrics.gap) {
                        Image(systemName: file.present ? "checkmark.circle.fill" : "circle.dashed")
                            .foregroundStyle(file.present ? Color.green : Color.secondary)
                            .frame(width: 18)
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: Metrics.tight) {
                                Text(file.name).font(.system(.callout, design: .monospaced))
                                    .foregroundStyle(file.present ? .primary : .secondary)
                                if file.link { SeverityBadge(text: "link") }
                                if let lines = file.lines {
                                    Text("\(lines) lines").font(.caption).foregroundStyle(.secondary)
                                }
                            }
                            Text(file.present ? file.loads : "Not present. " + file.loads)
                                .font(.caption).foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer(minLength: Metrics.gap)
                        Text(file.rules.joined(separator: " "))
                            .font(.caption2.monospaced()).foregroundStyle(.tertiary)
                    }
                    .padding(.vertical, Metrics.header)
                    if index < files.count - 1 { Divider() }
                }
            }
        }
    }

    private func rulesLink(for kind: String) -> some View {
        let count = state.rules.filter { $0.applies_to == kind && $0.reported_by != "advisory" }.count
        return Button("\(count) rules apply") {
            ruleFilter = kind
            ruleSearch = ""
            tab = 2
        }
        .buttonStyle(.link)
        .font(.caption)
        .disabled(count == 0)
    }

    // MARK: Rules reference

    private var filteredRules: [RuleEntry] {
        let query = ruleSearch.trimmingCharacters(in: .whitespaces).lowercased()
        return state.rules.filter { rule in
            (ruleFilter == "all" || rule.applies_to == ruleFilter)
                && (query.isEmpty || rule.id.lowercased().contains(query) || rule.summary.lowercased().contains(query)
                    || rule.group.lowercased().contains(query))
        }
    }

    /// Group names in catalog order, each with its rules.
    private var ruleGroups: [(name: String, appliesTo: String, rules: [RuleEntry])] {
        var groups: [(name: String, appliesTo: String, rules: [RuleEntry])] = []
        for rule in filteredRules {
            if let index = groups.firstIndex(where: { $0.name == rule.group }) {
                groups[index].rules.append(rule)
            } else {
                groups.append((rule.group, rule.applies_to, [rule]))
            }
        }
        return groups
    }

    private func appliesToTitle(_ id: String) -> String {
        id == "instructions" ? "Instruction files (lint)" : ArtifactKind.named(id).title
    }

    private var rulesReference: some View {
        VStack(alignment: .leading, spacing: Metrics.section) {
            GroupBox {
                VStack(alignment: .leading, spacing: Metrics.gap) {
                    HStack(spacing: Metrics.gap) {
                        Picker("Applies to", selection: $ruleFilter) {
                            Text("All").tag("all")
                            Divider()
                            ForEach(["skill", "hook", "rule", "command", "agent", "script"], id: \.self) { kind in
                                Text(ArtifactKind.named(kind).title).tag(kind)
                            }
                            Divider()
                            Text("Instruction files (lint)").tag("instructions")
                        }
                        .fixedSize()
                        TextField("Search by ID or text", text: $ruleSearch)
                            .textFieldStyle(.roundedBorder)
                            .frame(maxWidth: 280)
                        Spacer()
                        Text("\(filteredRules.count) rule(s)").font(.caption).foregroundStyle(.secondary)
                    }
                    Text("Errors fail validate and lint (exit 3); warnings fail only with --strict. Rules with a lock make the artifact unloadable and cannot be silenced with validate.allow. The same list is in the terminal: skillctl rules --for <kind>.")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } label: {
                Label("Rules", systemImage: "book")
            }
            if state.rules.isEmpty {
                ProgressView().frame(maxWidth: .infinity, minHeight: 80)
            } else if ruleGroups.isEmpty {
                Text("No rule matches \"\(ruleSearch)\".").foregroundStyle(.secondary)
            }
            ForEach(ruleGroups, id: \.name) { group in
                GroupBox {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(group.rules.enumerated()), id: \.element.id) { index, rule in
                            ruleRow(rule)
                            if index < group.rules.count - 1 { Divider() }
                        }
                    }
                } label: {
                    HStack {
                        Text(group.name)
                        Text(appliesToTitle(group.appliesTo)).font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Text("\(group.rules.count)").font(.caption).foregroundStyle(.tertiary)
                    }
                }
            }
        }
    }

    private func ruleRow(_ rule: RuleEntry) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Metrics.gap) {
            Text(rule.id).font(.system(.callout, design: .monospaced).weight(.semibold))
                .frame(width: 48, alignment: .leading)
                .textSelection(.enabled)
            SeverityBadge(text: rule.severity).frame(width: 64, alignment: .leading)
            Text(rule.summary).font(.callout)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            if rule.reported_by == "advisory" {
                Text("advisory").font(.caption2).foregroundStyle(.tertiary)
                    .help("Reported after pin changes and by status --budget, not by validate")
            }
            if !rule.allow_listable {
                Image(systemName: "lock.fill").font(.caption).foregroundStyle(.tertiary)
                    .help("Cannot be silenced with validate.allow")
            }
        }
        .padding(.vertical, Metrics.header)
    }

    private func scopeCard<Content: View>(title: String, symbol: String, trailing: String? = nil,
                                          @ViewBuilder content: () -> Content) -> some View {
        GroupBox {
            content().frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            HStack {
                Label(title, systemImage: symbol)
                Spacer()
                if let trailing { Text(trailing).font(.caption).foregroundStyle(.secondary) }
            }
        }
    }

    private var lint: some View {
        VStack(alignment: .leading, spacing: Metrics.section) {
            GroupBox {
                VStack(alignment: .leading, spacing: Metrics.gap) {
                    HStack(spacing: Metrics.gap) {
                        Menu(lintFolder?.path ?? "Choose a folder…") {
                            ForEach(instanceFolders, id: \.self) { url in
                                Button(url.path) { chooseFolder(url) }
                            }
                            Divider()
                            Button("Other folder…") { chooseFolder(Panels.chooseFolder(prompt: "Lint")) }
                        }
                        .fixedSize()
                        Button("Lint") {
                            guard let lintFolder else { return }
                            running = true
                            Task {
                                lintFindings = await state.lint(folder: lintFolder) ?? []
                                lintFiles = await state.lintFiles(folder: lintFolder) ?? lintFiles
                                running = false
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(lintFolder == nil || running)
                        if running { ProgressView().controlSize(.small) }
                        Spacer()
                    }
                    Text("Read-only checks I301–I306, I308 and I309 on AGENTS.md, CLAUDE.md, CLAUDE.local.md and .cursor/rules; each finding carries the suggested fix. The files are team-owned and changed through review, never by skillctl.")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } label: {
                Label("Folder", systemImage: "folder")
            }
            results(lintFindings,
                    emptyText: lintFiles?.contains(where: \.present) == false
                        ? "Nothing to lint: none of the instruction files below exist in this folder."
                        : "No instruction-file findings.",
                    prompt: "Choose a folder and press Lint.")
            if let lintFiles { lintedScope(lintFiles) }
        }
    }

    /// The findings, filling the rest of the screen on the same card surface.
    @ViewBuilder
    private func results(_ findings: [Finding]?, emptyText: String, prompt: String) -> some View {
        VStack(alignment: .leading, spacing: Metrics.header) {
            Label("Findings", systemImage: "list.bullet.rectangle").fontWeight(.semibold)
            Group {
                if let findings {
                    FindingsTable(findings: findings, emptyText: emptyText)
                } else {
                    Text(prompt)
                        .foregroundStyle(.secondary)
                        .padding(Metrics.card)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: Metrics.radius).fill(Color.secondary.opacity(0.08)))
            .overlay(RoundedRectangle(cornerRadius: Metrics.radius).strokeBorder(Color.secondary.opacity(0.12)))
            .clipShape(RoundedRectangle(cornerRadius: Metrics.radius))
        }
    }
}
