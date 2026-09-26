//! PAC (Pointer Authentication) viability probe for arm64 targets.
//!
//! Tests whether the `PACGA` instruction (generic auth code, ARMv8.3-A) is
//! functional in user mode on the running hardware. Apple may gate selected
//! PAC instructions off via `HCR_EL2.API` / `SCTLR_EL1.{En*A,En*B,EnIB,EnIA}`
//! and friends; this probe answers definitively whether `PACGA` is exposed
//! on a given device, which is the prerequisite for any future PAC-signed
//! IC slot scheme on aarch64.
//!
//! Three checks:
//!   1. `PACGA` returns a nonzero auth code in the upper 32 bits of `Xd`.
//!      (PACGA contract: lower 32 bits of `Xd` are zero; upper 32 bits hold
//!      the auth code. All-zero output = stubbed/NOPed by the implementation.)
//!   2. Same inputs produce same outputs (deterministic).
//!   3. Different inputs produce different outputs (input-dependent).
//!
//! All three pass = PAC is usable for IC validity. Any one failing = drop
//! the PAC-signed IC follow-on for this target.
//!
//! Hardware support:
//!   - A12 (iPhone XS, iPad Mini 5, etc.) — first Apple SoC with PAC (v8.3-A)
//!   - A13 / S6+ (Apple Watch SE2's S8 = A13-derived) — has PAC
//!   - Older (A11 / S5 / pre-2018, the A8 and A10X Apple TVs tvOS 18 still
//!     supports) — no PAC. PACGA is not a hint-space instruction, so there it
//!     would be undefined: the probe runs it only when the CPU reports the
//!     feature and otherwise returns an all-zero result.

/// PACGA, assembled for PAuth whatever the build's baseline CPU; call it
/// only where PAuth (`paca` and `pacg`, which Rust enables together) is present.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
#[target_feature(enable = "paca,pacg")]
unsafe fn pacga(addr: u64, modifier: u64) -> u64 {
    let result: u64;
    unsafe {
        core::arch::asm!(
            "pacga {out}, {addr}, {modifier}",
            out = lateout(reg) result,
            addr = in(reg) addr,
            modifier = in(reg) modifier,
            options(nomem, nostack, preserves_flags),
        );
    }
    result
}

/// Result of the PAC probe. All counts are 0 or 1 booleans.
#[repr(C)]
#[derive(Default)]
pub struct PacProbeResult {
    /// `pacga(0xDEADBEEF_DEADBEEF, 0x0123456789ABCDEF)`. On a working
    /// implementation, upper 32 bits are nonzero and lower 32 bits are zero.
    pub code1: u64,
    /// Same inputs as `code1`. Should equal `code1`.
    pub code2: u64,
    /// Different `addr` (XOR'd by 1). Should differ from `code1`.
    pub code3: u64,
    /// 1 = PACGA returned a nonzero code (hardware decoded the instruction
    /// and produced output, not stubbed).
    pub nonzero: u8,
    /// 1 = code1 == code2 (deterministic for same inputs).
    pub deterministic: u8,
    /// 1 = code1 != code3 (input-dependent).
    pub input_dep: u8,
    /// 1 = lower 32 bits of code1 are zero (matches PACGA contract).
    pub low_zero: u8,
    /// 1 = all checks passed; PAC is usable for IC validity here.
    pub supported: u8,
}

/// Run the PAC probe. Safe to call on any target — returns an all-zero
/// result without PAuth and on non-aarch64.
#[no_mangle]
pub extern "C" fn bench_pac_probe() -> PacProbeResult {
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("paca")
        && std::arch::is_aarch64_feature_detected!("pacg")
    {
        let addr = 0xDEAD_BEEF_DEAD_BEEFu64;
        let modifier = 0x0123_4567_89AB_CDEFu64;

        // SAFETY: the CPU reports PAuth's generic authentication.
        let (code1, code2, code3) = unsafe {
            (pacga(addr, modifier), pacga(addr, modifier), pacga(addr ^ 1, modifier))
        };

        let nonzero = (code1 != 0) as u8;
        let deterministic = (code1 == code2) as u8;
        let input_dep = (code1 != code3) as u8;
        let low_zero = ((code1 & 0xFFFF_FFFF) == 0) as u8;
        let supported =
            (nonzero == 1 && deterministic == 1 && input_dep == 1 && low_zero == 1) as u8;

        return PacProbeResult {
            code1,
            code2,
            code3,
            nonzero,
            deterministic,
            input_dep,
            low_zero,
            supported,
        };
    }
    PacProbeResult::default()
}
