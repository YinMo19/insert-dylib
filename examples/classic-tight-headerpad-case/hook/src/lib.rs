use sighook::{HookContext, instrument_no_original};

const ADD_INSN_OFFSET: u64 = 0;
const LOG_MSG: &[u8] = b"[+] hooked: now x0 is 99.\n";

extern "C" fn replace_logic(_address: u64, ctx: *mut HookContext) {
    unsafe {
        let _ = libc::write(
            libc::STDERR_FILENO,
            LOG_MSG.as_ptr() as *const libc::c_void,
            LOG_MSG.len(),
        );
        (*ctx).regs.named.x0 = 99;
    }
}

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static INIT_ARRAY: extern "C" fn() = init;

extern "C" fn init() {
    unsafe {
        let target_address = {
            let symbol = libc::dlsym(libc::RTLD_DEFAULT, c"calculate".as_ptr());
            if symbol.is_null() {
                return;
            }
            symbol as u64 + ADD_INSN_OFFSET
        };

        let _ = instrument_no_original(target_address, replace_logic);
    }
}
