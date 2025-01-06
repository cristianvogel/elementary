extern crate elem;

use elem::std::prelude::*;

/// http://jakegoulding.com/rust-ffi-omnibus/objects/
pub struct EngineHandles {
    main: elem::engine::MainHandle,
    proc: elem::engine::ProcessHandle,
}

#[no_mangle]
pub extern "C" fn elem_engine_new(sample_rate: f64, block_size: usize) -> *mut EngineHandles {
    let (mut main, proc) = elem::engine::new_engine(sample_rate, block_size);

    // So assuming that I have a static audio process that I want to run elsewhere, all I have to
    // do here is build it so that the engine state is as I want before we return over the ffi.
    //
    // In this "Hello World" of examples, let's imagine I want to just output a tone indefinitely
    // as incoming calls to process continue.
    let cycle = root(sin(mul2(
        constant!({key: None, value: 2.0 * std::f64::consts::PI}),
        phasor(constant!({key: None, value: 110.0})),
    )));

    let _ = main.render(elem::engine::ResolvedDirective {
        graph: Some(vec![cycle]),
        resources: None,
    });

    Box::into_raw(Box::new(EngineHandles { main, proc }))
}

#[no_mangle]
pub extern "C" fn elem_engine_free(ptr: *mut EngineHandles) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(ptr));
    }
}

#[no_mangle]
pub extern "C" fn elem_engine_process(
    ptr: *mut EngineHandles,
    input_data: *const f32,
    output_data: *mut f32,
    num_channels: usize,
    num_frames: usize,
) {
    if ptr.is_null() {
        return;
    }

    let handles = unsafe { &mut *ptr };

    handles
        .proc
        .process(input_data, output_data, num_channels, num_frames);
}
