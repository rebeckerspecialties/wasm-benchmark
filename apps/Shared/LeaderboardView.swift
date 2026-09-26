// The leaderboard: one expandable row per engine (name, version and short
// commit, aggregate score), best score first, with the engine's benchmarks
// and their scores inside.
//
// Layout: a split view everywhere there is room for one. On iPhone, iPad,
// Mac and Vision Pro it is a NavigationSplitView (the leaderboard in the
// sidebar, the selected result or the score chart in the detail column; in
// compact width it collapses to a stack). On Apple TV the leaderboard takes
// the left column and the right one shows what the Siri Remote's focus is
// on. Apple Watch gets a stack. The rows are system controls — disclosure
// groups, buttons, navigation links — so touch, the Digital Crown, the
// remote's focus engine, pointer and gaze all work without custom handling.

import SwiftUI

extension Color {
    /// WebAssembly purple.
    static let brand = Color(red: 0.396, green: 0.310, blue: 0.941)
}

struct RootView: View {
    @Environment(BenchmarkSession.self) private var session
    @Environment(\.scenePhase) private var scenePhase
    @State private var selection: ResultKey?
    #if os(tvOS)
    @FocusState private var tvFocus: TVRow?
    #endif

    var body: some View {
        content
            .tint(.brand)
            .task { session.autorunIfRequested() }
            .onChange(of: scenePhase) { _, phase in
                session.sceneActivityChanged(active: phase == .active)
            }
    }

    @ViewBuilder private var content: some View {
        #if os(watchOS)
        NavigationStack {
            LeaderboardList(selection: nil)
                .navigationDestination(for: ResultKey.self) { ResultDetailView(key: $0) }
        }
        #elseif os(tvOS)
        HStack(alignment: .top, spacing: 60) {
            NavigationStack {
                LeaderboardList(selection: nil, tvFocus: $tvFocus)
            }
            .frame(width: 880)
            TVDetailPane(focused: tvFocus)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        NavigationSplitView {
            LeaderboardList(selection: $selection)
                .navigationSplitViewColumnWidth(min: 320, ideal: 420, max: 560)
        } detail: {
            if let selection {
                ResultDetailView(key: selection)
            } else {
                OverviewView()
            }
        }
        #endif
    }
}

struct LeaderboardList: View {
    @Environment(BenchmarkSession.self) private var session
    let selection: Binding<ResultKey?>?
    #if os(tvOS)
    var tvFocus: FocusState<TVRow?>.Binding
    #endif
    @State private var expanded: Set<UInt32> = []

    var body: some View {
        list
            #if !os(tvOS)
            .navigationTitle("WasmBench")
            #endif
            .animation(.snappy, value: session.ranking)
            #if os(iOS) || os(visionOS) || os(macOS)
            .toolbar {
                ToolbarItem(placement: .primaryAction) { RunButton() }
                ToolbarItem(placement: .automatic) {
                    ShareLink(item: session.summaryText) {
                        Label("Share Results", systemImage: "square.and.arrow.up")
                    }
                    .disabled(session.completed == 0)
                }
            }
            #endif
            #if os(tvOS)
            .onPlayPauseCommand { session.toggle() }
            #endif
    }

    @ViewBuilder private var list: some View {
        if let selection {
            List(selection: selection) { sections }
                #if os(iOS) || os(visionOS)
                .listStyle(.insetGrouped)
                #endif
        } else {
            List { sections }
        }
    }

    @ViewBuilder private var sections: some View {
        #if os(tvOS)
        // The title as the first section's header scrolls away with the
        // list, where a navigation title would stay over the rows.
        Section {
            controls
        } header: {
            Text("WasmBench")
                .font(.title2.bold())
                .textCase(nil)
        }
        #else
        Section { controls }
        #endif
        Section {
            ForEach(session.ranking) { engine in
                #if os(tvOS)
                EngineGroup(engine: engine, isExpanded: expandedBinding(engine), tvFocus: tvFocus)
                #else
                EngineGroup(engine: engine, isExpanded: expandedBinding(engine))
                #endif
            }
        } header: {
            Text("Engines")
        } footer: {
            Text("100 is the typical engine on an iPhone XS. Each benchmark scores 100 × its reference time ÷ the median time per call, and \(Format.score(Scoring.failurePenalty)) if the engine cannot run it. An engine's score is the average of its benchmark scores. Higher is better.")
        }
    }

    @ViewBuilder private var controls: some View {
        // First on the watch and the TV: the button is the first thing in
        // reach and where the focus engine starts.
        #if os(watchOS)
        RunButton()
        #elseif os(tvOS)
        RunButton()
            .focused(tvFocus, equals: .control)
        #endif
        StatusView()
    }

    private func expandedBinding(_ engine: EngineInfo) -> Binding<Bool> {
        Binding {
            expanded.contains(engine.id)
        } set: { open in
            if open { expanded.insert(engine.id) } else { expanded.remove(engine.id) }
        }
    }
}

// MARK: - Engine rows

struct EngineGroup: View {
    @Environment(BenchmarkSession.self) private var session
    let engine: EngineInfo
    @Binding var isExpanded: Bool
    #if os(tvOS)
    var tvFocus: FocusState<TVRow?>.Binding
    #endif

    var body: some View {
        #if os(watchOS) || os(tvOS)
        // No DisclosureGroup on watchOS and tvOS: the engine row is a button
        // that shows or hides the benchmark rows under it, which keeps every
        // row a focus stop for the Siri Remote.
        Button {
            withAnimation(.snappy) { isExpanded.toggle() }
        } label: {
            HStack(spacing: Metrics.spacing) {
                EngineSummaryRow(engine: engine)
                #if os(tvOS)
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .rotationEffect(.degrees(isExpanded ? 90 : 0))
                    .accessibilityHidden(true)
                #endif
            }
        }
        #if os(tvOS)
        .focused(tvFocus, equals: .engine(engine.id))
        #endif
        .accessibilityHint(isExpanded ? "Hides this engine's benchmarks" : "Shows this engine's benchmarks")
        if isExpanded {
            children
                .padding(.leading, Metrics.childIndent)
        }
        #else
        DisclosureGroup(isExpanded: $isExpanded) {
            children
        } label: {
            // In the split view's selectable list a tap on the label would
            // go to selection, leaving only the chevron to expand the group;
            // the whole row toggles it instead.
            EngineSummaryRow(engine: engine)
                .contentShape(.rect)
                .onTapGesture {
                    withAnimation(.snappy) { isExpanded.toggle() }
                }
        }
        #endif
    }

    @ViewBuilder private var children: some View {
        if session.isAvailable(engine) {
            ForEach(session.benchmarks) { benchmark in
                let key = ResultKey(engine: engine.id, benchmark: benchmark.id)
                #if os(tvOS)
                // The detail pane follows focus, so selecting does nothing
                // more; the button makes the row a focus stop.
                Button {} label: {
                    BenchmarkRow(engine: engine, benchmark: benchmark)
                }
                .focused(tvFocus, equals: .result(key))
                #else
                NavigationLink(value: key) {
                    BenchmarkRow(engine: engine, benchmark: benchmark)
                }
                #endif
            }
        } else {
            Text(engine.linked ? "This engine did not start on this device." : "This build does not include this engine.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }
}

struct EngineSummaryRow: View {
    @Environment(BenchmarkSession.self) private var session
    let engine: EngineInfo

    var body: some View {
        let standing = session.standing(engine)
        let rank = session.rank(of: engine)
        let running = session.current?.engine == engine.id
        content(standing: standing, rank: rank, running: running)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(accessibilityLabel(rank: rank))
            .accessibilityValue(accessibilityValue(standing))
    }

    @ViewBuilder
    private func content(standing: Standing, rank: Int?, running: Bool) -> some View {
        #if os(watchOS)
        // Rank, name and score on the first line; the version gets the whole
        // second line.
        VStack(alignment: .leading, spacing: 1) {
            HStack(spacing: 5) {
                RankBadge(rank: rank, running: running)
                Text(engine.name)
                    .font(.headline)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                Spacer(minLength: 2)
                score(standing)
            }
            Text(engine.compactVersionLine)
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        #else
        HStack(spacing: Metrics.spacing) {
            RankBadge(rank: rank, running: running)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline) {
                    Text(engine.name)
                        .font(.headline)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    score(standing)
                }
                HStack(alignment: .firstTextBaseline) {
                    Text(engine.versionLine)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 4)
                    if standing.total > 0 {
                        Text("\(standing.scored) of \(standing.total)")
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.secondary)
                            .fixedSize()
                    }
                }
            }
        }
        .padding(.vertical, Metrics.rowPadding)
        #endif
    }

    private func score(_ standing: Standing) -> some View {
        Text(standing.score.map(Format.score) ?? "—")
            .font(Metrics.engineScoreFont.monospacedDigit())
            .foregroundStyle(scoreStyle(standing.score))
            .contentTransition(.numericText(value: standing.score ?? 0))
            .fixedSize()
    }

    private func scoreStyle(_ score: Double?) -> AnyShapeStyle {
        guard let score else { return AnyShapeStyle(.tertiary) }
        return score < 0 ? AnyShapeStyle(.red) : AnyShapeStyle(.primary)
    }

    private func accessibilityLabel(rank: Int?) -> String {
        let place = rank.map { "Rank \($0), " } ?? ""
        return "\(place)\(engine.name), \(engine.versionLine)"
    }

    private func accessibilityValue(_ standing: Standing) -> String {
        guard let score = standing.score else {
            return session.isAvailable(engine) ? "No score yet" : "Not available"
        }
        let failed = standing.failed > 0 ? ", \(standing.failed) failed" : ""
        return "Score \(Format.score(score)), \(standing.scored) of \(standing.total) benchmarks\(failed)"
    }
}

struct RankBadge: View {
    let rank: Int?
    let running: Bool

    var body: some View {
        ZStack {
            if running {
                ProgressView()
                    #if os(watchOS)
                    .scaleEffect(0.6)
                    #endif
            } else if let rank {
                Circle().fill(rank == 1 ? AnyShapeStyle(Color.brand) : AnyShapeStyle(.quaternary))
                Text("\(rank)")
                    .font(Metrics.rankFont.weight(.bold).monospacedDigit())
                    .foregroundStyle(rank == 1 ? .white : .primary)
            } else {
                Circle().strokeBorder(.quaternary, lineWidth: 1.5)
            }
        }
        .frame(width: Metrics.badge, height: Metrics.badge)
        .accessibilityHidden(true)
    }
}

// MARK: - Benchmark rows

/// A benchmark's name with its parameters after it, smaller and dimmed:
/// "fib_tail  100000 · return_call". One paragraph, so a long one wraps
/// instead of truncating.
struct BenchmarkTitle: View {
    let benchmark: BenchmarkInfo
    var nameFont = Metrics.benchmarkNameFont
    var parametersFont = Metrics.benchmarkParametersFont

    var body: some View {
        let parts = benchmark.titleParts
        if let parameters = parts.parameters {
            Text("\(Text(parts.name).font(nameFont))  \(Text(parameters).font(parametersFont).foregroundStyle(.secondary))")
        } else {
            Text(parts.name).font(nameFont)
        }
    }
}

struct BenchmarkRow: View {
    @Environment(BenchmarkSession.self) private var session
    let engine: EngineInfo
    let benchmark: BenchmarkInfo

    var body: some View {
        let outcome = session.outcome(engine, benchmark)
        #if os(watchOS)
        // The name gets the whole width; the result is one line under it.
        VStack(alignment: .leading, spacing: 2) {
            BenchmarkTitle(benchmark: benchmark)
            WatchResultLine(outcome: outcome, benchmark: benchmark)
        }
        .accessibilityElement(children: .combine)
        #else
        VStack(alignment: .leading, spacing: 2) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                BenchmarkTitle(benchmark: benchmark)
                Spacer(minLength: 8)
                trailing(outcome)
            }
            if let detail = detail(outcome) {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .accessibilityElement(children: .combine)
        #endif
    }

    #if !os(watchOS)
    private func detail(_ outcome: Outcome) -> String? {
        switch outcome {
        case .measured(let m): "\(Format.duration(ns: Double(m.medianNs))) per call"
        case .failed(let message): Format.failureSummary(message)
        case .skipped(let reason): "Skipped: \(reason)"
        case .running, .pending: nil
        }
    }

    @ViewBuilder private func trailing(_ outcome: Outcome) -> some View {
        switch outcome {
        case .running:
            ProgressView()
        case .measured(let m):
            if let score = Scoring.score(m, reference: benchmark.referenceNs) {
                Text(Format.score(score))
                    .font(.body.weight(.semibold).monospacedDigit())
                    .accessibilityLabel("Score \(Format.score(score))")
            } else {
                Text("—").foregroundStyle(.tertiary)
            }
        case .failed:
            PenaltyText(font: .body.weight(.semibold))
        case .skipped:
            Image(systemName: "forward.end.circle")
                .foregroundStyle(.secondary)
                .accessibilityLabel("Skipped")
        case .pending:
            Text("—").foregroundStyle(.tertiary)
        }
    }
    #endif
}

/// A failed benchmark's score, in red.
struct PenaltyText: View {
    let font: Font

    var body: some View {
        Text(Format.score(Scoring.failurePenalty))
            .font(font.monospacedDigit())
            .foregroundStyle(.red)
            .accessibilityLabel("Failed, score \(Format.score(Scoring.failurePenalty))")
    }
}

#if os(watchOS)
/// The watch's result line: the score, then the median time in a smaller
/// style — or why there is no score.
struct WatchResultLine: View {
    let outcome: Outcome
    let benchmark: BenchmarkInfo

    var body: some View {
        switch outcome {
        case .measured(let m):
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                if let score = Scoring.score(m, reference: benchmark.referenceNs) {
                    Text(Format.score(score))
                        .font(.headline.monospacedDigit())
                        .accessibilityLabel("Score \(Format.score(score))")
                }
                Text(Format.duration(ns: Double(m.medianNs)))
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        // The engine's message is in the benchmark's detail view.
        case .failed:
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                PenaltyText(font: .headline)
                Text("Failed")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        case .skipped:
            Label("Skipped", systemImage: "forward.end.circle")
                .font(.footnote)
                .foregroundStyle(.secondary)
        case .running:
            Text("Running…")
                .font(.footnote)
                .foregroundStyle(.secondary)
        case .pending:
            Text("—")
                .font(.headline)
                .foregroundStyle(.tertiary)
        }
    }
}
#endif

// MARK: - Status and controls

struct StatusView: View {
    @Environment(BenchmarkSession.self) private var session

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(session.device)
                .font(.footnote.monospacedDigit())
                .foregroundStyle(.secondary)
            switch session.phase {
            case .idle:
                Text(idleText)
                    .font(.subheadline)
            case .running, .paused:
                ProgressView(value: Double(session.completed), total: Double(max(session.planned, 1))) {
                    Text(currentText)
                        .font(.subheadline)
                        .lineLimit(2)
                } currentValueLabel: {
                    Text("\(session.completed) of \(session.planned)")
                        .monospacedDigit()
                }
            case .finished:
                Text("Finished in \(elapsed).")
                    .font(.subheadline)
            case .stopped:
                Text("Stopped after \(session.completed) of \(session.planned).")
                    .font(.subheadline)
            }
        }
        .padding(.vertical, Metrics.rowPadding)
        .accessibilityElement(children: .combine)
    }

    private var idleText: String {
        let engines = session.engines.filter(session.isAvailable).count
        #if os(watchOS)
        return "\(engines) engines × \(session.benchmarks.count) benchmarks"
        #else
        return "\(engines) WebAssembly interpreters × \(session.benchmarks.count) benchmarks. A run takes a few minutes; keep the app open."
        #endif
    }

    private var currentText: String {
        if session.phase == .paused { return "Paused until the app is back in the foreground" }
        guard let key = session.current,
              let engine = session.engine(id: key.engine),
              let benchmark = session.benchmark(id: key.benchmark)
        else { return "Starting…" }
        return "\(benchmark.titleParts.name) on \(engine.name)"
    }

    private var elapsed: String {
        guard let start = session.startedAt, let end = session.finishedAt else { return "" }
        return Format.elapsed(end.timeIntervalSince(start))
    }
}

struct RunButton: View {
    @Environment(BenchmarkSession.self) private var session

    var body: some View {
        Button {
            session.toggle()
        } label: {
            if session.isRunning {
                Label("Stop", systemImage: "stop.fill")
            } else {
                #if os(watchOS)
                Label("Run", systemImage: "play.fill")
                #else
                Label(session.phase == .idle ? "Run Benchmarks" : "Run Again", systemImage: "play.fill")
                #endif
            }
        }
        .disabled(session.engines.filter(session.isAvailable).isEmpty)
    }
}

// MARK: - Apple TV detail pane

#if os(tvOS)
/// The leaderboard row the Siri Remote's focus is on.
enum TVRow: Hashable {
    case control
    case engine(UInt32)
    case result(ResultKey)
}

/// The right-hand column on Apple TV: the focused benchmark's
/// measurements, or the engines' scores as a chart.
struct TVDetailPane: View {
    let focused: TVRow?

    var body: some View {
        Group {
            if case .result(let key) = focused {
                ResultSummaryView(key: key)
            } else {
                OverviewView(showsStatus: false)
            }
        }
        .padding(.top, 48)
        .padding(.trailing, 60)
    }
}
#endif

enum Metrics {
    #if os(watchOS)
    static let engineScoreFont = Font.headline
    static let rankFont = Font.caption2
    static let benchmarkNameFont = Font.headline
    static let benchmarkParametersFont = Font.footnote
    #else
    static let engineScoreFont = Font.title3.weight(.semibold)
    static let rankFont = Font.footnote
    static let benchmarkNameFont = Font.subheadline.weight(.medium)
    static let benchmarkParametersFont = Font.caption
    #endif
    #if os(tvOS)
    static let badge: CGFloat = 44
    static let spacing: CGFloat = 20
    static let rowPadding: CGFloat = 4
    /// Benchmark rows sit under the engine's name.
    static let childIndent: CGFloat = badge + spacing
    static let chartRow: CGFloat = 72
    #elseif os(watchOS)
    static let badge: CGFloat = 18
    static let spacing: CGFloat = 5
    static let rowPadding: CGFloat = 0
    static let childIndent: CGFloat = 0
    static let chartRow: CGFloat = 28
    #else
    static let badge: CGFloat = 28
    static let spacing: CGFloat = 12
    static let rowPadding: CGFloat = 2
    static let chartRow: CGFloat = 44
    #endif
}
