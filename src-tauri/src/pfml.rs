//! PLaMo 3 Translate 用 PFML3 プロンプトの組み立てと長文の分割
//!
//! 仕様: https://huggingface.co/pfnet/plamo-3-translate-0.8b/blob/main/README_ja.md

use serde::Deserialize;

pub const STOP_TOKENS: [&str; 2] = ["<|plamo:tag|>", "<|plamo:bos|>"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Default)]
pub struct PromptOptions<'a> {
    pub source_lang: Option<&'a str>,
    pub target_lang: &'a str,
    /// "plain" | "polite" | "easy" (翻訳先が日本語のときのみ有効)
    /// "easy" (やさしい日本語) は公式には記載がないが、モデルが実際に反応する
    pub form: Option<&'a str>,
    pub context: Option<&'a str>,
    pub glossary: &'a [GlossaryEntry],
}

/// 予約トークン (<|plamo:...|>) が本文に紛れ込まないよう無害化する
pub fn sanitize(s: &str) -> String {
    s.replace("<|plamo:", "<|plamo\u{200B}:")
}

fn lang_attr(lang: Option<&str>) -> String {
    match lang {
        Some(l) if !l.is_empty() => format!("<|plamo:key|>lang<|plamo:val|>{}", l),
        _ => String::new(),
    }
}

/// PFML3 プロンプトを組み立てる。
/// `history` は (原文, 訳文) の対訳履歴で、文書の順に並べる。
pub fn build_prompt(text: &str, history: &[(String, String)], opt: &PromptOptions) -> String {
    let mut p = String::from("<|plamo:tag|>dataset<|plamo:msg|>translation");

    if let Some(ctx) = opt.context.map(str::trim).filter(|c| !c.is_empty()) {
        p.push_str("<|plamo:tag|>context<|plamo:msg|>");
        p.push_str(&sanitize(ctx));
    }

    let src_attr = lang_attr(opt.source_lang);
    let tgt_attr = lang_attr(Some(opt.target_lang));

    for (src, tgt) in history {
        p.push_str("<|plamo:tag|>input");
        p.push_str(&src_attr);
        p.push_str("<|plamo:msg|>");
        p.push_str(&sanitize(src));
        p.push_str("<|plamo:tag|>output");
        p.push_str(&tgt_attr);
        p.push_str("<|plamo:msg|>");
        p.push_str(&sanitize(tgt));
    }

    let glossary: Vec<_> = opt
        .glossary
        .iter()
        .filter(|g| !g.source.trim().is_empty() && !g.target.trim().is_empty())
        .collect();
    if !glossary.is_empty() {
        p.push_str("<|plamo:tag|>glossary");
        for g in glossary {
            p.push_str("<|plamo:index|>");
            p.push_str(&sanitize(g.source.trim()));
            p.push_str("<|plamo:item|>");
            p.push_str(&sanitize(g.target.trim()));
        }
    }

    p.push_str("<|plamo:tag|>input");
    p.push_str(&src_attr);
    p.push_str("<|plamo:msg|>");
    p.push_str(&sanitize(text));

    p.push_str("<|plamo:tag|>output");
    p.push_str(&tgt_attr);
    if opt.target_lang == "ja" {
        if let Some(form) = opt.form.filter(|f| matches!(*f, "plain" | "polite" | "easy")) {
            p.push_str("<|plamo:key|>form<|plamo:val|>");
            p.push_str(form);
        }
    }
    p.push_str("<|plamo:msg|>");
    p
}

/// 生成結果の末尾に残った停止トークンを取り除く
pub fn strip_stop_tokens(s: &str) -> &str {
    let mut end = s.len();
    for t in STOP_TOKENS {
        if let Some(i) = s.find(t) {
            end = end.min(i);
        }
    }
    &s[..end]
}

/// 翻訳単位。前後の空白はそのまま出力に残し、`core` だけを翻訳する。
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub lead: String,
    pub core: String,
    pub trail: String,
}

fn split_ws(s: &str) -> Segment {
    let start = s.len() - s.trim_start().len();
    let end = s.trim_end().len();
    if end <= start {
        return Segment { lead: s.to_string(), core: String::new(), trail: String::new() };
    }
    Segment {
        lead: s[..start].to_string(),
        core: s[start..end].to_string(),
        trail: s[end..].to_string(),
    }
}

/// 長文を行単位でまとめて、おおよそ `max_chars` 文字以下のセグメントに分割する。
/// 各セグメントを連結すると元の文字列に戻る。
pub fn segment(text: &str, max_chars: usize) -> Vec<Segment> {
    if text.chars().count() <= max_chars {
        return vec![split_ws(text)];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for line in text.split_inclusive('\n') {
        let len = line.chars().count();
        // 段落の区切り (空行) か、上限を超えるときに区切る
        let is_blank = line.trim().is_empty();
        if cur_len > 0 && cur_len + len > max_chars && !is_blank {
            chunks.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        cur.push_str(line);
        cur_len += len;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks.iter().map(|c| split_ws(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_prompt_matches_readme() {
        let p = build_prompt(
            "The meeting has been rescheduled to next Monday.",
            &[],
            &PromptOptions { target_lang: "ja", ..Default::default() },
        );
        assert_eq!(
            p,
            "<|plamo:tag|>dataset<|plamo:msg|>translation\
<|plamo:tag|>input<|plamo:msg|>The meeting has been rescheduled to next Monday.\
<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>ja<|plamo:msg|>"
        );
    }

    #[test]
    fn all_options_prompt_matches_readme() {
        let history = vec![
            ("Open the project dashboard.".to_string(), "プロジェクトの管理画面を開きます。".to_string()),
            ("It lists all active tasks.".to_string(), "進行中のタスクがすべて表示されます。".to_string()),
        ];
        let glossary = vec![GlossaryEntry { source: "owner".into(), target: "オーナー".into() }];
        let p = build_prompt(
            "You can also filter them by owner.",
            &history,
            &PromptOptions {
                source_lang: Some("en"),
                target_lang: "ja",
                form: Some("polite"),
                context: Some("Help page of a project management tool."),
                glossary: &glossary,
            },
        );
        assert_eq!(
            p,
            "<|plamo:tag|>dataset<|plamo:msg|>translation\
<|plamo:tag|>context<|plamo:msg|>Help page of a project management tool.\
<|plamo:tag|>input<|plamo:key|>lang<|plamo:val|>en<|plamo:msg|>Open the project dashboard.\
<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>ja<|plamo:msg|>プロジェクトの管理画面を開きます。\
<|plamo:tag|>input<|plamo:key|>lang<|plamo:val|>en<|plamo:msg|>It lists all active tasks.\
<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>ja<|plamo:msg|>進行中のタスクがすべて表示されます。\
<|plamo:tag|>glossary<|plamo:index|>owner<|plamo:item|>オーナー\
<|plamo:tag|>input<|plamo:key|>lang<|plamo:val|>en<|plamo:msg|>You can also filter them by owner.\
<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>ja<|plamo:key|>form<|plamo:val|>polite<|plamo:msg|>"
        );
    }

    #[test]
    fn glossary_without_history_matches_readme() {
        let glossary = vec![
            GlossaryEntry { source: "dashboard".into(), target: "管理画面".into() },
            GlossaryEntry { source: "project".into(), target: "プロジェクト".into() },
        ];
        let p = build_prompt(
            "Please open the project dashboard.",
            &[],
            &PromptOptions { target_lang: "ja", glossary: &glossary, ..Default::default() },
        );
        assert_eq!(
            p,
            "<|plamo:tag|>dataset<|plamo:msg|>translation\
<|plamo:tag|>glossary<|plamo:index|>dashboard<|plamo:item|>管理画面<|plamo:index|>project<|plamo:item|>プロジェクト\
<|plamo:tag|>input<|plamo:msg|>Please open the project dashboard.\
<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>ja<|plamo:msg|>"
        );
    }

    #[test]
    fn easy_form_is_passed() {
        let p = build_prompt("Hello.", &[], &PromptOptions { target_lang: "ja", form: Some("easy"), ..Default::default() });
        assert!(p.ends_with("<|plamo:val|>ja<|plamo:key|>form<|plamo:val|>easy<|plamo:msg|>"));
    }

    #[test]
    fn form_is_ignored_for_non_japanese() {
        let p = build_prompt("こんにちは", &[], &PromptOptions { target_lang: "en", form: Some("plain"), ..Default::default() });
        assert!(!p.contains("form"));
    }

    #[test]
    fn reserved_tokens_are_sanitized() {
        let p = build_prompt("a<|plamo:tag|>b", &[], &PromptOptions { target_lang: "ja", ..Default::default() });
        assert_eq!(p.matches("<|plamo:tag|>").count(), 3);
    }

    #[test]
    fn strip_stop() {
        assert_eq!(strip_stop_tokens("こんにちは<|plamo:tag|>"), "こんにちは");
        assert_eq!(strip_stop_tokens("abc"), "abc");
    }

    #[test]
    fn segment_roundtrip() {
        let text = "  Hello.\n\nWorld line one.\nline two\n\n\nThird paragraph is here.\n";
        for max in [5, 10, 20, 1000] {
            let segs = segment(text, max);
            let joined: String = segs.iter().map(|s| format!("{}{}{}", s.lead, s.core, s.trail)).collect();
            assert_eq!(joined, text, "max={max}");
        }
        let segs = segment(text, 1000);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].lead, "  ");
        assert_eq!(segs[0].trail, "\n");
    }
}
