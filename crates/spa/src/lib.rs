pub mod mem;
pub mod pod;

// Pipewire seems to be built with the assumption that we're on 32-bit or 64-bit linux,
// and we will also make that assumption.
#[allow(unexpected_cfgs)]
const _: () = {
    cfg_select! {
        all(
            target_os = "linux",
            any(target_pointer_width = "32", target_pointer_width = "64"),
        ) => {},
        _ => {
            compiler_error!("we only support 32-bit and 64-bit linux currently");
        },
    }
};

// Pipewire *also* seems to assume that `c_float == f32` and `c_double == f64`.
//
// We will assume as such here too. This will cause a compiler error if there's a mismatch in what
// either are considered for our target platform.
#[allow(clippy::let_unit_value)]
const _: () = {
    const fn do_nothing<T>(_: T)
    where
        T: Copy,
    {
    }

    let _ = do_nothing::<std::ffi::c_float>(0.0_f32);
    let _ = do_nothing::<std::ffi::c_double>(0.0_f64);
};
