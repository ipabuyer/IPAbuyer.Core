//! FFI 端到端测试：经导出函数走通数据库、登录（mock）与长任务轮询。
//! Windows 专属用例通过假 ipatool 脚本（`.cmd`）覆盖同步与下载队列全链路。

use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use ipabuyer_core::ffi::{self, FFI_ERR_FAILED, FFI_ERR_INVALID_ARG, FFI_OK};
use ipabuyer_core::ipatool::response_parser::NormalizedText;
use ipabuyer_core::purchases::sync_service::{LogLevel, LogMessage};

fn cstring(value: &str) -> CString {
    CString::new(value).unwrap()
}

fn read_json(ptr: *mut c_char) -> serde_json::Value {
    let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    unsafe { ffi::ipabuyer_core_free_string(ptr) };
    serde_json::from_str(&text).expect("valid json")
}

fn unique_temp(tag: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "ipabuyer_core_ffi_{tag}_{}_{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    path
}

#[test]
fn version_matches_crate_version() {
    let ptr = ffi::ipabuyer_core_version();
    let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    unsafe { ffi::ipabuyer_core_free_string(ptr) };
    assert_eq!(text, env!("CARGO_PKG_VERSION"));
}

#[test]
fn invalid_arguments_report_last_error() {
    let mut handle: *mut ipabuyer_core::db::PurchasedAppsDb = std::ptr::null_mut();
    let code = unsafe { ffi::db::ipabuyer_core_db_open(std::ptr::null(), &mut handle) };
    assert_eq!(code, FFI_ERR_INVALID_ARG);

    let error = ffi::ipabuyer_core_last_error();
    let text = unsafe { CStr::from_ptr(error) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { ffi::ipabuyer_core_free_string(error) };
    assert!(
        text.contains("path"),
        "last error should mention the argument: {text}"
    );
}

#[test]
fn db_roundtrip_via_ffi() {
    let path = unique_temp("db");
    let path_c = cstring(&path.to_string_lossy());
    let mut handle = std::ptr::null_mut();
    assert_eq!(
        unsafe { ffi::db::ipabuyer_core_db_open(path_c.as_ptr(), &mut handle) },
        FFI_OK
    );

    let save = cstring("com.e2e");
    let account = cstring("user@test.com");
    assert_eq!(
        unsafe {
            ffi::db::ipabuyer_core_db_save_purchased_app(
                handle,
                save.as_ptr(),
                account.as_ptr(),
                std::ptr::null(),
            )
        },
        FFI_OK
    );

    let mut out = std::ptr::null_mut();
    assert_eq!(
        unsafe { ffi::db::ipabuyer_core_db_get_purchased_apps(handle, account.as_ptr(), &mut out) },
        FFI_OK
    );
    let value = read_json(out);
    assert_eq!(value[0]["app_id"], "com.e2e");
    assert_eq!(value[0]["status"], "purchased");

    let ids = cstring(r#"["com.e2e", "com.other"]"#);
    assert_eq!(
        unsafe {
            ffi::db::ipabuyer_core_db_bulk_mark_purchased(handle, ids.as_ptr(), account.as_ptr())
        },
        FFI_OK
    );

    let mut count: i64 = 0;
    assert_eq!(
        unsafe { ffi::db::ipabuyer_core_db_get_total_count(handle, std::ptr::null(), &mut count) },
        FFI_OK
    );
    assert_eq!(count, 2);

    assert_eq!(unsafe { ffi::db::ipabuyer_core_db_close(handle) }, FFI_OK);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn mock_login_via_ffi_returns_success() {
    let exe = cstring("irrelevant-for-mock.cmd");
    let account = cstring("test");
    let password = cstring("test");
    let auth_code = cstring("000000");
    let passphrase = cstring("passphrase");
    let cancel = AtomicBool::new(false);
    let mut out = std::ptr::null_mut();

    let code = unsafe {
        ffi::auth::ipabuyer_core_auth_login(
            exe.as_ptr(),
            account.as_ptr(),
            password.as_ptr(),
            auth_code.as_ptr(),
            passphrase.as_ptr(),
            0,
            &cancel,
            &mut out,
        )
    };

    assert_eq!(code, FFI_OK);
    let value = read_json(out);
    assert_eq!(value["status"], "Success");
    assert_eq!(value["message"]["kind"], "key");
    assert_eq!(value["message"]["key"], "LoginService/Status/Success");
}

#[test]
fn is_mock_account_via_ffi() {
    let username = cstring("TEST");
    let password = cstring("test");
    let mut out = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            ffi::auth::ipabuyer_core_is_mock_account(username.as_ptr(), password.as_ptr(), &mut out)
        },
        FFI_OK
    );
    assert_eq!(read_json(out), serde_json::json!(true));
}

fn write_fake_ipatool(tag: &str, output_lines: &[&str]) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "ipabuyer_core_ffi_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let script = directory.join("fake_ipatool.cmd");
    let mut content = String::from("@echo off\r\n");
    for line in output_lines {
        content.push_str(&format!("echo {line}\r\n"));
    }
    std::fs::write(&script, content).unwrap();
    script
}

fn poll_until(deadline: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    predicate()
}

#[cfg(windows)]
#[test]
fn sync_end_to_end_with_fake_ipatool() {
    let script = write_fake_ipatool(
        "sync",
        &[
            r#"{"level":"info","count":1,"totalCount":1,"page":1,"apps":[{"bundleID":"com.e2e.sync","name":"E2E","price":0}]}"#,
        ],
    );
    let script_c = cstring(&script.to_string_lossy());
    let db_path = unique_temp("sync_db");
    let db_path_c = cstring(&db_path.to_string_lossy());
    let passphrase = cstring("passphrase");
    let account = cstring("user@test.com");
    let mut handle = std::ptr::null_mut();

    assert_eq!(
        unsafe {
            ffi::sync::ipabuyer_core_sync_create(
                db_path_c.as_ptr(),
                script_c.as_ptr(),
                passphrase.as_ptr(),
                account.as_ptr(),
                0,
                &mut handle,
            )
        },
        FFI_OK
    );

    let completed = poll_until(Duration::from_secs(30), || {
        let mut out = std::ptr::null_mut();
        unsafe { ffi::sync::ipabuyer_core_sync_status(handle, &mut out) };
        let status = read_json(out);
        status["outcome"].is_object()
    });
    assert!(completed, "sync must finish within the deadline");

    let mut out = std::ptr::null_mut();
    unsafe { ffi::sync::ipabuyer_core_sync_status(handle, &mut out) };
    let status = read_json(out);
    assert!(!status["running"].as_bool().unwrap());
    assert_eq!(status["outcome"]["kind"], "completed");
    assert_eq!(status["outcome"]["synced"], 1);
    assert!(status["logs"].as_array().expect("logs").len() >= 2);

    unsafe { ffi::sync::ipabuyer_core_sync_destroy(handle) };

    // 验证同步真的把已拥有 App 写进了数据库。
    let mut db = std::ptr::null_mut();
    assert_eq!(
        unsafe { ffi::db::ipabuyer_core_db_open(db_path_c.as_ptr(), &mut db) },
        FFI_OK
    );
    let account = cstring("user@test.com");
    let mut out = std::ptr::null_mut();
    unsafe { ffi::db::ipabuyer_core_db_get_purchased_apps(db, account.as_ptr(), &mut out) };
    let apps = read_json(out);
    assert_eq!(apps[0]["app_id"], "com.e2e.sync");
    assert_eq!(apps[0]["status"], "purchased");
    assert_eq!(unsafe { ffi::db::ipabuyer_core_db_close(db) }, FFI_OK);

    let _ = std::fs::remove_file(&db_path);
}

#[cfg(windows)]
#[test]
fn queue_end_to_end_with_fake_ipatool() {
    let output_directory = unique_temp("queue_out");
    std::fs::create_dir_all(&output_directory).unwrap();
    let script = write_fake_ipatool("queue", &[r#"{"success":true,"email":" e2e@test.com "}"#]);
    let script_c = cstring(&script.to_string_lossy());
    let output_dir_c = cstring(&output_directory.to_string_lossy());
    let passphrase = cstring("passphrase");
    let mut handle = std::ptr::null_mut();

    assert_eq!(
        unsafe {
            ffi::downloads::ipabuyer_core_queue_create(
                script_c.as_ptr(),
                output_dir_c.as_ptr(),
                passphrase.as_ptr(),
                0,
                1,
                &mut handle,
            )
        },
        FFI_OK
    );

    let item_json = cstring(
        r#"{"bundle_id":"com.e2e.queue","id":"42","name":"E2E App","developer":"Dev","artwork_url":null,"price":"free","version":"1.0"}"#,
    );
    let mut add_result: i32 = -1;
    assert_eq!(
        unsafe {
            ffi::downloads::ipabuyer_core_queue_add(handle, item_json.as_ptr(), &mut add_result)
        },
        FFI_OK
    );
    assert_eq!(add_result, 0, "first add should be Added");

    assert_eq!(
        unsafe { ffi::downloads::ipabuyer_core_queue_start(handle) },
        FFI_OK
    );

    let finished = poll_until(Duration::from_secs(30), || {
        let mut out = std::ptr::null_mut();
        unsafe { ffi::downloads::ipabuyer_core_queue_status(handle, &mut out) };
        let status = read_json(out);
        !status["running"].as_bool().unwrap()
    });
    assert!(finished, "queue must finish within the deadline");

    let mut out = std::ptr::null_mut();
    unsafe { ffi::downloads::ipabuyer_core_queue_status(handle, &mut out) };
    let status = read_json(out);
    assert_eq!(status["completed"], 1);
    assert_eq!(status["items"][0]["status"], "Success");
    assert!(status["logs"].as_array().expect("logs").len() >= 2);

    assert_eq!(
        unsafe { ffi::downloads::ipabuyer_core_queue_destroy(handle) },
        FFI_OK
    );
    let _ = std::fs::remove_dir_all(&output_directory);
    let _ = std::fs::remove_dir_all(script.parent().unwrap());
}

#[cfg(windows)]
#[test]
fn purchase_via_fake_ipatool_returns_outcome_and_logs() {
    let script = write_fake_ipatool(
        "purchase",
        &[r#"{"success":true,"email":" e2e@test.com "}"#],
    );
    let script_c = cstring(&script.to_string_lossy());
    let bundle_id = cstring("com.e2e.purchase");
    let passphrase = cstring("passphrase");
    let cancel = AtomicBool::new(false);
    let mut out = std::ptr::null_mut();

    // detailed_log = 1：命令行与输出应进入 logs 且密钥已脱敏。
    assert_eq!(
        unsafe {
            ffi::purchase::ipabuyer_core_purchase(
                script_c.as_ptr(),
                bundle_id.as_ptr(),
                passphrase.as_ptr(),
                1,
                &cancel,
                &mut out,
            )
        },
        FFI_OK
    );

    let value = read_json(out);
    assert_eq!(value["outcome"], "Purchased");
    assert!(value["raw_payload"].as_str().unwrap().contains("success"));

    let logs = value["logs"].as_array().expect("logs");
    let rendered: Vec<String> = logs
        .iter()
        .filter(|log| log["level"] == "ipatool")
        .map(|log| {
            log["message"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert!(
        rendered
            .iter()
            .any(|line| line.starts_with("ipatool purchase") && line.contains("\"***\"")),
        "command line log missing or not sanitized: {rendered:?}"
    );

    // 取消路径：cancel 置位后应返回失败码并写 last_error。
    let cancel_now = AtomicBool::new(true);
    let mut out_cancel = std::ptr::null_mut();
    let code = unsafe {
        ffi::purchase::ipabuyer_core_purchase(
            script_c.as_ptr(),
            bundle_id.as_ptr(),
            passphrase.as_ptr(),
            0,
            &cancel_now,
            &mut out_cancel,
        )
    };
    assert_eq!(code, FFI_ERR_FAILED);
    let error = ffi::ipabuyer_core_last_error();
    let text = unsafe { CStr::from_ptr(error) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { ffi::ipabuyer_core_free_string(error) };
    assert_eq!(text, "canceled");

    let _ = std::fs::remove_dir_all(script.parent().unwrap());
}

#[cfg(windows)]
#[test]
fn sync_create_copies_strings_before_worker_reads_them() {
    // 回归：宿主字符串仅在调用期间有效，create 返回后立即释放（模拟 C# 门面 finally 的释放时序），
    // 工作线程必须使用 create 内复制的 owned 字符串。
    let script = write_fake_ipatool(
        "sync_copy",
        &[
            r#"{"level":"info","count":1,"totalCount":1,"page":1,"apps":[{"bundleID":"com.e2e.copy","name":"E2E","price":0}]}"#,
        ],
    );
    let db_path = unique_temp("sync_copy_db");
    let passphrase = cstring("passphrase");
    let account = cstring("user@test.com");
    let mut handle = std::ptr::null_mut();

    let code = {
        let db_path_c = cstring(&db_path.to_string_lossy());
        let script_c = cstring(&script.to_string_lossy());
        unsafe {
            ffi::sync::ipabuyer_core_sync_create(
                db_path_c.as_ptr(),
                script_c.as_ptr(),
                passphrase.as_ptr(),
                account.as_ptr(),
                0,
                &mut handle,
            )
        }
    };
    assert_eq!(code, FFI_OK);

    let completed = poll_until(Duration::from_secs(30), || {
        let mut out = std::ptr::null_mut();
        unsafe { ffi::sync::ipabuyer_core_sync_status(handle, &mut out) };
        let status = read_json(out);
        status["outcome"].is_object()
    });
    assert!(completed, "sync must finish within the deadline");

    let mut out = std::ptr::null_mut();
    unsafe { ffi::sync::ipabuyer_core_sync_status(handle, &mut out) };
    let status = read_json(out);
    assert_eq!(
        status["outcome"]["kind"], "completed",
        "worker must operate on copied strings, outcome: {:?}",
        status["outcome"]
    );

    unsafe { ffi::sync::ipabuyer_core_sync_destroy(handle) };
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(script.parent().unwrap());
}

#[test]
fn normalized_text_is_exhaustively_shaped() {
    // 确认 FFI 日志契约的两种形态可被稳定区分。
    let keyed = NormalizedText::Keyed {
        key: "PurchaseSync/Log/Start",
        args: vec!["user".to_string()],
    };
    let raw = LogMessage::raw(LogLevel::Info, "plain".to_string()).message;

    assert!(matches!(keyed, NormalizedText::Keyed { .. }));
    assert!(matches!(raw, NormalizedText::Raw(_)));
}
