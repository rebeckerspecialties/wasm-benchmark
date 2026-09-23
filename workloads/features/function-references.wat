;; typed function references: (ref $t), ref.func, call_ref, br_on_null.
(module
  (type $t (func (param i32) (result i32)))
  (func $inc (type $t) (i32.add (local.get 0) (i32.const 1)))
  (elem declare func $inc)
  (func (export "run") (param i32) (result i32)
    (local $f (ref null $t))
    (local.set $f (ref.func $inc))
    (block $null
      (return (call_ref $t (local.get 0) (br_on_null $null (local.get $f)))))
    (i32.const -1)))
