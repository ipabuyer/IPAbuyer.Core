//! 购买导出。
//!
//! JSON 契约：
//! - 出参 → `{"outcome":"Purchased","raw_payload":"...","logs":[...]}`
//! - `outcome` 取值：`Purchased` / `AlreadyOwned` / `NeedsOwnedConfirmation` / `Failed`
//!   （`Skipped` 属宿主侧前置策略，Core 不会返回）
//! - 取消：返回 [`FFI_ERR_FAILED`]，`last_error` 为 `canceled`
//!
//! `detailed_log` 非 0 时，`logs` 携带命令行与输出行（已脱敏）。

use std::ffi::c_char;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde_json::json;

use crate::ipatool::client::{ClientError, IpatoolClient};
use crate::purchases::response_interpreter::{self, PurchaseOutcome};
use crate::purchases::sync_service::LogMessage;

use super::sync::log_message_json;
use super::*;

/// 执行购买命令并解释结果。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_purchase(
    exe_path: *const c_char,
    bundle_id: *const c_char,
    passphrase: *const c_char,
    detailed_log: i32,
    cancel: *const AtomicBool,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(exe_path) = (unsafe { read_cstr(exe_path) }) else {
            return invalid_arg("exe_path");
        };
        let Ok(bundle_id) = (unsafe { read_cstr(bundle_id) }) else {
            return invalid_arg("bundle_id");
        };
        let Ok(passphrase) = (unsafe { read_cstr(passphrase) }) else {
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
        let outcome_result = if detailed_log != 0 {
            let mut sink = |log: LogMessage| logs.push(log);
            client.purchase_app(bundle_id, Some(passphrase), cancel_flag, Some(&mut sink))
        } else {
            client.purchase_app(bundle_id, Some(passphrase), cancel_flag, None)
        };

        let (outcome_name, raw_payload) = match outcome_result {
            Err(ClientError::Canceled) => {
                set_last_error("canceled");
                return FFI_ERR_FAILED;
            }
            Ok(result) => {
                let payload = result.output_or_error_raw();
                let payload_ref = if payload.trim().is_empty() {
                    None
                } else {
                    Some(payload.as_str())
                };
                (response_interpreter::interpret(payload_ref), Some(payload))
            }
        };

        let value = json!({
            "outcome": purchase_outcome_name(outcome_name),
            "raw_payload": raw_payload,
            "logs": logs.iter().map(log_message_json).collect::<Vec<_>>(),
        });
        unsafe { *out_json = alloc_json(&value) };
        FFI_OK
    })
}

fn purchase_outcome_name(outcome: PurchaseOutcome) -> &'static str {
    match outcome {
        PurchaseOutcome::Skipped => "Skipped",
        PurchaseOutcome::Purchased => "Purchased",
        PurchaseOutcome::AlreadyOwned => "AlreadyOwned",
        PurchaseOutcome::NeedsOwnedConfirmation => "NeedsOwnedConfirmation",
        PurchaseOutcome::Failed => "Failed",
    }
}
