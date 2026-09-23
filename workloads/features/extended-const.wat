;; extended-const: i32.add / i32.mul inside a global's constant initializer.
(module
  (global $g i32 (i32.add (i32.const 40) (i32.mul (i32.const 1) (i32.const 2))))
  (func (export "run") (param i32) (result i32)
    (i32.sub (i32.add (global.get $g) (local.get 0)) (i32.const 41))))
