//! Per-process CPU accounting from `proc_pid_rusage(RUSAGE_INFO_V6)` and
//! the physical-footprint ledger from `task_vm_info`.
//!
//! `rusage_info_v6` splits CPU time, instructions and cycles into totals
//! and P-core-only parts (`ri_user_ptime`, `ri_pinstructions`,
//! `ri_pcycles`). The kernel keeps those from the always-on fixed
//! counters, so they work without kpc privileges and on cores whose PMU
//! xctrace cannot read (A12, S8). Two uses:
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

    /// Prefix of `struct task_vm_info` (<mach/task_info.h>) through
    /// `ledger_phys_footprint_peak`; `task_info` fills as many words as
    /// the passed count allows.
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
    }

    pub const TASK_VM_INFO: c_uint = 22;

    extern "C" {
        pub fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut c_void) -> c_int;
        pub fn getpid() -> c_int;
        pub fn mach_timebase_info(info: *mut MachTimebaseInfo) -> c_int;
        pub fn mach_task_self() -> c_uint;
        pub fn task_info(task: c_uint, flavor: c_uint, out: *mut c_int, count: *mut c_uint)
            -> c_int;
    }
}

/// Snapshot of this process's rusage counters.
#[cfg(target_vendor = "apple")]
pub fn proc_usage() -> Option<ProcUsage> {
    use apple::*;
    let mut ri = RusageInfoV6::default();
    let rc = unsafe {
        proc_pid_rusage(getpid(), RUSAGE_INFO_V6, &mut ri as *mut _ as *mut std::os::raw::c_void)
    };
    if rc != 0 {
        return None;
    }
    // rusage_info times are Mach absolute-time units.
    let mut tb = MachTimebaseInfo::default();
    unsafe { mach_timebase_info(&mut tb) };
    let to_ns = |t: u64| -> u64 {
        if tb.denom == 0 {
            t
        } else {
            ((t as u128) * (tb.numer as u128) / (tb.denom as u128)) as u64
        }
    };
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
