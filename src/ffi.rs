//! C ABI 导出层（FFI）。
//!
//! 约定（AGENTS.md 硬性约束 5/6 与 DEVELOPMENT.md 第 5 节）：
//! - 导出函数统一前缀 `ipabuyer_core_`，只使用 C ABI；
//! - 所有导出函数体必须经 [`catch`] 包裹（`catch_unwind`），禁止 panic 穿越 FFI；
//! - 动作类导出返回 `i32` 状态码：[`FFI_OK`]=0、[`FFI_ERR_INVALID_ARG`]=1、
//!   [`FFI_ERR_FAILED`]=2、[`FFI_ERR_PANIC`]=3；失败描述经
//!   `ipabuyer_core_last_error()` 获取（线程本地，读取即清除）；
//! - 数据类导出经 `out_json`/`out_handle` 出参写回；JSON 字符串一律 UTF-8；
//! - Core 分配的字符串由宿主调用 `ipabuyer_core_free_string` 释放；
//! - 长任务（同步、下载队列）采用轮询句柄：`*_create` / `*_status` / `*_cancel` / `*_destroy`。

pub mod auth;
pub mod catalog;
pub mod db;
pub mod downloads;
pub mod sync;

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::AssertUnwindSafe;

pub const FFI_OK: i32 = 0;
pub const FFI_ERR_INVALID_ARG: i32 = 1;
pub const FFI_ERR_FAILED: i32 = 2;
pub const FFI_ERR_PANIC: i32 = 3;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// 返回 Core 版本号（语义化版本，与 Cargo.toml 一致；UTF-8、NUL 结尾）。
/// 宿主用毕须调用 [`ipabuyer_core_free_string`]。
#[unsafe(no_mangle)]
pub extern "C" fn ipabuyer_core_version() -> *mut c_char {
    alloc_string(env!("CARGO_PKG_VERSION").to_string())
}

/// 读取当前线程最近一次错误的描述（UTF-8、NUL 结尾；无错误为空串）。
/// 读取即清除。宿主用毕须调用 [`ipabuyer_core_free_string`]。
#[unsafe(no_mangle)]
pub extern "C" fn ipabuyer_core_last_error() -> *mut c_char {
    LAST_ERROR.with(|slot| match slot.borrow_mut().take() {
        Some(text) => text.into_raw(),
        None => CString::default().into_raw(),
    })
}

/// 释放由 Core 返回的字符串；空指针为安全no-op。
///
/// # Safety
/// `ptr` 必须是本 Core 导出函数返回的指针，且只能释放一次。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// 写入"无效参数"错误并返回对应状态码。
pub(crate) fn invalid_arg(name: &'static str) -> i32 {
    set_last_error(format!("invalid argument: {name}"));
    FFI_ERR_INVALID_ARG
}

/// FFI 边界的 panic 拦截：所有导出函数体都应经此包裹。
pub(crate) fn catch<T>(fallback: T, operation: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(AssertUnwindSafe(operation)) {
        Ok(value) => value,
        Err(panic) => {
            set_last_error(format!("panic: {}", panic_message(&panic)));
            fallback
        }
    }
}

pub(crate) fn set_last_error(message: impl Into<String>) {
    let text = message.into().replace('\0', " ");
    if let Ok(cstring) = CString::new(text) {
        LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(cstring));
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// # Safety
/// `ptr` 必须指向有效的 NUL 结尾 UTF-8 字符串（可为空指针，将得到 Err）。
pub(crate) unsafe fn read_cstr<'a>(ptr: *const c_char) -> Result<&'a str, &'static str> {
    if ptr.is_null() {
        return Err("null pointer");
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "invalid utf-8")
}

/// # Safety
/// 同 [`read_cstr`]；空指针映射为 `None`（可选参数）。
pub(crate) unsafe fn read_opt_cstr<'a>(
    ptr: *const c_char,
) -> Result<Option<&'a str>, &'static str> {
    if ptr.is_null() {
        return Ok(None);
    }
    Ok(Some(unsafe { read_cstr(ptr)? }))
}

/// 分配一个由 Core 持有的字符串供宿主读取（宿主用 free_string 释放）。
pub(crate) fn alloc_string(value: String) -> *mut c_char {
    CString::new(value.replace('\0', " "))
        .unwrap_or_default()
        .into_raw()
}

/// 分配一个 JSON 字符串供宿主读取（宿主用 free_string 释放）。
pub(crate) fn alloc_json<T: serde::Serialize>(value: &T) -> *mut c_char {
    match serde_json::to_string(value) {
        Ok(text) => CString::new(text.replace('\0', " "))
            .unwrap_or_default()
            .into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
