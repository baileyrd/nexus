;; Audit-2026-05-01 P1-1: minimal WASM probe for capability-denial test.
;;
;; Imports `host::read_file` and exports a single `probe()` function that
;; calls it once with a small fixed payload. The test harness asserts the
;; returned i32 is `HOST_CAPABILITY_DENIED` (-1001) when the sandbox's
;; CapabilitySet does not include `Capability::FsRead`, and a non-negative
;; bytes-written count when it does.
;;
;; The data segment lays out the literal "test.md" at offset 0 (path the
;; probe reads), reserving 100..4196 as a scratch output buffer.

(module
  ;; host::read_file(path_ptr, path_len, out_ptr, out_cap) -> i32
  (import "host" "read_file"
    (func $host_read_file (param i32 i32 i32 i32) (result i32)))

  ;; One linear-memory page is enough for "test.md" + a 4 KiB output buffer.
  (memory (export "memory") 1)

  ;; Place "test.md" (7 bytes, no trailing NUL) at offset 0.
  (data (i32.const 0) "test.md")

  (func (export "probe") (result i32)
    ;; read_file(path_ptr=0, path_len=7, out_ptr=100, out_cap=4096)
    i32.const 0
    i32.const 7
    i32.const 100
    i32.const 4096
    call $host_read_file)
)
