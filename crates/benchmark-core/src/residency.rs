//! Per-process CPU accounting from `proc_pid_rusage(RUSAGE_INFO_V6)` and
//! the physical-footprint ledger from `task_vm_info`.
//!
//! `rusage_info_v6` splits CPU time, instructions and cycles into totals
//! and P-core-only parts (`ri_user_ptime`, `ri_pinstructions`,
//! `ri_pcycles`). The kernel keeps those from the always-on fixed
//! counters, so they work without kpc privileges and on cores whose PMU
//! xctrace cannot read (A12, S8).
//!
//! `proc_pid_rusage` is declared in <libproc.h>, which only the macOS SDK
//! ships, so an app linking it on iOS, tvOS, watchOS or visionOS would use
//! an interface that is not public there. Those builds read the same
//! counters for this process from `task_info(TASK_POWER_INFO_V2)` (CPU and
//! P-core time) and `task_inspect(TASK_INSPECT_BASIC_COUNTS)` (instructions
//! and cycles), both in <mach/task.h>; they have no P-core instruction or
//! cycle split, which nothing reads. Two uses:
//!
//! - E-core residency of a measurement window = 1 - P-time / total time,
//!   measured instead of assumed from the QoS / `taskpolicy -b` setting.
//! - Instructions and cycles per window (IPC) on every device.

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcUsage {
    /// user + system CPU time, ns.
    pub cpu_ns: u64,
    /// user + system CPU time spent on P-cores, ns.
    pub p_cpu_ns: u64,
    pub instructions: u64,
    pub cycles: u64,
    pub p_instructions: u64,
    pub p_cycles: u64,
}

impl ProcUsage {
    pub fn since(&self, before: &ProcUsage) -> ProcUsage {
        ProcUsage {
            cpu_ns: self.cpu_ns.saturating_sub(before.cpu_ns),
            p_cpu_ns: self.p_cpu_ns.saturating_sub(before.p_cpu_ns),
            instructions: self.instructions.saturating_sub(before.instructions),
            cycles: self.cycles.saturating_sub(before.cycles),
            p_instructions: self.p_instructions.saturating_sub(before.p_instructions),
            p_cycles: self.p_cycles.saturating_sub(before.p_cycles),
        }
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    use std::os::raw::{c_int, c_uint, c_void};

    /// `struct rusage_info_v6` from <sys/resource.h> (macOS 26 / iOS 26 SDK).
    #[repr(C)]
    #[derive(Default)]
    pub struct RusageInfoV6 {
        pub ri_uuid: [u8; 16],
        pub ri_user_time: u64,
        pub ri_system_time: u64,
        pub ri_pkg_idle_wkups: u64,
        pub ri_interrupt_wkups: u64,
        pub ri_pageins: u64,
        pub ri_wired_size: u64,
        pub ri_resident_size: u64,
        pub ri_phys_footprint: u64,
        pub ri_proc_start_abstime: u64,
        pub ri_proc_exit_abstime: u64,
        pub ri_child_user_time: u64,
        pub ri_child_system_time: u64,
        pub ri_child_pkg_idle_wkups: u64,
        pub ri_child_interrupt_wkups: u64,
        pub ri_child_pageins: u64,
        pub ri_child_elapsed_abstime: u64,
        pub ri_diskio_bytesread: u64,
        pub ri_diskio_byteswritten: u64,
        pub ri_cpu_time_qos_default: u64,
        pub ri_cpu_time_qos_maintenance: u64,
        pub ri_cpu_time_qos_background: u64,
        pub ri_cpu_time_qos_utility: u64,
        pub ri_cpu_time_qos_legacy: u64,
        pub ri_cpu_time_qos_user_initiated: u64,
        pub ri_cpu_time_qos_user_interactive: u64,
        pub ri_billed_system_time: u64,
        pub ri_serviced_system_time: u64,
        pub ri_logical_writes: u64,
        pub ri_lifetime_max_phys_footprint: u64,
        pub ri_instructions: u64,
        pub ri_cycles: u64,
        pub ri_billed_energy: u64,
        pub ri_serviced_energy: u64,
        pub ri_interval_max_phys_footprint: u64,
        pub ri_runnable_time: u64,
        pub ri_flags: u64,
        pub ri_user_ptime: u64,
        pub ri_system_ptime: u64,
        pub ri_pinstructions: u64,
        pub ri_pcycles: u64,
        pub ri_energy_nj: u64,
        pub ri_penergy_nj: u64,
        pub ri_secure_time_in_system: u64,
        pub ri_secure_ptime_in_system: u64,
        pub ri_neural_footprint: u64,
        pub ri_lifetime_max_neural_footprint: u64,
        pub ri_interval_max_neural_footprint: u64,
        pub ri_conclave_footprint: u64,
        pub ri_page_wait_time_mach: u64,
        pub ri_page_cache_hits: u64,
        pub ri_reserved: [u64; 6],
    }

    pub const RUSAGE_INFO_V6: c_int = 6;

    #[repr(C)]
    #[derive(Default)]
    pub struct MachTimebaseInfo {
        pub numer: u32,
        pub denom: u32,
    }

    /// Prefix of `struct task_vm_info` (<mach/task_info.h>) through the
    /// rev3 ledger block. The kernel fills a revision's fields only when the
    /// passed count covers the whole revision (TASK_VM_INFO_REV3_COUNT here),
    /// so the other 20 rev3 ledgers must be present even though only
    /// `ledger_phys_footprint_peak` is read.
    #[repr(C)]
    #[derive(Default)]
    pub struct TaskVmInfo {
        pub virtual_size: u64,
        pub region_count: i32,
        pub page_size: i32,
        pub resident_size: u64,
        pub resident_size_peak: u64,
        pub device: u64,
        pub device_peak: u64,
        pub internal: u64,
        pub internal_peak: u64,
        pub external: u64,
        pub external_peak: u64,
        pub reusable: u64,
        pub reusable_peak: u64,
        pub purgeable_volatile_pmap: u64,
        pub purgeable_volatile_resident: u64,
        pub purgeable_volatile_virtual: u64,
        pub compressed: u64,
        pub compressed_peak: u64,
        pub compressed_lifetime: u64,
        pub phys_footprint: u64,
        pub min_address: u64,
        pub max_address: u64,
        pub ledger_phys_footprint_peak: i64,
        pub ledger_rev3_rest: [i64; 20],
    }

    pub const TASK_VM_INFO: c_uint = 22;

    /// `struct task_power_info_v2` (<mach/task_info.h>), arm64 layout.
    #[repr(C)]
    #[derive(Default)]
    pub struct TaskPowerInfoV2 {
        pub total_user: u64,
        pub total_system: u64,
        pub task_interrupt_wakeups: u64,
        pub task_platform_idle_wakeups: u64,
        pub task_timer_wakeups_bin_1: u64,
        pub task_timer_wakeups_bin_2: u64,
        pub gpu_energy: [u64; 4],
        pub task_energy: u64,
        pub task_ptime: u64,
        pub task_pset_switches: u64,
    }

    pub const TASK_POWER_INFO_V2: c_uint = 26;

    /// `struct task_inspect_basic_counts` (<mach/task_inspect.h>).
    #[repr(C)]
    #[derive(Default)]
    pub struct TaskInspectBasicCounts {
        pub instructions: u64,
        pub cycles: u64,
    }

    pub const TASK_INSPECT_BASIC_COUNTS: c_uint = 1;

    extern "C" {
        #[cfg(target_os = "macos")]
        pub fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut c_void) -> c_int;
        pub fn getpid() -> c_int;
        pub fn mach_timebase_info(info: *mut MachTimebaseInfo) -> c_int;
        pub fn mach_task_self() -> c_uint;
        pub fn task_info(task: c_uint, flavor: c_uint, out: *mut c_int, count: *mut c_uint)
            -> c_int;
        pub fn task_inspect(task: c_uint, flavor: c_uint, out: *mut c_int, count: *mut c_uint)
            -> c_int;
    }

    /// Mach absolute-time units to nanoseconds.
    pub fn mach_to_ns(t: u64) -> u64 {
        let mut tb = MachTimebaseInfo::default();
        unsafe { mach_timebase_info(&mut tb) };
        if tb.denom == 0 {
            t
        } else {
            ((t as u128) * (tb.numer as u128) / (tb.denom as u128)) as u64
        }
    }
}

/// Snapshot of this process's rusage counters.
#[cfg(target_os = "macos")]
pub fn proc_usage() -> Option<ProcUsage> {
    proc_usage_of(unsafe { apple::getpid() })
}

/// Snapshot of this process's counters, from the public task interfaces.
#[cfg(all(target_vendor = "apple", not(target_os = "macos")))]
pub fn proc_usage() -> Option<ProcUsage> {
    task_usage()
}

/// This process's CPU time, P-core time, instructions and cycles from
/// `task_info(TASK_POWER_INFO_V2)` and `task_inspect(TASK_INSPECT_BASIC_COUNTS)`.
#[cfg(target_vendor = "apple")]
pub fn task_usage() -> Option<ProcUsage> {
    use apple::*;
    use std::os::raw::{c_int, c_uint};
    let word = std::mem::size_of::<c_int>();
    let mut power = TaskPowerInfoV2::default();
    let mut count = (std::mem::size_of::<TaskPowerInfoV2>() / word) as c_uint;
    let rc = unsafe {
        task_info(mach_task_self(), TASK_POWER_INFO_V2, &mut power as *mut _ as *mut c_int, &mut count)
    };
    if rc != 0 {
        return None;
    }
    let mut counts = TaskInspectBasicCounts::default();
    let mut count = (std::mem::size_of::<TaskInspectBasicCounts>() / word) as c_uint;
    let rc = unsafe {
        task_inspect(mach_task_self(), TASK_INSPECT_BASIC_COUNTS, &mut counts as *mut _ as *mut c_int,
                     &mut count)
    };
    if rc != 0 {
        counts = TaskInspectBasicCounts::default();
    }
    // task_power_info times are Mach absolute-time units.
    Some(ProcUsage {
        cpu_ns: mach_to_ns(power.total_user + power.total_system),
        p_cpu_ns: mach_to_ns(power.task_ptime),
        instructions: counts.instructions,
        cycles: counts.cycles,
        p_instructions: 0,
        p_cycles: 0,
    })
}

/// Rusage counters of process `pid` (this process, or an exited child that
/// has not been reaped yet — see the `rusage_exec` bin).
#[cfg(target_os = "macos")]
pub fn proc_usage_of(pid: i32) -> Option<ProcUsage> {
    use apple::*;
    let mut ri = RusageInfoV6::default();
    let rc = unsafe {
        proc_pid_rusage(pid, RUSAGE_INFO_V6, &mut ri as *mut _ as *mut std::os::raw::c_void)
    };
    if rc != 0 {
        return None;
    }
    // rusage_info times are Mach absolute-time units.
    let to_ns = mach_to_ns;
    Some(ProcUsage {
        cpu_ns: to_ns(ri.ri_user_time + ri.ri_system_time),
        p_cpu_ns: to_ns(ri.ri_user_ptime + ri.ri_system_ptime),
        instructions: ri.ri_instructions,
        cycles: ri.ri_cycles,
        p_instructions: ri.ri_pinstructions,
        p_cycles: ri.ri_pcycles,
    })
}

#[cfg(not(target_vendor = "apple"))]
pub fn proc_usage() -> Option<ProcUsage> {
    None
}

/// Other processes' counters need `proc_pid_rusage`, which only macOS has
/// publicly; here the only process is this one.
#[cfg(all(target_vendor = "apple", not(target_os = "macos")))]
pub fn proc_usage_of(pid: i32) -> Option<ProcUsage> {
    if pid == unsafe { apple::getpid() } { task_usage() } else { None }
}

#[cfg(not(target_vendor = "apple"))]
pub fn proc_usage_of(_pid: i32) -> Option<ProcUsage> {
    None
}

/// `(phys_footprint, ledger_phys_footprint_peak)` in bytes: the current
/// footprint and the process-lifetime high-water mark jetsam enforces.
#[cfg(target_vendor = "apple")]
pub fn phys_footprint() -> Option<(u64, u64)> {
    use apple::*;
    let mut info = TaskVmInfo::default();
    let mut count = (std::mem::size_of::<TaskVmInfo>() / std::mem::size_of::<std::os::raw::c_int>())
        as std::os::raw::c_uint;
    let rc = unsafe {
        task_info(mach_task_self(), TASK_VM_INFO, &mut info as *mut _ as *mut std::os::raw::c_int,
                  &mut count)
    };
    if rc != 0 {
        return None;
    }
    Some((info.phys_footprint, info.ledger_phys_footprint_peak.max(0) as u64))
}

#[cfg(not(target_vendor = "apple"))]
pub fn phys_footprint() -> Option<(u64, u64)> {
    None
}
