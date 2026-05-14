// Synthetic SPC-DSP-like audio workload.
//
// We don't ship a full SPC700 + DSP emulator (multi-kloc; out of session
// scope). Instead we exercise the *characteristics* the WasmEdge WASI-audio
// hot path puts on the interpreter:
//   - per-voice state machines (8 voices)
//   - 4-bit BRR-style sample decoding from a packed sample memory
//   - 4-tap Gaussian-style interpolation per voice
//   - per-voice envelope (attack-decay-sustain-release)
//   - a 1-pole IIR low-pass over the mix
//
// Output: 1000 frames × 512 samples = 512000 i16 mix samples generated.
// Returns an i32 checksum (sum of samples mod 2^31) so the host can
// verify the bytecode produced the same sample stream.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

const VOICES: usize = 8;
const SAMPLE_MEM: usize = 4096;
const FRAMES: usize = 1000;
const SAMPLES_PER_FRAME: usize = 512;

#[derive(Copy, Clone)]
struct Voice {
    sample_pos: i32,    // q12 — fixed-point position into SAMPLE
    sample_inc: i32,    // q12 — pitch
    env: i32,           // 0..0x7FF envelope
    env_phase: u8,      // 0=attack 1=decay 2=sustain 3=release
    sample_off: u16,    // base offset into SAMPLE memory
}

static mut SAMPLE: [i16; SAMPLE_MEM] = [0; SAMPLE_MEM];
static mut VOICES_STATE: [Voice; VOICES] = [Voice {
    sample_pos: 0,
    sample_inc: 0,
    env: 0,
    env_phase: 0,
    sample_off: 0,
}; VOICES];
static mut LPF_STATE: i32 = 0;

fn init(seed: u32) {
    let mut s = seed;
    unsafe {
        // Fill the sample memory with a deterministic pseudo-random PCM
        // pattern (scaled to roughly i16 range).
        let mut i = 0;
        while i < SAMPLE_MEM {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            SAMPLE[i] = ((s as i32) >> 16) as i16;
            i += 1;
        }
        // Per-voice setup.
        let mut v = 0;
        while v < VOICES {
            VOICES_STATE[v] = Voice {
                sample_pos: 0,
                sample_inc: 0x800 + (v as i32 * 0x80), // varying pitches
                env: 0,
                env_phase: 0,
                sample_off: ((v * SAMPLE_MEM / VOICES) & 0xFFFF) as u16,
            };
            v += 1;
        }
        LPF_STATE = 0;
    }
}

#[inline]
fn fetch_sample(off: usize, idx: usize) -> i32 {
    unsafe { SAMPLE[(off + idx) & (SAMPLE_MEM - 1)] as i32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_dsp(seed: i32) -> i32 {
    init(seed as u32);
    let mut checksum: i32 = 0;
    unsafe {
        let mut frame = 0;
        while frame < FRAMES {
            let mut s = 0;
            while s < SAMPLES_PER_FRAME {
                let mut mix: i32 = 0;
                let mut v = 0;
                while v < VOICES {
                    let voice = &mut VOICES_STATE[v];
                    // 4-tap Gaussian-ish interpolation: weights {1,2,2,1}/6.
                    let int_pos = (voice.sample_pos >> 12) as usize;
                    let s0 = fetch_sample(voice.sample_off as usize, int_pos);
                    let s1 = fetch_sample(voice.sample_off as usize, int_pos + 1);
                    let s2 = fetch_sample(voice.sample_off as usize, int_pos + 2);
                    let s3 = fetch_sample(voice.sample_off as usize, int_pos + 3);
                    let interp = (s0 + 2 * s1 + 2 * s2 + s3) / 6;
                    // Envelope state machine.
                    match voice.env_phase {
                        0 => { voice.env += 0x10; if voice.env >= 0x7FF { voice.env = 0x7FF; voice.env_phase = 1; } }
                        1 => { voice.env -= 0x4; if voice.env < 0x500 { voice.env = 0x500; voice.env_phase = 2; } }
                        2 => { /* sustain */ }
                        _ => { voice.env -= 0x2; if voice.env < 0 { voice.env = 0; } }
                    }
                    mix += (interp * voice.env) >> 11;
                    voice.sample_pos = voice.sample_pos.wrapping_add(voice.sample_inc);
                    if voice.sample_pos >= ((SAMPLE_MEM as i32) << 12) {
                        voice.sample_pos -= (SAMPLE_MEM as i32) << 12;
                    }
                    v += 1;
                }
                // 1-pole low-pass (alpha ≈ 0.25 in q15).
                LPF_STATE = LPF_STATE + ((mix - LPF_STATE) >> 2);
                let out = LPF_STATE.clamp(-32768, 32767) as i16;
                checksum = checksum.wrapping_add(out as i32);
                s += 1;
            }
            // Cycle through envelope phases over the run so all branches fire.
            if frame == FRAMES / 4 {
                let mut v = 0;
                while v < VOICES { VOICES_STATE[v].env_phase = 1; v += 1; }
            } else if frame == FRAMES / 2 {
                let mut v = 0;
                while v < VOICES { VOICES_STATE[v].env_phase = 2; v += 1; }
            } else if frame == 3 * FRAMES / 4 {
                let mut v = 0;
                while v < VOICES { VOICES_STATE[v].env_phase = 3; v += 1; }
            }
            frame += 1;
        }
    }
    checksum & 0x7FFFFFFF
}
