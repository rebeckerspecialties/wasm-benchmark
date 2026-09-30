//! The task_info / task_inspect counters the iOS, tvOS, watchOS and
//! visionOS builds read agree with proc_pid_rusage, which macOS builds (and
//! the measurements before them) read.

#[cfg(target_os = "macos")]
#[test]
fn task_counters_match_rusage() {
    use benchmark_core::residency::{proc_usage_of, task_usage};
    let pid = std::process::id() as i32;
    let (r0, t0) = (proc_usage_of(pid).unwrap(), task_usage().unwrap());
    let mut x = 0u64;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_millis(300) {
        for i in 0..10_000u64 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(i);
        }
    }
    std::hint::black_box(x);
    let (r1, t1) = (proc_usage_of(pid).unwrap(), task_usage().unwrap());
    let (r, t) = (r1.since(&r0), t1.since(&t0));
    println!("rusage {r:?}\ntask   {t:?}");
    let close = |a: u64, b: u64| (a as f64 - b as f64).abs() <= 0.05 * (a.max(b) as f64) + 1e6;
    assert!(close(r.cpu_ns, t.cpu_ns), "cpu_ns {} vs {}", r.cpu_ns, t.cpu_ns);
    assert!(close(r.p_cpu_ns, t.p_cpu_ns), "p_cpu_ns {} vs {}", r.p_cpu_ns, t.p_cpu_ns);
    assert!(close(r.instructions, t.instructions), "instructions {} vs {}", r.instructions, t.instructions);
    assert!(close(r.cycles, t.cycles), "cycles {} vs {}", r.cycles, t.cycles);
    assert!(t.cpu_ns > 200_000_000 && t.instructions > 0 && t.cycles > 0);
}
