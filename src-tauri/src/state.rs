use crate::settings::Settings;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum EngineStatus {
    /// 初期状態
    Idle,
    /// モデルまたは llama.cpp が未ダウンロード
    #[serde(rename_all = "camelCase")]
    SetupRequired { missing: Vec<String>, download_bytes: u64 },
    #[serde(rename_all = "camelCase")]
    Downloading { label: String, downloaded: u64, total: u64 },
    Extracting,
    Starting,
    #[serde(rename_all = "camelCase")]
    Ready { endpoint: String, model: String },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

pub struct ServerProcess {
    pub child: std::process::Child,
    /// Windows のジョブオブジェクト (アプリ終了時に子プロセスも確実に終了させる)
    #[allow(dead_code)]
    pub job: Option<crate::engine::JobHandle>,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub status: Mutex<EngineStatus>,
    pub server: Mutex<Option<ServerProcess>>,
    /// 起動・インストール処理の多重実行防止
    pub engine_lock: tokio::sync::Mutex<()>,
    pub cancel_download: AtomicBool,
    /// インターネットからのダウンロード用 (システムのプロキシ設定に従う)
    pub http: reqwest::Client,
    /// 翻訳サーバーとの通信用 (プロキシを経由しない)
    pub local_http: reqwest::Client,
    /// ショートカット登録に失敗したときのメッセージ
    pub shortcut_error: Mutex<Option<String>>,
    /// 翻訳リクエストの世代番号。新しい翻訳が始まると古いストリームは打ち切られる。
    pub translate_gen: AtomicU64,
    /// フロントエンドの準備前に取得した選択文字列
    pub pending_selection: Mutex<Option<String>>,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
    pub fn status(&self) -> EngineStatus {
        self.status.lock().unwrap().clone()
    }
}
