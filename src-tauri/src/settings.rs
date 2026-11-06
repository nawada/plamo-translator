use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_SHORTCUT: &str = "Ctrl+K";

/// 推論エンジンの動かし方
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EngineMode {
    /// アプリが llama.cpp (llama-server) を自動ダウンロードして起動する
    Builtin,
    /// 既に起動している OpenAI 互換サーバー (llama-server / vLLM など) を使う
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// グローバルショートカット (例: "Ctrl+K", "Ctrl+Shift+KeyT")
    pub shortcut: String,
    /// ショートカット押下時に選択中の文字列をコピーして翻訳する
    pub capture_selection: bool,

    pub engine_mode: EngineMode,
    /// モデルサイズ: "0.8b" | "2b" | "8b"
    pub model_size: String,
    /// 量子化: "Q4_K_M" | "Q4_0" | "Q3_K_M" | "Q2_K"
    pub model_quant: String,
    /// llama.cpp のビルド種別: "vulkan" | "cpu" | "cuda-12.4" | "cuda-13.4"
    pub backend: String,
    /// コンテキスト長 (入力 + 出力の合計トークン)
    pub ctx_size: u32,
    /// 翻訳が無い状態がこの秒数続いたらモデルを GPU メモリから解放する (0 = 解放しない)。
    /// 次の翻訳時に自動で再読み込みされる (llama-server の --sleep-idle-seconds)
    pub idle_unload_secs: u32,
    /// llama-server に追加で渡す引数 (空白区切り)
    pub extra_args: String,
    /// 任意の llama-server.exe を使う場合のパス (空なら自動ダウンロード)
    pub custom_server_path: String,
    /// 任意の GGUF を使う場合のパス (空なら自動ダウンロード)
    pub custom_model_path: String,

    /// External モード時の接続先 (例: http://127.0.0.1:8000)
    pub external_url: String,
    pub external_model: String,
    pub external_api_key: String,

    /// 既定の翻訳先言語
    pub target_lang: String,
    /// 原文が翻訳先と同じ言語 (日本語) のときに使う翻訳先
    pub secondary_lang: String,
    /// 日本語の原文を自動で secondary_lang に翻訳する
    pub auto_switch: bool,
    /// 日本語の文体: "auto" | "plain" | "polite"
    pub form: String,
    /// 入力中に自動で翻訳する
    pub auto_translate: bool,

    /// ×ボタンでタスクトレイに格納する
    pub close_to_tray: bool,
    /// Windows 起動時に自動起動
    pub launch_at_startup: bool,
    /// 起動時にウィンドウを表示せずトレイに常駐
    pub start_minimized: bool,
    /// ショートカットで呼び出したとき常に最前面に表示
    pub always_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: DEFAULT_SHORTCUT.into(),
            capture_selection: true,
            engine_mode: EngineMode::Builtin,
            model_size: "0.8b".into(),
            model_quant: "Q4_K_M".into(),
            backend: "vulkan".into(),
            ctx_size: 16384,
            idle_unload_secs: 300,
            extra_args: String::new(),
            custom_server_path: String::new(),
            custom_model_path: String::new(),
            external_url: "http://127.0.0.1:8000".into(),
            external_model: "plamo-3-translate".into(),
            external_api_key: String::new(),
            target_lang: "ja".into(),
            secondary_lang: "en".into(),
            auto_switch: true,
            form: "auto".into(),
            auto_translate: true,
            close_to_tray: true,
            launch_at_startup: false,
            start_minimized: false,
            always_on_top: false,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(|s| s.sanitized())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    /// 不正な値を既定値に戻す
    pub fn sanitized(mut self) -> Self {
        let d = Settings::default();
        if crate::catalog::find_model(&self.model_size, &self.model_quant).is_none() {
            self.model_size = d.model_size.clone();
            self.model_quant = d.model_quant.clone();
        }
        if crate::catalog::find_backend(&self.backend).is_none() {
            self.backend = d.backend.clone();
        }
        self.ctx_size = self.ctx_size.clamp(2048, 32768);
        self.idle_unload_secs = self.idle_unload_secs.min(24 * 3600);
        if self.shortcut.trim().is_empty() {
            self.shortcut = d.shortcut.clone();
        }
        if !matches!(self.form.as_str(), "auto" | "plain" | "polite") {
            self.form = "auto".into();
        }
        self
    }

    /// エンジン (llama-server) の再起動が必要な設定変更か
    pub fn engine_changed(&self, other: &Settings) -> bool {
        self.engine_mode != other.engine_mode
            || self.model_size != other.model_size
            || self.model_quant != other.model_quant
            || self.backend != other.backend
            || self.ctx_size != other.ctx_size
            || self.idle_unload_secs != other.idle_unload_secs
            || self.extra_args != other.extra_args
            || self.custom_server_path != other.custom_server_path
            || self.custom_model_path != other.custom_model_path
            || self.external_url != other.external_url
            || self.external_model != other.external_model
            || self.external_api_key != other.external_api_key
    }

    pub fn model_path(&self, data_dir: &Path) -> Option<PathBuf> {
        if !self.custom_model_path.trim().is_empty() {
            return Some(PathBuf::from(self.custom_model_path.trim()));
        }
        crate::catalog::find_model(&self.model_size, &self.model_quant)
            .map(|m| data_dir.join("models").join(m.file_name()))
    }

    pub fn server_path(&self, data_dir: &Path) -> PathBuf {
        if !self.custom_server_path.trim().is_empty() {
            return PathBuf::from(self.custom_server_path.trim());
        }
        crate::catalog::backend_dir(data_dir, &self.backend).join(crate::catalog::SERVER_EXE)
    }
}
