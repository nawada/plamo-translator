//! Completions API (OpenAI 互換) へのストリーミング翻訳リクエスト

use crate::pfml::{self, GlossaryEntry, PromptOptions};
use crate::state::{AppState, EngineStatus};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

/// 1 回のリクエストで翻訳する最大文字数 (これを超えると行単位で分割して順に翻訳する)
const SEGMENT_CHARS: usize = 1500;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
    pub text: String,
    pub source_lang: Option<String>,
    pub target_lang: String,
    pub form: Option<String>,
    pub context: Option<String>,
    #[serde(default)]
    pub glossary: Vec<GlossaryEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkEvent {
    request_id: u64,
    delta: String,
}

enum Outcome {
    Done(String),
    Cancelled,
}

pub async fn translate(app: &AppHandle, req: TranslateRequest, request_id: u64) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    state.translate_gen.store(request_id, Ordering::SeqCst);

    let (endpoint, model, api_key) = match state.status() {
        EngineStatus::Ready { endpoint, model } => {
            let s = state.settings();
            let key = if s.engine_mode == crate::settings::EngineMode::External {
                s.external_api_key
            } else {
                String::new()
            };
            (endpoint, model, key)
        }
        EngineStatus::Starting => return Err("翻訳エンジンを起動中です。しばらくお待ちください。".into()),
        EngineStatus::Downloading { .. } | EngineStatus::Extracting => {
            return Err("モデルをダウンロード中です。完了までお待ちください。".into())
        }
        _ => return Err("翻訳エンジンが準備できていません。".into()),
    };

    let source_lang = req.source_lang.as_deref().filter(|s| !s.is_empty() && *s != "auto");
    let form = req.form.as_deref().filter(|f| *f != "auto");
    let opt = PromptOptions {
        source_lang,
        target_lang: &req.target_lang,
        form,
        context: req.context.as_deref(),
        glossary: &req.glossary,
    };

    let segments = pfml::segment(&req.text, SEGMENT_CHARS);
    let mut out = String::new();
    // 直前の対訳 1 組を履歴として渡し、訳語・文体をそろえる
    let mut history: Vec<(String, String)> = Vec::new();

    let emit = |delta: &str| {
        let _ = app.emit("translation-chunk", ChunkEvent { request_id, delta: delta.to_string() });
    };

    for seg in segments {
        if !seg.lead.is_empty() {
            out.push_str(&seg.lead);
            emit(&seg.lead);
        }
        if !seg.core.is_empty() {
            let prompt = pfml::build_prompt(&seg.core, &history, &opt);
            let max_tokens = (seg.core.chars().count() * 3 + 128).min(8192);
            match stream_completion(app, &endpoint, &model, &api_key, &prompt, max_tokens, request_id, &emit).await? {
                Outcome::Done(text) => {
                    out.push_str(&text);
                    history.clear();
                    history.push((seg.core.clone(), text));
                }
                Outcome::Cancelled => return Ok(None),
            }
        }
        if !seg.trail.is_empty() {
            out.push_str(&seg.trail);
            emit(&seg.trail);
        }
    }
    Ok(Some(out))
}

#[allow(clippy::too_many_arguments)]
async fn stream_completion(
    app: &AppHandle,
    endpoint: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
    max_tokens: usize,
    request_id: u64,
    emit: &impl Fn(&str),
) -> Result<Outcome, String> {
    let state = app.state::<AppState>();
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "temperature": 0,
        "max_tokens": max_tokens,
        "stop": pfml::STOP_TOKENS,
        "stream": true,
        // vLLM 用: 文頭トークンを付ける (llama.cpp は自動で付与)
        "add_special_tokens": true,
        "cache_prompt": true,
    });
    let mut req = state.local_http.post(format!("{}/v1/completions", endpoint)).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await.map_err(|e| format!("翻訳サーバーに接続できません: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("翻訳に失敗しました (HTTP {}): {}", status, text.chars().take(300).collect::<String>()));
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut text = String::new();
    // 停止トークンがストリーム上で分割されて届く場合に備えて、末尾を少し保留して送る
    let mut emitted = 0usize;

    while let Some(chunk) = stream.next().await {
        if state.translate_gen.load(Ordering::SeqCst) != request_id {
            return Ok(Outcome::Cancelled);
        }
        let chunk = chunk.map_err(|e| format!("翻訳中に接続が切れました: {}", e))?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if let Some(err) = v.get("error") {
                return Err(format!("翻訳に失敗しました: {}", err));
            }
            if let Some(t) = v["choices"][0]["text"].as_str() {
                text.push_str(t);
            }
        }
        let clean = pfml::strip_stop_tokens(&text);
        // "<|plamo:" の途中で止まっている可能性がある末尾は保留
        let safe_end = match clean.rfind("<|") {
            Some(i) if clean.len() - i < 16 => i,
            _ => clean.len(),
        };
        if safe_end > emitted {
            emit(&clean[emitted..safe_end]);
            emitted = safe_end;
        }
    }
    if state.translate_gen.load(Ordering::SeqCst) != request_id {
        return Ok(Outcome::Cancelled);
    }
    let clean = pfml::strip_stop_tokens(&text).to_string();
    if clean.len() > emitted {
        emit(&clean[emitted..]);
    }
    Ok(Outcome::Done(clean))
}
