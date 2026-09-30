# CPU baseline and tuning per platform, sourced by the device-library build
# scripts (build-lib.sh, the runtimes' build-*.sh, tinywasm-ab-build-ios.sh,
# tos-bench/build-ios.sh). The macOS host CLIs keep their own A12 flags.
#
# The baseline is the oldest chip the app runs on there, so the compilers
# may use every instruction it has; the tuning is the chip to schedule for.
#
#   iOS / iPadOS      A12 (UIRequiredDeviceCapabilities requires one): LSE
#                     atomics, ARMv8.3. Tuned for the A13.
#   watchOS arm64_32  Every watch on watchOS 11, and on watchOS 26 the ones
#                     without arm64: Series 6-8, SE (2nd gen), Ultra (1st
#                     gen). Their S6-S8 are built from the A13's efficiency
#                     cores (LLVM and Zig define apple-s6..s8 as the A13):
#                     baseline A13, with the A13's own tuning, which measured
#                     best on those cores (AGENTS.md, CPU baselines).
#   watchOS arm64     Series 9, Ultra 2 and later on watchOS 26 and later
#                     (the slice's minimum OS is 26): S9 and later, built
#                     from the A16's efficiency cores or newer (LLVM and Zig
#                     define apple-s9 / apple-s10 as the A16). Baseline A16.
#   tvOS (and its     Apple TV 4K only: baseline A10X (1st gen), tuned for
#   simulator)        the A12 (2nd gen). The Apple TV HD's A8 lacks
#                     instructions a build for the A10 may use (CRC32, RDM).
#   everything else   A12: the macOS dev host (so an instruction-set bug
#                     shows up on the Mac), the iOS, watchOS and visionOS
#                     simulators, and visionOS.
#
# LLVM gives every Apple core the same scheduling model, and in LLVM 22 the
# A10 to A13 also share one tuning (fusion and alignment preferences): the
# A13 tuning on iOS and the A12 tuning on tvOS leave the code as the
# baseline alone would. They take effect if LLVM's models diverge. Zig has
# no separate tuning: its CPU is the baseline.

RUST_CPU_DEFAULT="-C target-cpu=apple-a12"
RUST_CPU_IOS="-C target-cpu=apple-a12 -Z tune-cpu=apple-a13"
RUST_CPU_WATCH32="-C target-cpu=apple-a13"
RUST_CPU_WATCH64="-C target-cpu=apple-a16"
RUST_CPU_TVOS="-C target-cpu=apple-a10 -Z tune-cpu=apple-a12"

CC_CPU_DEFAULT="-mcpu=apple-a12"
CC_CPU_IOS="-mcpu=apple-a12 -mtune=apple-a13"
CC_CPU_WATCH32="-mcpu=apple-a13"
CC_CPU_WATCH64="-mcpu=apple-a16"
CC_CPU_TVOS="-mcpu=apple-a10 -mtune=apple-a12"

# Elsewhere Zig keeps its default for the target (apple_m1 on macOS, apple_m2
# on visionOS). Before these, the iOS and tvOS builds of the Zig runtimes were
# apple_a7 and both watchOS slices apple_s4.
ZIG_CPU_IOS="apple_a12"
ZIG_CPU_WATCH32="apple_a13"
ZIG_CPU_WATCH64="apple_a16"
ZIG_CPU_TVOS="apple_a10"
