import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LANGUAGES, guessLanguage, languageName, looksJapanese } from "./languages";
import { setupScrollSync } from "./scrollsync";

// ---------------------------------------------------------------------------
// 型 (Rust 側の構造体と対応)
// ---------------------------------------------------------------------------

interface Settings {
  shortcut: string;
  captureSelection: boolean;
  engineMode: "builtin" | "external";
  modelSize: string;
  modelQuant: string;
  backend: string;
  ctxSize: number;
  idleUnloadSecs: number;
  extraArgs: string;
  customServerPath: string;
  customModelPath: string;
  externalUrl: string;
  externalModel: string;
  externalApiKey: string;
  targetLang: string;
  secondaryLang: string;
  autoSwitch: boolean;
  form: "auto" | "plain" | "polite" | "easy";
  autoTranslate: boolean;
  closeToTray: boolean;
  launchAtStartup: boolean;
  startMinimized: boolean;
  alwaysOnTop: boolean;
}

type EngineStatus =
  | { state: "idle" }
  | { state: "setupRequired"; missing: string[]; downloadBytes: number }
  | { state: "downloading"; label: string; downloaded: number; total: number }
  | { state: "extracting" }
  | { state: "starting" }
  | { state: "ready"; endpoint: string; model: string }
  | { state: "error"; message: string };

interface CatalogModel {
  size: string;
  quant: string;
  bytes: number;
  label: string;
  installed: boolean;
}

interface Catalog {
  models: CatalogModel[];
  backends: { id: string; label: string; approxBytes: number }[];
  llamaCppTag: string;
}

interface AppInfo {
  version: string;
  dataDir: string;
  logPath: string;
  shortcutError: string | null;
}

interface GlossaryEntry {
  source: string;
  target: string;
}

// ---------------------------------------------------------------------------
// DOM
// ---------------------------------------------------------------------------

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

const sourceText = $<HTMLTextAreaElement>("source-text");
const targetText = $("target-text");
const targetPlaceholder = $("target-placeholder");
const scrollSync = setupScrollSync(sourceText, targetText);
const sourceLang = $<HTMLSelectElement>("source-lang");
const targetLang = $<HTMLSelectElement>("target-lang");
const formSelect = $<HTMLSelectElement>("form");
const formWrap = $("form-wrap");
const autoSwitchNote = $("auto-switch-note");
const detectedNote = $("detected");
const charCount = $("char-count");
const translateStatus = $("translate-status");
const btnClear = $("btn-clear");
const btnCopy = $<HTMLButtonElement>("btn-copy");
const copyLabel = $("copy-label");
const glossaryInput = $<HTMLTextAreaElement>("glossary");
const contextInput = $<HTMLTextAreaElement>("context");
const glossaryCount = $("glossary-count");
const contextBadge = $("context-badge");
const enginePill = $("engine-pill");
const engineText = $("engine-text");
const setup = $("setup");
const dialog = $<HTMLDialogElement>("settings-dialog");
const toastEl = $("toast");

// ---------------------------------------------------------------------------
// 状態
// ---------------------------------------------------------------------------

let settings: Settings;
let status: EngineStatus = { state: "idle" };
let catalog: Catalog | null = null;
let appInfo: AppInfo | null = null;

let requestSeq = 0;
/** 「言語を自動検出」時に推定した原文の言語 */
let detectedLang: string | null = null;
let detectSeq = 0;
let activeRequest = 0;
let output = "";
let lastKey = "";
let pendingTranslate = false;
let debounceTimer: number | undefined;
let startedAt = 0;

const store = {
  get(key: string, fallback = ""): string {
    try {
      return localStorage.getItem(key) ?? fallback;
    } catch {
      return fallback;
    }
  },
  set(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* 保存できなくても動作に支障はない */
    }
  },
};

// ---------------------------------------------------------------------------
// ユーティリティ
// ---------------------------------------------------------------------------

function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`;
  return `${n} B`;
}

let toastTimer: number | undefined;
function toast(message: string, error = false) {
  toastEl.textContent = message;
  toastEl.classList.toggle("error", error);
  toastEl.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toastEl.classList.add("hidden"), error ? 6000 : 2200);
}

function errorMessage(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

function prettyShortcut(s: string): string {
  return s
    .split("+")
    .map((p) => p.replace(/^Key([A-Z])$/, "$1").replace(/^Digit(\d)$/, "$1"))
    .map((p) => (p.toLowerCase() === "commandorcontrol" ? "Ctrl" : p))
    .join(" + ");
}

function parseGlossary(text: string): GlossaryEntry[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.split(/\s*(?:=>|→|\t)\s*/))
    .filter((parts) => parts.length >= 2 && parts[0].trim() && parts[1].trim())
    .map(([source, target]) => ({ source: source.trim(), target: target.trim() }));
}

// ---------------------------------------------------------------------------
// 言語選択
// ---------------------------------------------------------------------------

function fillLanguageSelect(sel: HTMLSelectElement, withAuto: boolean) {
  sel.innerHTML = "";
  if (withAuto) sel.add(new Option("言語を自動検出", "auto"));
  for (const l of LANGUAGES) sel.add(new Option(l.name, l.code));
}

/** 実際に使う翻訳先 (日本語の原文を自動で別言語にする処理を含む) */
function effectiveTarget(text: string): { lang: string; switched: boolean } {
  const tgt = targetLang.value;
  const src = sourceLang.value;
  const srcIsTarget = src === tgt || (src === "auto" && tgt === "ja" && looksJapanese(text));
  if (settings.autoSwitch && srcIsTarget) {
    const alt = tgt === "ja" ? settings.secondaryLang : "ja";
    if (alt !== tgt) return { lang: alt, switched: true };
  }
  return { lang: tgt, switched: false };
}

/** 自動検出時に原文の言語を推定して表示する */
async function updateDetected(text: string) {
  const seq = ++detectSeq;
  let lang: string | null = null;
  if (sourceLang.value === "auto" && text.trim()) {
    try {
      lang = await invoke<string | null>("detect_language", { text: text.slice(0, 2000) });
    } catch {
      lang = null;
    }
  }
  if (seq !== detectSeq) return;
  detectedLang = lang;
  detectedNote.textContent = lang ? `検出: ${languageName(lang)}` : "";
}

function updateTargetUi() {
  const { lang, switched } = effectiveTarget(sourceText.value);
  autoSwitchNote.textContent = switched ? `→ ${languageName(lang)}に翻訳` : "";
  formWrap.classList.toggle("hidden", lang !== "ja");
}

// ---------------------------------------------------------------------------
// 翻訳
// ---------------------------------------------------------------------------

function renderOutput(streaming: boolean, error = false) {
  targetText.textContent = output;
  targetText.classList.toggle("streaming", streaming);
  targetText.classList.toggle("error", error);
  targetPlaceholder.classList.toggle("hidden", output.length > 0 || streaming);
  btnCopy.disabled = !output || error;
  if (!streaming) scrollSync.resync();
}

function updateSourceUi() {
  const n = [...sourceText.value].length;
  charCount.textContent = `${n.toLocaleString()} 文字`;
  btnClear.classList.toggle("hidden", n === 0);
  updateTargetUi();
}

function scheduleTranslate(delay = 450) {
  clearTimeout(debounceTimer);
  if (!settings.autoTranslate && delay > 0) return;
  debounceTimer = window.setTimeout(() => void translateNow(), delay);
}

async function translateNow(force = false) {
  clearTimeout(debounceTimer);
  const text = sourceText.value;
  await updateDetected(text);
  const fromLang = sourceLang.value === "auto" ? detectedLang : null;
  if (!text.trim()) {
    if (activeRequest) void invoke("cancel_translation");
    activeRequest = 0;
    output = "";
    lastKey = "";
    translateStatus.textContent = "";
    renderOutput(false);
    return;
  }
  if (status.state !== "ready") {
    pendingTranslate = true;
    translateStatus.textContent = "翻訳エンジンの準備ができ次第、翻訳します…";
    return;
  }
  pendingTranslate = false;

  const { lang } = effectiveTarget(text);
  const request = {
    text,
    sourceLang: sourceLang.value === "auto" ? null : sourceLang.value,
    targetLang: lang,
    form: lang === "ja" ? formSelect.value : null,
    context: contextInput.value.trim() || null,
    glossary: parseGlossary(glossaryInput.value),
  };
  const key = JSON.stringify(request);
  if (!force && key === lastKey) return;
  lastKey = key;

  const id = ++requestSeq;
  activeRequest = id;
  output = "";
  startedAt = performance.now();
  translateStatus.textContent = "翻訳中…";
  renderOutput(true);

  try {
    const result = await invoke<string | null>("translate", { request, requestId: id });
    if (id !== activeRequest || result === null) return;
    output = result;
    renderOutput(false);
    const sec = ((performance.now() - startedAt) / 1000).toFixed(1);
    const from = fromLang ? `${languageName(fromLang)}から` : "";
    translateStatus.textContent = `${from}${languageName(lang)}に翻訳しました (${sec} 秒)`;
  } catch (e) {
    if (id !== activeRequest) return;
    lastKey = "";
    output = errorMessage(e);
    renderOutput(false, true);
    translateStatus.textContent = "";
  }
}

// ---------------------------------------------------------------------------
// エンジンの状態表示
// ---------------------------------------------------------------------------

function modelLabel(): string {
  if (settings.engineMode === "external") return "外部サーバー";
  return `${settings.modelSize.toUpperCase()} · ${settings.modelQuant}`;
}

function renderStatus() {
  const s = status;
  const title = $("setup-title");
  const desc = $("setup-desc");
  const missing = $("setup-missing");
  const progress = $("setup-progress");
  const bar = $("progress-bar");
  const btnInstall = $("btn-install");
  const btnCancel = $("btn-cancel-install");
  const btnRetry = $("btn-retry");

  missing.innerHTML = "";
  progress.classList.add("hidden");
  bar.classList.remove("indeterminate");
  btnInstall.classList.add("hidden");
  btnCancel.classList.add("hidden");
  btnRetry.classList.add("hidden");
  setup.dataset.kind = "";

  switch (s.state) {
    case "idle":
    case "starting":
      enginePill.dataset.state = "busy";
      engineText.textContent = "起動中…";
      setup.classList.add("hidden");
      break;
    case "ready":
      enginePill.dataset.state = "ready";
      engineText.textContent = `準備完了 · ${modelLabel()}`;
      setup.classList.add("hidden");
      if (pendingTranslate || (sourceText.value.trim() && !output)) void translateNow(true);
      break;
    case "setupRequired":
      enginePill.dataset.state = "error";
      engineText.textContent = "セットアップが必要";
      setup.classList.remove("hidden");
      title.textContent = "翻訳モデルの準備";
      desc.textContent =
        `初回のみ、翻訳エンジンとモデルをダウンロードします (合計 約 ${formatBytes(s.downloadBytes)})。` +
        "\nダウンロード後はインターネット接続なしで翻訳できます。";
      for (const m of s.missing) {
        const li = document.createElement("li");
        li.textContent = m;
        missing.appendChild(li);
      }
      btnInstall.classList.remove("hidden");
      break;
    case "downloading": {
      enginePill.dataset.state = "busy";
      engineText.textContent = "ダウンロード中…";
      setup.classList.remove("hidden");
      title.textContent = "ダウンロード中";
      desc.textContent = "完了すると自動で翻訳エンジンが起動します。ウィンドウを閉じても処理は続きます。";
      progress.classList.remove("hidden");
      $("progress-label").textContent = s.label;
      const pct = s.total ? (s.downloaded / s.total) * 100 : 0;
      $("progress-num").textContent = s.total
        ? `${formatBytes(s.downloaded)} / ${formatBytes(s.total)} (${pct.toFixed(0)}%)`
        : formatBytes(s.downloaded);
      bar.style.width = `${pct}%`;
      if (!s.total) bar.classList.add("indeterminate");
      btnCancel.classList.remove("hidden");
      break;
    }
    case "extracting":
      enginePill.dataset.state = "busy";
      engineText.textContent = "展開中…";
      setup.classList.remove("hidden");
      title.textContent = "展開中";
      desc.textContent = "llama.cpp を展開しています…";
      progress.classList.remove("hidden");
      $("progress-label").textContent = "展開中";
      $("progress-num").textContent = "";
      bar.classList.add("indeterminate");
      break;
    case "error":
      enginePill.dataset.state = "error";
      engineText.textContent = "エラー";
      setup.classList.remove("hidden");
      setup.dataset.kind = "error";
      title.textContent = "翻訳エンジンを起動できませんでした";
      desc.textContent = s.message;
      btnRetry.classList.remove("hidden");
      if (settings.engineMode === "builtin") btnInstall.classList.remove("hidden");
      break;
  }
  if (s.state !== "ready" && activeRequest && translateStatus.textContent === "翻訳中…") {
    translateStatus.textContent = "";
  }
}

// ---------------------------------------------------------------------------
// 設定ダイアログ
// ---------------------------------------------------------------------------

const sf = {
  shortcut: $<HTMLInputElement>("s-shortcut"),
  capture: $<HTMLInputElement>("s-capture"),
  ontop: $<HTMLInputElement>("s-ontop"),
  autoTranslate: $<HTMLInputElement>("s-auto-translate"),
  autoSwitch: $<HTMLInputElement>("s-auto-switch"),
  secondary: $<HTMLSelectElement>("s-secondary"),
  mode: $<HTMLSelectElement>("s-mode"),
  model: $<HTMLSelectElement>("s-model"),
  backend: $<HTMLSelectElement>("s-backend"),
  ctx: $<HTMLSelectElement>("s-ctx"),
  idle: $<HTMLSelectElement>("s-idle"),
  extra: $<HTMLInputElement>("s-extra"),
  serverPath: $<HTMLInputElement>("s-server-path"),
  modelPath: $<HTMLInputElement>("s-model-path"),
  url: $<HTMLInputElement>("s-url"),
  extModel: $<HTMLInputElement>("s-ext-model"),
  apiKey: $<HTMLInputElement>("s-api-key"),
  startup: $<HTMLInputElement>("s-startup"),
  minimized: $<HTMLInputElement>("s-minimized"),
  tray: $<HTMLInputElement>("s-tray"),
};
let shortcutValue = "";

function syncModeUi() {
  const external = sf.mode.value === "external";
  $("builtin-opts").classList.toggle("hidden", external);
  $("external-opts").classList.toggle("hidden", !external);
}

async function openSettings() {
  catalog = await invoke<Catalog>("get_catalog");
  appInfo = await invoke<AppInfo>("get_app_info");

  shortcutValue = settings.shortcut;
  sf.shortcut.value = prettyShortcut(settings.shortcut);
  sf.capture.checked = settings.captureSelection;
  sf.ontop.checked = settings.alwaysOnTop;
  sf.autoTranslate.checked = settings.autoTranslate;
  sf.autoSwitch.checked = settings.autoSwitch;
  sf.secondary.value = settings.secondaryLang;
  sf.mode.value = settings.engineMode;

  sf.model.innerHTML = "";
  for (const m of catalog.models) {
    const label = `${m.size.toUpperCase()} · ${m.quant} — ${formatBytes(m.bytes)} ${m.label}${m.installed ? " ✓" : ""}`;
    sf.model.add(new Option(label, `${m.size}|${m.quant}`));
  }
  sf.model.value = `${settings.modelSize}|${settings.modelQuant}`;
  sf.backend.innerHTML = "";
  for (const b of catalog.backends) sf.backend.add(new Option(b.label, b.id));
  sf.backend.value = settings.backend;
  sf.ctx.value = String(settings.ctxSize);
  if (!sf.ctx.value) {
    sf.ctx.add(new Option(settings.ctxSize.toLocaleString(), String(settings.ctxSize)));
    sf.ctx.value = String(settings.ctxSize);
  }
  sf.idle.value = String(settings.idleUnloadSecs);
  if (!sf.idle.value) {
    sf.idle.add(new Option(`${settings.idleUnloadSecs} 秒後`, String(settings.idleUnloadSecs)));
    sf.idle.value = String(settings.idleUnloadSecs);
  }
  sf.extra.value = settings.extraArgs;
  sf.serverPath.value = settings.customServerPath;
  sf.modelPath.value = settings.customModelPath;
  sf.url.value = settings.externalUrl;
  sf.extModel.value = settings.externalModel;
  sf.apiKey.value = settings.externalApiKey;
  sf.startup.checked = settings.launchAtStartup;
  sf.minimized.checked = settings.startMinimized;
  sf.tray.checked = settings.closeToTray;

  $("about-version").textContent = `PLaMo翻訳 v${appInfo.version} (llama.cpp ${catalog.llamaCppTag})`;
  $("settings-error").classList.add("hidden");
  syncModeUi();
  if (!dialog.open) dialog.showModal();
}

async function saveSettings() {
  const [modelSize, modelQuant] = sf.model.value.split("|");
  const next: Settings = {
    ...settings,
    shortcut: shortcutValue,
    captureSelection: sf.capture.checked,
    alwaysOnTop: sf.ontop.checked,
    autoTranslate: sf.autoTranslate.checked,
    autoSwitch: sf.autoSwitch.checked,
    secondaryLang: sf.secondary.value,
    engineMode: sf.mode.value as Settings["engineMode"],
    modelSize,
    modelQuant,
    backend: sf.backend.value,
    ctxSize: Number(sf.ctx.value),
    idleUnloadSecs: Number(sf.idle.value),
    extraArgs: sf.extra.value.trim(),
    customServerPath: sf.serverPath.value.trim().replace(/^"(.*)"$/, "$1"),
    customModelPath: sf.modelPath.value.trim().replace(/^"(.*)"$/, "$1"),
    externalUrl: sf.url.value.trim() || "http://127.0.0.1:8000",
    externalModel: sf.extModel.value.trim() || "plamo-3-translate",
    externalApiKey: sf.apiKey.value,
    launchAtStartup: sf.startup.checked,
    startMinimized: sf.minimized.checked,
    closeToTray: sf.tray.checked,
  };
  try {
    settings = await invoke<Settings>("save_settings", { settings: next });
    dialog.close();
    applySettingsToUi();
    lastKey = "";
    scheduleTranslate(0);
    toast("設定を保存しました");
  } catch (e) {
    const el = $("settings-error");
    el.textContent = errorMessage(e);
    el.classList.remove("hidden");
  }
}

/** 言語や文体など、メイン画面で変更した設定を保存する */
async function persistQuick(patch: Partial<Settings>) {
  settings = { ...settings, ...patch };
  try {
    settings = await invoke<Settings>("save_settings", { settings });
  } catch (e) {
    toast(errorMessage(e), true);
  }
}

function applySettingsToUi() {
  targetLang.value = settings.targetLang;
  formSelect.value = settings.form;
  const sc = prettyShortcut(settings.shortcut);
  sourceText.placeholder = settings.captureSelection
    ? `翻訳するテキストを入力\n\nどのアプリでも、テキストを選択して ${sc} を押すと翻訳できます`
    : `翻訳するテキストを入力\n\n${sc} でこのウィンドウを呼び出せます`;
  updateTargetUi();
  renderStatus();
}

// ショートカットの記録
const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight",
]);

function shortcutFromEvent(e: KeyboardEvent): { value: string; complete: boolean } {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (MODIFIER_CODES.has(e.code)) return { value: mods.join("+"), complete: false };
  let key = e.code;
  key = key.replace(/^Key([A-Z])$/, "$1").replace(/^Digit(\d)$/, "$1");
  const isFn = /^F\d{1,2}$/.test(key);
  return { value: [...mods, key].join("+"), complete: mods.length > 0 || isFn };
}

sf.shortcut.addEventListener("focus", () => {
  sf.shortcut.classList.add("recording");
  sf.shortcut.value = "キーを押してください…";
  void invoke("suspend_shortcut", { suspend: true });
});
sf.shortcut.addEventListener("blur", () => {
  sf.shortcut.classList.remove("recording");
  sf.shortcut.value = prettyShortcut(shortcutValue);
  void invoke("suspend_shortcut", { suspend: false });
});
sf.shortcut.addEventListener("keydown", (e) => {
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) {
    sf.shortcut.blur();
    return;
  }
  const { value, complete } = shortcutFromEvent(e);
  sf.shortcut.value = value ? prettyShortcut(value) + (complete ? "" : " + …") : "キーを押してください…";
  if (complete) {
    shortcutValue = value;
    sf.shortcut.blur();
  }
});
$("s-shortcut-reset").addEventListener("click", () => {
  shortcutValue = "Ctrl+K";
  sf.shortcut.value = prettyShortcut(shortcutValue);
});
sf.mode.addEventListener("change", syncModeUi);
$("btn-save-settings").addEventListener("click", () => void saveSettings());
$("open-data").addEventListener("click", (e) => {
  e.preventDefault();
  if (appInfo) void invoke("open_path", { path: appInfo.dataDir });
});
$("open-log").addEventListener("click", (e) => {
  e.preventDefault();
  if (appInfo) void invoke("open_path", { path: appInfo.logPath });
});

// ---------------------------------------------------------------------------
// イベント
// ---------------------------------------------------------------------------

sourceText.addEventListener("input", () => {
  updateSourceUi();
  scheduleTranslate();
});

sourceLang.addEventListener("change", () => {
  store.set("sourceLang", sourceLang.value);
  updateTargetUi();
  scheduleTranslate(0);
});

targetLang.addEventListener("change", () => {
  void persistQuick({ targetLang: targetLang.value });
  updateTargetUi();
  scheduleTranslate(0);
});

formSelect.addEventListener("change", () => {
  void persistQuick({ form: formSelect.value as Settings["form"] });
  scheduleTranslate(0);
});

function updateExtrasUi() {
  const n = parseGlossary(glossaryInput.value).length;
  glossaryCount.textContent = `${n} 件`;
  glossaryCount.classList.toggle("hidden", n === 0);
  contextBadge.classList.toggle("hidden", !contextInput.value.trim());
}

glossaryInput.addEventListener("input", () => {
  store.set("glossary", glossaryInput.value);
  updateExtrasUi();
  scheduleTranslate(800);
});
contextInput.addEventListener("input", () => {
  store.set("context", contextInput.value);
  updateExtrasUi();
  scheduleTranslate(800);
});

btnClear.addEventListener("click", () => {
  sourceText.value = "";
  updateSourceUi();
  void translateNow();
  sourceText.focus();
});

function swapLanguages() {
  const text = sourceText.value;
  const { lang: tgt } = effectiveTarget(text);
  const src = sourceLang.value === "auto" ? (text && detectedLang) || guessLanguage(text || output) : sourceLang.value;
  sourceLang.value = tgt;
  store.set("sourceLang", tgt);
  targetLang.value = src;
  void persistQuick({ targetLang: src });
  if (output && !targetText.classList.contains("error")) {
    sourceText.value = output;
    output = "";
    renderOutput(false);
  }
  updateSourceUi();
  scheduleTranslate(0);
}
$("btn-swap").addEventListener("click", swapLanguages);

async function copyOutput() {
  if (!output) return;
  try {
    await navigator.clipboard.writeText(output);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = output;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
  copyLabel.textContent = "コピーしました";
  setTimeout(() => (copyLabel.textContent = "コピー"), 1500);
}
btnCopy.addEventListener("click", () => void copyOutput());

enginePill.addEventListener("click", () => void openSettings());
$("btn-settings").addEventListener("click", () => void openSettings());
$("btn-setup-settings").addEventListener("click", () => void openSettings());
$("btn-install").addEventListener("click", async () => {
  try {
    await invoke("install_engine");
  } catch (e) {
    toast(errorMessage(e), true);
  }
});
$("btn-cancel-install").addEventListener("click", () => void invoke("cancel_install"));
$("btn-retry").addEventListener("click", () => void invoke("restart_engine"));

document.addEventListener("keydown", (e) => {
  if (dialog.open) return;
  if (e.ctrlKey && e.key === "Enter") {
    e.preventDefault();
    void translateNow(true);
  } else if (e.ctrlKey && e.shiftKey && e.code === "KeyC") {
    e.preventDefault();
    void copyOutput();
  } else if (e.ctrlKey && e.shiftKey && e.code === "KeyS") {
    e.preventDefault();
    swapLanguages();
  } else if (e.ctrlKey && e.key === ",") {
    e.preventDefault();
    void openSettings();
  } else if (e.key === "Escape") {
    e.preventDefault();
    void invoke("hide_window");
  }
});

/** ショートカットで取得した選択文字列を取り込んで翻訳する */
async function takeSelection() {
  const text = await invoke<string | null>("take_pending_selection");
  if (!text) return;
  sourceText.value = text;
  updateSourceUi();
  sourceText.focus();
  sourceText.setSelectionRange(0, 0);
  sourceText.scrollTop = 0;
  void translateNow(true);
}

// ---------------------------------------------------------------------------
// 初期化
// ---------------------------------------------------------------------------

async function init() {
  fillLanguageSelect(sourceLang, true);
  fillLanguageSelect(targetLang, false);
  fillLanguageSelect(sf.secondary, false);

  settings = await invoke<Settings>("get_settings");
  sourceLang.value = store.get("sourceLang", "auto");
  if (!sourceLang.value) sourceLang.value = "auto";
  glossaryInput.value = store.get("glossary");
  contextInput.value = store.get("context");
  updateExtrasUi();

  await listen<EngineStatus>("engine-status", (e) => {
    status = e.payload;
    renderStatus();
  });
  await listen<{ requestId: number; delta: string }>("translation-chunk", (e) => {
    if (e.payload.requestId !== activeRequest) return;
    output += e.payload.delta;
    renderOutput(true);
  });
  await listen("selection-captured", () => void takeSelection());
  await listen("focus-input", () => {
    sourceText.focus();
    sourceText.select();
  });
  await listen("open-settings", () => void openSettings());

  status = await invoke<EngineStatus>("get_engine_status");
  applySettingsToUi();
  updateSourceUi();
  renderOutput(false);

  appInfo = await invoke<AppInfo>("get_app_info");
  if (appInfo.shortcutError) toast(appInfo.shortcutError, true);

  await takeSelection();
  sourceText.focus();
}

void init();
