//! C ABI 导出层。
//!
//! 约定（AGENTS.md 硬性约束 5/6）：
//! - 导出函数统一前缀 `ipabuyer_core_`，只使用 C ABI；
//! - 所有导出函数体必须 `catch_unwind` 包裹，禁止 panic 穿越 FFI；
//! - Core 分配的字符串由宿主调用 `ipabuyer_core_free_string` 释放。

use std::ffi::{CString, c_char};

/// 返回 Core 版本号（语义化版本，UTF-8、NUL 结尾）。宿主用毕须调用 [`ipabuyer_core_free_string`]。
#[unsafe(no_mangle)]
pub extern "C" fn ipabuyer_core_version() -> *mut c_char {
    catch_unwind_string(env!("CARGO_PKG_VERSION"))
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

/// FFI 边界的 panic 拦截种子：后续所有导出函数都按此模式包裹。
fn catch_unwind_string(value: &'static str) -> *mut c_char {
    std::panic::catch_unwind(|| CString::new(value).unwrap_or_default())
        .unwrap_or_default()
        .into_raw()
}
