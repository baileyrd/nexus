;; C81: minimal WASM probe for host::http_request capability + policy gates.
;;
;; Imports `host::http_request` and exports a single `probe()` function that
;; calls it once with a small fixed JSON request payload. The test harness
;; asserts the returned i32 is `HOST_CAPABILITY_DENIED` (-1001) when the
;; sandbox's CapabilitySet does not include `Capability::NetHttp`, and
;; `HOST_ERROR` (-1) when NetHttp is granted but the injected
;; `NetworkPolicy` is still closed (the default) — proving both gates fire
;; through the real wasmtime host-fn dispatch, mirroring
;; `denial_probe.wat`'s read_file coverage.
;;
;; The data segment lays out the literal request JSON at offset 0,
;; reserving 100..4196 as a scratch output buffer.

(module
  ;; host::http_request(req_ptr, req_len, out_ptr, out_cap) -> i32
  (import "host" "http_request"
    (func $host_http_request (param i32 i32 i32 i32) (result i32)))

  ;; One linear-memory page is enough for the request JSON + a 4 KiB output buffer.
  (memory (export "memory") 1)

  ;; Place `{"method":"GET","url":"https://api.example.com/x"}` (50 bytes) at offset 0.
  (data (i32.const 0) "{\22method\22:\22GET\22,\22url\22:\22https://api.example.com/x\22}")

  (func (export "probe") (result i32)
    ;; http_request(req_ptr=0, req_len=50, out_ptr=100, out_cap=4096)
    i32.const 0
    i32.const 50
    i32.const 100
    i32.const 4096
    call $host_http_request)
)
