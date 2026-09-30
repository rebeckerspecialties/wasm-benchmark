(module
  (memory 1 1 shared)
  ;; One non-atomic load and one non-atomic store per iteration.
  (func (export "plain") (param $n i32) (result i32)
    (local $i i32)
    (loop $l
      (i32.store (i32.and (local.get $i) (i32.const 0xfffc))
        (i32.add (i32.load (i32.and (local.get $i) (i32.const 0xfffc))) (local.get $i)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (local.get $n))))
    (i32.load (i32.const 0)))
  ;; One atomic read-modify-write per iteration.
  (func (export "atomic") (param $n i32) (result i32)
    (local $i i32)
    (loop $l
      (drop (i32.atomic.rmw.add (i32.and (local.get $i) (i32.const 0xfffc)) (i32.const 1)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (local.get $n))))
    (i32.atomic.load (i32.const 0))))
