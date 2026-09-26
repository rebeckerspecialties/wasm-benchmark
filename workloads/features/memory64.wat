;; memory64: i64-indexed memory, i64 addresses on load/store.
(module
  (memory i64 1)
  (func (export "run") (param i32) (result i32)
    (i64.store (i64.const 65528) (i64.extend_i32_u (local.get 0)))
    (i32.wrap_i64 (i64.add (i64.load (i64.const 65528)) (i64.const 1)))))
