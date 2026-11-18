# Research & Design Decisions

---
**Purpose**: 構造編集・選択・クリップボードの設計判断に至った調査結果とトレードオフを記録する。
---

## Summary

- **Feature**: `structural-editing`
- **Discovery Scope**: 既存 TUI スプレッドシートへの大規模な拡張（Extension / Complex Integration）
- **Key Findings**:
  - 現在の編集は `Sheet.edits: HashMap<CellAddr, Cell>` のオーバーレイで、行・列の座標を変更する構造編集は存在しない。
  - 遅延ロードは `SheetData::Lazy(LazySheet)` とスピルファイルで実現されており、構造編集には eager 化または座標マッピングが必要。
  - `io/xlsx_write.rs` は変更セルだけを `sheetData` にパッチする。構造変更を保存するには、行・セル参照の再配置と関連 OOXML 範囲の変換が必要。
  - `formualizer` は `formualizer::parse` を再エクスポートしており、数式 AST とトークンスパンを使って参照文字列だけを差し替えられる。ただし外部クレート依存のため、実装は `io` に置く。
  - システムクリップボードは `arboard` の `get_text()` で読み取れるが、OSC 52 は書き込み専用である。
  - 非 Mono 配色は構造編集と独立した責務のため、別 spec `non-mono-theme` に分離する。

## Research Log

### 既存の選択モデルと編集オーバーレイ

- **Context**: 行・列・矩形選択と構造編集を追加するため、既存の選択・編集 API を確認した。
- **Sources Consulted**: `src/domain/selection.rs`, `src/domain/cell.rs`, `src/domain/sheet.rs`, `src/domain/edit.rs`, `src/app/state.rs`, `src/ui/grid.rs`
- **Findings**:
  - `SelectionMode` は `Cell` と `Row` のみで、`Selection::range(row_cols)` は列数のみを受け取る。
  - `Sheet` は `data`（eager / lazy）と `edits` を持ち、`set_edit_cell` が境界・レイアウト・フィルタ・数式セル一覧を更新する。
  - `EditTransaction` は `changes: Vec<EditChange>` のみで、構造変更を表現できない。
  - `GridWidget` は `ScreenView.selection` を通じて選択範囲を描画するため、選択形状を `Selection::range` に集約すれば描画側の変更を最小化できる。
- **Implications**:
  - `SelectionMode` に `Column` と `Block` を追加し、`range(rows, cols)` へ API を変更する。
  - `EditTransaction` を `ops: Vec<EditOp>` に拡張し、セル変更と構造変更を同じ履歴で扱う。
  - 選択範囲の描画は既存の `Selection::range` 経由を維持する。

### 遅延ロードと構造編集

- **Context**: 大容量シートでも構造編集を提供できるか、またメモリ上限をどう扱うかを検討した。
- **Sources Consulted**: `src/domain/row_store.rs`, `src/domain/sheet.rs`, `src/io/row_cache.rs`, `src/io/loader.rs`
- **Findings**:
  - `LazySheet` は `RowSource` から行ウィンドウを読み込み、`SheetData::Lazy` として保持する。
  - 構造編集で全セルの座標を変更する場合、スピルファイル上の行を直接書き換える API はない。
  - `SheetData::Eager(HashMap<CellAddr, Cell>)` へ変換すれば、既存の表示・フィルタ・レイアウト・コピーの読み取り経路をそのまま使える。
- **Implications**:
  - 最初の構造編集時にシートを eager 化する方式を採用する。
  - `--structural-max-cells` で上限を設け、lazy / eager の両方で上限を検査する。
  - 通常閲覧時は遅延ロードを維持する。

### 座標変換方式の比較

- **Context**: 構造編集後のセル座標をどのように表現するか。
- **Alternatives**:
  1. シートを eager 化し、`SheetData` と `edits` のキーを物理的に移動する。
  2. 現在座標から元座標へのマッピングを読み取り時に適用する。
  3. 構造操作ログを都度リプレイする。
- **Findings**:
  - 方式 1 は既存の読み取り経路を変更せず、実装リスクが最も低い。一方でメモリ使用量が増える。
  - 方式 2 は遅延ロードを維持できるが、`cell` / `read_cell` / `collect_cells` / `ensure_rows` / フィルタ / レイアウト / 結合のすべてにマッピングを適用する必要があり、変更範囲が大きい。
  - 方式 3 は操作数に比例して読み取りコストが増え、ランダムアクセスに不利。
- **Implications**:
  - 初版は方式 1 を採用する。
  - 保存時の元座標追跡のため、現在行・列から元の行・列への `origin_rows` / `origin_cols` を別途保持する。
  - 将来、大規模シート向けに方式 2 へ移行できるよう、構造操作を `StructureEngine` に閉じ込める。

### OOXML 構造保存

- **Context**: 行・列の挿入削除を xlsx に保存する方法を検討した。
- **Sources Consulted**: `src/io/xlsx_write.rs`, `src/io/sheet_meta.rs`, `src/io/xlsx.rs`
- **Findings**:
  - 現在の保存は `sheetData` 内の変更セルを置換する差分パッチで、行・セル参照の再配置は行わない。
  - セルの生 XML を保持しているため、未編集セルのスタイル属性を維持したまま `r` 属性だけを変更できる。
  - `dimension`、`cols`、`mergeCells`、`autoFilter`、`conditionalFormatting`、`pane`、行・列属性などは個別に変換が必要。
  - 共有数式・配列数式は既存のセル編集でも拒否しており、構造変換でも安全な変換が難しい。
  - テーブルパーツは列数式や構造化参照を含むことがあり、初版で安全に変換するのは難しい。
  - 保存後も `edits` と `origin` をそのまま残すと、2 回目の保存で二重変換になる。
- **Implications**:
  - 全面再生成ではなく、元の生 XML を保持したまま「現在座標へ再配置する」変換を採用する。
  - 変換できない構造を検出した場合は保存を拒否し、元ファイルを変更しない。
  - 保存成功後に origin を identity へ戻し、セル状態を正規化する契約を追加する。
  - `calcChain` 削除と `calcPr fullCalcOnLoad` は既存処理を維持する。

### 数式参照の更新とドメイン境界

- **Context**: 行・列の挿入削除に合わせて数式内の A1 参照を更新する必要がある。
- **Sources Consulted**: `Cargo.toml`, `Cargo.lock`, `~/.cargo/registry/src/.../formualizer-parse-4.2.0/src/parser.rs`, `src/io/formula.rs`, `.kiro/steering/tech.md`, `.kiro/steering/structure.md`
- **Findings**:
  - `formualizer` 0.14 は `formualizer::parse` を再エクスポートしている。
  - `Parser` は AST を生成し、`ASTNode::visit_refs` で参照を列挙できる。
  - `ASTNodeType::Reference` は元の参照文字列と `ReferenceType` を保持し、`source_token` に開始・終了位置がある。
  - `ReferenceType` はセル・範囲・シート名・絶対参照フラグを保持する。
  - steering では `domain` が外部クレートに依存しないことを必須としている。
- **Implications**:
  - 数式参照の書き換え実装は `src/io/formula_ref.rs` に置く。
  - `domain` の構造操作は数式文字列を直接解析せず、`AppState` の構造フローが domain の構造操作と io の数式更新を順に呼ぶ。
  - 数式全体の pretty print は行わず、トークンスパンで参照部分だけを差し替える。
  - パースできない数式や構造化参照は保存拒否の対象として報告する。

### クリップボード連携

- **Context**: Vim の `clipboard=unnamedplus` に近い挙動を実現する方法を調査した。
- **Sources Consulted**: `src/io/clipboard.rs`, `Cargo.toml`, `~/.cargo/registry/src/.../arboard-3.7.0/src/lib.rs`, Vim の `clipboard` オプション仕様
- **Findings**:
  - 既存の `ClipboardSink` は `write` のみで、読み取り API がない。
  - `arboard::Clipboard::get_text()` でシステムクリップボードを読み取れる。
  - OSC 52 は端末への書き込みのみで、読み取りは標準化されておらず、環境依存の問い合わせが必要。
  - `--clipboard auto` は SSH / TMUX では OSC 52 のみ、ローカルでは `system-clipboard` feature があれば OS クリップボードを使う。
  - 既存の `CopyResult` は書き込み結果のバイト数と sink 名のみで、読み取ったテキストを返さない。
- **Implications**:
  - `ClipboardService::read()` を追加し、テキストと sink 名を返す `ClipboardRead` を定義する。
  - `unnamedplus` を既定にし、読み取り不能時は内部レジスタへフォールバックする。
  - 直前の自アプリ書き込みと一致する場合は、書式・数式を保持する内部レジスタを優先する。
  - `Effect::WriteClipboard` の結果を `AppState` に返し、書き込み成功時だけ `last_text` を更新する。

### 構造 undo と状態正規化

- **Context**: 構造変更を undo / redo し、保存後の状態を一貫させる方法を検討した。
- **Findings**:
  - 既存の `EditLog` は `EditTransaction.changes` を前提にしている。
  - セルデータ全体のスナップショットを構造操作ごとに保存すると、undo 履歴のメモリが大きい。
  - 構造変更はセルの移動、削除セル、メタデータ（結合・フィルタ・レイアウト・固定ペイン・条件付き書式・origin）に分けられる。
  - 保存後はディスク上の座標が現在座標になるため、origin を identity に戻し、`edits` を base に畳み込む必要がある。
- **Implications**:
  - `EditTransaction` を `ops` に拡張し、`EditOp::Structure` に操作・削除セル・メタデータ差分・数式変更を保持する。
  - セルデータは差分、メタデータは O(行数 + 列数) のスナップショットとして保持する。
  - 保存成功後に `normalize_after_structural_save` で origin を identity に戻し、`edits` を base に統合する。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| 座標マッピング | 現在座標から元座標への写像を読み取り時に適用 | 遅延ロードを維持できる | すべての読み取り経路に写像が必要で変更範囲が広い | 将来の大規模シート向け |
| eager 化 + 物理移動 | 構造編集時にシートを eager 化し、セルキーを移動 | 既存の表示・フィルタ・レイアウト経路を維持できる | メモリ使用量が増える | 初版の採用方式 |
| 操作ログのリプレイ | 構造操作を読み取りごとに適用 | 追加の座標データが少ない | ランダムアクセスが遅く、複雑な操作で破綻しやすい | 不採用 |

## Design Decisions

### Decision: 構造編集時の eager 化

- **Context**: 遅延ロードと構造編集の両立。
- **Alternatives Considered**:
  1. 座標マッピングを全読み取り経路へ導入する。
  2. 構造編集時だけ eager 化する。
- **Selected Approach**: 最初の構造編集時に `SheetData::Lazy` を `SheetData::Eager` へ変換し、`--structural-max-cells` を上限とする。eager 済みのシートも同じ上限を検査する。
- **Rationale**: 既存の `Sheet` 読み取り API、フィルタ、レイアウト、コピーをそのまま利用でき、初版の実装リスクを抑えられる。
- **Trade-offs**: 大容量シートではメモリ使用量が増える。上限超過時は構造編集できない。
- **Follow-up**: 将来、座標マッピング方式へ移行する場合は `StructureEngine` の境界を維持する。

### Decision: 生 XML の再配置による構造保存

- **Context**: 構造変更を xlsx へ保存する方法。
- **Alternatives Considered**:
  1. セルモデルからシート全体を再生成する。
  2. 元の生 XML を保持し、行・セル参照だけを再配置する。
- **Selected Approach**: 元の `sheetData` を行・セル単位でパースし、`origin_rows` / `origin_cols` に従って現在座標へ再配置する。未編集セルは元の XML とスタイル属性を保持する。
- **Rationale**: 既存のスタイル・表示形式・罫線など、`Cell` モデルが完全には保持しない情報を失わない。
- **Trade-offs**: XML 変換ロジックが複雑になる。共有数式・配列数式・テーブルなどは個別対応が必要。
- **Follow-up**: 対応できない構造は保存を拒否する。

### Decision: 数式参照更新は io、構造操作は domain

- **Context**: steering の依存方向を守りつつ数式参照を更新する。
- **Alternatives Considered**:
  1. `FormulaRefEngine` を domain に置く。
  2. `io/formula_ref.rs` に置き、app が構造操作と数式更新を順に呼ぶ。
- **Selected Approach**: 2 を採用する。domain の `StructureEngine` はセル・メタデータだけを変換し、`AppState` の構造フローが `io::formula_ref` を呼んで数式を更新する。
- **Rationale**: `domain` は外部クレートに依存しないという steering の必須条件を守れる。
- **Trade-offs**: 構造操作と数式更新の呼び出し順序を app が保証する必要がある。
- **Follow-up**: 構造操作と数式更新を 1 つのユースケースとしてテストする。

### Decision: ワークブック単位の保存計画と保存後の正規化

- **Context**: 構造変更だけの保存、クロスシート数式、定義名、再計算用一時ブックを既存の保存パイプラインへ接続する。
- **Alternatives Considered**:
  1. 既存のセル編集保存に構造変換を追加する。
  2. ワークブック全体の `WorkbookSavePlan` を構築し、構造変換・数式更新・定義名更新を一括で行う。
- **Selected Approach**: 2 を採用する。`save_workbook` と `write_temp_workbook` は構造 dirty なシートを含め、保存成功後に origin を identity に戻して `edits` を base に統合する。
- **Rationale**: 構造変更だけの保存、再計算、2 回目以降の保存を一貫して扱える。
- **Trade-offs**: 保存経路の変更範囲が広い。
- **Follow-up**: 連続 2 回保存、構造変更のみの保存、クロスシート数式のラウンドトリップテストを追加する。

### Decision: テーブル・共有数式は fail closed

- **Context**: 初版で安全に変換できる OOXML 構造の範囲を決める。
- **Alternatives Considered**:
  1. テーブル・共有数式も変換する。
  2. 検出時は保存を拒否する。
- **Selected Approach**: 2 を採用する。共有数式、配列数式、テーブルパーツ、構造化参照を検出した場合は `SaveError::UnsupportedStructure` を返す。
- **Rationale**: データ破損を防ぐことを優先する。
- **Trade-offs**: 一部のワークブックは構造保存できない。
- **Follow-up**: 将来、テーブルと共有数式の変換を別 spec で追加する。

### Decision: `unnamedplus` 既定とリッチレジスタ優先

- **Context**: Vim の `clipboard=unnamedplus` に近い挙動と、スプレッドシートの書式・数式保持の両立。
- **Alternatives Considered**:
  1. 常にシステムクリップボードのテキストだけを使う。
  2. 常に内部レジスタを使い、`"+p` だけシステムクリップボードを使う。
  3. `unnamedplus` を既定にし、直前の自アプリ書き込みと一致する場合だけ内部レジスタを優先する。
- **Selected Approach**: 3 を採用する。
- **Rationale**: 外部アプリでコピーした内容は Vim と同じように貼り付けられ、`yy` → `p` のような即時操作では書式・数式を失わない。
- **Trade-offs**: Vim の厳密なテキストレジスタ動作とは少し異なる。
- **Follow-up**: システムクリップボード読み取り不能環境では内部レジスタへフォールバックする。

### Decision: 非 Mono 配色は別 spec

- **Context**: 配色変更は構造編集・クリップボード・保存から独立して検証できる。
- **Selected Approach**: `non-mono-theme` spec に分離する。
- **Rationale**: 設計レビューの承認単位を分け、実装順序と失敗時の切り分けを明確にする。
- **Trade-offs**: spec が 2 つになる。
- **Follow-up**: 構造編集 spec は配色の契約に依存しない。

## Risks & Mitigations

- **Risk**: eager 化によるメモリ増加 — `--structural-max-cells` と明確なエラーで緩和する。
- **Risk**: OOXML 構造変換の不完全さ — 生 XML を保持し、未対応構造は保存拒否する。ラウンドトリップテストを充実させる。
- **Risk**: 数式参照の更新漏れ — AST の参照列挙とスパン差し替えを使い、同一シート・クロスシート・定義名のテストを追加する。実装は io に置き、domain の依存方向を守る。
- **Risk**: 共有数式・配列数式・テーブルの破損 — 検出時は保存を拒否し、元ファイルを変更しない。
- **Risk**: 保存後の二重変換 — 保存成功後に origin を identity に戻し、`edits` を base に統合する。
- **Risk**: クリップボード読み取りの環境差 — `system-clipboard` feature とローカル環境を前提とし、利用不可時は内部レジスタへフォールバックする。
- **Risk**: `dd` の挙動変更による混乱 — README とヘルプで明示し、内容クリアは `d$` や `V` + `Delete` で行えるようにする。

## References

- `src/domain/selection.rs` — 既存の選択モデル。
- `src/domain/sheet.rs` — eager / lazy と編集オーバーレイ。
- `src/domain/row_store.rs` — 遅延ロードの行ソース。
- `src/domain/edit.rs` — 既存の undo / redo トランザクション。
- `src/io/xlsx_write.rs` — 既存の OOXML 差分保存。
- `src/io/clipboard.rs` — OSC 52 と OS クリップボードの抽象化。
- `formualizer-parse 4.2.0` — `Parser`、`ASTNode::visit_refs`、`Token` のスパン情報。
- `arboard 3.7.0` — `Clipboard::get_text`。
- Vim `:help clipboard` — `unnamed` / `unnamedplus` のレジスタ仕様。
