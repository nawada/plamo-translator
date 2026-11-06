//! 推論エンジン (llama.cpp の llama-server) のダウンロード・起動・停止

use crate::catalog;
use crate::settings::{EngineMode, Settings};
use crate::state::{AppState, EngineStatus, ServerProcess};
use futures_util::StreamExt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

pub const SERVED_MODEL_NAME: &str = "plamo-3-translate";

pub fn set_status(app: &AppHandle, status: EngineStatus) {
    let state = app.state::<AppState>();
    *state.status.lock().unwrap() = status.clone();
    let _ = app.emit("engine-status", status);
}

/// 不足しているファイルと、そのダウンロード量
fn missing_files(settings: &Settings, data_dir: &Path) -> (Vec<String>, u64) {
    let mut missing = Vec::new();
    let mut bytes = 0;
    if !settings.server_path(data_dir).exists() {
        if settings.custom_server_path.trim().is_empty() {
            let b = catalog::find_backend(&settings.backend).unwrap();
            missing.push(format!("llama.cpp ({})", b.label));
            bytes += b.approx_bytes;
        } else {
            missing.push(format!("llama-server が見つかりません: {}", settings.custom_server_path));
        }
    }
    match settings.model_path(data_dir) {
        Some(p) if p.exists() => {}
        _ => {
            if settings.custom_model_path.trim().is_empty() {
                let m = catalog::find_model(&settings.model_size, &settings.model_quant).unwrap();
                missing.push(format!("PLaMo 3 Translate {} ({})", m.size.to_uppercase(), m.quant));
                bytes += m.bytes;
            } else {
                missing.push(format!("モデルが見つかりません: {}", settings.custom_model_path));
            }
        }
    }
    (missing, bytes)
}

/// 設定に従ってエンジンを起動する。必要なファイルが無い場合は SetupRequired にする。
pub async fn start(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _guard = state.engine_lock.lock().await;
    start_locked(app).await;
}

async fn start_locked(app: &AppHandle) {
    let state = app.state::<AppState>();
    stop_server(&state);
    let settings = state.settings();

    if settings.engine_mode == EngineMode::External {
        set_status(app, EngineStatus::Starting);
        let base = settings.external_url.trim_end_matches('/').to_string();
        let ok = state
            .local_http
            .get(format!("{}/v1/models", base))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            set_status(app, EngineStatus::Ready { endpoint: base, model: settings.external_model.clone() });
        } else {
            set_status(
                app,
                EngineStatus::Error {
                    message: format!("外部サーバー {} に接続できません。サーバーが起動しているか確認してください。", base),
                },
            );
        }
        return;
    }

    let (missing, bytes) = missing_files(&settings, &state.data_dir);
    if !missing.is_empty() {
        set_status(app, EngineStatus::SetupRequired { missing, download_bytes: bytes });
        return;
    }

    set_status(app, EngineStatus::Starting);
    match spawn_server(&settings, &state).await {
        Ok(endpoint) => set_status(
            app,
            EngineStatus::Ready { endpoint, model: SERVED_MODEL_NAME.into() },
        ),
        Err(e) => {
            stop_server(&state);
            set_status(app, EngineStatus::Error { message: e });
        }
    }
}

/// 不足ファイルをダウンロードしてからエンジンを起動する
pub async fn install_and_start(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _guard = state.engine_lock.lock().await;
    state.cancel_download.store(false, Ordering::SeqCst);
    let settings = state.settings();

    let result = install_locked(app, &settings).await;
    match result {
        Ok(()) => {
            start_locked(app).await;
            Ok(())
        }
        Err(e) => {
            let (missing, bytes) = missing_files(&settings, &state.data_dir);
            if state.cancel_download.load(Ordering::SeqCst) {
                set_status(app, EngineStatus::SetupRequired { missing, download_bytes: bytes });
            } else {
                set_status(app, EngineStatus::Error { message: e.clone() });
            }
            Err(e)
        }
    }
}

async fn install_locked(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let data_dir = state.data_dir.clone();

    // llama.cpp
    if settings.custom_server_path.trim().is_empty() && !settings.server_path(&data_dir).exists() {
        let backend = catalog::find_backend(&settings.backend).ok_or("不明なバックエンドです")?;
        let dir = catalog::backend_dir(&data_dir, backend.id);
        let dl_dir = data_dir.join("downloads");
        for (url, name) in backend.urls() {
            let zip_path = dl_dir.join(&name);
            download(app, &url, &zip_path, &format!("llama.cpp ({})", name)).await?;
            set_status(app, EngineStatus::Extracting);
            let (zp, d) = (zip_path.clone(), dir.clone());
            tokio::task::spawn_blocking(move || extract_zip(&zp, &d))
                .await
                .map_err(|e| e.to_string())??;
            let _ = std::fs::remove_file(&zip_path);
        }
        if !settings.server_path(&data_dir).exists() {
            return Err("llama-server が展開されたファイル内に見つかりませんでした".into());
        }
    }

    // モデル
    if settings.custom_model_path.trim().is_empty() {
        let model = catalog::find_model(&settings.model_size, &settings.model_quant)
            .ok_or("不明なモデルです")?;
        let path = settings.model_path(&data_dir).unwrap();
        if !path.exists() {
            let label = format!("PLaMo 3 Translate {} ({})", model.size.to_uppercase(), model.quant);
            download(app, &model.url(), &path, &label).await?;
        }
    }
    Ok(())
}

/// レジューム対応のダウンロード
async fn download(app: &AppHandle, url: &str, dest: &Path, label: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let mut existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut req = state.http.get(url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", existing));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("ダウンロードに失敗しました ({}): {}", label, e))?;
    let status = resp.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // 既に完了している可能性
        tokio::fs::rename(&part, dest).await.map_err(|e| e.to_string())?;
        return Ok(());
    }
    if !status.is_success() {
        return Err(format!("ダウンロードに失敗しました ({}): HTTP {}", label, status));
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resumed {
        existing = 0;
    }
    let total = resp.content_length().map(|l| l + existing).unwrap_or(0);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .await
        .map_err(|e| e.to_string())?;

    let mut downloaded = existing;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if state.cancel_download.load(Ordering::SeqCst) {
            let _ = file.flush().await;
            return Err("ダウンロードを中止しました".into());
        }
        let chunk = chunk.map_err(|e| format!("ダウンロード中にエラーが発生しました ({}): {}", label, e))?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(150) {
            last_emit = Instant::now();
            set_status(app, EngineStatus::Downloading { label: label.into(), downloaded, total });
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && downloaded < total {
        return Err(format!("ダウンロードが途中で切断されました ({})", label));
    }
    tokio::fs::rename(&part, dest).await.map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip を開けません: {}", e))?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        // アーカイブ内にフォルダがあっても、実行ファイルと DLL は同じ場所に置く
        let Some(name) = rel.file_name() else { continue };
        if entry.is_dir() {
            continue;
        }
        let out = dest.join(name);
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, buf).map_err(|e| format!("{}: {}", out.display(), e))?;
    }
    Ok(())
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

pub fn server_log_path(state: &AppState) -> PathBuf {
    state.log_dir.join("llama-server.log")
}

fn log_tail(path: &Path, lines: usize) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(8192)));
    let mut s = String::new();
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_ok() {
        s = String::from_utf8_lossy(&bytes).into_owned();
    }
    let v: Vec<&str> = s.lines().collect();
    v[v.len().saturating_sub(lines)..].join("\n")
}

async fn spawn_server(settings: &Settings, state: &AppState) -> Result<String, String> {
    let server = settings.server_path(&state.data_dir);
    let model = settings.model_path(&state.data_dir).ok_or("モデルが指定されていません")?;
    let port = free_port()?;

    std::fs::create_dir_all(&state.log_dir).map_err(|e| e.to_string())?;
    let log_path = server_log_path(state);
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;

    let mut cmd = std::process::Command::new(&server);
    cmd.arg("-m")
        .arg(&model)
        .args(["-c", &settings.ctx_size.to_string()])
        .args(["-np", "1"])
        .args(["--host", "127.0.0.1"])
        .args(["--port", &port.to_string()])
        .args(["--alias", SERVED_MODEL_NAME])
        .args(settings.extra_args.split_whitespace())
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(log2);
    if let Some(dir) = server.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("llama-server を起動できません ({}): {}", server.display(), e))?;
    let job = JobHandle::attach(&child);
    *state.server.lock().unwrap() = Some(ServerProcess { child, job });

    let endpoint = format!("http://127.0.0.1:{}", port);
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        tokio::time::sleep(Duration::from_millis(300)).await;
        {
            let mut guard = state.server.lock().unwrap();
            match guard.as_mut() {
                Some(p) => {
                    if let Ok(Some(code)) = p.child.try_wait() {
                        *guard = None;
                        return Err(format!(
                            "llama-server が終了しました ({})。\n{}",
                            code,
                            log_tail(&log_path, 12)
                        ));
                    }
                }
                None => return Err("起動が中断されました".into()),
            }
        }
        if let Ok(r) = state
            .local_http
            .get(format!("{}/health", endpoint))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if r.status().is_success() {
                return Ok(endpoint);
            }
        }
        if Instant::now() > deadline {
            return Err(format!(
                "llama-server の起動がタイムアウトしました。\n{}",
                log_tail(&log_path, 12)
            ));
        }
    }
}

pub fn stop_server(state: &AppState) {
    if let Some(mut p) = state.server.lock().unwrap().take() {
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
}

/// 子プロセスをジョブオブジェクトに入れ、親 (このアプリ) が異常終了しても道連れにする
pub struct JobHandle(#[allow(dead_code)] isize);

impl JobHandle {
    #[cfg(windows)]
    fn attach(child: &std::process::Child) -> Option<JobHandle> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            AssignProcessToJobObject(job, child.as_raw_handle() as _);
            Some(JobHandle(job as isize))
        }
    }

    #[cfg(not(windows))]
    fn attach(_child: &std::process::Child) -> Option<JobHandle> {
        None
    }
}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}
