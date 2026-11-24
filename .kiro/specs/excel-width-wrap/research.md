# Research & Design Decisions

---
**Purpose**: 既存のレイアウト・描画・編集経路を調査し、xlsx 明示列幅での折り返し表示を既存モードと干渉させずに実現する設計判断を記録する。

**Usage**:
- 実装時の制約と再利用可能な既存処理を共有する。
- 却下した方式と採用理由を残す。
---

## Summary
- **Feature**: `excel-width-wrap`
- **Discovery Scope**: Extension（既存の Excel 準拠レイアウト・はみ出し・行高・編集経路の拡張）
- **Key Findings**:
  - はみ出し幅の計算は `LayoutEngine::render_row` と `LayoutEngine::measure_cell` に重複して存在し、同じ規則を維持する必要がある。
  - 行高は読み込み時に `LayoutSnapshot.row_heights` として確定し、`LayoutEngine` / `ViewportPlanner` / freeze / 詳細表示が共通で参照する。
  - fit-width は `Sheet.layout` を変更せず `ColumnWidths` を派生させる設計であり、本仕様の追加行高も表示モード専用に分離するのが整合的である。
  - 明示行高は現在、自動計算の行高を置き換える。本仕様では「折り返しに必要な行数の下限」として扱う必要がある。
  - lazy 読み込みはセルをスピルファイルへ書き出すため、読み込み後の全セル再走査を避ける必要がある。

## Research Log

### 既存のはみ出し・切り詰め規則
- **Context**: 折り返しを既存の横方向はみ出しと矛盾させないため。
- **Sources Consulted**: `src/domain/layout.rs` の `render_row` / `measure_cell`、`.kiro/specs/spreadsheet-tui/design.md`、`.kiro/specs/view-consistency/design.md`
- **Findings**:
  - 対象は単一行（改行なし・`wrap_text` なし）に限定される。
  - 右端に可視罫線がある場合は拡張しない。
  - 右隣が値を持つセル、結合範囲、左罫線を持つセル、使用範囲の終端で停止する。
  - 空セルへ拡張するたびにガター 1 を加算する。
  - 全文が拡張後の幅に収まらない場合は `…` で切り詰める。
- **Implications**: 新しい折り返しはこの幅計算を共有し、「収まるなら従来のはみ出し」「収まらないなら同じ幅で折り返し」と分岐する必要がある。計算を複製するとグリッドと詳細がずれる。

### 行高の決定経路
- **Context**: 折り返し行数をどこで確定し、誰が参照するか。
- **Sources Consulted**: `LayoutStatsBuilder::finish`、`LayoutSnapshot`、`LayoutEngine::plan` / `effective_freeze` / `measure_cell`、`ViewportPlanner::ensure_visible`、`src/ui/grid.rs`
- **Findings**:
  - 読み込み時に `row_demand`、`wrap_demands`、結合範囲、Excel 明示行高から `row_heights: Vec<u8>` が確定する。
  - `row_heights` はスクロール位置、固定行の高さ、詳細表示、グリッド描画に共通で使われる。
  - 行挿入・削除時は `structure.rs` が `row_heights` をシフトする。
- **Implications**: 追加の折り返し行高は `row_heights` と別に保持しても、全参照箇所で「実効行高」を返す共通関数が必要になる。

### fit-width と content レイアウト
- **Context**: 新機能が既存の表示モードを壊さない条件。
- **Sources Consulted**: `.kiro/specs/fit-width/requirements.md` / `design.md`、`AppState::column_widths`、`ColumnWidths`
- **Findings**:
  - fit-width は `ColumnWidths::fit` で列幅だけを縮小し、行高を再計算しない。
  - `ColumnWidths::from_sheet` は fit-width 無効時の列幅をそのまま使う。
  - content レイアウトは Excel 明示幅を受け取らないため `fixed_columns` が空になる。
- **Implications**: `ColumnWidths` に xlsx 幅優先モードのフラグを持たせ、fit-width では追加行高を参照しない。content レイアウトは自然に対象外になる。

### eager / lazy 読み込み
- **Context**: 折り返し行高を読み込み時に確定する方法。
- **Sources Consulted**: `src/io/xlsx.rs` の eager / lazy 経路、`SpillRowCache`、`SheetData`
- **Findings**:
  - eager は全セルを `HashMap<CellAddr, Cell>` に保持するため、レイアウト確定後に全セルを参照できる。
  - lazy は行単位のセルをスピルファイルへ書き出し、メモリ上の全セルを保持しない。
  - どちらも `LayoutStatsBuilder::finish` の前に最終的な使用範囲が確定する。
- **Implications**: eager は確定後の後処理で正確に計算できる。lazy は読み込み中に候補セルの停止位置だけを保持し、使用範囲と列幅の確定後に折り返し行数を求める必要がある。

### 編集・数式再計算
- **Context**: 初期表示だけでなく、内容変更後の行高をどう更新するか。
- **Sources Consulted**: `Sheet::set_edit_cell`、`Workbook::apply_formula_results`、`LayoutSnapshot::grow_for_cell`、`src/domain/structure.rs`
- **Findings**:
  - `grow_for_cell` は自動列幅・wrap 行高を grow-only で更新する。
  - 固定列は列幅拡張の対象外、固定行は行高拡張の対象外になっている。
  - 値の変更は編集オーバーレイと数式オーバーレイの両方から発生する。
- **Implications**: 固定列の折り返しは `grow_for_cell` の既存責務とは別に、行単位の再評価処理を設ける。明示行高も下限として拡張するため、固定行であっても grow-only の対象にする。

### 結合セル
- **Context**: 明示幅を含む結合範囲で折り返すか。
- **Sources Consulted**: `LayoutStatsBuilder::finish` の merge demand、`render_row` の merge branch、`merge_total_height`
- **Findings**:
  - 結合範囲は横方向へはみ出さず、範囲全体の幅で折り返すか切り詰める。
  - 既存の `wrap_text` 結合は確定列幅で折り返し行数を計算し、行へ均等配分する。
  - 非 `wrap_text` 結合は長文でも 1 行として扱われ、切り詰められる。
- **Implications**: 結合範囲が明示列幅の列を含む場合だけ、非 `wrap_text` でも同じ均等配分で折り返し行高を追加する。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| A: `row_heights` を直接拡張 | 読み込み時に `row_heights` を折り返し行数まで拡大 | 変更が最小。既存のスクロール・freeze・詳細が自動で追随 | fit-width に切り替えても高い行が残る。モード分離が弱い | 不採用 |
| B: 表示専用の追加行高テーブル | `LayoutSnapshot` に `wrap_row_heights` を追加し、xlsx 幅優先時だけ加算 | fit-width / content の既存挙動を維持できる。`ColumnWidths` と同じ表示時派生の考え方 | 行高の参照箇所を共通関数へ置き換える必要がある | 採用 |
| C: 描画時に動的計算 | 表示する行だけ折り返し行高をその場で計算 | 元レイアウトを変更しない。編集反映が自然 | スクロール・freeze が行高を事前に必要とし、未ロード行では計算できない | 不採用 |
| D: 明示列幅では常に折り返す | 空セルへのはみ出しを廃止 | 実装が単純 | 既存の Excel 風はみ出しとユーザー決定に反する | 不採用 |

## Design Decisions

### Decision: 表示専用の追加行高 `wrap_row_heights`
- **Context**: 折り返し行高を fit-width / content に影響させず、スクロールと詳細でも同じ高さを使う必要がある。
- **Alternatives Considered**:
  1. `row_heights` を直接書き換える。
  2. 表示モードごとに高さを派生させる。
- **Selected Approach**: `LayoutSnapshot` に `wrap_row_heights: BTreeMap<u32, u8>` を追加し、`display_row_height(row, xlsx_priority)` で `row_heights` との最大値を返す。`ColumnWidths::from_sheet` を xlsx 幅優先、`ColumnWidths::fit` を非優先とする。
- **Rationale**: `ColumnWidths` が列幅の表示時派生を担っているのと同じ責務分離になる。fit-width では追加行高を参照しないため、既存の「行高を再計算しない」契約を維持できる。
- **Trade-offs**: LayoutEngine / ViewportPlanner / 詳細の行高参照を共通関数へ置き換える必要がある。
- **Follow-up**: 行挿入・削除、列構造変更、編集・再計算でテーブルを更新するテストを追加する。

### Decision: はみ出し可能幅で折り返す
- **Context**: 空セルへはみ出せる場合と、全文が収まらない場合の規則を決める必要がある。
- **Alternatives Considered**:
  1. 自列幅だけで折り返す。
  2. 常に空セルへはみ出し、収まらない分だけ切り詰める。
  3. はみ出し可能幅を使い、収まらなければその幅で折り返す。
- **Selected Approach**: 既存のはみ出し可能幅を計算し、全文が収まるなら従来どおり 1 行ではみ出し、収まらない場合だけその幅で折り返す。
- **Rationale**: 既存の Excel 風はみ出しを維持しつつ、全文を表示できる。ユーザーが選択した「空セルへは従来通りはみ出し」と一致する。
- **Trade-offs**: 折り返し行は複数列にまたがる幅でレイアウトされるため、`RenderCell.width` はスロット幅より広いままになる。
- **Follow-up**: グリッドと詳細で同じ幅・行数になることを統合テストで確認する。

### Decision: 対象は単一行かつ非 `wrap_text` の明示列幅セル
- **Context**: 既存の `wrap_text`・明示改行・content レイアウトの規則を重複させない。
- **Alternatives Considered**:
  1. すべての長文セルを対象にする。
  2. `wrap_text` セルも含めて行高を再計算する。
  3. 明示列幅・単一行・非 `wrap_text` に限定する。
- **Selected Approach**: 3 を採用する。`wrap_text` と明示改行は既存の折り返し・切り詰め規則を維持する。
- **Rationale**: 既存の view-consistency と excel-layout の契約を変えず、今回の課題である「はみ出してしまう長文」に限定できる。
- **Trade-offs**: 明示行高 1 行の `wrap_text` セルは従来どおり `…` になる。
- **Follow-up**: `excel_layout.xlsx` の既存期待値が変わらないことを確認する。

### Decision: 明示行高は下限、`--max-row-lines` は安全上限
- **Context**: 「全体を表示」と Excel 行高・暴走防止のバランス。
- **Alternatives Considered**:
  1. 明示行高を維持して切り詰める。
  2. 明示行高も必要な行数まで拡張する。
  3. `--max-row-lines` も無視する。
- **Selected Approach**: 明示行高を下限として拡張し、`--max-row-lines` を上限とする。上限超過分は `…` とし、詳細表示で全文を確認できる。
- **Rationale**: ユーザー決定と一致し、極端に高い行で画面が使えなくなるのを防ぐ。
- **Trade-offs**: 既定の 10 行を超える長文は折り返しても途中で切れる。
- **Follow-up**: 上限到達時の `truncated` と詳細表示をテストする。

### Decision: eager は後処理、lazy は停止位置メタデータ
- **Context**: lazy 読み込みで全セルを保持せずに正確な折り返し行高を求める。
- **Alternatives Considered**:
  1. 読み込み後にスピルファイルを再走査する。
  2. 候補行の全セルをメモリに保持する。
  3. 候補セルごとに右方向の停止位置だけを保持する。
- **Selected Approach**: eager はレイアウト確定後に全セルを参照して計算する。lazy は読み込み中に候補セルと停止位置（値・罫線・結合）だけを保持し、最終使用範囲と列幅の確定後に折り返し行数を計算する。
- **Rationale**: lazy のメモリ使用をセル数に比例させず、追加 I/O も避けられる。
- **Trade-offs**: 停止位置を求めるための走査ロジックが必要になる。
- **Follow-up**: eager / lazy で同じ `wrap_row_heights` になることを fixture で確認する。

### Decision: 編集後は行単位で grow-only 再評価
- **Context**: 編集で長文になった場合や、隣接セルが埋まって折り返しが必要になった場合。
- **Alternatives Considered**:
  1. 編集セルだけを再評価する。
  2. 行全体の固定列セルを再評価する。
  3. 何もしない。
- **Selected Approach**: 影響行の固定列セルをすべて再評価し、必要な行高を grow-only で加算する。結合セルは結合範囲単位で再評価する。列構造変更時は対象シートの追加行高を再計算する。
- **Rationale**: 隣接セルの編集で左側の長文が塞がれるケースも正しく反映できる。既存の grow-only 方針とも一致する。
- **Trade-offs**: 編集のたびに行の列数分の走査が入る。固定列が存在するシートに限定される。
- **Follow-up**: 編集・数式再計算・行/列構造編集のテストを追加する。

## Risks & Mitigations
- **グリッドと詳細の不一致** — はみ出し幅・折り返し判定を共通ヘルパー化し、`measure_cell` と `render_row` が同じ結果を使うテストを追加する。
- **lazy のメモリ増加** — 候補セルの停止位置だけを保持し、行全体やセル文字列の複製を避ける。
- **fit-width の退行** — `ColumnWidths::fit` では `wrap_row_heights` を参照しないことを単体・統合テストで固定する。
- **行高の暴走** — `--max-row-lines` でクランプし、上限時は `…` を表示する。
- **明示行高の互換性** — 既存の `excel_layout.xlsx` では非 `wrap_text` の短い値しか対象にならないため、既存期待値を維持できることを確認する。
- **構造編集後の古い行高** — 行挿入・削除でシフトし、列挿入・削除では追加行高を再計算する。
- **極端に高い行でグリッドが空になる** — `--max-row-lines` の既定値と既存の上限運用を維持し、必要に応じて別specで部分描画を検討する。

## References
- `.kiro/specs/excel-layout/design.md` — 明示列幅・明示行高の確定順序。
- `.kiro/specs/fit-width/design.md` — 表示時 `ColumnWidths` と行高を再計算しない契約。
- `.kiro/specs/view-consistency/design.md` — 切り詰め・折り返し・freeze の整合。
- `.kiro/specs/spreadsheet-tui/design.md` — 横方向はみ出しの既存規則。
- `src/domain/layout.rs` — `LayoutStatsBuilder`、`LayoutEngine`、`ColumnWidths`。
- `src/io/xlsx.rs` — eager / lazy 読み込みと `LayoutStatsBuilder` の呼び出し。
- `src/domain/sheet.rs` / `src/domain/workbook.rs` / `src/domain/structure.rs` — 編集・数式反映・構造変更。
- `src/domain/viewport.rs` — 行高を使うスクロール補正。
- [textwrap 0.16.4 documentation](https://docs.rs/textwrap/0.16.4/textwrap/) — 折り返し行数の計算。
- [unicode-width 0.2 documentation](https://docs.rs/unicode-width/0.2/) — 表示幅の計算。
