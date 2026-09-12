//! 认证导出：登录、双重验证、登出与登录状态查询。
//!
//! JSON 契约：
//! - 登录/验证码结果 → `{"status":"Success","message":{"kind":"key","key":"LoginService/Status/Success","args":[]},"raw_payload":null,"logs":[]}`
//! - `auth_info` → `{"payload":"...","is_success":true,"has_explicit_failure":false,"email":"user@example.com","is_account_missing":false,"logs":[]}`
//! - 登出 → `{"success":true,"logs":[]}`
//! - `is_mock_account` → `true` | `false`
//!
//! `status` 取值：`Success` / `RequiresTwoFactor` / `InvalidCredential` /
//! `AuthCodeInvalid` / `NetworkError` / `Timeout` / `UnknownError`。
//!
//! `detailed_log` 非 0 时，`logs` 携带命令行与输出行（已脱敏）；
//! 为 0 时 `logs` 为空数组。

use std::ffi::c_char;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde_json::json;

use crate::auth::login::{self, LoginResult, LoginStatus, Message};
use crate::ipatool::client::IpatoolClient;
use crate::purchases::sync_service::LogMessage;

use super::*;

/// 判断是否为模拟账户（JSON 布尔）。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8 或 NULL。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_is_mock_account(
    username: *const c_char,
    password: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(username) = (unsafe { read_opt_cstr(username) }) else {
            return invalid_arg("username");
        };
        let Ok(password) = (unsafe { read_opt_cstr(password) }) else {
            return invalid_arg("password");
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let result = login::is_mock_account(username, password);
        unsafe { *out_json = alloc_json(&json!(result)) };
        FFI_OK
    })
}

/// 登录：先用占位验证码触发双重验证码下发。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_auth_login(
    exe_path: *const c_char,
    account: *const c_char,
    password: *const c_char,
    auth_code: *const c_char,
    passphrase: *const c_char,
    detailed_log: i32,
    cancel: *const AtomicBool,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let arguments = unsafe {
            read_login_arguments(exe_path, account, password, auth_code, passphrase, cancel)
        };
        let LoginArguments {
            exe_path,
            account,
            password,
            passphrase,
            cancel,
            ..
        } = match arguments {
            Ok(arguments) => arguments,
            Err(code) => return code,
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let client = IpatoolClient::new(PathBuf::from(&exe_path));
        let mut logs: Vec<LogMessage> = Vec::new();
        let result = if detailed_log != 0 {
            let mut sink = |log: LogMessage| logs.push(log);
            login::login(
                &client,
                &account,
                &password,
                Some(&passphrase),
                cancel,
                Some(&mut sink),
            )
        } else {
            login::login(
                &client,
                &account,
                &password,
                Some(&passphrase),
                cancel,
                None,
            )
        };
        write_login_result(out_json, result, logs)
    })
}

/// 双重验证阶段：携带真实验证码完成登录。
///
/// # Safety
/// 同 [`ipabuyer_core_auth_login`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_auth_verify_code(
    exe_path: *const c_char,
    account: *const c_char,
    password: *const c_char,
    auth_code: *const c_char,
    passphrase: *const c_char,
    detailed_log: i32,
    cancel: *const AtomicBool,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let arguments = unsafe {
            read_login_arguments(exe_path, account, password, auth_code, passphrase, cancel)
        };
        let LoginArguments {
            exe_path,
            account,
            password,
            auth_code,
            passphrase,
            cancel,
        } = match arguments {
            Ok(arguments) => arguments,
            Err(code) => return code,
        };
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let client = IpatoolClient::new(PathBuf::from(&exe_path));
        let mut logs: Vec<LogMessage> = Vec::new();
        let result = if detailed_log != 0 {
            let mut sink = |log: LogMessage| logs.push(log);
            login::verify_auth_code(
                &client,
                &account,
                &password,
                Some(&passphrase),
                &auth_code,
                cancel,
                Some(&mut sink),
            )
        } else {
            login::verify_auth_code(
                &client,
                &account,
                &password,
                Some(&passphrase),
                &auth_code,
                cancel,
                None,
            )
        };
        write_login_result(out_json, result, logs)
    })
}

/// 退出登录（JSON：`{"success":bool,"logs":[]}`）。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_auth_logout(
    exe_path: *const c_char,
    detailed_log: i32,
    cancel: *const AtomicBool,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(exe_path) = (unsafe { read_cstr(exe_path) }) else {
            return invalid_arg("exe_path");
        };
        if cancel.is_null() {
            return invalid_arg("cancel");
        }
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let cancel_flag = unsafe { &*cancel };
        let client = IpatoolClient::new(PathBuf::from(exe_path));
        let mut logs: Vec<LogMessage> = Vec::new();
        let outcome = if detailed_log != 0 {
            let mut sink = |log: LogMessage| logs.push(log);
            client.auth_logout(cancel_flag, Some(&mut sink))
        } else {
            client.auth_logout(cancel_flag, None)
        };
        let success = match outcome {
            Ok(result) => result.is_success_response(),
            Err(_) => false,
        };
        let value = json!({
            "success": success,
            "logs": logs.iter().map(super::sync::log_message_json).collect::<Vec<_>>(),
        });
        unsafe { *out_json = alloc_json(&value) };
        FFI_OK
    })
}

/// 查询登录状态（JSON：`payload` 原文 + 解析出的 `is_success`/`has_explicit_failure`/`email`/`is_account_missing`）。
///
/// # Safety
/// `passphrase` 可为 NULL；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_auth_info(
    exe_path: *const c_char,
    passphrase: *const c_char,
    detailed_log: i32,
    cancel: *const AtomicBool,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(exe_path) = (unsafe { read_cstr(exe_path) }) else {
            return invalid_arg("exe_path");
        };
        let Ok(passphrase) = (unsafe { read_opt_cstr(passphrase) }) else {
            return invalid_arg("passphrase");
        };
        if cancel.is_null() {
            return invalid_arg("cancel");
        }
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let cancel_flag = unsafe { &*cancel };
        let client = IpatoolClient::new(PathBuf::from(exe_path));
        let mut logs: Vec<LogMessage> = Vec::new();
        let outcome = if detailed_log != 0 {
            let mut sink = |log: LogMessage| logs.push(log);
            client.auth_info(passphrase, cancel_flag, Some(&mut sink))
        } else {
            client.auth_info(passphrase, cancel_flag, None)
        };
        let result = match outcome {
            Ok(result) => result,
            Err(_) => {
                set_last_error("canceled");
                return FFI_ERR_FAILED;
            }
        };
        let payload = result.output_or_error_raw();
        let value = json!({
            "payload": payload,
            "is_success": crate::ipatool::response_parser::is_success(Some(&payload)),
            "has_explicit_failure": crate::ipatool::response_parser::has_explicit_failure(Some(&payload)),
            "email": crate::ipatool::response_parser::extract_email(Some(&payload)),
            "is_account_missing": crate::ipatool::response_parser::is_account_missing_from_keyring(Some(&payload)),
            "logs": logs.iter().map(super::sync::log_message_json).collect::<Vec<_>>(),
        });
        unsafe { *out_json = alloc_json(&value) };
        FFI_OK
    })
}

struct LoginArguments {
    exe_path: String,
    account: String,
    password: String,
    auth_code: String,
    passphrase: String,
    cancel: &'static AtomicBool,
}

/// # Safety
/// 各指针须为 NUL 结尾 UTF-8 或 NULL（可选字段）。
unsafe fn read_login_arguments(
    exe_path: *const c_char,
    account: *const c_char,
    password: *const c_char,
    auth_code: *const c_char,
    passphrase: *const c_char,
    cancel: *const AtomicBool,
) -> Result<LoginArguments, i32> {
    macro_rules! read {
        ($ptr:expr, $name:literal) => {
            match unsafe { read_cstr($ptr) } {
                Ok(value) => value.to_string(),
                Err(error) => {
                    set_last_error(format!("invalid argument {}: {error}", $name));
                    return Err(invalid_arg($name));
                }
            }
        };
    }

    if cancel.is_null() {
        return Err(invalid_arg("cancel"));
    }
    // 调用方契约：cancel 标志的生命周期覆盖整个登录调用（同步语义下自动满足）。
    let cancel: &'static AtomicBool = unsafe { &*(cancel as *const AtomicBool) };
    Ok(LoginArguments {
        exe_path: read!(exe_path, "exe_path"),
        account: read!(account, "account"),
        password: read!(password, "password"),
        auth_code: read!(auth_code, "auth_code"),
        passphrase: read!(passphrase, "passphrase"),
        cancel,
    })
}

fn write_login_result(
    out_json: *mut *mut c_char,
    result: LoginResult,
    logs: Vec<LogMessage>,
) -> i32 {
    let message = match &result.message {
        Message::Key { key, args } => json!({
            "kind": "key",
            "key": key,
            "args": args,
        }),
        Message::Raw(text) => json!({
            "kind": "raw",
            "text": text,
        }),
    };
    let value = json!({
        "status": login_status_name(result.status),
        "message": message,
        "raw_payload": result.raw_payload,
        "logs": logs.iter().map(super::sync::log_message_json).collect::<Vec<_>>(),
    });
    unsafe { *out_json = alloc_json(&value) };
    FFI_OK
}

fn login_status_name(status: LoginStatus) -> &'static str {
    match status {
        LoginStatus::Success => "Success",
        LoginStatus::RequiresTwoFactor => "RequiresTwoFactor",
        LoginStatus::InvalidCredential => "InvalidCredential",
        LoginStatus::AuthCodeInvalid => "AuthCodeInvalid",
        LoginStatus::NetworkError => "NetworkError",
        LoginStatus::Timeout => "Timeout",
        LoginStatus::UnknownError => "UnknownError",
    }
}
