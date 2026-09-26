;; relaxed SIMD: f32x4.relaxed_madd and i32x4.relaxed_dot_i8x16_i7x16_add_s,
;; with inputs whose result is the same under every allowed lowering.
(module
  (func (export "run") (param i32) (result i32)
    (local $m i32)
    (local $d i32)
    (local.set $m
      (i32x4.extract_lane 0
        (i32x4.trunc_sat_f32x4_s
          (f32x4.relaxed_madd
            (f32x4.splat (f32.convert_i32_s (local.get 0)))
            (f32x4.splat (f32.const 1))
            (f32x4.splat (f32.const 0))))))
    (local.set $d
      (i32x4.extract_lane 0
        (i32x4.relaxed_dot_i8x16_i7x16_add_s
          (v128.const i8x16 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
          (v128.const i8x16 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
          (v128.const i32x4 0 0 0 0))))
    (i32.add (local.get $m) (local.get $d))))
