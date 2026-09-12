//! App Catalog 搜索导出。
//!
//! JSON 契约：
//! - `catalog_search` 入参 `purchased_apps_json` → `[{"app_id":"...","status":"purchased"}, ...]` 或 NULL
//! - `catalog_search` 出参 → `[{"bundle_id":"...","id":"1","name":"...","developer":"...",
//!    "artwork_url":"...","price":"free","version":"1.0","purchased":"purchased"}, ...]`

use std::collections::HashMap;
use std::ffi::c_char;

use crate::appcatalog::search_client;
use crate::appcatalog::search_parser;

use super::*;

/// 搜索 App Store 并解析为带购买状态的结果列表。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_catalog_search(
    name: *const c_char,
    limit: i64,
    country: *const c_char,
    purchased_apps_json: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(name) = (unsafe { read_cstr(name) }) else {
            return invalid_arg("name");
        };
        let Ok(country) = unsafe { read_opt_cstr(country) }.map(|value| value.unwrap_or("")) else {
            return invalid_arg("country");
        };
        let Ok(purchased_apps_json) = (unsafe { read_opt_cstr(purchased_apps_json) }) else {
            return invalid_arg("purchased_apps_json");
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let purchased_apps = purchased_apps_json
            .and_then(|payload| serde_json::from_str::<Vec<PurchasedAppJson>>(payload).ok())
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| (entry.app_id, entry.status))
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        // 国家/地区合法性由宿主在调用前校验（Core 只做归一化）。
        let response = search_client::search(name, limit, country);
        if response.timed_out {
            set_last_error("catalog search timed out");
            return FFI_ERR_FAILED;
        }

        let payload = response.output_or_error_raw();
        if payload.trim().is_empty() {
            set_last_error("empty catalog response");
            return FFI_ERR_FAILED;
        }

        match search_parser::parse(&payload, &purchased_apps) {
            Some(items) => {
                unsafe { *out_json = alloc_json(&items) };
                FFI_OK
            }
            None => {
                set_last_error("invalid catalog response");
                FFI_ERR_FAILED
            }
        }
    })
}

/// 已购买记录入参条目（用于解析 purchased_apps_json）。
#[derive(serde::Deserialize)]
pub struct PurchasedAppJson {
    pub app_id: String,
    pub status: String,
}
