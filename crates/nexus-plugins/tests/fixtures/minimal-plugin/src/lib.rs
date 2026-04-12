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
        0 | 1 | 2 => b"{}".to_vec(), // lifecycle hooks
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
