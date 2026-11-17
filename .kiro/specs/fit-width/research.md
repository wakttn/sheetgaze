# Research & Design Decisions

## Summary
- **Feature**: `fit-width`
- **Discovery Scope**: Extension（既存のレイアウト・スクロール・描画への統合）
- **Key Findings**:
  - `Sheet.layout` は読み込み時に確定し、描画・横スクロール・freeze・詳細の切り詰め判定が同じ `LayoutSnapshot` を参照している。
  - 表示幅を変えるには `LayoutEngine::plan` だけでなく `ViewportPlanner` と `LayoutEngine::measure_cell` / `truncated_at` も同じ幅を見る必要がある。
  - `Sheet.layout` を直接書き換えると、数式再計算の grow-only 更新、再読込、元のレイアウトへの復元が複雑になるため、表示時だけ派生する方が安全である。
  - 行高は列幅から計算されているため、フィット幅では行高を再計算しない方針を明示する必要がある。

## Research Log

### 既存のレイアウト確定と利用箇所
- **Context**: フィット幅をどこで適用するか判断するため。
- **Sources Consulted**: `src/domain/layout.rs`、`src/domain/viewport.rs`、`src/domain/sheet.rs`、`src/app/state.rs`。
- **Findings**:
  - `LayoutStatsBuilder::finish` が `LayoutSnapshot { column_widths, row_heights, fixed_columns, fixed_rows }` を生成する。
  - `LayoutEngine::plan` は `sheet.layout` から列幅を取得し、`RenderPlan` のセル幅・結合幅・はみ出し幅を決める。
  - `ViewportPlanner::ensure_visible` は同じ列幅で横スクロール位置を決める。
  - `LayoutEngine::measure_cell` / `truncated_at` はセル詳細の切り詰め判定に使われる。
  - `LayoutEngine::effective_freeze` は固定列の幅を `viewport.width` と比較する。
- **Implications**: フィット幅はこれらすべてに同じ値で渡す必要がある。`RenderPlan` だけを変えると、フォーカス移動や詳細表示と矛盾する。

### 表示時の派生 vs `Sheet.layout` の書き換え
- **Context**: 元の列幅へ戻すトグルを安全に実現する方法の比較。
- **Sources Consulted**: 既存の `grow_for_cell`、`WorkbookModel::apply_formula_results`、`replace_workbook`。
- **Findings**:
  - `Sheet.layout` は数式再計算時に grow-only で更新される。
  - reload は新しい `WorkbookModel` を作るため、状態を `Sheet` に持たせると再適用が必要になる。
  - フィット幅は端末幅に依存するため、リサイズのたびに再計算が必要になる。
- **Implications**: `Sheet.layout` は元のレイアウトのまま保持し、`AppState` が表示用の列幅テーブルを都度生成する。これにより、再計算・再読込・リサイズは既存の流れに乗る。

### 比例縮小アルゴリズム
- **Context**: 全列を画面内に収めつつ、元の列幅の大小を保つ方法。
- **Sources Consulted**: Excel の列幅単位、`unicode-width`、既存の `CELL_GUTTER`。
- **Findings**:
  - 列幅は端末表示桁（`u16`）で、全角文字も `unicode-width` により表示幅へ換算済み。
  - 描画では各列の後ろに 1 桁のガターがある。
  - 単純な均等割りは元の列幅の意図（長い列・短い列）を失う。
  - 比率配分では最低 1 桁を確保しないと、極端に狭い列で内容が完全に失われる。
- **Implications**: 各列 1 桁を確保した残りを元の幅の比率で配分する。元の幅を超えない。端数は最大剰余順、同点は列インデックス順で決定的に配分する。1 桁でも収まらない場合は `limited` として横スクロールを残す。

### 行高と wrap_text の扱い
- **Context**: 列幅を縮小したときに折り返し行数が増えるか。
- **Sources Consulted**: `LayoutStatsBuilder::finish`、`wrap_display`、`is_truncated`。
- **Findings**:
  - 行高は読み込み時に確定しており、`wrap_text` セルの折り返し行数もその時点の列幅で計算されている。
  - 表示時に列幅だけを縮めると、折り返し行が不足して `…` になる可能性がある。
  - 行高を再計算するには全セルの再走査が必要で、lazy 読み込みやリサイズ時のコストが大きい。
- **Implications**: 初版では行高を再計算しない。`wrap_text` セルは既存の切り詰め規則で `…` を表示し、詳細表示で全文を確認できる。README の制約に明記する。

### CLI と print モード
- **Context**: 起動時からフィット幅を有効にする方法。
- **Sources Consulted**: `src/config.rs`、`src/main.rs`、`src/app/state.rs`。
- **Findings**:
  - `AppState::new` が `Config` を受け取り、print モードも同じ `AppState::view` を使う。
  - `--print` は `AppState::set_size_print` の後で `state.view()` を呼ぶ。
- **Implications**: `Cli` に `--fit-width` を追加し、`AppState::new` で `fit_width` の初期値にすれば、TUI と print の両方に自動で適用される。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| A: `Sheet.layout` を書き換える | トグル時に列幅を差し替え、OFF で復元する | 既存の参照箇所を変更しなくてよい | 数式再計算・リサイズ・再読込・復元の整合が難しい | 不採用 |
| B: 表示時だけ列幅を派生 | `ColumnWidths` を `AppState` が生成し、描画・スクロール・詳細へ渡す | 元レイアウト不変、リサイズ・再読込に強い、テストしやすい | `LayoutEngine` / `ViewportPlanner` のシグネチャ変更が必要 | 採用 |
| C: UI 描画時だけ縮小 | `GridWidget` で幅を変換する | 変更範囲が小さい | 横スクロール・フォーカス・詳細と矛盾する | 不採用 |

## Design Decisions

### Decision: 表示用列幅 `ColumnWidths` を導入する
- **Context**: フィット幅を描画・スクロール・詳細で一致させる必要がある。
- **Alternatives Considered**:
  1. `Sheet.layout` を直接変更する。
  2. 各コンポーネントで個別にフィット計算する。
- **Selected Approach**: `ColumnWidths` を `domain/layout.rs` に追加し、`AppState` が `fit_width` と `viewport.width` から生成して `LayoutEngine` / `ViewportPlanner` に渡す。
- **Rationale**: 一つの表示幅テーブルを全コンポーネントで共有でき、元の `Sheet.layout` を不変に保てる。
- **Trade-offs**: 公開シグネチャが増えるが、フィット幅の有無を呼び出し側で明示できる。
- **Follow-up**: 既存テストの呼び出しを `ColumnWidths::from_sheet` で更新する。

### Decision: 最低 1 桁 + 比例配分
- **Context**: 全列を一望しつつ、元の列幅の大小を残す。
- **Alternatives Considered**:
  1. 均等割り。
  2. 比率配分（最低 1 桁）。
  3. 内容幅を無視した固定幅。
- **Selected Approach**: 各列に 1 桁を確保し、残りを元の列幅の比率で配分する。元の幅を超えず、端数は最大剰余順、同点は列インデックス順で配分する。
- **Rationale**: 長い列を相対的に広く保ちつつ、全列を表示できる。
- **Trade-offs**: 元の幅が 1 桁の列は拡大されない。1 桁でも収まらない場合は横スクロールが残る。
- **Follow-up**: 境界テストで合計幅と上限を固定する。

### Decision: 行高は再計算しない
- **Context**: 列幅の縮小に伴う折り返し行数の変化。
- **Alternatives Considered**:
  1. 表示時に全セルを再走査して行高を再計算する。
  2. 行高は元のままにし、切り詰め表示を許容する。
- **Selected Approach**: 行高は元のまま。`wrap_text` セルは既存の `…` 表示で切り詰める。
- **Rationale**: リサイズ時の全セル走査を避け、lazy 読み込みの利点を維持する。
- **Trade-offs**: フィット幅では折り返し内容の一部が見えなくなることがある。
- **Follow-up**: README の制約に明記し、詳細表示で全文を確認できることを保証する。

### Decision: `w` キーと `--fit-width`
- **Context**: 操作と起動時の適用。
- **Alternatives Considered**:
  1. `w` キーのみ。
  2. CLI のみ。
  3. 両方。
- **Selected Approach**: Normal / Visual モードの `w` でトグルし、`--fit-width` で起動時から有効にする。print モードにも適用する。
- **Rationale**: 対話的な閲覧とスクリプト・スナップショットの両方で再現できる。
- **Trade-offs**: キーバインドが 1 つ増える。
- **Follow-up**: ヘルプ・README・CLI テストを同期する。

## Risks & Mitigations
- **横スクロールと描画の幅不一致** — `ColumnWidths` を `LayoutEngine` と `ViewportPlanner` の両方に渡し、統合テストでフォーカス可視性を確認する。
- **極端に多い列でのメモリ・計算量** — 列数に比例する `Vec<u16>` と O(列数) の計算に限定し、セル数には依存させない。1 桁でも収まらない場合は `fit:limited` とする。
- **wrap_text の切り詰め増加** — 既存の `…` 表示と詳細表示を使い、README の制約に明記する。
- **既存テストのシグネチャ変更漏れ** — コンパイルエラーで検出できるよう、ヘルパーを一箇所に集約する。
- **reload 後の状態消失** — `AppState.fit_width` を `replace_workbook` で保持し、統合テストで確認する。

## References
- `src/domain/layout.rs` — `LayoutSnapshot`、`LayoutEngine`、`measure_cell`、`truncated_at`。
- `src/domain/viewport.rs` — `ViewportPlanner::ensure_visible`。
- `src/app/state.rs` — `AppState::view`、`ensure_focus_visible`、`detail_view`。
- `src/config.rs` — CLI 定義。
- `src/ui/grid.rs` — `RenderPlan` の描画。
- `.kiro/specs/excel-layout/design.md` — Excel 明示幅と content レイアウトの契約。
- `.kiro/specs/view-consistency/design.md` — 切り詰め・折り返し・freeze の整合。
