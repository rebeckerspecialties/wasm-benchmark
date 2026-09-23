;; tail calls: 1M-deep mutually recursive return_call; without real tail
;; calls this exhausts any interpreter's call stack.
(module
  (func $even (param i32) (result i32)
    (if (result i32) (i32.eqz (local.get 0))
      (then (i32.const 1))
      (else (return_call $odd (i32.sub (local.get 0) (i32.const 1))))))
  (func $odd (param i32) (result i32)
    (if (result i32) (i32.eqz (local.get 0))
      (then (i32.const 0))
      (else (return_call $even (i32.sub (local.get 0) (i32.const 1))))))
  (func (export "run") (param i32) (result i32)
    (i32.add (call $even (i32.const 1000000)) (local.get 0))))
