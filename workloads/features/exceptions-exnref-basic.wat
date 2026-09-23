;; exception handling (exnref proposal), basic form: try_table with a plain
;; `catch` of a tag carrying an i32 payload, thrown from a callee. No
;; catch_ref / throw_ref (exceptions-exnref.wat covers those).
(module
  (tag $e (param i32))
  (func $thrower (param i32)
    (throw $e (local.get 0)))
  (func (export "run") (param i32) (result i32)
    (block $caught (result i32)
      (try_table (catch $e $caught)
        (call $thrower (local.get 0)))
      (i32.const -1)
      (return))
    (i32.const 1)
    (i32.add)))
