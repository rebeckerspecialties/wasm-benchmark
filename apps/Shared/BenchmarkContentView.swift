// Shared SwiftUI view used by all three app targets (watchOS, iOS, macOS).
// Each target's project.yml entry includes this file via the
// `apps/Shared` source path.
//
// The view runs every workload defined by `WORKLOADS` once on appear,
// streams a per-workload report to stderr (visible via simctl /
// xcodebuild console), and renders a list summary on screen.

import SwiftUI

/// Static catalog of workloads exposed by benchmark-core's C ABI.
/// Each entry is `(human label, FFI runner, default input)`.
struct Workload: Identifiable, Sendable {
    let id: Int
    let label: String
    /// Closure that invokes the C entry point and returns a `BenchReport`.
    /// Marked `@Sendable` so the catalog itself is `Sendable` — the
    /// runner closure is invoked from a background queue.
    let run: @Sendable () -> BenchReport
}

// Each workload appears twice — once for each runtime — so the device
// run produces a side-by-side comparison.
let WORKLOADS: [Workload] = [
    Workload(id:  0, label: "[Pulley] fib(30)",                        run: { bench_run_fib(30) }),
    Workload(id:  1, label: "[ WAMR ] fib(30)",                        run: { bench_run_fib_wamr(30) }),
    Workload(id:  2, label: "[Pulley] fib_tail(100000) [return_call]", run: { bench_run_fib_tail(100000) }),
    Workload(id:  3, label: "[ WAMR ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wamr(100000) }),
    Workload(id:  4, label: "[Pulley] factorial(20)",                  run: { bench_run_factorial(20) }),
    Workload(id:  5, label: "[ WAMR ] factorial(20)",                  run: { bench_run_factorial_wamr(20) }),
    Workload(id:  6, label: "[Pulley] sieve(10000)",                   run: { bench_run_sieve(10000) }),
    Workload(id:  7, label: "[ WAMR ] sieve(10000)",                   run: { bench_run_sieve_wamr(10000) }),
    Workload(id:  8, label: "[Pulley] crc32(64KB)",                    run: { bench_run_crc32() }),
    Workload(id:  9, label: "[ WAMR ] crc32(64KB)",                    run: { bench_run_crc32_wamr() }),
    Workload(id: 10, label: "[Pulley] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd() }),
    Workload(id: 11, label: "[ WAMR ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wamr() }),
    Workload(id: 12, label: "[Pulley] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma() }),
    Workload(id: 13, label: "[ WAMR ] matmul relaxed-simd FMA (no-op)",run: { bench_run_matmul_fma_wamr() }),
    Workload(id: 14, label: "[Pulley] convolution 256×256",            run: { bench_run_convolution() }),
    Workload(id: 15, label: "[ WAMR ] convolution 256×256",            run: { bench_run_convolution_wamr() }),
    Workload(id: 16, label: "[Pulley] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp() }),
    Workload(id: 17, label: "[ WAMR ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wamr() }),
    Workload(id: 18, label: "[Pulley] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory() }),
    Workload(id: 19, label: "[ WAMR ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wamr() }),
    Workload(id: 20, label: "[Pulley] call_indirect (200K dispatches)", run: { bench_run_call_indirect() }),
    Workload(id: 21, label: "[ WAMR ] call_indirect (200K dispatches)", run: { bench_run_call_indirect_wamr() }),
    // sqlite3 speedtest1 — single-shot, no WAMR comparison until
    // libiwasm.a is rebuilt with WAMR_BUILD_LIBC_WASI=1.
    Workload(id: 22, label: "[Pulley] sqlite3 speedtest1 (in-mem)", run: { bench_run_sqlite3() }),
    // Hand-written graphql-js validation-shape benchmarks. Two compilers,
    // same workload, very different `call_indirect` density:
    //   AS port (61 KB, 13 call_indirect): optimizer-friendly baseline
    //   Porffor port (121 KB, 98 call_indirect): preserves megamorphic
    //                                            dispatch shape — primary
    //                                            target for our optimization
    //                                            work.
    Workload(id: 23, label: "[Pulley] graphql-validation (AS)",      run: { bench_run_graphql_validation_as() }),
    Workload(id: 24, label: "[Pulley] graphql-validation (Porffor)", run: { bench_run_graphql_validation_porf() }),
    // xmrsplayer rendering 15 s of unreal.s3m (Scream Tracker 3 module)
    // through 31 call_indirect sites. Real-world dispatch-shaped workload
    // alongside the synthetic call_indirect.wasm and the JS-on-wasm
    // graphql-validation cases. Per-call wallclock is dominated by 15 s
    // of synthesis, so the harness usually settles on 1 iteration on
    // weak cores and a handful on M-class.
    Workload(id: 25, label: "[Pulley] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer() }),
    Workload(id: 26, label: "[ WAMR ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wamr() }),
    // C++-style vtable dispatch (StarlingMonkey-shaped pure-virtual
    // hierarchy). Four entry points sweep the IC's polymorphism
    // dimension — mono is the best case for a 1-way IC, bi/poly4/
    // poly6 test progressively worse polymorphism. Pulley-only
    // since the IC question is Pulley-specific.
    Workload(id: 27, label: "[Pulley] vtable_mono (200K)",  run: { bench_run_vtable_mono() }),
    Workload(id: 28, label: "[Pulley] vtable_bi (200K)",    run: { bench_run_vtable_bi() }),
    Workload(id: 29, label: "[Pulley] vtable_poly4 (200K)", run: { bench_run_vtable_poly4() }),
    Workload(id: 30, label: "[Pulley] vtable_poly6 (200K)", run: { bench_run_vtable_poly6() }),
    // WAMR variants for everything the wasm side can support. graphql-
    // validation Porffor on WAMR may fail at load (Porffor uses wasm
    // exceptions; our WAMR build has WAMR_BUILD_EXCE_HANDLING=0); the
    // harness reports the error string from wasm_runtime_get_exception
    // and the row is shown as ERROR. Treat that as data: "WAMR
    // can't run this shape with this build" is the cross-runtime
    // signal we want.
    Workload(id: 31, label: "[ WAMR ] graphql-validation (AS)",      run: { bench_run_graphql_validation_as_wamr() }),
    Workload(id: 32, label: "[ WAMR ] graphql-validation (Porffor)", run: { bench_run_graphql_validation_porf_wamr() }),
    Workload(id: 33, label: "[ WAMR ] vtable_mono (200K)",  run: { bench_run_vtable_mono_wamr() }),
    Workload(id: 34, label: "[ WAMR ] vtable_bi (200K)",    run: { bench_run_vtable_bi_wamr() }),
    Workload(id: 35, label: "[ WAMR ] vtable_poly4 (200K)", run: { bench_run_vtable_poly4_wamr() }),
    Workload(id: 36, label: "[ WAMR ] vtable_poly6 (200K)", run: { bench_run_vtable_poly6_wamr() }),
    // wasm3 (pure C interpreter) variants. wasm3 doesn't implement
    // SIMD, wasm exceptions, or WASI, so matmul_simd / matmul_fma /
    // graphql-validation Porffor will fail at load — the row reports
    // ERROR with wasm3's error string. xmrsplayer uses `return_call`,
    // which wasm3 *does* implement, so it should run (subject to the
    // 256 KiB wasm3 stack budget; see crates/benchmark-core/src/wasm3.rs).
    Workload(id: 37, label: "[wasm3 ] fib(30)",                        run: { bench_run_fib_wasm3(30) }),
    Workload(id: 38, label: "[wasm3 ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wasm3(100000) }),
    Workload(id: 39, label: "[wasm3 ] factorial(20)",                  run: { bench_run_factorial_wasm3(20) }),
    Workload(id: 40, label: "[wasm3 ] sieve(10000)",                   run: { bench_run_sieve_wasm3(10000) }),
    Workload(id: 41, label: "[wasm3 ] crc32(64KB)",                    run: { bench_run_crc32_wasm3() }),
    Workload(id: 42, label: "[wasm3 ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wasm3() }),
    Workload(id: 43, label: "[wasm3 ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_wasm3() }),
    Workload(id: 44, label: "[wasm3 ] convolution 256×256",            run: { bench_run_convolution_wasm3() }),
    Workload(id: 45, label: "[wasm3 ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wasm3() }),
    Workload(id: 46, label: "[wasm3 ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wasm3() }),
    Workload(id: 47, label: "[wasm3 ] call_indirect (200K dispatches)",run: { bench_run_call_indirect_wasm3() }),
    Workload(id: 48, label: "[wasm3 ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wasm3() }),
    Workload(id: 49, label: "[wasm3 ] graphql-validation (AS)",        run: { bench_run_graphql_validation_as_wasm3() }),
    Workload(id: 50, label: "[wasm3 ] graphql-validation (Porffor)",   run: { bench_run_graphql_validation_porf_wasm3() }),
    Workload(id: 51, label: "[wasm3 ] vtable_mono (200K)",             run: { bench_run_vtable_mono_wasm3() }),
    Workload(id: 52, label: "[wasm3 ] vtable_bi (200K)",               run: { bench_run_vtable_bi_wasm3() }),
    Workload(id: 53, label: "[wasm3 ] vtable_poly4 (200K)",            run: { bench_run_vtable_poly4_wasm3() }),
    Workload(id: 54, label: "[wasm3 ] vtable_poly6 (200K)",            run: { bench_run_vtable_poly6_wasm3() }),
    // WasmEdge variants. WasmEdge is the incumbent production runtime
    // (the WatchOS audio app ships it). Built with
    // WASMEDGE_USE_LLVM=OFF + the 27-patch Apple-mobile enablement
    // stack — pure-interpreter, App-Store-eligible. SIMD + wasm-
    // exceptions are both enabled in the same build (WAMR can't do
    // this), so graphql-validation Porffor loads successfully on this
    // path (it traps at run-time on the missing host import — same
    // shape as Pulley would without the host stub).
    Workload(id: 55, label: "[WE    ] fib(30)",                        run: { bench_run_fib_wasmedge(30) }),
    Workload(id: 56, label: "[WE    ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wasmedge(100000) }),
    Workload(id: 57, label: "[WE    ] factorial(20)",                  run: { bench_run_factorial_wasmedge(20) }),
    Workload(id: 58, label: "[WE    ] sieve(10000)",                   run: { bench_run_sieve_wasmedge(10000) }),
    Workload(id: 59, label: "[WE    ] crc32(64KB)",                    run: { bench_run_crc32_wasmedge() }),
    Workload(id: 60, label: "[WE    ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wasmedge() }),
    Workload(id: 61, label: "[WE    ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_wasmedge() }),
    Workload(id: 62, label: "[WE    ] convolution 256×256",            run: { bench_run_convolution_wasmedge() }),
    Workload(id: 63, label: "[WE    ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wasmedge() }),
    Workload(id: 64, label: "[WE    ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wasmedge() }),
    Workload(id: 65, label: "[WE    ] call_indirect (200K dispatches)",run: { bench_run_call_indirect_wasmedge() }),
    Workload(id: 66, label: "[WE    ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wasmedge() }),
    Workload(id: 67, label: "[WE    ] graphql-validation (AS)",        run: { bench_run_graphql_validation_as_wasmedge() }),
    Workload(id: 68, label: "[WE    ] graphql-validation (Porffor)",   run: { bench_run_graphql_validation_porf_wasmedge() }),
    Workload(id: 69, label: "[WE    ] vtable_mono (200K)",             run: { bench_run_vtable_mono_wasmedge() }),
    Workload(id: 70, label: "[WE    ] vtable_bi (200K)",               run: { bench_run_vtable_bi_wasmedge() }),
    Workload(id: 71, label: "[WE    ] vtable_poly4 (200K)",            run: { bench_run_vtable_poly4_wasmedge() }),
    Workload(id: 72, label: "[WE    ] vtable_poly6 (200K)",            run: { bench_run_vtable_poly6_wasmedge() }),
]

struct WorkloadResult: Identifiable {
    let id: Int
    let label: String
    let report: BenchReport
    let errorText: String?
}

struct BenchmarkContentView: View {
    @State private var results: [WorkloadResult] = []
    @State private var running: Bool = false
    @State private var currentLabel: String = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Pulley vs WAMR vs wasm3 vs WasmEdge")
                    .font(.title3.bold())
                Text("workload set • \(WORKLOADS.count) cases")
                    .font(.caption)
                if running {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.small)
                        Text("running \(currentLabel)…")
                    }
                    .font(.caption)
                }
                Button(running ? "Running…" : "Run all") { runAll() }
                    .disabled(running)
                ForEach(results) { r in
                    WorkloadRow(result: r)
                }
            }
            .padding()
        }
        .onAppear { runAll() }
    }

    private func runAll() {
        guard !running else { return }
        running = true
        results = []
        // WAMR's stack-guard setup must run on the main thread before
        // any worker thread tries to load a wasm module. The actual
        // runtime call into wasm_runtime_init() returns 0 if the build
        // didn't link in libiwasm.a (e.g. older device libs).
        let wamrOk = bench_init_wamr() == 1
        FileHandle.standardError.write(Data("wamr init: \(wamrOk ? "ok" : "unavailable")\n".utf8))
        // wasm3 has no process-global state, but we call its init
        // symmetrically so all three runtimes' availability is logged
        // up-front in the same line shape.
        let wasm3Ok = bench_init_wasm3() == 1
        FileHandle.standardError.write(Data("wasm3 init: \(wasm3Ok ? "ok" : "unavailable")\n".utf8))
        // WasmEdge — same shape; reports "unavailable" if libwasmedge.a
        // wasn't linked in (e.g. host-only build before
        // scripts/build-wasmedge.sh has run for this target).
        let wasmedgeOk = bench_init_wasmedge() == 1
        FileHandle.standardError.write(Data("wasmedge init: \(wasmedgeOk ? "ok" : "unavailable")\n".utf8))
        // One-shot PAC viability probe. Useful as a planning input for
        // the future PAC-signed IC slot scheme; not a benchmark.
        let pac = bench_pac_probe()
        FileHandle.standardError.write(Data(String(
            format: "pac probe: supported=%d code1=0x%016llx code2=0x%016llx code3=0x%016llx | nonzero=%d deterministic=%d input_dep=%d low_zero=%d\n",
            pac.supported, pac.code1, pac.code2, pac.code3,
            pac.nonzero, pac.deterministic, pac.input_dep, pac.low_zero
        ).utf8))
        // Optional `WORKLOADS` env-var filter (comma-separated, case-
        // insensitive substring match against the workload label).
        // Optional `RUNTIMES` env-var filter (comma-separated; valid
        // values are `pulley`, `wamr`, `wasm3`) to keep only the
        // matching runtime — useful for PMU traces where you want to
        // isolate signal from one runtime without the others'
        // identical-across-builds dispatch overhead diluting the trace
        // aggregate. Without filters, every workload runs on every
        // runtime that supports it.
        let workloads: [Workload] = {
            // watchOS doesn't propagate `devicectl --environment-variables`
            // to ProcessInfo (verified empirically — iOS does, watchOS
            // doesn't). For PR-time targeted runs on the watch, edit
            // WATCHOS_WORKLOADS_FILTER below to a comma-separated needle
            // list ("xmrsplayer") or empty string ("") for "all". iOS /
            // macOS continue to read the env vars, so the iPhone /
            // M-series runner is unaffected.
            #if os(watchOS)
            let WATCHOS_WORKLOADS_FILTER = "call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6,graphql-validation"
            let WATCHOS_RUNTIMES_FILTER = ""
            let env = WATCHOS_WORKLOADS_FILTER
            let runtimesEnv = WATCHOS_RUNTIMES_FILTER
            #else
            let env = ProcessInfo.processInfo.environment["WORKLOADS"] ?? ""
            let runtimesEnv = ProcessInfo.processInfo.environment["RUNTIMES"] ?? ""
            #endif
            let trimmedW = env.trimmingCharacters(in: .whitespaces)
            let trimmedR = runtimesEnv.trimmingCharacters(in: .whitespaces)
            let needles = trimmedW
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
                .filter { !$0.isEmpty }
            let runtimes = trimmedR
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
                .filter { !$0.isEmpty }
            if !needles.isEmpty || !runtimes.isEmpty {
                let joinedW = needles.isEmpty ? "(any)" : needles.joined(separator: ", ")
                let joinedR = runtimes.isEmpty ? "(any)" : runtimes.joined(separator: ", ")
                FileHandle.standardError.write(
                    Data("WORKLOADS filter: \(joinedW); RUNTIMES filter: \(joinedR)\n".utf8)
                )
            }
            return WORKLOADS.filter { w in
                let lc = w.label.lowercased()
                let workloadOk = needles.isEmpty
                    || needles.contains(where: { lc.contains($0) })
                let runtimeOk = runtimes.isEmpty
                    || runtimes.contains(where: { rt in
                        // Labels look like `[Pulley] call_indirect ...`
                        // or `[ WAMR ] call_indirect ...` or
                        // `[wasm3 ] call_indirect ...`. Case-insensitive
                        // substring on the prefix is unambiguous.
                        switch rt {
                        case "pulley":
                            return lc.contains("[pulley]")
                        case "wamr":
                            return lc.contains("[ wamr ]")
                        case "wasm3", "m3":
                            return lc.contains("[wasm3 ]")
                        case "wasmedge", "we":
                            return lc.contains("[we    ]")
                        default:
                            return false
                        }
                    })
                return workloadOk && runtimeOk
            }
        }()
        // .utility QoS pins worker scheduling to efficiency cores on
        // Apple Silicon (P-cores are reserved for .userInitiated+).
        // Matches our M4 E-core measurement methodology (`taskpolicy -b`)
        // so iPhone XS / SE2 numbers are directly comparable to M4 E-core
        // numbers. .background was tried first but iOS may suspend
        // .background work aggressively even with the app foregrounded;
        // .utility is the lowest QoS that keeps the worker running
        // continuously while still preferring E-cores.
        let qosOverride = ProcessInfo.processInfo.environment["BENCH_QOS"]?
            .trimmingCharacters(in: .whitespaces).lowercased() ?? ""
        let chosenQoS: DispatchQoS.QoSClass
        switch qosOverride {
        case "user-initiated", "userinitiated", "p":
            chosenQoS = .userInitiated
        case "user-interactive", "userinteractive":
            chosenQoS = .userInteractive
        default:
            chosenQoS = .utility
        }
        DispatchQueue.global(qos: chosenQoS).async {
            for w in workloads {
                DispatchQueue.main.async { currentLabel = w.label }
                var report = w.run()
                let rendered = formatReport(&report)
                FileHandle.standardError.write(
                    Data(("[\(w.label)] " + rendered.replacingOccurrences(of: "\n", with: " | ") + "\n").utf8)
                )
                let errText: String? = report.ok != 1
                    ? rendered.replacingOccurrences(of: "ERROR: ", with: "")
                    : nil
                let result = WorkloadResult(id: w.id, label: w.label, report: report, errorText: errText)
                DispatchQueue.main.async { results.append(result) }
            }
            DispatchQueue.main.async {
                running = false
                currentLabel = ""
            }
        }
    }

}

// Free function (no `self`) — safe to call from a background queue under
// Swift 6 strict concurrency. Frees and nils `error_msg` so the report
// can be stored without a dangling C pointer.
fileprivate func formatReport(_ report: inout BenchReport) -> String {
        if report.ok != 1 {
            let msg: String
            if let cstr = report.error_msg {
                msg = String(cString: cstr)
                bench_free_error_msg(report.error_msg)
                report.error_msg = nil
            } else {
                msg = "(no message)"
            }
            return "ERROR: \(msg)"
        }
        let loadMs = Double(report.load_ns) / 1_000_000.0
        let minMs = Double(report.run_ns_min) / 1_000_000.0
        let medMs = Double(report.run_ns_median) / 1_000_000.0
        let p99Ms = Double(report.run_ns_p99) / 1_000_000.0
        let userMs = Double(report.cpu_user_ns) / 1_000_000.0
        let sysMs = Double(report.cpu_system_ns) / 1_000_000.0
        let rssKB = Double(report.rss_peak_bytes) / 1024.0
        return String(
            format: "result=%d  iter=%u  load=%.3fms  min=%.3f median=%.3f p99=%.3f ms  cpu(u/s)=%.2f/%.2f ms  rss=%.0fKB  faults=%llu",
            report.result, report.iterations,
            loadMs, minMs, medMs, p99Ms,
            userMs, sysMs, rssKB, report.page_faults
        )
}

struct WorkloadRow: View {
    let result: WorkloadResult

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(result.label)
                .font(.caption.bold())
            Text(detail)
                .font(.system(.caption2, design: .monospaced))
                .foregroundColor(result.report.ok == 1 ? .primary : .red)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var detail: String {
        if result.report.ok != 1 {
            return "ERR: \(result.errorText ?? "(no message)")"
        }
        let medMs = Double(result.report.run_ns_median) / 1_000_000.0
        let p99Ms = Double(result.report.run_ns_p99) / 1_000_000.0
        let rssKB = Double(result.report.rss_peak_bytes) / 1024.0
        return String(
            format: "= %d  iter=%u  med %.2f / p99 %.2f ms  rss %.0fKB",
            result.report.result, result.report.iterations,
            medMs, p99Ms, rssKB
        )
    }
}
