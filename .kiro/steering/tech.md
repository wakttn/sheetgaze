# Technology Stack

## Architecture

レイヤード構成とし、中核のドメインモデルを I/O・TUI から独立させる。依存方向は `domain` ← `io` / `ui` / `input` / `config` ← `app` ← `main` の一方向とする。`domain` は外部クレート（ratatui、calamine、crossterm）に依存しない。

## Core Technologies

- **Language**: Rust 2024 edition
- **MSRV**: 1.88（ratatui 0.30 系の要件に合わせる）
- **TUI**: ratatui 0.30.2 + crossterm 0.29
- **CLI**: clap 4.6
- **Error**: thiserror 2.0

## Key Libraries

- **xlsx 読み込み**: calamine 0.38（値・数式・スタイル・結合範囲・ストリーミングセルリーダー）
- **数式計算**: formualizer 0.14（`xlsx-recalc` feature、Excel 互換関数）
- **csv 読み込み**: csv 1.4（RFC 4180、引用符内改行）
- **文字コード判定**: chardetng 1.0 + encoding_rs 0.8（BOM 優先、Shift_JIS / CP932 / EUC-JP / UTF-16）
- **シートメタデータ**: OOXML の `autoFilter` / `pane` / `cols` / `row` を quick-xml 0.42 + zip 9.5 で読み取り、`FilterEngine` / `FreezePane` / `ExcelLayout` へ変換
- **xlsx 書き戻し**: zip 9.5 で既存エントリを保持し、quick-xml 0.42 で worksheet XML の変更セルだけを差分更新する
- **遅延ロード**: tempfile 3.27 のスピルファイル + postcard 1.1 の行キャッシュ
- **表示幅**: unicode-width 0.2（全角・結合文字）
- **折り返し**: textwrap 0.16（`wrap_text` セルのみ）
- **クリップボード**: OSC 52（base64 0.23）を第一手段、arboard 3.7 をローカル環境向けの補助
- **日付表示**: chrono 0.4（calamine の `chrono` feature 経由）

## Development Standards

### Type Safety

- 公開境界では型付き enum / newtype を使い、`String` の意味をコメントに頼らない
- `unsafe` は原則禁止。必要な場合は安全性コメントとテストを必須とする
- エラーは `thiserror` によるドメイン別 enum とし、`unwrap` / `expect` を本番経路で使わない

### Code Quality

- `cargo fmt --all` と `cargo clippy --all-targets -- -D warnings` を必須とする
- モジュールは単一責任に保ち、`domain` から `ui` / `io` への逆依存を作らない
- 表示ロジックとデータ変換を分離し、レイアウトは純粋関数としてテストする

### Testing

- ドメインロジックは単体テスト、ローダーは fixture を使った統合テスト
- TUI は `ratatui::backend::TestBackend` とスナップショットで検証する
- クリップボードと数式計算は trait で差し替え、外部環境なしでテストする

## Development Environment

### Required Tools

- Rust 1.88 以上（stable）
- `cargo`, `rustfmt`, `clippy`
- xlsx / csv のテスト fixture を生成できる環境（開発時のみ）

### Common Commands

```bash
# Dev: cargo run -- path/to/book.xlsx
# Build: cargo build --release
# Test: cargo test --all-targets
# Lint: cargo clippy --all-targets -- -D warnings
# Format: cargo fmt --all
```

## Key Technical Decisions

- **読み込みと計算の分離**: calamine のストリーミングセルリーダーで表示モデルを行単位に構築し、formualizer で数式のみを再計算する。計算は別スレッドで行い、UI をブロックしない。
- **編集オーバーレイ**: ベースの読み取りモデルは不変に保ち、`Sheet.edits` と undo トランザクションで未保存変更を保持する。eager / lazy の両方で同じ編集経路を使う。
- **OOXML 差分保存**: 変更セルだけを worksheet XML に反映し、zip の他のエントリを保持する。テキストは `inlineStr`、数式は `<f>` とキャッシュ、再計算は `calcPr fullCalcOnLoad` で促す。保存は一時ファイル + atomic rename で行う。
- **Excel 準拠レイアウト**: Excel の明示列幅・行高（`customWidth` / `customHeight`）を優先し、未指定の行・列は表示内容から TUI 用の幅・行数を決める。`--layout content` で完全自動に切り替えられ、上限は CLI で設定可能にする。明示列幅の列では、はみ出し可能幅に全文が収まらない単一行の値を折り返し、表示専用の追加行高で全体を見せる。追加行高は fit-width では使用せず、`--max-row-lines` を安全上限とする。
- **表示時フィット幅**: 元の `Sheet.layout` は変更せず、表示時だけ全列を端末幅へ比例縮小する `ColumnWidths` を派生させる。`--fit-width` と `w` で切り替え、横スクロール・固定列・詳細表示も同じ表示幅を使う。
- **遅延ロード**: 閾値超過時はメタデータと行インデックスを先に構築し、セル本体はスピルファイルから行ウィンドウ単位で読み込む。メモリはキャッシュ行数に比例させる。
- **検索とフィルタの分離**: 検索は全シートの一致セルを収集し、シートごとの可視性で移動を制御する。Excel フィルタは読み取り専用の条件マスクとして評価し、表示・移動・コピー・置換対象に適用する。
- **クリップボード抽象化**: OSC 52 と OS クリップボードを同一 trait の実装として扱い、環境に応じて選択する。
- **純粋 Rust の維持**: コア機能は Rust クレートのみで完結させ、外部オフィススイートや外部プロセスを呼び出さない。

---

_Document standards and patterns, not every dependency_
