//! 已购买列表同步导出：轮询式长任务句柄。
//!
//! JSON 契约：
//! - `sync_status` → `{"running":bool,"progress":{"synced":i64,"total":i64},
//!    "outcome":null|{"kind":"completed"|"failed"|"canceled"|"invalid_account",...},"logs":[...]}`
//! - `logs` 条目 → `{"level":"info","message":{"kind":"key","key":"...","args":[...]}}`
//!   或 `{"level":"...","message":{"kind":"raw","text":"..."}}`
//!
//! 生命周期：`sync_create`（派生后台线程）→ 轮询 `sync_status` /
//! `sync_cancel` → `sync_destroy`（取消并等待线程结束）。`db_path` 由宿主
//! 传入，同步线程内自建数据库连接，与宿主连接互不干扰。

use std::ffi::c_char;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::json;

use crate::db::PurchasedAppsDb;
use crate::ipatool::client::IpatoolClient;
use crate::ipatool::response_parser::NormalizedText;
use crate::purchases::sync_service::{LogLevel, LogMessage, PurchaseSyncService};

use super::*;

/// 同步任务句柄。
pub struct SyncHandle {
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<SyncState>>,
    worker: Option<thread::JoinHandle<()>>,
}

#[derive(Default)]
struct SyncState {
    running: bool,
    synced: i64,
    total: i64,
    outcome: Option<serde_json::Value>,
    logs: Vec<serde_json::Value>,
}

/// 创建并启动一次全量同步。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8；`out_handle` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_sync_create(
    db_path: *const c_char,
    exe_path: *const c_char,
    passphrase: *const c_char,
    account: *const c_char,
    detailed_log: i32,
    out_handle: *mut *mut SyncHandle,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(db_path) = (unsafe { read_cstr(db_path) }) else {
            return invalid_arg("db_path");
        };
        let Ok(exe_path) = (unsafe { read_cstr(exe_path) }) else {
            return invalid_arg("exe_path");
        };
        let Ok(passphrase) = (unsafe { read_cstr(passphrase) }) else {
            return invalid_arg("passphrase");
        };
        let Ok(account) = (unsafe { read_cstr(account) }) else {
            return invalid_arg("account");
        };
        if out_handle.is_null() {
            return invalid_arg("out_handle");
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(SyncState {
            running: true,
            ..SyncState::default()
        }));

        let worker_cancel = Arc::clone(&cancel);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || {
            let service = PurchaseSyncService::new();
            let client = IpatoolClient::new(PathBuf::from(exe_path));

            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut db = match PurchasedAppsDb::open(PathBuf::from(&db_path)) {
                    Ok(db) => db,
                    Err(error) => {
                        fail(&worker_state, error.to_string());
                        return;
                    }
                };
                let outcome = service.sync(
                    account,
                    Some(passphrase),
                    &client,
                    &mut db,
                    &worker_cancel,
                    detailed_log != 0,
                    &mut |synced, total| {
                        let mut snapshot = worker_state.lock().expect("sync state lock");
                        snapshot.synced = synced;
                        snapshot.total = total;
                    },
                    &mut |log| {
                        let mut snapshot = worker_state.lock().expect("sync state lock");
                        snapshot.logs.push(log_message_json(&log));
                    },
                );
                finish(&worker_state, &outcome);
            }));
            if run.is_err() {
                fail(&worker_state, "panic in sync worker".to_string());
            }
            let mut snapshot = worker_state.lock().expect("sync state lock");
            snapshot.running = false;
        });

        unsafe {
            *out_handle = Box::into_raw(Box::new(SyncHandle {
                cancel,
                state,
                worker: Some(worker),
            }))
        };
        FFI_OK
    })
}

/// 轮询同步状态（JSON 结构见模块文档）。
///
/// # Safety
/// `handle` 须有效；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_sync_status(
    handle: *mut SyncHandle,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let state = handle_state(handle);
        let snapshot = state.lock().expect("sync state lock");
        let value = json!({
            "running": snapshot.running,
            "progress": { "synced": snapshot.synced, "total": snapshot.total },
            "outcome": snapshot.outcome,
            "logs": snapshot.logs,
        });
        unsafe { *out_json = alloc_json(&value) };
        FFI_OK
    })
}

/// 请求取消同步（异步；完成状态经 `sync_status` 轮询获取）。
///
/// # Safety
/// `handle` 须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_sync_cancel(handle: *mut SyncHandle) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }
        let handle = unsafe { &*handle };
        handle.cancel.store(true, Ordering::Relaxed);
        FFI_OK
    })
}

/// 请求取消并等待线程结束后释放句柄。
///
/// # Safety
/// `handle` 须有效且只能销毁一次；销毁后不得再使用。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_sync_destroy(handle: *mut SyncHandle) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }

        let mut handle = unsafe { Box::from_raw(handle) };
        handle.cancel.store(true, Ordering::Relaxed);
        if let Some(worker) = handle.worker.join_or_noop() {
            let _ = worker;
        }
        FFI_OK
    })
}

trait JoinOrNoop {
    fn join_or_noop(&mut self) -> Option<thread::Result<()>>;
}

impl JoinOrNoop for Option<thread::JoinHandle<()>> {
    fn join_or_noop(&mut self) -> Option<thread::Result<()>> {
        self.take().map(|worker| worker.join())
    }
}

fn handle_state(handle: *mut SyncHandle) -> Arc<Mutex<SyncState>> {
    let handle = unsafe { &*handle };
    Arc::clone(&handle.state)
}

fn finish(state: &Arc<Mutex<SyncState>>, outcome: &crate::purchases::sync_service::SyncOutcome) {
    let mut snapshot = state.lock().expect("sync state lock");
    snapshot.outcome = Some(match outcome {
        crate::purchases::sync_service::SyncOutcome::Completed { synced, total } => json!({
            "kind": "completed", "synced": synced, "total": total,
        }),
        crate::purchases::sync_service::SyncOutcome::AlreadyRunning => json!({
            "kind": "already_running",
        }),
        crate::purchases::sync_service::SyncOutcome::Canceled => json!({
            "kind": "canceled",
        }),
        crate::purchases::sync_service::SyncOutcome::InvalidAccount => json!({
            "kind": "invalid_account",
        }),
        crate::purchases::sync_service::SyncOutcome::Failed { message } => json!({
            "kind": "failed", "message": message,
        }),
    });
}

fn fail(state: &Arc<Mutex<SyncState>>, message: String) {
    let mut snapshot = state.lock().expect("sync state lock");
    snapshot.outcome = Some(json!({ "kind": "failed", "message": message }));
    snapshot.running = false;
}

/// 供队列导出复用的日志序列化。
pub(crate) fn log_message_json(log: &LogMessage) -> serde_json::Value {
    let level = match log.level {
        LogLevel::Info => "info",
        LogLevel::Tip => "tip",
        LogLevel::Success => "success",
        LogLevel::Error => "error",
        LogLevel::Ipatool => "ipatool",
    };
    let message = match &log.message {
        NormalizedText::Keyed { key, args } => json!({
            "kind": "key", "key": key, "args": args,
        }),
        NormalizedText::Raw(text) => json!({
            "kind": "raw", "text": text,
        }),
    };
    json!({ "level": level, "message": message })
}
