# PLaMo翻訳

Preferred Networks の翻訳特化 LLM [PLaMo 3 Translate](https://huggingface.co/pfnet/plamo-3-translate-0.8b) を
ローカルで動かす、DeepL ライクな Windows 向けデスクトップ翻訳アプリです (Tauri 2 製)。

## 特長

- **どこからでも呼び出し**: 任意のアプリでテキストを選択して **Ctrl + K** を押すと、アプリが前面に出て選択文字列を翻訳します
  (ショートカットは設定で変更可能)。
- **完全ローカル**: 初回に llama.cpp と GGUF モデル (0.8B Q4_K_M なら約 0.6 GB) を自動ダウンロード。以降はオフラインで翻訳できます。
- **ストリーミング表示**: 訳文を生成しながら逐次表示。入力中は自動で翻訳します。
- **53 言語対応**、日本語の原文は自動で英語 (設定で変更可) に翻訳。
- **翻訳オプション**: 文体 (です・ます調 / だ・である調)、用語集、参考文脈。
- **長文対応**: 長い文章は段落単位に分割し、直前の対訳を履歴として渡しながら順に翻訳します。
- **GPU メモリは使うときだけ**: 一定時間 (既定 5 分) 翻訳しないとモデルを GPU メモリから解放し、次の翻訳時に自動で再読み込みします。
- タスクトレイ常駐、Windows 起動時の自動起動、多重起動防止。
- 外部の OpenAI 互換サーバー (llama-server / vLLM) にも接続可能。

## 使い方

1. インストーラ (`PLaMo翻訳_x.y.z_x64-setup.exe` または `.msi`) を実行します。
2. 初回起動時に「ダウンロードして開始」を押すと、翻訳エンジン (llama.cpp) とモデルがダウンロードされ、自動で起動します。
3. 任意のアプリで文字列を選択し **Ctrl + K** を押すと翻訳されます。

| 操作 | キー |
| --- | --- |
| 選択文字列を翻訳 / アプリを呼び出す | Ctrl + K (変更可) |
| すぐに翻訳 | Ctrl + Enter |
| 訳文をコピー | Ctrl + Shift + C |
| 言語を入れ替え | Ctrl + Shift + S |
| 設定 | Ctrl + , |
| ウィンドウを隠す | Esc |

### 設定できる項目

- 呼び出しショートカット、選択文字列の自動翻訳の有無、最前面表示
- 入力中の自動翻訳、日本語原文の翻訳先
- モデルサイズ (0.8B / 2B / 8B) と量子化形式、実行デバイス (Vulkan / CPU / CUDA)、最大長
- 未使用時に GPU メモリを解放するまでの時間 (解放しない / 30 秒〜1 時間)。解放後の最初の翻訳は再読み込みのぶん遅くなります
- llama-server の追加引数、任意の llama-server.exe / GGUF ファイルの指定
- 外部サーバー (URL・モデル名・API キー)
- 自動起動、起動時にトレイへ格納、閉じるボタンの動作

データ (モデル・llama.cpp) は `%LOCALAPPDATA%\jp.plamo-translater.desktop`、
設定は `%APPDATA%\jp.plamo-translater.desktop\settings.json` に保存されます。

## 仕組み

```
[任意のアプリ] --Ctrl+K--> 選択文字列を Ctrl+C でコピー (クリップボードは元に戻す)
      |
      v
[Tauri (Rust)] --PFML3 プロンプト--> llama-server /v1/completions (stream)
      |                                   ^ 子プロセスとして起動・管理
      v
[WebView UI] <-- 訳文を逐次表示
```

- プロンプトはモデルカードの推奨どおり [PFML3 形式](https://huggingface.co/pfnet/plamo-3-translate-0.8b/blob/main/README_ja.md#pfml3プロンプト形式) を直接組み立て、
  Completions API に `stop: ["<|plamo:tag|>", "<|plamo:bos|>"]`, `temperature: 0` で送信します (`src-tauri/src/pfml.rs`)。
- llama.cpp は動作確認済みのリリース (`src-tauri/src/catalog.rs` の `LLAMA_CPP_TAG`) を GitHub Releases から取得します。
- llama-server はジョブオブジェクトに入れて起動するため、アプリが異常終了しても残りません。
- GPU メモリの解放は llama-server の `--sleep-idle-seconds` を使っています。

## 開発

必要なもの: Node.js 20+, Rust (stable), Windows では Visual Studio Build Tools と WebView2。

```powershell
npm ci
npx tauri dev      # 開発実行
npx tauri build    # インストーラ作成 (src-tauri/target/release/bundle/)
cd src-tauri; cargo test --lib   # PFML3 プロンプト生成のテスト
```

E2E テスト (初回セットアップ → 別ウィンドウで選択して Ctrl+K → 訳文表示までを自動操作し、スクリーンショットを保存):

```powershell
powershell -STA -File scripts\e2e-test.ps1 -Exe <plamo-translater.exe のパス> -OutDir <出力先>
```

WSL で開発している場合は、Windows 側の Rust / Node でビルドするスクリプトを使えます。

```bash
scripts/build-windows.sh build   # Windows 側 (C:\Users\<user>\prj\plamo_translater) に同期してビルド
scripts/build-windows.sh test    # ユニットテスト
scripts/build-windows.sh dev     # 開発実行
```

## ライセンスについて

このアプリのソースコードは [MIT License](LICENSE) です。

PLaMo 3 Translate のモデルは [PLaMo Community License](https://plamo.preferredai.jp/info/plamo-community-license-ja) で提供されています。
個人・非商用利用は登録なしで無償利用できますが、商用利用には条件があります。詳細はライセンス本文を確認してください。
llama.cpp は MIT License です。
