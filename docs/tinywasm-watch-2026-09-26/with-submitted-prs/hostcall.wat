(module
  (import "env" "f" (func $hf (param i32) (result i32)))
  (func $wf (param i32) (result i32) local.get 0 i32.const 1 i32.add)
  (func (export "host") (param $n i32) (result i32) (local $acc i32)
    (loop $l
      local.get $acc call $hf local.set $acc
      local.get $n i32.const 1 i32.sub local.tee $n
      br_if $l)
    local.get $acc)
  (func (export "wasm") (param $n i32) (result i32) (local $acc i32)
    (loop $l
      local.get $acc call $wf local.set $acc
      local.get $n i32.const 1 i32.sub local.tee $n
      br_if $l)
    local.get $acc)
  (func (export "inline") (param $n i32) (result i32) (local $acc i32)
    (loop $l
      local.get $acc i32.const 1 i32.add local.set $acc
      local.get $n i32.const 1 i32.sub local.tee $n
      br_if $l)
    local.get $acc))
