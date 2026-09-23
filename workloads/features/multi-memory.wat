;; multi-memory: two memories, store into one, memory.copy across them.
(module
  (memory $a 1)
  (memory $b 1)
  (func (export "run") (param i32) (result i32)
    (i32.store $a (i32.const 16) (local.get 0))
    (memory.copy $b $a (i32.const 32) (i32.const 16) (i32.const 4))
    (i32.add (i32.load $b (i32.const 32)) (i32.const 1))))
