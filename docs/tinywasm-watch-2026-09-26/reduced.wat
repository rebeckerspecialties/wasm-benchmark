;; Reduced from the benchmark's worst row ("multi-memory twin: one memory"):
;; a hash loop over bytes. Every op is cheap, so the interpreter's per-op
;; overhead is most of the cost.
(module
  (memory 1)
  (func (export "run") (param $n i32) (result i32)
    (local $i i32) (local $h i32)
    (loop $l
      (local.set $h
        (i32.mul
          (i32.rotl (i32.xor (local.get $h) (i32.load8_u (local.get $i))) (i32.const 5))
          (i32.const -1640531535)))
      (br_if $l (i32.lt_u (local.tee $i (i32.add (local.get $i) (i32.const 1)))
                          (local.get $n))))
    (local.get $h)))
