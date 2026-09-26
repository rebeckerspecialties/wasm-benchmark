// The engines and benchmarks benchmark-core exports (`bench_catalog_json`),
// one benchmark's outcome, and how outcomes turn into scores.
//
// Scores follow JetStream and MotionMark: higher is better. A benchmark's
// score is 100 x its reference time / the measured median time per call,
// where the reference is the typical engine on an iPhone XS
// (crates/benchmark-core/src/score_reference.rs). A benchmark the engine
// cannot run (an error, a trap, a wrong result, or a known failure the app
// does not attempt) scores -5. An engine's score is the mean of its
// benchmark scores, so every benchmark it cannot run costs it.

import Foundation

// MARK: - Catalog

struct EngineInfo: Decodable, Identifiable, Hashable, Sendable {
    /// `bench_run_case` runtime id.
    let id: UInt32
    /// `RUNTIMES=` token.
    let token: String
    /// Row prefix of the console lines the harness scripts parse (`[Pulley]`).
    let prefix: String
    let name: String
    /// Project the engine ships in when that differs from its name.
    let upstream: String
    /// Release plus commits past it ("2.4.1+364").
    let version: String
    /// Short hash of the pinned commit; empty for crates.io releases.
    let commit: String
    let patches: Int
    /// Whether this build links the engine.
    let linked: Bool

    /// "wasmtime 49.0.0+9 · 0d9aebd6"
    var versionLine: String {
        let release = [upstream, version].filter { !$0.isEmpty }.joined(separator: " ")
        return [release, commit].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// "49.0.0+9 · 0d9aebd6", for the watch's narrow rows.
    var compactVersionLine: String {
        [version, commit].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    var patchesLine: String? {
        switch patches {
        case 0: nil
        case 1: "1 patch"
        default: "\(patches) patches"
        }
    }
}

struct BenchmarkInfo: Decodable, Identifiable, Hashable, Sendable {
    let id: String
    let label: String
    /// Nanoseconds per call of the typical engine on an iPhone XS.
    let referenceNs: Double?
    /// Why the app does not run it, if it doesn't.
    let appExcluded: String?
    /// Why the watch app does not run it, if it doesn't.
    let watchExcluded: String?

    enum CodingKeys: String, CodingKey {
        case id, label
        case referenceNs = "reference_ns"
        case appExcluded = "app_excluded"
        case watchExcluded = "watch_excluded"
    }
}

extension BenchmarkInfo {
    /// The label split for display: "fib_tail(100000) [return_call]" is the
    /// name "fib_tail" and the parameters "100000 · return_call" (the
    /// bracketed parts, joined).
    var titleParts: (name: String, parameters: String?) {
        guard let open = label.firstIndex(where: { $0 == "(" || $0 == "[" }) else {
            return (label, nil)
        }
        var groups: [String] = []
        var current = ""
        var depth = 0
        for ch in label[open...] {
            switch ch {
            case "(", "[":
                if depth > 0 { current.append(ch) }
                depth += 1
            case ")", "]":
                depth -= 1
                if depth == 0 {
                    groups.append(current.trimmingCharacters(in: .whitespaces))
                    current = ""
                } else {
                    current.append(ch)
                }
            default:
                if depth > 0 { current.append(ch) }
            }
        }
        let name = label[..<open].trimmingCharacters(in: .whitespaces)
        return (name, groups.isEmpty ? nil : groups.joined(separator: " · "))
    }
}

/// A row the app leaves out: the engine keeps its memory for as long as
/// the process lives.
struct SkipInfo: Decodable, Hashable, Sendable {
    let engine: String
    let benchmark: String
    let reason: String

    enum CodingKeys: String, CodingKey {
        case engine, reason
        case benchmark = "case"
    }
}

struct Catalog: Decodable, Sendable {
    let engines: [EngineInfo]
    let cases: [BenchmarkInfo]
    let skips: [SkipInfo]

    static func load() -> Catalog {
        guard let json = bench_catalog_json() else {
            return Catalog(engines: [], cases: [], skips: [])
        }
        defer { bench_free_cstring(json) }
        do {
            return try JSONDecoder().decode(Catalog.self, from: Data(String(cString: json).utf8))
        } catch {
            FileHandle.standardError.write(Data("catalog: \(error)\n".utf8))
            return Catalog(engines: [], cases: [], skips: [])
        }
    }

    func skipReason(engine: EngineInfo, benchmark: BenchmarkInfo) -> String? {
        skips.first { $0.engine == engine.token && $0.benchmark == benchmark.id }?.reason
    }
}

/// `std::env::var` through benchmark-core: on watchOS, `devicectl
/// --environment-variables` reaches it but not `ProcessInfo`.
func environmentValue(_ name: String) -> String? {
    guard let value = bench_getenv(name) else { return nil }
    defer { bench_free_cstring(value) }
    return String(cString: value)
}

// MARK: - Results

struct ResultKey: Hashable, Sendable {
    let engine: UInt32
    let benchmark: String
}

/// One `BenchReport` that succeeded.
struct Measurement: Hashable, Sendable {
    let result: Int32
    let iterations: UInt32
    let loadNs: UInt64
    let minNs: UInt64
    let medianNs: UInt64
    let p99Ns: UInt64
    let cpuUserNs: UInt64
    let cpuSystemNs: UInt64
    let rssPeakBytes: UInt64
    let pageFaults: UInt64
    let pCoreNs: UInt64
    let instructions: UInt64
    let cycles: UInt64

    init(_ r: BenchReport) {
        result = r.result
        iterations = r.iterations
        loadNs = r.load_ns
        minNs = r.run_ns_min
        medianNs = r.run_ns_median
        p99Ns = r.run_ns_p99
        cpuUserNs = r.cpu_user_ns
        cpuSystemNs = r.cpu_system_ns
        rssPeakBytes = r.rss_peak_bytes
        pageFaults = r.page_faults
        pCoreNs = r.p_cpu_ns
        instructions = r.instructions
        cycles = r.cycles
    }

    /// Share of the timed window's CPU time that ran on efficiency cores.
    var efficiencyCoreShare: Double? {
        let cpu = Double(cpuUserNs + cpuSystemNs)
        return cpu > 0 ? 1 - min(Double(pCoreNs) / cpu, 1) : nil
    }

    var instructionsPerCycle: Double? {
        cycles > 0 ? Double(instructions) / Double(cycles) : nil
    }
}

enum Outcome: Hashable, Sendable {
    case pending
    case running
    case measured(Measurement)
    /// The engine returned an error: a missing feature, a trap or a wrong
    /// result.
    case failed(String)
    /// The app did not run it, for the reason given.
    case skipped(String)

    var isFinished: Bool {
        switch self {
        case .pending, .running: false
        case .measured, .failed, .skipped: true
        }
    }
}

enum Scoring {
    /// A failed benchmark's score.
    static let failurePenalty: Double = -5

    /// 100 x reference / median; nil without a reference or a measurable time.
    static func score(_ m: Measurement, reference: Double?) -> Double? {
        guard let reference, m.medianNs > 0 else { return nil }
        return 100 * reference / Double(m.medianNs)
    }

    /// An arithmetic mean, not a geometric one: failures score below zero.
    static func mean(_ values: [Double]) -> Double? {
        guard !values.isEmpty else { return nil }
        return values.reduce(0, +) / Double(values.count)
    }
}

/// An engine's place on the leaderboard.
struct Standing: Hashable, Sendable {
    /// Mean of the benchmark scores, failures included; nil before the
    /// first result.
    var score: Double?
    /// Benchmarks with a score from a measurement.
    var scored = 0
    /// Benchmarks that failed, each scoring `Scoring.failurePenalty`.
    var failed = 0
    /// Benchmarks that finished (scored, failed or skipped).
    var finished = 0
    var total = 0
}

// MARK: - Formatting

enum Format {
    /// Whole numbers from 10 up ("1,234") and for whole values ("−5"), two
    /// significant digits otherwise. Negative scores get a minus sign, not a
    /// hyphen.
    static func score(_ s: Double) -> String {
        let magnitude = abs(s)
        let digits = magnitude >= 10 || magnitude == magnitude.rounded()
            ? magnitude.formatted(.number.precision(.fractionLength(0)))
            : magnitude.formatted(.number.precision(.significantDigits(2)))
        return s < 0 ? "\u{2212}\(digits)" : digits
    }

    /// Three significant digits with a unit: "126 ms", "570 µs", "1.06 s".
    static func duration(ns: Double) -> String {
        let (value, unit): (Double, String) = switch ns {
        case ..<1_000: (ns, "ns")
        case ..<1_000_000: (ns / 1_000, "µs")
        case ..<1_000_000_000: (ns / 1_000_000, "ms")
        default: (ns / 1_000_000_000, "s")
        }
        return "\(value.formatted(.number.precision(.significantDigits(3)))) \(unit)"
    }

    static func bytes(_ b: UInt64) -> String {
        Int64(clamping: b).formatted(.byteCount(style: .memory))
    }

    static func percent(_ f: Double) -> String {
        f.formatted(.percent.precision(.fractionLength(1)))
    }

    /// "2 min 41 s"
    static func elapsed(_ seconds: TimeInterval) -> String {
        Duration.seconds(seconds.rounded()).formatted(.units(allowed: [.hours, .minutes, .seconds], width: .abbreviated))
    }

    /// An engine error's root cause for a one-line row: the last clause of
    /// "wasm3 m3_FindFunction(sieve) failed: incorrect type on stack" is the
    /// part that differs between rows. The detail view has the whole message.
    static func failureSummary(_ message: String) -> String {
        let line = message.split(separator: "\n", maxSplits: 1).first.map(String.init) ?? message
        if line.hasPrefix("wrong result") { return "Wrong result" }
        if line.hasPrefix("N/A") {
            return line.components(separatedBy: ": ").dropFirst().joined(separator: ": ")
        }
        let cause = line.components(separatedBy: ": ").last(where: { !$0.isEmpty }) ?? line
        return cause.prefix(1).uppercased() + cause.dropFirst()
    }
}
