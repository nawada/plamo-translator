//! 原文の言語推定 (「言語を自動検出」時の表示用)

use whatlang::{Detector, Lang};

/// whatlang の言語と、PLaMo 3 Translate の言語コードの対応 (対応言語のみ)
const LANGS: &[(Lang, &str)] = &[
    (Lang::Jpn, "ja"),
    (Lang::Eng, "en"),
    (Lang::Cmn, "zh"),
    (Lang::Kor, "ko"),
    (Lang::Fra, "fr"),
    (Lang::Deu, "de"),
    (Lang::Spa, "es"),
    (Lang::Ita, "it"),
    (Lang::Rus, "ru"),
    (Lang::Vie, "vi"),
    (Lang::Tha, "th"),
    (Lang::Ind, "id"),
    (Lang::Ara, "ar"),
    (Lang::Nld, "nl"),
    (Lang::Aze, "az"),
    (Lang::Hye, "hy"),
    (Lang::Urd, "ur"),
    (Lang::Uzb, "uz"),
    (Lang::Est, "et"),
    (Lang::Cat, "ca"),
    (Lang::Guj, "gu"),
    (Lang::Khm, "km"),
    (Lang::Hrv, "hr"),
    (Lang::Kat, "ka"),
    (Lang::Jav, "jv"),
    (Lang::Sin, "si"),
    (Lang::Slk, "sk"),
    (Lang::Slv, "sl"),
    (Lang::Srp, "sr"),
    (Lang::Tgl, "tl"),
    (Lang::Tam, "ta"),
    (Lang::Tel, "te"),
    (Lang::Nep, "ne"),
    (Lang::Hun, "hu"),
    (Lang::Pan, "pa"),
    (Lang::Hin, "hi"),
    (Lang::Bul, "bg"),
    (Lang::Heb, "he"),
    (Lang::Bel, "be"),
    (Lang::Pes, "fa"),
    (Lang::Ben, "bn"),
    (Lang::Pol, "pl"),
    (Lang::Mar, "mr"),
    (Lang::Mal, "ml"),
    (Lang::Mya, "my"),
    (Lang::Lav, "lv"),
    (Lang::Lit, "lt"),
];

/// 推定に使う最大文字数
const SAMPLE_CHARS: usize = 2000;

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}')
}

/// 簡体字・繁体字のどちらかを、片方にしかない常用字の出現数で判定する
fn chinese_variant(text: &str) -> &'static str {
    const HANS: &str = "这们说时会为对国学发过还没现问开关见进动长几样东车门马书经点间";
    const HANT: &str = "這們說時會為對國學發過還沒現問開關見進動長幾樣東車門馬書經點間";
    let hans = text.chars().filter(|c| HANS.contains(*c)).count();
    let hant = text.chars().filter(|c| HANT.contains(*c)).count();
    if hant > hans {
        "zh-hant"
    } else {
        "zh-hans"
    }
}

/// テキストの言語を推定し、PLaMo 3 Translate の言語コードで返す。推定できなければ None。
pub fn detect_language(text: &str) -> Option<String> {
    let sample: String = crate::pfml::normalize_styled(&text.chars().take(SAMPLE_CHARS).collect::<String>());
    if sample.trim().is_empty() {
        return None;
    }
    // 仮名を含む漢字交じりの文は中国語と誤判定されやすいので先に判定する
    if sample.chars().filter(|c| is_kana(*c)).count() >= 2 {
        return Some("ja".into());
    }
    let detector = Detector::with_allowlist(LANGS.iter().map(|(l, _)| *l).collect());
    let lang = detector.detect_lang(&sample)?;
    let code = LANGS.iter().find(|(l, _)| *l == lang)?.1;
    Some(if code == "zh" { chinese_variant(&sample).into() } else { code.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_languages() {
        let cases = [
            ("The meeting has been rescheduled to next Monday.", "en"),
            ("会議は来週の月曜日に変更されました。", "ja"),
            ("会议已改到下周一，请大家注意时间。", "zh-hans"),
            ("會議已改到下週一，請大家注意時間。這個問題我們說過了。", "zh-hant"),
            ("회의가 다음 주 월요일로 변경되었습니다.", "ko"),
            ("La réunion a été reportée à lundi prochain.", "fr"),
            ("Das Treffen wurde auf nächsten Montag verschoben.", "de"),
            ("La reunión se ha aplazado hasta el próximo lunes.", "es"),
            ("Встреча перенесена на следующий понедельник, потому что мы ещё не готовы.", "ru"),
        ];
        for (text, want) in cases {
            assert_eq!(detect_language(text).as_deref(), Some(want), "{text}");
        }
    }

    #[test]
    fn styled_letters_are_detected() {
        assert_eq!(detect_language("𝗧𝗵𝗲 𝗺𝗲𝗲𝘁𝗶𝗻𝗴 𝗵𝗮𝘀 𝗯𝗲𝗲𝗻 𝗿𝗲𝘀𝗰𝗵𝗲𝗱𝘂𝗹𝗲𝗱 𝘁𝗼 𝗻𝗲𝘅𝘁 𝗠𝗼𝗻𝗱𝗮𝘆.").as_deref(), Some("en"));
    }

    #[test]
    fn empty_text_is_none() {
        assert_eq!(detect_language("   \n"), None);
        assert_eq!(detect_language("12345 !!"), None);
    }

    #[test]
    fn all_codes_are_supported_languages() {
        // フロントエンドの言語一覧 (src/languages.ts) にあるコードだけを返す
        let ts = include_str!("../../src/languages.ts");
        for (_, code) in LANGS {
            if *code == "zh" {
                continue;
            }
            assert!(ts.contains(&format!("code: \"{code}\"")), "{code}");
        }
    }
}
