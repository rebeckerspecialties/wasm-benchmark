// One benchmark on one engine: its score and every measurement behind it,
// or why it did not run. And the overview the wide layouts show beside the
// leaderboard until a benchmark is selected.

import Charts
import SwiftUI

/// The measurements behind a score, as (label, value) rows.
enum MeasurementRows {
    static func timing(_ m: Measurement, reference: Double?) -> [(String, String)] {
        var rows = [("Median per call", Format.duration(ns: Double(m.medianNs)))]
        if let reference {
            rows.append(("Reference per call", Format.duration(ns: reference)))
        }
        rows += [
            ("Fastest call", Format.duration(ns: Double(m.minNs))),
            ("99th percentile", Format.duration(ns: Double(m.p99Ns))),
            ("Timed calls", m.iterations.formatted()),
            ("Load and compile", Format.duration(ns: Double(m.loadNs))),
        ]
        return rows
    }

    static func processor(_ m: Measurement) -> [(String, String)] {
        var rows = [("CPU time", Format.duration(ns: Double(m.cpuUserNs + m.cpuSystemNs)))]
        if let share = m.efficiencyCoreShare {
            rows.append(("On efficiency cores", Format.percent(share)))
        }
        if let ipc = m.instructionsPerCycle {
            rows.append(("Instructions per cycle", ipc.formatted(.number.precision(.fractionLength(2)))))
        }
        rows.append(("Process peak memory", Format.bytes(m.rssPeakBytes)))
        return rows
    }
}

struct ResultDetailView: View {
    @Environment(BenchmarkSession.self) private var session
    let key: ResultKey

    var body: some View {
        if let engine = session.engine(id: key.engine), let benchmark = session.benchmark(id: key.benchmark) {
            let parts = benchmark.titleParts
            Form {
                if let parameters = parts.parameters {
                    Section {
                        Text(parameters)
                            .foregroundStyle(.secondary)
                    }
                }
                result(session.outcome(engine, benchmark), benchmark: benchmark)
                Section("Engine") {
                    LabeledContent("Name", value: engine.name)
                    if !engine.versionLine.isEmpty {
                        LabeledContent("Version", value: engine.versionLine)
                    }
                    if let patches = engine.patchesLine {
                        LabeledContent("Carried patches", value: patches)
                    }
                }
            }
            .navigationTitle(parts.name)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
        } else {
            ContentUnavailableView("No Result", systemImage: "questionmark.circle")
        }
    }

    @ViewBuilder
    private func result(_ outcome: Outcome, benchmark: BenchmarkInfo) -> some View {
        switch outcome {
        case .measured(let m):
            Section("Timing") {
                if let score = Scoring.score(m, reference: benchmark.referenceNs) {
                    LabeledContent("Score") {
                        Text(Format.score(score))
                            .font(.title2.weight(.semibold).monospacedDigit())
                            .foregroundStyle(.primary)
                    }
                }
                ForEach(MeasurementRows.timing(m, reference: benchmark.referenceNs), id: \.0) {
                    LabeledContent($0.0, value: $0.1)
                }
            }
            Section("Processor") {
                ForEach(MeasurementRows.processor(m), id: \.0) {
                    LabeledContent($0.0, value: $0.1)
                }
            }
        case .failed(let message):
            Section("Did not complete") {
                Text(message)
                    .font(.callout.monospaced())
                    #if os(iOS) || os(macOS) || os(visionOS)
                    .textSelection(.enabled)
                    #endif
            }
        case .skipped(let reason):
            Section("Not run") {
                Text(reason)
            }
        case .running:
            Section { Label("Running…", systemImage: "hourglass") }
        case .pending:
            Section { Text("This benchmark has not run yet.").foregroundStyle(.secondary) }
        }
    }
}

#if os(tvOS)
/// The Apple TV detail pane's view of one result: everything on one screen,
/// since nothing in the pane takes focus to scroll it.
struct ResultSummaryView: View {
    @Environment(BenchmarkSession.self) private var session
    let key: ResultKey

    var body: some View {
        if let engine = session.engine(id: key.engine), let benchmark = session.benchmark(id: key.benchmark) {
            VStack(alignment: .leading, spacing: 32) {
                VStack(alignment: .leading, spacing: 8) {
                    BenchmarkTitle(benchmark: benchmark, nameFont: .title2.bold(), parametersFont: .title3)
                    Text("\(engine.name) · \(engine.versionLine)")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                switch session.outcome(engine, benchmark) {
                case .measured(let m):
                    if let score = Scoring.score(m, reference: benchmark.referenceNs) {
                        HStack(alignment: .firstTextBaseline, spacing: 16) {
                            Text(Format.score(score))
                                .font(.system(size: 88, weight: .bold).monospacedDigit())
                            Text("score")
                                .font(.title3)
                                .foregroundStyle(.secondary)
                        }
                    }
                    let rows = MeasurementRows.timing(m, reference: benchmark.referenceNs) + MeasurementRows.processor(m)
                    Grid(alignment: .leading, horizontalSpacing: 32, verticalSpacing: 14) {
                        ForEach(0..<(rows.count + 1) / 2, id: \.self) { i in
                            GridRow {
                                cell(rows[2 * i])
                                if 2 * i + 1 < rows.count { cell(rows[2 * i + 1]) }
                            }
                        }
                    }
                case .failed(let message):
                    Label("Did not complete", systemImage: "xmark.circle")
                        .font(.title3)
                    Text(message)
                        .font(.callout.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(12)
                case .skipped(let reason):
                    Label("Not run", systemImage: "forward.end.circle")
                        .font(.title3)
                    Text(reason)
                        .foregroundStyle(.secondary)
                case .running:
                    Label("Running…", systemImage: "hourglass")
                        .font(.title3)
                case .pending:
                    Text("This benchmark has not run yet.")
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private func cell(_ row: (String, String)) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(row.0)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(row.1)
                .font(.headline.monospacedDigit())
        }
    }
}
#endif

/// The detail column before a benchmark is selected: the engines' scores
/// as a chart, with the run's status and how scores work.
struct OverviewView: View {
    @Environment(BenchmarkSession.self) private var session
    /// Off where the leaderboard beside it already shows the status.
    var showsStatus = true

    private struct Bar: Identifiable {
        let id: UInt32
        let name: String
        let score: Double
        let leader: Bool
    }

    var body: some View {
        let bars = session.ranking.compactMap { engine -> Bar? in
            guard let score = session.standing(engine).score else { return nil }
            return Bar(id: engine.id, name: engine.name, score: score, leader: session.rank(of: engine) == 1)
        }
        Group {
            if bars.isEmpty {
                ContentUnavailableView {
                    Label("No Scores Yet", systemImage: "gauge.with.dots.needle.67percent")
                } description: {
                    Text(session.isRunning
                         ? "Scores appear as the first benchmark finishes on each engine."
                         : "Run the benchmarks to rank the WebAssembly interpreters on this device.")
                } actions: {
                    if !session.isRunning { RunButton().buttonStyle(.borderedProminent) }
                }
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        #if os(tvOS)
                        Text("Scores")
                            .font(.title2.bold())
                        #endif
                        if showsStatus { StatusView() }
                        Chart(bars) { bar in
                            BarMark(
                                x: .value("Score", bar.score),
                                y: .value("Engine", bar.name)
                            )
                            .foregroundStyle(bar.leader ? Color.brand : Color.brand.opacity(0.45))
                            .annotation(position: .trailing, alignment: .leading) {
                                Text(Format.score(bar.score))
                                    .font(.callout.weight(.semibold).monospacedDigit())
                            }
                        }
                        .chartYAxis {
                            AxisMarks(position: .leading) { _ in
                                AxisValueLabel()
                                    .font(.callout)
                            }
                        }
                        .chartXAxisLabel("Score (higher is better)")
                        .frame(height: CGFloat(bars.count) * Metrics.chartRow + 40)
                        .animation(.snappy, value: bars.map(\.score))
                        Text(footnote)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    .padding()
                    .frame(maxWidth: 760, alignment: .leading)
                }
            }
        }
        .navigationTitle("Scores")
    }

    private var footnote: String {
        #if os(tvOS)
        "Move to a benchmark in an engine's list to see its measurements here. 100 is the typical engine on an iPhone XS; an engine's score is the geometric mean of its benchmark scores."
        #else
        "Select a benchmark in an engine's list to see its measurements. 100 is the typical engine on an iPhone XS; an engine's score is the geometric mean of its benchmark scores."
        #endif
    }
}
