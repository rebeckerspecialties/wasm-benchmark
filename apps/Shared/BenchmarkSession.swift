// One app session: runs the benchmarks on a background queue, records each
// outcome and keeps the engines ranked by score.
//
// Launched by hand, the app waits for Run and then goes benchmark by
// benchmark across every engine, so all engines have run the same
// benchmarks whenever the leaderboard re-sorts. Launched by the harness
// scripts (scripts/run-device-pass.sh and friends set WORKLOADS, RUNTIMES,
// BENCH_TARGET_MS, ...; BENCH_AUTORUN=1 works too) it starts on its own,
// runs every selected case engine by engine, and writes the console lines
// scripts/summarize-pass.py parses, ending with BENCH_DONE.

import Foundation
import Observation
import os
#if canImport(UIKit)
import UIKit
#endif

// MARK: - Harness settings

/// Settings the harness scripts pass as environment variables.
struct HarnessConfig: Sendable {
    /// WORKLOADS: case-insensitive substrings of the row label to keep.
    var workloads: [String]
    /// RUNTIMES: engine tokens to keep.
    var runtimes: [String]
    /// WORKLOADS_EXCLUDE: row label substrings to leave out.
    var excludes: [String]
    /// FEMTOVG_E2E: scenes to run the femtovg E2E on instead of the rows.
    var femtovgScenes: String?
    /// BENCH_REVEAL: an engine token to expand and scroll to when the run
    /// finishes, so a screenshot shows its rows. Does not start a run.
    var reveal: String?

    private static let triggers = [
        "BENCH_AUTORUN", "WORKLOADS", "RUNTIMES", "WORKLOADS_EXCLUDE", "BENCH_TARGET_MS", "FEMTOVG_E2E",
    ]

    /// nil when no harness variable is set (a launch by hand).
    static func fromEnvironment() -> HarnessConfig? {
        let set = triggers.filter { name in
            let value = environmentValue(name) ?? ""
            return !value.isEmpty && !(name == "BENCH_AUTORUN" && value == "0")
        }
        guard !set.isEmpty else { return nil }
        func list(_ name: String) -> [String] {
            (environmentValue(name) ?? "")
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
                .filter { !$0.isEmpty }
        }
        let scenes = environmentValue("FEMTOVG_E2E") ?? ""
        return HarnessConfig(
            workloads: list("WORKLOADS"),
            runtimes: list("RUNTIMES"),
            excludes: list("WORKLOADS_EXCLUDE"),
            femtovgScenes: scenes.isEmpty ? nil : scenes,
            reveal: list("BENCH_REVEAL").first.map { aliases[$0] ?? $0 }
        )
    }

    private static let aliases = ["m3": "wasm3", "we": "wasmedge", "tinywm": "tinywasm"]

    /// The label is the console row label, "[Pulley] fib(30)".
    func includes(engine: EngineInfo, label: String) -> Bool {
        let lc = label.lowercased()
        let workloadOK = (workloads.isEmpty || workloads.contains { lc.contains($0) })
            && !excludes.contains { lc.contains($0) }
        return workloadOK && includes(engine: engine)
    }

    func includes(engine: EngineInfo) -> Bool {
        runtimes.isEmpty || runtimes.contains { (Self.aliases[$0] ?? $0) == engine.token }
    }
}

/// BENCH_QOS: `.utility` (the default) keeps the run on the efficiency
/// cores, which is what the benchmarks measure; `user-initiated` and
/// `user-interactive` let it onto the performance cores.
func benchmarkQoS() -> DispatchQoS.QoSClass {
    switch environmentValue("BENCH_QOS")?.trimmingCharacters(in: .whitespaces).lowercased() {
    case "user-initiated", "userinitiated", "p": .userInitiated
    case "user-interactive", "userinteractive": .userInteractive
    default: .utility
    }
}

func consoleLog(_ line: String) {
    FileHandle.standardError.write(Data((line + "\n").utf8))
}

// MARK: - Session

@MainActor
@Observable
final class BenchmarkSession {
    enum Phase: Equatable {
        case idle
        case running
        /// Waiting for the app to come back to the foreground.
        case paused
        case finished
        case stopped
    }

    let catalog: Catalog
    let engines: [EngineInfo]
    /// "iPhone11,6 · iOS 18.7"
    let device: String
    let harness: HarnessConfig?

    /// The benchmarks this run lists: the device's suite, or the harness's
    /// selection.
    private(set) var benchmarks: [BenchmarkInfo]
    private(set) var phase: Phase = .idle
    private(set) var outcomes: [ResultKey: Outcome] = [:]
    private(set) var standings: [UInt32: Standing] = [:]
    /// Engines by score, best first; re-sorted after every result.
    private(set) var ranking: [EngineInfo] = []
    private(set) var current: ResultKey?
    private(set) var completed = 0
    private(set) var planned = 0
    private(set) var startedAt: Date?
    private(set) var finishedAt: Date?

    /// Engines that are linked and initialized.
    @ObservationIgnored private let available: Set<UInt32>
    @ObservationIgnored private var control: RunControl?
    @ObservationIgnored private var sceneActive = true
    @ObservationIgnored private var autoran = false

    init() {
        let catalog = Catalog.load()
        self.catalog = catalog
        engines = catalog.engines
        harness = HarnessConfig.fromEnvironment()
        device = DeviceDescription.current
        #if os(watchOS)
        benchmarks = catalog.cases.filter { $0.appExcluded == nil && $0.watchExcluded == nil }
        #else
        benchmarks = catalog.cases.filter { $0.appExcluded == nil }
        #endif
        available = Self.initializeEngines(catalog.engines)
        resetStandings()
    }

    var isRunning: Bool { phase == .running || phase == .paused }

    func isAvailable(_ engine: EngineInfo) -> Bool { available.contains(engine.id) }

    func outcome(_ engine: EngineInfo, _ benchmark: BenchmarkInfo) -> Outcome {
        outcomes[ResultKey(engine: engine.id, benchmark: benchmark.id)] ?? .pending
    }

    func standing(_ engine: EngineInfo) -> Standing { standings[engine.id] ?? Standing() }

    /// 1-based leaderboard position, for engines with a score.
    func rank(of engine: EngineInfo) -> Int? {
        guard standing(engine).score != nil else { return nil }
        return ranking.firstIndex(of: engine).map { $0 + 1 }
    }

    func engine(id: UInt32) -> EngineInfo? { engines.first { $0.id == id } }
    func benchmark(id: String) -> BenchmarkInfo? { catalog.cases.first { $0.id == id } }

    // MARK: Control

    func toggle() {
        if isRunning { stop() } else { start() }
    }

    /// Runs the device's suite, benchmark by benchmark across the engines.
    func start() {
        guard !isRunning else { return }
        #if os(watchOS)
        benchmarks = catalog.cases.filter { $0.appExcluded == nil && $0.watchExcluded == nil }
        #else
        benchmarks = catalog.cases.filter { $0.appExcluded == nil }
        #endif
        var plan: [PlanItem] = []
        for benchmark in benchmarks {
            for engine in engines where isAvailable(engine) {
                plan.append(PlanItem(
                    engine: engine,
                    benchmark: benchmark,
                    skip: catalog.skipReason(engine: engine, benchmark: benchmark)
                ))
            }
        }
        begin(plan, mode: .interactive)
    }

    func stop() {
        control?.cancel()
    }

    /// Starts the harness's run once, when the harness launched the app.
    func autorunIfRequested() {
        guard let harness, !autoran else { return }
        autoran = true
        #if os(iOS) || os(macOS)
        if let scenes = harness.femtovgScenes {
            runFemtovgE2E(scenes: scenes)
            return
        }
        #endif
        if !harness.workloads.isEmpty || !harness.runtimes.isEmpty {
            let w = harness.workloads.isEmpty ? "(any)" : harness.workloads.joined(separator: ", ")
            let r = harness.runtimes.isEmpty ? "(any)" : harness.runtimes.joined(separator: ", ")
            consoleLog("WORKLOADS filter: \(w); RUNTIMES filter: \(r)")
        }
        // Every case, including the ones the app's own suite leaves out: the
        // harness chooses with WORKLOADS / WORKLOADS_EXCLUDE.
        var plan: [PlanItem] = []
        for engine in engines {
            for benchmark in catalog.cases
            where harness.includes(engine: engine, label: "\(engine.prefix) \(benchmark.label)") {
                plan.append(PlanItem(engine: engine, benchmark: benchmark, skip: nil))
            }
        }
        let listed = Set(plan.map(\.benchmark.id))
        benchmarks = catalog.cases.filter { listed.contains($0.id) }
        begin(plan, mode: .harness)
    }

    /// Pauses a hand-started run while the app is not in the foreground and
    /// measures the interrupted benchmark again when it returns.
    func sceneActivityChanged(active: Bool) {
        sceneActive = active
        control?.setActive(active)
    }

    // MARK: Running

    private func begin(_ plan: [PlanItem], mode: Runner.Mode) {
        outcomes = [:]
        for item in plan {
            outcomes[ResultKey(engine: item.engine.id, benchmark: item.benchmark.id)] = .pending
        }
        completed = 0
        planned = plan.count
        current = nil
        startedAt = .now
        finishedAt = nil
        phase = .running
        resetStandings()
        setIdleTimerDisabled(true)

        let control = RunControl(active: mode == .harness || sceneActive)
        self.control = control
        let session = self
        let post: @Sendable (RunEvent) -> Void = { event in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { session.apply(event) }
            }
        }
        let queue = DispatchQueue(label: "wasmbench.runner", qos: DispatchQoS(qosClass: benchmarkQoS(), relativePriority: 0))
        queue.async {
            Runner.execute(plan, mode: mode, control: control, post: post)
        }
    }

    private func apply(_ event: RunEvent) {
        switch event {
        case .started(let key):
            current = key
            outcomes[key] = .running
        case .finished(let key, let outcome):
            outcomes[key] = outcome
            completed += 1
            if current == key { current = nil }
            updateStanding(engineID: key.engine)
            rerank()
        case .paused(let paused):
            if isRunning { phase = paused ? .paused : .running }
        case .done(let cancelled):
            finish(cancelled: cancelled)
        }
    }

    private func finish(cancelled: Bool) {
        phase = cancelled ? .stopped : .finished
        current = nil
        finishedAt = .now
        control = nil
        setIdleTimerDisabled(false)
        for (key, outcome) in outcomes where outcome == .running || outcome == .pending {
            outcomes[key] = .pending
        }
        if harness != nil {
            consoleLog(winnerSummary())
            // The launchers wait for this line and then terminate the app,
            // which does not exit by itself.
            consoleLog("BENCH_DONE")
        }
    }

    private func resetStandings() {
        standings = [:]
        for engine in engines { updateStanding(engineID: engine.id) }
        rerank()
    }

    private func updateStanding(engineID: UInt32) {
        var standing = Standing()
        var scores: [Double] = []
        for benchmark in benchmarks {
            let key = ResultKey(engine: engineID, benchmark: benchmark.id)
            guard let outcome = outcomes[key] else { continue }
            standing.total += 1
            switch outcome {
            case .measured(let m):
                standing.finished += 1
                if let score = Scoring.score(m, reference: benchmark.referenceNs) {
                    scores.append(score)
                    standing.scored += 1
                }
            case .failed:
                standing.finished += 1
                standing.failed += 1
                scores.append(Scoring.failurePenalty)
            case .skipped:
                // Only the device's memory stops a benchmark the app attempts;
                // that is not the engine's failure.
                standing.finished += 1
            case .pending, .running:
                break
            }
        }
        if outcomes.isEmpty, available.contains(engineID) { standing.total = benchmarks.count }
        standing.score = Scoring.mean(scores)
        standings[engineID] = standing
    }

    /// Best score first; engines without a score keep catalog order after
    /// them, and engines this build cannot run come last.
    private func rerank() {
        let order = Dictionary(uniqueKeysWithValues: engines.enumerated().map { ($1.id, $0) })
        ranking = engines.sorted { a, b in
            let (sa, sb) = (standings[a.id]?.score, standings[b.id]?.score)
            if let sa, let sb, sa != sb { return sa > sb }
            if (sa != nil) != (sb != nil) { return sa != nil }
            if isAvailable(a) != isAvailable(b) { return isAvailable(a) }
            return order[a.id, default: 0] < order[b.id, default: 0]
        }
    }

    private func setIdleTimerDisabled(_ disabled: Bool) {
        // Keep the screen on: auto-lock would suspend the app mid-run (a
        // devicectl launch into an awake device does not reset the idle timer).
        #if os(iOS) || os(tvOS)
        UIApplication.shared.isIdleTimerDisabled = disabled
        #endif
    }

    // MARK: Summaries

    /// Plain-text leaderboard for sharing.
    var summaryText: String {
        var lines = ["WasmBench — \(device)"]
        for engine in ranking where isAvailable(engine) {
            let standing = standing(engine)
            let score = standing.score.map(Format.score) ?? "—"
            let rank = rank(of: engine).map { "\($0). " } ?? "   "
            let failed = standing.failed > 0 ? ", \(standing.failed) failed" : ""
            lines.append("\(rank)\(engine.name) (\(engine.versionLine)): \(score) — \(standing.scored) of \(standing.total) benchmarks\(failed)")
        }
        lines.append("Score: 100 = the typical engine on an iPhone XS, \(Format.score(Scoring.failurePenalty)) for a benchmark the engine cannot run; higher is better.")
        return lines.joined(separator: "\n")
    }

    /// The harness's one-line verdict: which engine had the lowest median on
    /// the most benchmarks that at least two engines completed (medians
    /// within 1 % split the win).
    private func winnerSummary() -> String {
        var byBenchmark: [String: [String: UInt64]] = [:]
        for (key, outcome) in outcomes {
            guard case .measured(let m) = outcome, let engine = engine(id: key.engine) else { continue }
            byBenchmark[key.benchmark, default: [:]][engine.token] = m.medianNs
        }
        var wins: [String: Double] = [:]
        var comparable = 0
        for medians in byBenchmark.values where medians.count >= 2 {
            comparable += 1
            let best = Double(medians.values.min()!) * 1.01
            let top = medians.filter { Double($0.value) <= best }
            for token in top.keys { wins[token, default: 0] += 1 / Double(top.count) }
        }
        guard comparable > 0 else {
            return "No comparable workloads yet (need at least one workload completed on ≥2 runtimes)."
        }
        let ranked = wins.sorted { $0.value > $1.value }
        func fmt(_ x: Double) -> String { x == x.rounded() ? String(Int(x)) : String(format: "%.1f", x) }
        let first = ranked[0]
        if ranked.count >= 2, first.value - ranked[1].value <= 1 {
            let second = ranked[1]
            return "🤝 tie: \(first.key) & \(second.key) both ≈ \(fmt(max(first.value, second.value))) / \(comparable) workloads"
        }
        return "🏆 \(first.key) wins \(fmt(first.value)) / \(comparable) workloads"
    }

    // MARK: Engines

    /// Runs every engine's init on the main thread (WAMR's stack-guard setup
    /// must happen there before any worker thread loads a module) and logs
    /// the lines the harness logs have always started with.
    private static func initializeEngines(_ engines: [EngineInfo]) -> Set<UInt32> {
        let inits: [(UInt32, String, () -> UInt8)] = [
            (1, "wamr", bench_init_wamr),
            (2, "wasm3", bench_init_wasm3),
            (3, "wasmedge", bench_init_wasmedge),
            (4, "zwasm", bench_init_zwasm),
            (5, "wasmz", bench_init_wasmz),
            (6, "tinywasm", bench_init_tinywasm),
        ]
        var ready: Set<UInt32> = [0]  // Pulley has no process-global state
        for (id, name, initialize) in inits {
            let ok = initialize() == 1
            if ok { ready.insert(id) }
            consoleLog("\(name) init: \(ok ? "ok" : "unavailable")")
        }
        let pac = bench_pac_probe()
        consoleLog(String(
            format: "pac probe: supported=%d code1=0x%016llx code2=0x%016llx code3=0x%016llx | nonzero=%d deterministic=%d input_dep=%d low_zero=%d",
            pac.supported, pac.code1, pac.code2, pac.code3,
            pac.nonzero, pac.deterministic, pac.input_dep, pac.low_zero
        ))
        let linked = Set(engines.filter(\.linked).map(\.id))
        return ready.intersection(linked)
    }

    // MARK: femtovg E2E

    #if os(iOS) || os(macOS)
    /// FEMTOVG_E2E=0,1 runs the femtovg E2E (docs/femtovg-e2e-abi.md) on
    /// every scene listed, on each engine in RUNTIMES (all if unset), with
    /// FEMTOVG_FRAMES / FEMTOVG_PASSES frames and passes (121 / 2). Each
    /// result is one `FEMTOVG_E2E {json}` console line.
    private func runFemtovgE2E(scenes: String) {
        let frames = UInt32(environmentValue("FEMTOVG_FRAMES") ?? "") ?? 121
        let passes = UInt32(environmentValue("FEMTOVG_PASSES") ?? "") ?? 2
        let selected = engines.filter { harness?.includes(engine: $0) ?? true }
        let runtimes = selected.map(\.id)
        let sceneIDs = scenes.split(separator: ",").compactMap { UInt32($0.trimmingCharacters(in: .whitespaces)) }
        phase = .running
        startedAt = .now
        setIdleTimerDisabled(true)
        let session = self
        DispatchQueue(label: "wasmbench.e2e", qos: DispatchQoS(qosClass: benchmarkQoS(), relativePriority: 0)).async {
            for runtime in runtimes {
                for scene in sceneIDs {
                    guard let line = bench_femtovg_e2e(runtime, scene, frames, passes) else { continue }
                    consoleLog("FEMTOVG_E2E " + String(cString: line))
                    bench_free_cstring(line)
                }
            }
            consoleLog("FEMTOVG_E2E done")
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    session.phase = .finished
                    session.finishedAt = .now
                    session.setIdleTimerDisabled(false)
                }
            }
        }
    }
    #endif
}

// MARK: - Runner (background queue)

struct PlanItem: Sendable {
    let engine: EngineInfo
    let benchmark: BenchmarkInfo
    /// Set when the app leaves this row out.
    let skip: String?
}

enum RunEvent: Sendable {
    case started(ResultKey)
    case finished(ResultKey, Outcome)
    case paused(Bool)
    case done(cancelled: Bool)
}

/// Cancellation and foreground state, shared with the runner's queue.
final class RunControl: Sendable {
    private struct State {
        var cancelled = false
        var active: Bool
        /// The app left the foreground since the current measurement began.
        var interrupted = false
    }

    private let state: OSAllocatedUnfairLock<State>

    init(active: Bool) {
        state = OSAllocatedUnfairLock(initialState: State(active: active))
    }

    func cancel() { state.withLock { $0.cancelled = true } }
    var isCancelled: Bool { state.withLock { $0.cancelled } }
    var isActive: Bool { state.withLock { $0.active } }
    var wasInterrupted: Bool { state.withLock { $0.interrupted } }

    func setActive(_ active: Bool) {
        state.withLock {
            $0.active = active
            if !active { $0.interrupted = true }
        }
    }

    func beginMeasurement() {
        state.withLock { $0.interrupted = !$0.active }
    }
}

enum Runner {
    enum Mode: Sendable {
        /// Started with Run: pauses in the background, skips memory-heavy rows.
        case interactive
        /// Started by the harness: runs exactly the rows it selected.
        case harness
    }

    #if os(watchOS)
    /// Rows start only while this much memory is left before the jetsam limit.
    private static let memoryFloor: UInt64 = 48 << 20
    #elseif !os(macOS)
    private static let memoryFloor: UInt64 = 192 << 20
    #endif

    static func execute(
        _ plan: [PlanItem],
        mode: Mode,
        control: RunControl,
        post: @Sendable (RunEvent) -> Void
    ) {
        var cancelled = false
        for item in plan {
            if control.isCancelled {
                cancelled = true
                break
            }
            let key = ResultKey(engine: item.engine.id, benchmark: item.benchmark.id)
            let label = "\(item.engine.prefix) \(item.benchmark.label)"
            // A known engine failure the app does not attempt scores like
            // any other failure.
            if let skip = item.skip {
                post(.finished(key, .failed("N/A — not run: \(skip)")))
                continue
            }
            if mode == .interactive {
                waitUntilActive(control, post: post)
                #if !os(macOS)
                // 0 where there is no jetsam limit (the simulator).
                let left = UInt64(os_proc_available_memory())
                if left > 0, left < memoryFloor {
                    post(.finished(key, .skipped("Only \(Format.bytes(left)) of memory was left for the app")))
                    continue
                }
                #endif
            }
            var outcome: Outcome
            var attempts = 0
            while true {
                attempts += 1
                control.beginMeasurement()
                post(.started(key))
                outcome = measure(item, label: label)
                // A measurement the app was suspended in is not a result.
                guard mode == .interactive, control.wasInterrupted, !control.isCancelled, attempts < 3 else { break }
                waitUntilActive(control, post: post)
            }
            post(.finished(key, outcome))
        }
        post(.done(cancelled: cancelled || control.isCancelled))
    }

    private static func waitUntilActive(_ control: RunControl, post: @Sendable (RunEvent) -> Void) {
        guard !control.isActive else { return }
        post(.paused(true))
        while !control.isActive && !control.isCancelled {
            Thread.sleep(forTimeInterval: 0.25)
        }
        post(.paused(false))
    }

    private static func measure(_ item: PlanItem, label: String) -> Outcome {
        var report = item.benchmark.id.withCString { bench_run_case(item.engine.id, $0) }
        let rendered = render(&report)
        consoleLog("[\(label)] " + rendered.replacingOccurrences(of: "\n", with: " | "))
        if report.ok == 1 {
            return .measured(Measurement(report))
        }
        return .failed(rendered.replacingOccurrences(of: "ERROR: ", with: ""))
    }

    /// The console line scripts/summarize-pass.py parses (LOG_RE). Frees and
    /// clears `error_msg`.
    private static func render(_ report: inout BenchReport) -> String {
        if report.ok != 1 {
            var message = "(no message)"
            if let cstr = report.error_msg {
                message = String(cString: cstr)
                bench_free_error_msg(cstr)
                report.error_msg = nil
            }
            return "ERROR: \(message)"
        }
        let ms = { (ns: UInt64) in Double(ns) / 1_000_000.0 }
        let cpu = Double(report.cpu_user_ns + report.cpu_system_ns)
        let eShare = cpu > 0 ? 1.0 - min(Double(report.p_cpu_ns) / cpu, 1.0) : -1.0
        let ipc = report.cycles > 0 ? Double(report.instructions) / Double(report.cycles) : -1.0
        return String(
            format: "result=%d  iter=%u  load=%.3fms  min=%.3f median=%.3f p99=%.3f ms  cpu(u/s)=%.2f/%.2f ms  rss=%.0fKB  faults=%llu  e_share=%.3f  ipc=%.2f  insns=%llu  cycles=%llu",
            report.result, report.iterations,
            ms(report.load_ns), ms(report.run_ns_min), ms(report.run_ns_median), ms(report.run_ns_p99),
            ms(report.cpu_user_ns), ms(report.cpu_system_ns), Double(report.rss_peak_bytes) / 1024.0,
            report.page_faults, eShare, ipc, report.instructions, report.cycles
        )
    }
}

// MARK: - Device

enum DeviceDescription {
    /// Model identifier and OS: "iPhone11,6 · iOS 18.7".
    @MainActor static var current: String {
        let version = ProcessInfo.processInfo.operatingSystemVersion
        return "\(model) · \(osName) \(version.majorVersion).\(version.minorVersion)"
    }

    private static var model: String {
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] {
            return "\(simulated) Simulator"
        }
        #if os(macOS)
        var size = 0
        sysctlbyname("hw.model", nil, &size, nil, 0)
        var bytes = [CChar](repeating: 0, count: max(size, 1))
        sysctlbyname("hw.model", &bytes, &size, nil, 0)
        return String(decoding: bytes.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, as: UTF8.self)
        #else
        var info = utsname()
        uname(&info)
        return withUnsafeBytes(of: info.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
        #endif
    }

    @MainActor private static var osName: String {
        #if os(visionOS)
        "visionOS"
        #elseif os(iOS)
        UIDevice.current.userInterfaceIdiom == .pad ? "iPadOS" : "iOS"
        #elseif os(tvOS)
        "tvOS"
        #elseif os(watchOS)
        "watchOS"
        #else
        "macOS"
        #endif
    }
}
