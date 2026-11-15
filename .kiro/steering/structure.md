# Project Structure

## Organization Philosophy

機能ではなく責務で層を分けるレイヤード構成を採用する。中心は `domain` に置き、I/O・端末・入力・設定はアダプタとして外側に配置する。アプリケーション状態とイベントループがアダプタを組み立てる。

## Directory Patterns

### Domain

**Location**: `src/domain/`
**Purpose**: ワークブックモデル、セル値、結合範囲、選択、レイアウト計算、検索・フィルタの評価、コピー用シリアライズなど、外部 I/O に依存しない中核ロジック
**Example**: `cell.rs`, `sheet.rs`, `layout.rs`, `selection.rs`, `filter.rs`, `search.rs`, `row_store.rs`, `freeze.rs`

### I/O Adapters

**Location**: `src/io/`
**Purpose**: calamine / csv / formualizer / clipboard / 文字コード判定 / OOXML フィルタとの境界。外部型をドメイン型へ変換し、外部エラーをドメインエラーへ写像する
**Example**: `xlsx.rs`, `csv.rs`, `formula.rs`, `clipboard.rs`, `sheet_meta.rs`, `encoding.rs`, `search.rs`, `row_cache.rs`

### UI

**Location**: `src/ui/`
**Purpose**: ratatui の描画。ドメインのレイアウト結果を端末セルへ変換する。ビジネスロジックを持たない
**Example**: `grid.rs`, `detail.rs`, `status.rs`, `theme.rs`, `filter_panel.rs`, `search_prompt.rs`

### Application

**Location**: `src/app/`
**Purpose**: 状態機械、キー入力の解釈、イベントループ、バックグラウンド計算の調整
**Example**: `state.rs`, `action.rs`, `keymap.rs`, `event_loop.rs`

### Configuration

**Location**: `src/config.rs`
**Purpose**: CLI 引数と既定値。`LayoutLimits` などのドメイン設定へ変換する
**Example**: `max_col_width`, `max_row_lines`, `clipboard_mode`

## Naming Conventions

- **Files**: snake_case（例: `event_loop.rs`）
- **Types / Traits**: PascalCase（例: `WorkbookModel`, `ClipboardSink`）
- **Functions / Modules**: snake_case
- **Errors**: `LoadError`, `FormulaError` のように対象 + `Error`
- **Constants**: SCREAMING_SNAKE_CASE。既定値は `DEFAULT_` 接頭辞

## Import Organization

```rust
// 標準ライブラリ
use std::path::Path;

// 外部クレート
use ratatui::widgets::Widget;

// クレート内（層の左方向のみ）
use crate::domain::cell::CellAddr;
use crate::io::xlsx::XlsxLoader;
```

**Path Aliases**:

- クレート内は `crate::domain::...` のような絶対パスを使い、深い相対パスを避ける

## Code Organization Principles

- 依存方向は `domain` ← `io` / `ui` / `input` / `config` ← `app` ← `main`。逆方向の import を禁止する
- `domain` は ratatui / crossterm / calamine / formualizer の型を公開しない
- 外部クレートの型はアダプタ内で完結させ、境界でドメイン型へ変換する
- 1 ファイル 1 責務を目安にし、UI 描画と状態遷移を混在させない
- テストは実装ファイルの `#[cfg(test)]` または `tests/` に置き、fixture は `tests/fixtures/` に集約する

---

_Document patterns, not file trees. New files following patterns shouldn't require updates_
