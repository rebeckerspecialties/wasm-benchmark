;; exception handling (legacy phase-3 proposal): try / catch / throw.
(module
  (tag $e (param i32))
  (func (export "run") (param i32) (result i32)
    try (result i32)
      local.get 0
      throw $e
    catch $e
      i32.const 1
      i32.add
    end))
