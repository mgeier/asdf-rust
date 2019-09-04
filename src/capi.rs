// https://dev.to/luzero/building-crates-so-they-look-like-c-abi-libraries-1ibn
// https://github.com/lu-zero/cargo-c

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::fmt::Display;
use std::panic::{catch_unwind, UnwindSafe};

use libc::c_char;

use crate::{Scene, Transform};

#[repr(C)]
#[derive(Default)]
pub struct AsdfTransform {
    flags: u8,
    pos: [f32; 3],
}

pub const ASDF_TRANSFORM_ACTIVE: u8 = 1;
pub const ASDF_TRANSFORM_POS: u8 = 1 << 1;

impl From<Option<Transform>> for AsdfTransform {
    fn from(t: Option<Transform>) -> AsdfTransform {
        let mut result = AsdfTransform::default();
        if let Some(t) = t {
            result.flags |= ASDF_TRANSFORM_ACTIVE;
            if let Some(pos) = t.translation {
                result.flags |= ASDF_TRANSFORM_POS;
                result.pos.copy_from_slice(pos.as_slice());
            }
        }
        result
    }
}

#[no_mangle]
pub unsafe extern "C" fn asdf_scene_new(
    filename: *const c_char,
    samplerate: u32,
    blocksize: u32,
    buffer_duration: f32,
) -> *mut Scene {
    handle_errors(
        || {
            let filename = CStr::from_ptr(filename).to_str().unwrap_display();
            Box::into_raw(Box::new(
                Scene::new(filename, samplerate, blocksize, buffer_duration).unwrap_display(),
            ))
        },
        std::ptr::null_mut(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn asdf_scene_free(ptr: *mut Scene) {
    if !ptr.is_null() {
        Box::from_raw(ptr);
    }
}

#[no_mangle]
pub unsafe extern "C" fn asdf_scene_file_sources(ptr: *mut Scene) -> u32 {
    assert!(!ptr.is_null());
    let scene = &mut *ptr;
    scene.file_sources()
}

#[no_mangle]
pub unsafe extern "C" fn asdf_scene_get_source_id(
    ptr: *mut Scene,
    index: libc::size_t,
) -> *mut c_char {
    // TODO: use handle_errors() once the ring buffer is UnwindSafe
    assert!(!ptr.is_null());
    let scene = &mut *ptr;
    CString::new(scene.get_source_id(index)).unwrap().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn asdf_string_free(string: *mut c_char) {
    if !string.is_null() {
        CString::from_raw(string);
    }
}

#[no_mangle]
pub unsafe extern "C" fn asdf_scene_get_source_transform(
    ptr: *mut Scene,
    source_idx: libc::size_t,
    frame: u64,
) -> AsdfTransform {
    // TODO: use handle_errors() once the ring buffer is UnwindSafe
    assert!(!ptr.is_null());
    let scene = &mut *ptr;
    scene.get_source_transform(source_idx, frame).into()
}

// TODO: possibility to report errors?
#[no_mangle]
pub unsafe extern "C" fn asdf_scene_seek(ptr: *mut Scene, frame: u64) -> bool {
    assert!(!ptr.is_null());
    let scene = &mut *ptr;
    scene.seek(frame)
}

/// Return value of `false` means un-recoverable error
#[no_mangle]
pub unsafe extern "C" fn asdf_scene_get_audio_data(
    ptr: *mut Scene,
    data: *const *mut f32,
    rolling: bool,
) -> bool {
    // TODO: use handle_errors() once the ring buffer is UnwindSafe
    assert!(!ptr.is_null());
    let scene = &mut *ptr;
    assert!(!data.is_null());
    let data = std::slice::from_raw_parts(data, scene.file_sources() as usize);
    // TODO: get error message if something is wrong!
    scene.get_audio_data(data, rolling)
}

/// The error message will be freed if another error occurs. It is the caller's
/// responsibility to make sure they're no longer using the string before
/// calling any other function which may fail.
#[no_mangle]
pub extern "C" fn asdf_scene_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::new("no error").unwrap());
}

fn set_error<D: Display>(error: D) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(error.to_string()).unwrap();
    });
}

fn handle_errors<F, T>(f: F, optb: T) -> T
where
    F: FnOnce() -> T + UnwindSafe,
{
    match catch_unwind(f) {
        Ok(value) => value,
        Err(e) => {
            if let Some(e) = e.downcast_ref::<&str>() {
                set_error(*e);
            } else if let Some(e) = e.downcast_ref::<String>() {
                set_error(e);
            } else {
                set_error("unknown error");
            }
            optb
        }
    }
}

trait ResultExt<T, E: Display> {
    fn unwrap_display(self) -> T;
}

impl<T, E: Display> ResultExt<T, E> for Result<T, E> {
    fn unwrap_display(self) -> T {
        match self {
            Ok(value) => value,
            Err(e) => panic!(e.to_string()),
        }
    }
}
