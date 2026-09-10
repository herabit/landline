pub mod mem;
pub mod pod;

#[allow(unexpected_cfgs)]
const _: () = {
    cfg_select! {
        any(target_pointer_width = "32", target_pointer_width = "64") => {}
        _ => {
            compiler_error!("we only support 32-bit and 64-bit platforms currently");
        }
    }
};
