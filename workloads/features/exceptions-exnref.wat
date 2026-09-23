;; exception handling (exnref, the standardized proposal): try_table with
;; catch_ref, rethrow of the captured exnref via throw_ref, caught outside.
(module
  (tag $e (param i32))
  (func (export "run") (param i32) (result i32)
    (block $outer (result i32)
      (try_table (catch $e $outer)
        (block $inner (result i32 exnref)
          (try_table (catch_ref $e $inner)
            (throw $e (local.get 0)))
          (unreachable))
        (throw_ref)
        (drop))
      (unreachable))
    (i32.const 1)
    (i32.add)))
