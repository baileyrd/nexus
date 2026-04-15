use std::alloc::{alloc, Layout};

#[link(wasm_import_module = "host")]
extern "C" {
    fn get_settings(out_ptr: i32, out_cap: i32) -> i32;
}

#[no_mangle]
pub extern "C" fn nexus_alloc(size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    unsafe {
        let layout = Layout::from_size_align(size as usize, 1).unwrap();
        alloc(layout) as u32
    }
}

#[no_mangle]
pub extern "C" fn nexus_dispatch(handler_id: u32, args_ptr: u32, args_len: u32) -> u64 {
    let result = match handler_id {
        0 | 1 | 2 | 3 | 4 | 5 | 6 => b"{}".to_vec(), // lifecycle hooks
        7 => {
            // on_settings_changed: echo settings back
            if args_len == 0 {
                b"{}".to_vec()
            } else {
                unsafe {
                    std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize).to_vec()
                }
            }
        }
        100 => {
            // echo handler
            if args_len == 0 {
                b"{}".to_vec()
            } else {
                unsafe {
                    std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize).to_vec()
                }
            }
        }
        101 => {
            // panel render: returns { content: "..." } as JSON
            br#"{"content":"Hello from the hello-nexus plugin panel!\n\nThis text was rendered by a WASM handler running in a sandboxed VM."}"#
                .to_vec()
        }
        102 => {
            // settings-tab render: returns { content: "..." } as JSON
            br#"{"content":"This tab is rendered by the hello-nexus plugin.\n\nA real plugin would expose configurable knobs here - schema-driven form controls, toggles, text inputs, and so on. For now, static text."}"#
                .to_vec()
        }
        103 => {
            // say-hi handler with event emission: the host extracts the
            // `events` array and emits a `plugin:event` Tauri event
            // for each entry. Proves the plugin -> frontend bus.
            br#"{"message":"Hello!","events":[{"topic":"com.nexus.hello.greeted","payload":{"message":"Hello from the WASM sandbox!"}}]}"#
                .to_vec()
        }
        104 => {
            // host-event observer: wraps whatever JSON arrived on the
            // subscription into a `com.nexus.hello.observed` plugin
            // event. The host-side poll_events loop walks our response
            // for this `events` array and emits each entry back to
            // the frontend, so host → plugin → frontend round-trips
            // end-to-end.
            let args = if args_len == 0 {
                b"null".to_vec()
            } else {
                unsafe {
                    std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize).to_vec()
                }
            };
            let mut out = Vec::with_capacity(args.len() + 96);
            out.extend_from_slice(br#"{"events":[{"topic":"com.nexus.hello.observed","payload":"#);
            out.extend_from_slice(&args);
            out.extend_from_slice(b"}]}");
            out
        }
        105 => {
            // echo-settings handler: asks the host for this plugin's
            // current validated settings JSON via `host::get_settings`
            // and re-emits it as a `com.nexus.hello.settings` plugin
            // event, proving the WASM-side settings read path.
            const CAP: i32 = 4096;
            let cap_u32 = CAP as u32;
            let buf_ptr = nexus_alloc(cap_u32);
            let payload: Vec<u8> = if buf_ptr == 0 {
                b"null".to_vec()
            } else {
                let n = unsafe { get_settings(buf_ptr as i32, CAP) };
                if n > 0 {
                    unsafe {
                        std::slice::from_raw_parts(buf_ptr as *const u8, n as usize).to_vec()
                    }
                } else {
                    b"{}".to_vec()
                }
            };
            let mut out = Vec::with_capacity(payload.len() + 96);
            out.extend_from_slice(br#"{"events":[{"topic":"com.nexus.hello.settings","payload":"#);
            out.extend_from_slice(&payload);
            out.extend_from_slice(b"}]}");
            out
        }
        _ => b"{\"error\":\"unknown handler\"}".to_vec(),
    };

    let result_ptr = nexus_alloc(result.len() as u32);
    if result_ptr != 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(result.as_ptr(), result_ptr as *mut u8, result.len());
        }
    }
    ((result_ptr as u64) << 32) | (result.len() as u64)
}
