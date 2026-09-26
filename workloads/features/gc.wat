;; GC: struct.new / struct.get / struct.set, array.new / array.get / array.set.
(module
  (type $pair (struct (field i32) (field (mut i32))))
  (type $arr (array (mut i32)))
  (func (export "run") (param i32) (result i32)
    (local $p (ref null $pair))
    (local $a (ref null $arr))
    (local.set $p (struct.new $pair (local.get 0) (i32.const 0)))
    (struct.set $pair 1 (local.get $p) (i32.const 1))
    (local.set $a (array.new $arr (i32.const 0) (i32.const 4)))
    (array.set $arr (local.get $a) (i32.const 2) (struct.get $pair 0 (local.get $p)))
    (i32.add (array.get $arr (local.get $a) (i32.const 2)) (struct.get $pair 1 (local.get $p)))))
