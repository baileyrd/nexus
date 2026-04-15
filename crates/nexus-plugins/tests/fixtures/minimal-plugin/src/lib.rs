use std::alloc::{alloc, Layout};

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
