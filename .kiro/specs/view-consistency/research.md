# Research & Design Decisions

## Summary

- **Feature**: `view-consistency`
- **Discovery Scope**: Extension（既存 `spreadsheet-tui` の表示・可視性改善）
- **Key Findings**:
  - 横方向のビューポート状態 `Viewport.left_col` は初期化とシート切り替え以外で更新されず、フォーカスが右へ移動しても表示が追従しない。
  - 通常モードのフォーカス表示は選択範囲の有無に依存しており、`ScreenView.focus` は UI で使用されていない。
  - 固定ペインは `disabled` フラグが描画計画に反映されず、無効化後も固定行・列として扱われる。
  - 遅延シートのフィルタ再評価は全行可視にフォールバックし、列マスクのキャッシュも存在しない。
  - `LayoutStatsBuilder` は `wrap_text` を無視し、遅延 xlsx では結合セルの幅要求も記録しない。
  - 数式再計算はセル値のみを更新し、レイアウト・切り詰め・フィルタマスクを更新しない。

## Research Log

### ビューポートとフォーカス追従

- **Context**: 要件 1 の横方向スクロールとフォーカス維持の実現方法を確認するため、既存の描画経路を調査した。
- **Sources Consulted**: `src/app/state.rs`, `src/domain/layout.rs`, `src/ui/grid.rs`, `src/ui/screen.rs`
- **Findings**:
  - `Viewport.left_col` は `AppState::new` と `switch_sheet` で 0 に設定されるのみで、フォーカス移動後に更新する経路がない。
  - `LayoutEngine::plan` は `viewport.left_col` をスクロール開始列として使うが、`RenderPlan.scroll_left_col` は UI で利用されていない。
  - 縦方向の `ensure_focus_visible` は可視行と固定行を考慮するが、横方向の同等処理は存在しない。
  - `Screen::render` はステータス 1 行、グリッド、タブ 1 行を縦に分割し、グリッド内でヘッダー 1 行を描画する。
- **Implications**: ビューポート調整を `AppState` の ad hoc 処理から純粋関数へ分離し、行・列の両軸を同じ規則で扱う必要がある。横方向の幅計算には列幅と固定列幅が必要なため、`LayoutSnapshot` と `EffectiveFreeze` を入力にする。

### フォーカス表示

- **Context**: 通常モードでフォーカスセルが強調されない原因を確認した。
- **Sources Consulted**: `src/domain/layout.rs`, `src/ui/grid.rs`, `src/ui/theme.rs`, `src/ui/view.rs`
- **Findings**:
  - `RenderCell.cursor` は `selection` がある場合の `selection.cursor == addr` でのみ true になる。
  - 通常モードでは `selection` が `None` のため、フォーカスセルを含むすべてのセルで `cursor` が false になる。
  - `ScreenView.focus` はステータス表示用に保持されているが、グリッド描画では参照されていない。
- **Implications**: `LayoutEngine::plan` にフォーカス座標を渡し、選択状態とは独立した `cursor` を生成する。選択範囲の `selected` とカーソルの `cursor` を別フラグとして維持する。

### 固定ペインの有効・無効

- **Context**: `freeze_merge_conflict.xlsx` が警告付きで無効化される一方、描画では固定される問題を確認した。
- **Sources Consulted**: `src/domain/freeze.rs`, `src/domain/layout.rs`, `src/app/state.rs`, `src/io/xlsx.rs`, `tests/fixtures_smoke.rs`
- **Findings**:
  - `xlsx` ローダーは検証失敗時に `FreezePane.disabled = true` を設定するが、`frozen_rows` / `frozen_cols` は残す。
  - `LayoutEngine::plan` と `AppState::view` は `freeze.frozen_rows` / `frozen_cols` を直接参照し、`disabled` を考慮しない。
  - `FreezePane::is_active()` はステータス表示と一部の描画条件だけで使われている。
  - 固定領域の表示上限（スクロール領域を残す制限）は描画計画内で暗黙に行われるが、状態表示に反映されない。
- **Implications**: `disabled` を単一の真実源とする `EffectiveFreeze` を導入し、描画・行読み込み・ナビゲーション・ステータスのすべてで同じ値を使う。`split` / `frozenSplit` / 結合交差の無効化は既存の検証結果をそのまま利用する。

### 遅延シートのフィルタ可視性

- **Context**: 遅延シートでフィルタを切り替えると全行表示に戻る問題を確認した。
- **Sources Consulted**: `src/domain/filter.rs`, `src/domain/sheet.rs`, `src/app/state.rs`, `src/io/xlsx.rs`, `src/io/csv.rs`
- **Findings**:
  - 読み込み時の可視マスクは `FilterEngine::build_visibility` で計算されるが、列ごとのマスクは保存されず、最終的な `VisibilityMap` だけが `Sheet` に残る。
  - `AppState::rebuild_visibility` は `SheetData::Lazy` の場合に `VisibilityMap::all` を返す。
  - `FilterEngine::build_visibility` は行ごとのセル参照クロージャを必要とし、遅延シートの未読み込み行には適用しにくい。
  - 数式再計算後も同じ経路で可視性を再構築するため、遅延シートではフィルタが失われる。
- **Implications**: フィルタ評価を「列マスクの構築」と「有効列マスクの合成」に分離する。列マスクは読み込み時に eager / lazy の両方で構築して `Sheet` に保持し、切替時はビット演算相当の合成だけで可視性を更新する。数式再計算後はマスクを dirty にし、必要なシートだけ再構築する。

### テキストレイアウトと結合セル

- **Context**: `wrap_text` の行高不足と、遅延 xlsx の結合セル幅が eager と異なる問題を確認した。
- **Sources Consulted**: `src/domain/layout.rs`, `src/io/xlsx.rs`, `tests/generate_fixtures.rs`, `tests/fixtures_smoke.rs`
- **Findings**:
  - `LayoutStatsBuilder::observe` の `_wrap_text` 引数は未使用で、行高は明示的な改行数だけで決まる。
  - 描画時の `wrap_display` は `wrap_text` を折り返すが、行高が 1 の場合に `max_lines = 1` で切り捨てる。
  - eager xlsx ローダーは結合アンカーの `observe_merge` を呼ぶが、lazy xlsx ローダーは結合範囲を後から構築するため呼ばない。
  - `LayoutStatsBuilder::finish` は `_merges` を受け取るが利用していない。
- **Implications**: 折り返し候補をレイアウト統計に保持し、列幅確定後に折り返し行数を計算する。結合セルの要求は eager / lazy の両方で同じ `observe_merge` 経路に通す。描画と詳細で同じ折り返し・切り詰め計算を共有する。

### 数式再計算後のレイアウトと切り詰め

- **Context**: 再計算結果がキャッシュ値より長い場合に列幅・行高が更新されない問題を確認した。
- **Sources Consulted**: `src/domain/workbook.rs`, `src/domain/layout.rs`, `src/app/state.rs`
- **Findings**:
  - `WorkbookModel::apply_formula_results` は `FormulaOverlay` と eager セルの表示文字列を更新するが、`LayoutSnapshot` は変更しない。
  - `LayoutSnapshot.truncated_cells` は `LayoutStatsBuilder::finish` で常に空の `HashSet` として生成され、詳細表示の `truncated` 判定に使われている。
  - `LayoutEngine::plan` はセルごとに `truncated` を計算するが、その結果を詳細表示と共有していない。
- **Implications**: レイアウトは grow-only の更新 API を用意し、再計算結果の表示値で列幅・行高を拡大する。切り詰めは静的な集合ではなく、描画と同じ計測関数からオンデマンドに導出する。

### 検索と可視性

- **Context**: 検索確定時に最初の一致が非表示行だと、その行へフォーカスする問題を確認した。
- **Sources Consulted**: `src/domain/search.rs`, `src/app/state.rs`
- **Findings**:
  - `SearchIndex::new` は `current = Some(0)` を設定し、可視性を考慮しない。
  - `hidden_matches` は `next_visible` / `previous_visible` 内で初めて更新される。
  - `AppState::rebuild_visibility` は `hidden_matches` を更新するが、現在位置が非表示になった場合の再選択を行わない。
- **Implications**: 検索結果構築時と可視性変更時に、可視一致の先頭へ現在位置を同期する。可視一致がない場合はフォーカスを変更しない。

### テストと fixture

- **Context**: 既存 fixture とテストで、今回の不整合を検証できるかを確認した。
- **Sources Consulted**: `tests/fixtures/xlsx/README.md`, `tests/fixtures_smoke.rs`, `tests/integration.rs`, `tests/tui.rs`
- **Findings**:
  - `overflow.xlsx` に 100 文字の `wrap_text` セルがあり、行高・折り返しの検証に利用できる。
  - `freeze_merge_conflict.xlsx` は固定無効化の警告を検証しているが、描画計画の固定行が空であることは検証していない。
  - `large_lazy.xlsx` は遅延行アクセスの検証に利用できるが、フィルタ付き遅延シートの切替テストはない。
  - `filters_values.xlsx` / `filters_custom.xlsx` は保存済みフィルタの初期可視性を検証している。
  - TUI テストは文字列包含の簡易検証で、通常モードのカーソルや横スクロールのスナップショットはない。
- **Implications**: 既存 fixture を再利用しつつ、`tests/view_consistency.rs` で planner・mask・layout の単体／統合テストを追加する。TUI テストには通常モードのカーソル、横スクロール、無効化された固定ペインを追加する。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| AppState 内で直接ビューポート計算 | 既存の `ensure_focus_visible` を拡張 | 変更範囲が小さい | 横方向・固定列・結合セルの規則が状態機械に混在し、テストしにくい | 既存の縦方向処理も重複する |
| 純粋な `ViewportPlanner` | 入力から新しい表示開始行・列を返す | 状態に依存せず単体テスト可能、UI と独立 | 固定列幅・結合範囲の入力設計が必要 | 既存のレイヤード方針に一致 |
| フィルタ切替のたびに全行再走査 | 既存 `build_visibility` を lazy に適用 | 実装が単純 | 切替のたびに全行 I/O、キャッシュを汚染、要件 6.1 に反する | 遅延シートで特に不利 |
| 列マスクのキャッシュと合成 | 読み込み時に列マスクを構築し、切替は合成のみ | 遅延シートでも高速、既存設計の「マスク合成」に一致 | メモリ使用量が行数 × フィルタ列数に比例 | Top10 は構築時に確定する |
| 固定ペインの生フィールド参照 | `frozen_rows` / `frozen_cols` を直接使う | 変更が少ない | disabled の扱いが分散し、今回の不整合を再発する | 単一の真実源が必要 |
| `EffectiveFreeze` の導入 | 有効・無効・表示上限を一つの値に集約 | 描画・行読み込み・状態表示が一致 | 既存のフィールド参照箇所の置換が必要 | 要件 3 の中心 |
| 数式結果で全レイアウト再計算 | eager の全セルを再計算 | 正確 | 遅延シートでは不可能、再計算コストが高い | grow-only の方が既存設計に一致 |
| grow-only レイアウト更新 | 変更セルの幅・行高だけを拡大 | 遅延シートでも適用可能、安定 | 幅が縮まない、折り返し行高の再計算に注意 | 既存設計の「grow-only」方針を採用 |

## Design Decisions

### Decision: ビューポート調整を純粋な `ViewportPlanner` に集約する

- **Context**: 横方向の追従がなく、縦方向の処理も `AppState` に閉じている。
- **Alternatives Considered**:
  1. `AppState::ensure_focus_visible` を拡張する — 変更範囲は小さいが、UI 状態と計算規則が混在する。
  2. `ViewportPlanner` を新設する — 入力と出力を明示でき、単体テストが容易。
- **Selected Approach**: `Sheet`、現在の `Viewport`、フォーカス、`EffectiveFreeze` を受け取り、`ViewportPlan { top_visible_row, left_col }` を返す純粋関数を `domain/viewport.rs` に置く。`AppState` は移動・リサイズ・シート切り替え・検索移動・フィルタ変更後にこの関数を呼ぶ。
- **Rationale**: 既存のレイヤード構成と「レイアウトは純粋関数としてテストする」という steering に一致する。横方向と縦方向の規則を一箇所にまとめられる。
- **Trade-offs**: `LayoutSnapshot` の列幅と `VisibilityMap` の可視行に依存するため、入力型が増える。表示開始列の規則をテストで固定する必要がある。
- **Follow-up**: 結合範囲が表示幅より広い場合の「左端優先」規則をテストする。

### Decision: 固定ペインは `EffectiveFreeze` を唯一の描画・操作入力にする

- **Context**: `disabled` と生の固定行・列が混在し、無効化後も固定される。
- **Alternatives Considered**:
  1. ローダーで `frozen_rows` / `frozen_cols` を 0 にクリアする — 警告情報と元の値が失われる。
  2. `FreezePane::active_rows` / `active_cols` と `EffectiveFreeze` を導入する — 無効理由と表示上限を保持したまま扱える。
- **Selected Approach**: `FreezePane` は要求値と `disabled` を保持し、`active_rows` / `active_cols` は無効時に 0 を返す。`LayoutEngine::effective_freeze` が表示領域に収まる行・列と `rows_limited` / `cols_limited` を計算し、`RenderPlan` と `AppState` が同じ値を使う。
- **Rationale**: 警告と実際の表示を一致させ、固定領域の制限も状態表示へ渡せる。
- **Trade-offs**: 固定行・列の参照箇所を `active_*` / `EffectiveFreeze` に置き換える必要がある。
- **Follow-up**: `freeze_merge_conflict.xlsx` の描画計画で固定行が空になることをテストする。

### Decision: フィルタは列マスクを読み込み時に構築し、切替は合成だけにする

- **Context**: 遅延シートの再評価が全行可視にフォールバックし、切替のたびに全行走査する設計になっている。
- **Alternatives Considered**:
  1. 切替時に `FilterEngine::build_visibility` を再実行する — 遅延シートで全行 I/O が必要。
  2. 列マスクを `Sheet` に保持し、`FilterState` の有効列だけを AND 合成する — 切替が O(行数 × 列数) のメモリ演算で済む。
- **Selected Approach**: `FilterMaskBuilder` が読み込み時に列ごとの一致マスクを構築し、`FilterMasks` として `Sheet` に保持する。`FilterEngine::combine` が有効列のマスクを合成して `VisibilityMap` を作る。Top10 はビルダーの `finish` 時に確定する。数式再計算後はマスクを dirty にし、可視性が必要になった時点で再構築する。
- **Rationale**: 要件 6.1 を満たし、元設計の「列マスクをビット演算で合成する」方針に一致する。eager / lazy で同じ合成経路を使える。
- **Trade-offs**: 行数 × フィルタ列数のマスクメモリが必要。Top10 は全列値の収集が必要。
- **Follow-up**: フィルタなし・未対応条件・`--no-filter` でマスクが全可視になることをテストする。

### Decision: 折り返し・結合・切り詰めは共有のセル計測関数に集約する

- **Context**: `LayoutStatsBuilder` と描画が別々に折り返し・切り詰めを計算し、`truncated_cells` は空のまま使われている。
- **Alternatives Considered**:
  1. `LayoutStatsBuilder` に折り返し行数を別実装で持たせる — 描画とのずれが再発する。
  2. `LayoutEngine::measure_cell` を共通関数とし、統計・描画・詳細が同じ折り返し・切り詰め規則を使う — 一貫性が高い。
- **Selected Approach**: 折り返し候補を `LayoutStatsBuilder` に保持し、列幅確定後に `finish` で行高を更新する。描画と詳細は `LayoutEngine::measure_cell` と同じ `CellMeasure` を使い、`truncated_at` を提供する。`LayoutSnapshot.truncated_cells` は廃止または未使用にし、詳細は `truncated_at` を使う。
- **Rationale**: 要件 5.1、5.2、5.5 の「グリッドと詳細の一致」を保証する。
- **Trade-offs**: 折り返し候補の保持と、列幅確定後の再計算が必要。
- **Follow-up**: `overflow.xlsx` の wrap セルで行高と詳細の切り詰めが一致することをテストする。

### Decision: 数式結果は grow-only でレイアウトを更新する

- **Context**: 再計算結果が長くなっても列幅・行高が変わらず、切り詰め判定も古い。
- **Alternatives Considered**:
  1. 数式結果ごとに全セルのレイアウトを再計算する — eager では可能だが、遅延シートでは全行読み込みが必要。
  2. 更新セルだけを grow-only で反映する — 遅延シートでも適用でき、既存設計と一致。
- **Selected Approach**: `LayoutSnapshot::grow_for_cell` を追加し、再計算結果の表示値で列幅・行高を必要な分だけ拡大する。`WorkbookModel::apply_formula_results` は `FormulaApplySummary` を返し、レイアウト変更とフィルタマスクの dirty 化を通知する。
- **Rationale**: 要件 5.4 を満たし、遅延シートのメモリ・I/O を増やさない。
- **Trade-offs**: 値が短くなっても幅・高さは縮まない。折り返し行高の再計算は grow-only の範囲に限定される。
- **Follow-up**: 再計算後に詳細の `truncated` が更新されることをテストする。

### Decision: 検索は可視一致へ同期してからフォーカスする

- **Context**: 検索確定時に非表示の一致へフォーカスし、`hidden_matches` も遅延して更新される。
- **Alternatives Considered**:
  1. `AppState` 側で可視一致を探す — 検索インデックスの状態と重複する。
  2. `SearchIndex::sync_visibility` を追加する — 検索の現在位置と件数を一箇所で管理できる。
- **Selected Approach**: `SearchIndex` に可視性同期メソッドを追加し、検索確定時・フィルタ変更時に呼ぶ。可視一致がなければ `current = None` とし、フォーカスを変更しない。
- **Rationale**: 要件 4.6、4.7 を満たし、既存の `next_visible` / `previous_visible` と整合する。
- **Trade-offs**: 可視一致の探索は一致件数に比例するが、検索結果の移動時と同じ範囲に留まる。
- **Follow-up**: 非表示一致だけの検索と、可視一致が後ろにある検索をテストする。

## Risks & Mitigations

- **遅延シートのフィルタマスク再構築コスト** — 数式再計算後に全行走査が必要になる。フィルタが無効なときは再構築を遅延し、可視性が必要なアクティブシートだけを対象にする。将来的なバックグラウンド化は別 spec とする。
- **列マスクのメモリ使用量** — 行数 × フィルタ列数の `Vec<bool>` を保持する。フィルタ列だけを対象にし、マスクを持たないシートでは空にする。
- **固定列と横スクロールの境界** — 固定列幅の合計が表示幅を超える場合、固定列を制限してスクロール列を残す。結合範囲が境界を跨ぐ場合は既存の無効化規則を優先する。
- **折り返しと全角文字** — `textwrap` 0.16 は `unicode-width` 機能が有効であり、表示幅で折り返せる。行高計算と描画で同じ関数を使う。
- **grow-only による余白** — 数式結果が短くなっても幅・高さは縮まない。表示の安定性を優先し、要件上は「縮小しない」と明記する。
- **既存テストのシグネチャ変更** — `LayoutEngine::plan` にフォーカス引数が増えるため、fixture テストを更新する。既存の CLI とファイル読み込み契約は変更しない。

## References

- `.kiro/steering/product.md` — 閲覧専用・応答性・端末制約の前提
- `.kiro/steering/tech.md` — レイヤード構成、純粋関数、テスト方針
- `.kiro/steering/structure.md` — `domain` / `io` / `ui` / `app` の依存方向
- `.kiro/specs/spreadsheet-tui/design.md` — 既存の `LayoutEngine`、`FilterEngine`、`FreezePane`、`CopySession` の契約
- `src/domain/layout.rs` — 現在のレイアウト統計と描画計画
- `src/domain/filter.rs` — 現在のフィルタ評価と可視マップ
- `src/domain/freeze.rs` — 固定ペインの検証
- `src/app/state.rs` — 状態遷移とフィルタ・検索・詳細の統合
- `src/io/xlsx.rs` — eager / lazy xlsx 読み込み
- `tests/fixtures/xlsx/README.md` — 既存 fixture の coverage
