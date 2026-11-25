// PLaMo 3 Translate の対応言語 (53 言語)
export interface Language {
  code: string;
  name: string;
}

export const LANGUAGES: Language[] = [
  { code: "ja", name: "日本語" },
  { code: "en", name: "英語" },
  { code: "zh-hans", name: "中国語（簡体字）" },
  { code: "zh-hant", name: "中国語（繁体字）" },
  { code: "ko", name: "韓国語" },
  { code: "fr", name: "フランス語" },
  { code: "de", name: "ドイツ語" },
  { code: "es", name: "スペイン語" },
  { code: "it", name: "イタリア語" },
  { code: "ru", name: "ロシア語" },
  { code: "vi", name: "ベトナム語" },
  { code: "th", name: "タイ語" },
  { code: "id", name: "インドネシア語" },
  { code: "ar", name: "アラビア語" },
  { code: "nl", name: "オランダ語" },
  { code: "az", name: "アゼルバイジャン語" },
  { code: "hy", name: "アルメニア語" },
  { code: "ur", name: "ウルドゥー語" },
  { code: "uz", name: "ウズベク語" },
  { code: "et", name: "エストニア語" },
  { code: "kk", name: "カザフ語" },
  { code: "ca", name: "カタルーニャ語" },
  { code: "gu", name: "グジャラート語" },
  { code: "km", name: "クメール語" },
  { code: "hr", name: "クロアチア語" },
  { code: "ka", name: "ジョージア語" },
  { code: "jv", name: "ジャワ語" },
  { code: "si", name: "シンハラ語" },
  { code: "sw", name: "スワヒリ語" },
  { code: "sk", name: "スロバキア語" },
  { code: "sl", name: "スロベニア語" },
  { code: "sr", name: "セルビア語" },
  { code: "ceb", name: "セブアノ語" },
  { code: "tl", name: "タガログ語" },
  { code: "ta", name: "タミル語" },
  { code: "te", name: "テルグ語" },
  { code: "ne", name: "ネパール語" },
  { code: "eu", name: "バスク語" },
  { code: "hu", name: "ハンガリー語" },
  { code: "pa", name: "パンジャーブ語" },
  { code: "hi", name: "ヒンディー語" },
  { code: "bg", name: "ブルガリア語" },
  { code: "he", name: "ヘブライ語" },
  { code: "be", name: "ベラルーシ語" },
  { code: "fa", name: "ペルシャ語" },
  { code: "bn", name: "ベンガル語" },
  { code: "pl", name: "ポーランド語" },
  { code: "mr", name: "マラーティー語" },
  { code: "ml", name: "マラヤーラム語" },
  { code: "ms", name: "マレー語" },
  { code: "my", name: "ミャンマー語" },
  { code: "lv", name: "ラトビア語" },
  { code: "lt", name: "リトアニア語" },
];

/** 翻訳先だけに出す「やさしい日本語」(日本語 + 文体 easy として翻訳する) */
export const EASY_JA = "ja-easy";

export function languageName(code: string): string {
  if (code === EASY_JA) return "やさしい日本語";
  return LANGUAGES.find((l) => l.code === code)?.name ?? code;
}

/** 日本語のテキストかどうかを簡易判定する (ひらがな・カタカナの有無で判断) */
export function looksJapanese(text: string): boolean {
  const sample = text.slice(0, 2000);
  const kana = sample.match(/[぀-ヿｦ-ﾟ]/g)?.length ?? 0;
  if (kana === 0) return false;
  const letters = sample.match(/[\p{L}]/gu)?.length ?? 1;
  const cjk = sample.match(/[぀-ヿ一-鿿ｦ-ﾟ]/g)?.length ?? 0;
  // 仮名を含み、文字の多くが日本語の文字なら日本語とみなす
  return kana >= 2 ? cjk / letters > 0.3 : cjk / letters > 0.6;
}

/** 原文の言語を推定する (自動検出時に入れ替えボタンで使う簡易版) */
export function guessLanguage(text: string): string {
  if (looksJapanese(text)) return "ja";
  const s = text.slice(0, 1000);
  if (/[가-힯]/.test(s)) return "ko";
  if (/[一-鿿]/.test(s)) return "zh-hans";
  if (/[Ѐ-ӿ]/.test(s)) return "ru";
  if (/[฀-๿]/.test(s)) return "th";
  if (/[؀-ۿ]/.test(s)) return "ar";
  return "en";
}
