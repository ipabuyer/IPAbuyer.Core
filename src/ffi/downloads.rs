//! 下载队列导出：轮询式长任务句柄。
//!
//! JSON 契约：
//! - `queue_add` 入参 → `{"bundle_id":"com.a","id":"1","name":"...","developer":"...",
//!   "artwork_url":null,"price":"free","version":"1.0"}`（`bundle_id`/`price` 必填，
//!   其余可空；`out_result` 写回 0=Added / 1=Updated / 2=Requeued / 3=Ignored）
//! - `queue_status` → `{"running":bool,"completed":null|i32,
//!   "items":[{"bundle_id":"...","status":"Pending","last_message":"..."}],"logs":[...]}`
//!   （`status` 取值 `Pending`/`Downloading`/`Success`/`Failed`/`Canceled`；
//!   `last_message` 为 resw 键名或错误原文，宿主负责渲染）
//! - `logs` 条目同 sync 模块契约

use std::ffi::c_char;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::json;

use crate::appcatalog::search_parser::SearchResult;
use crate::downloads::DownloadQueueStatus;
use crate::downloads::queue::{DownloadQueueService, StartQueueParams};
use crate::ipatool::client::{ClientError, IpatoolClient};
use crate::ipatool::result::IpatoolResult;
use crate::purchases::sync_service::LogMessage;

use super::sync::log_message_json;
use super::*;

/// 下载队列句柄。
pub struct QueueHandle {
    queue: Arc<DownloadQueueService>,
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<QueueState>>,
    worker: Option<thread::JoinHandle<()>>,
    exe_path: String,
    output_directory: String,
    passphrase: String,
    is_mock: bool,
    detailed_log: bool,
}

#[derive(Default)]
struct QueueState {
    running: bool,
    completed: Option<i32>,
    logs: Vec<serde_json::Value>,
}

impl QueueHandle {
    fn is_started(&self) -> bool {
        self.queue.is_running() || self.state.lock().expect("queue state lock").running
    }

    fn record_logs(&self, logs: Vec<LogMessage>) {
        let mut snapshot = self.state.lock().expect("queue state lock");
        for log in logs {
            snapshot.logs.push(log_message_json(&log));
        }
    }
}

/// 创建下载队列。
///
/// # Safety
/// 字符串参数须为 NUL 结尾 UTF-8；`out_handle` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_queue_create(
    exe_path: *const c_char,
    output_directory: *const c_char,
    passphrase: *const c_char,
    is_mock: i32,
    detailed_log: i32,
    out_handle: *mut *mut QueueHandle,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(exe_path) = (unsafe { read_cstr(exe_path) }) else {
            return invalid_arg("exe_path");
        };
        let Ok(output_directory) = (unsafe { read_cstr(output_directory) }) else {
            return invalid_arg("output_directory");
        };
        let Ok(passphrase) = (unsafe { read_cstr(passphrase) }) else {
            return invalid_arg("passphrase");
        };
        if out_handle.is_null() {
            return invalid_arg("out_handle");
        }

        unsafe {
            *out_handle = Box::into_raw(Box::new(QueueHandle {
                queue: Arc::new(DownloadQueueService::new()),
                cancel: Arc::new(AtomicBool::new(false)),
                state: Arc::new(Mutex::new(QueueState::default())),
                worker: None,
                exe_path: exe_path.to_string(),
                output_directory: output_directory.to_string(),
                passphrase: passphrase.to_string(),
                is_mock: is_mock != 0,
                detailed_log: detailed_log != 0,
            }))
        };
        FFI_OK
    })
}

/// 入队一个搜索结果条目；`out_result` 写回 0=Added / 1=Updated / 2=Requeued / 3=Ignored。
///
/// # Safety
/// `handle` 须有效；`item_json` 须为有效 JSON 对象；`out_result` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_queue_add(
    handle: *mut QueueHandle,
    item_json: *const c_char,
    out_result: *mut i32,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }
        let Ok(item_json) = (unsafe { read_cstr(item_json) }) else {
            return invalid_arg("item_json");
        };
        if out_result.is_null() {
            return invalid_arg("out_result");
        }

        let Ok(item) = serde_json::from_str::<QueueItemJson>(item_json) else {
            return invalid_arg("item_json does not match the contract");
        };
        let handle = unsafe { &*handle };
        let mut logs: Vec<LogMessage> = Vec::new();
        let result = handle.queue.add_or_update_from_search_result(
            &item.into_search_result(),
            &mut |log| logs.push(log),
            &mut || {},
        );
        handle.record_logs(logs);
        // 契约：0=Added / 1=Updated / 2=Requeued / 3=Ignored
        let code = match result {
            crate::downloads::queue::AddQueueResult::Added => 0,
            crate::downloads::queue::AddQueueResult::Updated => 1,
            crate::downloads::queue::AddQueueResult::Requeued => 2,
            crate::downloads::queue::AddQueueResult::Ignored => 3,
        };
        unsafe { *out_result = code };
        FFI_OK
    })
}

/// 按 bundle id 移除条目（运行中条目不可移除）。
///
/// # Safety
/// `handle` 须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_queue_remove(
    handle: *mut QueueHandle,
    bundle_id: *const c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        let Ok(bundle_id) = (unsafe { read_cstr(bundle_id) }) else {
            return invalid_arg("bundle_id");
        };
        if handle.is_null() {
            return invalid_arg("handle");
        }

        let handle = unsafe { &*handle };
        let mut logs: Vec<LogMessage> = Vec::new();
        let removed = handle.queue.remove_items(
            |item| item.bundle_id.eq_ignore_ascii_case(bundle_id),
            &mut |log| logs.push(log),
            &mut || {},
        );
        handle.record_logs(logs);
        if removed > 0 {
            FFI_OK
        } else {
            set_last_error("no matching item removed");
            FFI_ERR_FAILED
        }
    })
}

/// 启动队列（派生后台线程，串行处理全部可运行条目）。
///
/// # Safety
/// `handle` 须有效；已在运行时返回 FFI_ERR_FAILED 并写 last_error。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_queue_start(handle: *mut QueueHandle) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }

        let handle = unsafe { &mut *handle };
        if handle.is_started() {
            set_last_error("queue is already started");
            return FFI_ERR_FAILED;
        }

        let queue = Arc::clone(&handle.queue);
        let cancel = Arc::clone(&handle.cancel);
        let state = Arc::clone(&handle.state);
        let exe_path = handle.exe_path.clone();
        let output_directory = handle.output_directory.clone();
        let passphrase = handle.passphrase.clone();
        let is_mock = handle.is_mock;
        let detailed_log = handle.detailed_log;

        state.lock().expect("queue state lock").running = true;
        let worker = thread::spawn(move || {
            let client = IpatoolClient::new(PathBuf::from(exe_path));
            let runner = |item: &crate::downloads::DownloadQueueItem,
                          on_chunk: Option<&(dyn Fn(&str) + Sync)>,
                          cancel: &AtomicBool|
             -> Result<IpatoolResult, ClientError> {
                client.download_app(
                    &item.bundle_id,
                    &output_directory,
                    Some(&passphrase),
                    on_chunk,
                    cancel,
                )
            };

            let completed = queue.start_queue(StartQueueParams {
                output_directory: &output_directory,
                is_mock,
                detailed_log,
                cancel: &cancel,
                runner: &runner,
                on_log: &mut |log| {
                    state
                        .lock()
                        .expect("queue state lock")
                        .logs
                        .push(log_message_json(&log));
                },
                on_change: &mut || {},
            });

            let mut snapshot = state.lock().expect("queue state lock");
            snapshot.running = false;
            snapshot.completed = Some(completed);
        });

        handle.worker = Some(worker);
        FFI_OK
    })
}

/// 轮询队列状态（JSON 结构见模块文档）。
///
/// # Safety
/// `handle` 须有效；`out_json` 须可写。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_queue_status(
    handle: *mut QueueHandle,
    out_json: *mut *mut c_char,
) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }
        if out_json.is_null() {
            return invalid_arg("out_json");
        }

        let handle = unsafe { &*handle };
        let snapshot = handle.state.lock().expect("queue state lock");
        let items: Vec<serde_json::Value> = handle
            .queue
            .items_snapshot()
            .iter()
            .map(|item| {
                json!({
                    "bundle_id": item.bundle_id,
                    "status": status_name(item.status),
                    "last_message": item.last_message,
                })
            })
            .collect();
        let value = json!({
            "running": snapshot.running,
            "completed": snapshot.completed,
            "items": items,
            "logs": snapshot.logs,
        });
        unsafe { *out_json = alloc_json(&value) };
        FFI_OK
    })
}

/// 请求取消队列（取消当前下载并停止；异步生效）。
///
/// # Safety
/// `handle` 须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipabuyer_core_queue_cancel(handle: *mut QueueHandle) -> i32 {
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
pub unsafe extern "C" fn ipabuyer_core_queue_destroy(handle: *mut QueueHandle) -> i32 {
    catch(FFI_ERR_PANIC, || -> i32 {
        if handle.is_null() {
            return invalid_arg("handle");
        }

        let mut handle = unsafe { Box::from_raw(handle) };
        handle.cancel.store(true, Ordering::Relaxed);
        if let Some(worker) = handle.worker.take() {
            let _ = worker.join();
        }
        FFI_OK
    })
}

fn status_name(status: DownloadQueueStatus) -> &'static str {
    match status {
        DownloadQueueStatus::Pending => "Pending",
        DownloadQueueStatus::Downloading => "Downloading",
        DownloadQueueStatus::Success => "Success",
        DownloadQueueStatus::Failed => "Failed",
        DownloadQueueStatus::Canceled => "Canceled",
    }
}

/// 队列入参条目（与搜索结果字段对应）。
#[derive(serde::Deserialize)]
struct QueueItemJson {
    bundle_id: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    developer: Option<String>,
    #[serde(default)]
    artwork_url: Option<String>,
    price: String,
    #[serde(default)]
    version: Option<String>,
}

impl QueueItemJson {
    fn into_search_result(self) -> SearchResult {
        SearchResult {
            bundle_id: self.bundle_id,
            id: self.id,
            name: self.name,
            developer: self.developer,
            artwork_url: self.artwork_url,
            price: self.price,
            version: self.version,
            purchased: String::new(),
        }
    }
}
