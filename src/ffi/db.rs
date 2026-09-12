//! 数据库导出：句柄式 CRUD 与同步状态。
//!
//! JSON 契约（snake_case）：
//! - `db_get_purchased_apps` → `[{"app_id":"...","status":"purchased"}, ...]`
//! - `db_get_app_status` → `"purchased"` | `"not_purchased"` | `null`
//! - `db_get_last_sync_utc` → `"RFC 3339 时间"` | `null`
//! - `db_bulk_mark_purchased` 入参 → `["com.a", "com.b", ...]`

use std::ffi::c_char;
use std::path::PathBuf;

use crate::db::PurchasedAppsDb;

use super::*;

/// 打开（必要时创建并迁移）数据库，返回句柄。
///
/// # Safety
/// `path` 须为有效字符串；`out_handle` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_open(
    path: *const c_char,
    out_handle: *mut *mut PurchasedAppsDb,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(path) = (unsafe { read_cstr(path) }) else {
            return invalid_arg("path");
        };
        if out_handle.is_null() {
            return invalid_arg("out_handle");
        }

        match PurchasedAppsDb::open(PathBuf::from(path)) {
            Ok(db) => {
                unsafe { *out_handle = Box::into_raw(Box::new(db)) };
                FFI_OK
            }
            Err(error) => {
                set_last_error(error.to_string());
                FFI_ERR_FAILED
            }
        }
    })
}

/// 关闭并释放数据库句柄。
///
/// # Safety
/// `handle` 须为 `db_open` 返回且未被关闭；关闭后不得再使用。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_close(handle: *mut PurchasedAppsDb) -> i32 {
    catch(FFI_ERR_PANIC, || {
        if handle.is_null() {
            return invalid_arg("handle");
        }
        drop(unsafe { Box::from_raw(handle) });
        FFI_OK
    })
}

/// 保存已购买应用（upsert；`status` 可为 NULL，默认已购买）。
///
/// # Safety
/// `handle` 须有效；字符串参数须为 NUL 结尾 UTF-8。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_save_purchased_app(
    handle: *mut PurchasedAppsDb,
    app_id: *const c_char,
    account: *const c_char,
    status: *const c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(app_id) = (unsafe { read_cstr(app_id) }) else {
            return invalid_arg("app_id");
        };
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };
        let Ok(status) = (unsafe { read_opt_cstr(status) }) else {
            return invalid_arg("status");
        };

        with_db(handle, |db| db.save_purchased_app(app_id, account, status))
    })
}

/// 获取账户全部已购买应用（JSON 数组）。
///
/// # Safety
/// `handle` 须有效；`out_json` 须可写（用 `free_string` 释放）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_get_purchased_apps(
    handle: *mut PurchasedAppsDb,
    account: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        with_db_json(handle, out_json, |db| {
            let apps = db.get_purchased_apps(account)?;
            Ok(serde_json::to_value(apps.iter().map(|(app_id, status)| {
                serde_json::json!({ "app_id": app_id, "status": status })
            }).collect::<Vec<_>>())?)
        })
    })
}

/// 查询单个 App 状态（JSON：`"purchased"` | `"not_purchased"` | `null`）。
///
/// # Safety
/// 同 [`ipabuyer_core_db_get_purchased_apps`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_get_app_status(
    handle: *mut PurchasedAppsDb,
    app_id: *const c_char,
    account: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(app_id) = (unsafe { read_cstr(app_id) }) else {
            return invalid_arg("app_id");
        };
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        with_db_json(handle, out_json, |db| {
            let status = db.get_app_status(app_id, account)?;
            let status = status
                .as_deref()
                .map(|value| crate::purchases::status_policy::normalize_stored_status(Some(value)));
            Ok(serde_json::to_value(status)?)
        })
    })
}

/// 删除指定 App 的记录。
///
/// # Safety
/// `handle` 须有效；字符串参数须为 NUL 结尾 UTF-8。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_remove_purchased_app(
    handle: *mut PurchasedAppsDb,
    app_id: *const c_char,
    account: *const c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(app_id) = (unsafe { read_cstr(app_id) }) else {
            return invalid_arg("app_id");
        };
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };

        with_db(handle, |db| db.remove_purchased_app(app_id, account))
    })
}

/// 清除记录；`account` 为 NULL 时清除全部。
///
/// # Safety
/// `handle` 须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_clear_purchased_apps(
    handle: *mut PurchasedAppsDb,
    account: *const c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(account) = (unsafe { read_opt_cstr(account) }) else {
            return invalid_arg("account");
        };

        with_db(handle, |db| db.clear_purchased_apps(account))
    })
}

/// 记录总数（`account` 为 NULL 时统计全部），写入 `out_count`。
///
/// # Safety
/// `handle` 与 `out_count` 须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_get_total_count(
    handle: *mut PurchasedAppsDb,
    account: *const c_char,
    out_count: *mut i64,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(account) = (unsafe { read_opt_cstr(account) }) else {
            return invalid_arg("account");
        };
        if out_count.is_null() {
            return invalid_arg("out_count");
        }

        with_db(handle, |db| {
            db.get_total_count(account).map(|count| unsafe {
                *out_count = count;
            })
        })
    })
}

/// 批量标记为已购买；入参为 bundle id 的 JSON 数组。
///
/// # Safety
/// `handle` 须有效；`bundle_ids_json` 须为有效 JSON 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_bulk_mark_purchased(
    handle: *mut PurchasedAppsDb,
    bundle_ids_json: *const c_char,
    account: *const c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(bundle_ids_json) = (unsafe { read_cstr(bundle_ids_json) }) else {
            return invalid_arg("bundle_ids_json");
        };
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };

        let Ok(bundle_ids) = serde_json::from_str::<Vec<String>>(bundle_ids_json) else {
            return invalid_arg("bundle_ids_json is not a JSON string array");
        };

        with_db(handle, |db| {
            db.bulk_mark_purchased(&bundle_ids, account).map(|_| ())
        })
    })
}

/// 上次成功同步时间（JSON：RFC 3339 字符串或 `null`）。
///
/// # Safety
/// `handle` 须有效；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_get_last_sync_utc(
    handle: *mut PurchasedAppsDb,
    account: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        with_db_json(handle, out_json, |db| {
            let last = db.get_last_successful_sync_utc(account)?;
            Ok(serde_json::to_value(last)?)
        })
    })
}

/// 记录一次同步尝试（`succeeded` 非 0 即成功）。
///
/// # Safety
/// `handle` 须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_db_record_sync_attempt(
    handle: *mut PurchasedAppsDb,
    account: *const c_char,
    succeeded: i32,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };

        with_db(handle, |db| db.record_sync_attempt(account, succeeded != 0))
    })
}

fn with_db<F>(handle: *mut PurchasedAppsDb, operation: F) -> i32
where
    F: FnOnce(&PurchasedAppsDb) -> crate::db::Result<()>,
{
    if handle.is_null() {
        return invalid_arg("handle");
    }
    let db = unsafe { &*handle };
    match operation(db) {
        Ok(()) => FFI_OK,
        Err(error) => {
            set_last_error(error.to_string());
            FFI_ERR_FAILED
        }
    }
}

fn with_db_json<F>(handle: *mut PurchasedAppsDb, out_json: *mut *mut c_char, operation: F) -> i32
where
    F: FnOnce(&PurchasedAppsDb) -> std::result::Result<serde_json::Value, crate::db::DbError>,
{
    if handle.is_null() {
        return invalid_arg("handle");
    }
    let db = unsafe { &*handle };
    match operation(db) {
        Ok(value) => {
            unsafe { *out_json = alloc_json(&value) };
            FFI_OK
        }
        Err(error) => {
            set_last_error(error.to_string());
            FFI_ERR_FAILED
        }
    }
}
