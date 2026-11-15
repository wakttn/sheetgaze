# Research & Design Decisions

## Summary
- **Feature**: `excel-layout`
- **Discovery Scope**: Extension（既存レイアウト計算・シートメタデータ読み取り・数式再計算への統合）
- **Key Findings**:
  - 列幅・行高は `LayoutStatsBuilder` がセル内容から算出し、`Sheet.layout` に固定される。Excel の `<cols>` / `<row>` は現在まったく読まれていない。
  - `SheetMetadataParser` は eager / lazy の両方から呼ばれ、すでにシート XML を全走査している。列幅・行高の取得は同じパスに相乗りできる。
  - `LayoutStatsBuilder::finish` は折り返し行高を列幅確定後に計算するため、Excel 列幅の反映は `finish` の内部順序に組み込む必要がある。
  - `LayoutSnapshot::grow_for_cell` は数式再計算ですべての列・行を拡大する。Excel 明示値を維持するには「固定」の識別が必要。
  - ECMA-376 の `width` は 5 ピクセルのパディングを含む文字数単位、`ht` はポイント単位。端末への変換は近似になる。

## Research Log

### 既存レイアウト計算の構造
- **Context**: Excel 設定をどこで適用すれば折り返し・結合・切り詰めと矛盾しないか確認した。
- **Sources Consulted**: `src/domain/layout.rs`, `src/domain/sheet.rs`, `src/domain/viewport.rs`, `src/domain/workbook.rs`, `src/io/xlsx.rs`, `src/io/csv.rs`
- **Findings**:
  - `LayoutStatsBuilder::observe` がセルごとに最大表示幅と改行数を集計し、`finish` が列幅・行高を確定する。
  - 結合セルの幅要求は `observe_merge` → `finish` で列へ分配され、折り返し行数は分配後の合計幅で計算される。
  - `wrap_demands` の行高は列幅確定後に計算される。
  - `LayoutSnapshot { column_widths: Vec<u16>, row_heights: Vec<u8> }` が描画・スクロール・freeze の唯一の入力になる。
  - `LayoutEngine::measure_cell` と `ViewportPlanner` は `Sheet.layout` を参照するため、読み込み時に確定すれば描画側の変更は不要。
- **Implications**: Excel 列幅は `finish` 内で自動列幅を上書きし、その後で結合幅・wrap 行高を計算する順序が必須。読み込み後に上書きする方式では折り返し行高がずれる。

### シートメタデータ読み取りの拡張点
- **Context**: 列幅・行高を読む既存の適切な境界を探した。
- **Sources Consulted**: `src/io/sheet_meta.rs`, `src/io/xlsx.rs`
- **Findings**:
  - `SheetMetadataParser::parse` は `xl/workbook.xml` と各シート XML を読み、filter / freeze / 条件付き書式を `SheetMetadata` に格納する。
  - eager / lazy の両経路が同じ `SheetMetadataParser::parse` を呼ぶ。
  - `parse_sheet_xml` は Start / Empty イベントを走査しており、`col` / `row` / `sheetFormatPr` の追加は局所的。
  - テーブルパートも同じ関数で解析されるが、`<table>` には `col` / `row` が現れないため衝突しない。
- **Implications**: `SheetMetadata` にレイアウト情報を追加し、両経路で同じ変換を適用するのが最小変更。追加の ZIP 読み取りや全走査は不要。

### Excel 列幅・行高の単位
- **Context**: 文字数・ポイントを端末桁・行数へどう変換するか確認した。
- **Sources Consulted**: ECMA-376 `col` / `sheetFormatPr` リファレンス、Microsoft Open XML SDK の `SheetFormatProperties` 解説
- **Findings**:
  - `<col width>` は「通常スタイルのフォントにおける最大数字幅を 1 文字とした文字数」で、左右 2 ピクセルずつとグリッド線 1 ピクセルの計 5 ピクセルのパディングを含む。
  - `width = Truncate([{文字数} * {最大数字幅} + 5] / {最大数字幅} * 256) / 256`。Calibri 11pt（7px）では 8 文字 → 8.7109375。
  - `rust_xlsxwriter` の `set_column_width(0, 24)` は 24.7109375 を書き出す。端末では `floor(width)` により 24 桁が得られる。
  - `<row ht>` はポイント。`<sheetFormatPr defaultRowHeight>` は通常 15 で、`customHeight` は既定行高が手動設定されたことを示す。
  - Excel の行高はフォントの行送りに依存するため、端末行数への厳密な対応は存在しない。
- **Implications**: 列幅は小数切り捨て、行高は `ht / 15pt` の四捨五入で近似する。フォント差は仕様上の受容範囲とし、テストで変換規則を固定する。

### Excel 側の自動調整との関係
- **Context**: Excel は列幅・行高を常に保存するわけではないため、未指定時の扱いを決める必要があった。
- **Sources Consulted**: ECMA-376 `col` / `row` / `sheetFormatPr`、既存 fixture の XML
- **Findings**:
  - 列幅は `customWidth`、行高は `customHeight` が真の場合に明示設定と判断できる。
  - “best fit” 列も `width` を持つが、Excel が自動計算した結果であり、ユーザーの明示設定とは限らない。
  - 既存 fixture の `basic_types.xlsx` / `formulas.xlsx` / `styles.xlsx` / `borders.xlsx` / `merged_tail.xlsx` は `set_column_width` を持つ。
  - 結合セルに対する Excel の自動列幅拡張は存在しない。
- **Implications**: 明示値は確定値、未指定は従来の内容ベース自動計算とするハイブリッド方式が、Excel 準拠と TUI の可読性を両立する。結合幅の要求は未指定列にのみ分配する。

### 数式再計算と grow-only 更新
- **Context**: Excel 明示値を再計算後に拡大してよいか確認した。
- **Sources Consulted**: `src/domain/workbook.rs`, `src/domain/layout.rs`, `view-consistency` の設計
- **Findings**:
  - `WorkbookModel::apply_formula_results` が `LayoutSnapshot::grow_for_cell` を呼び、表示値に応じて列幅・行高を拡大する。
  - 現状はすべての列・行が拡大対象で、明示値と自動値の区別がない。
  - `grow_for_cell` は縮小しないため、明示値が自動計算より広い場合は問題ないが、狭い場合は再計算で広がってしまう。
- **Implications**: `LayoutSnapshot` に Excel 由来の列・行の集合を持たせ、`grow_for_cell` はその集合をスキップする。

### 既存仕様との関係
- **Context**: 既存 spec の決定事項と競合しないか確認した。
- **Sources Consulted**: `.kiro/specs/spreadsheet-tui/requirements.md`, `.kiro/specs/spreadsheet-tui/research.md`, `.kiro/specs/view-consistency/requirements.md`, `.kiro/steering/tech.md`
- **Findings**:
  - `spreadsheet-tui` は「Excel の列幅・行高をそのまま使わず内容から自動計算」を明示的に選択している。
  - `view-consistency` は折り返し行高・grow-only・切り詰めの一貫性を扱う。
  - steering の Key Technical Decisions にも「独自レイアウト」の記述がある。
- **Implications**: 本仕様は既存決定を置き換える delta として扱い、steering の該当記述を更新する。`view-consistency` の grow-only / 切り詰め規則は維持する。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| A: 読み込み後に `LayoutSnapshot` を上書き | `finish` の結果へ Excel 値を後から適用 | 変更が小さい | wrap 行高が旧列幅で計算され、結合幅も不整合 | 不採用 |
| B: `LayoutStatsBuilder::finish` に統合 | 列幅確定 → 結合幅 → wrap 行高 → 行高確定の順で Excel 値を反映 | 折り返し・結合・切り詰めが一貫 | `finish` の責務が増える | 採用 |
| C: Excel 値のみでレイアウトを全面置換 | 未指定も Excel 既定値に固定 | 見た目は最も Excel 的 | 未設定シートで列幅が狭く読みにくい | 不採用 |

## Design Decisions

### Decision: 明示値優先・未指定は自動計算のハイブリッド
- **Context**: Excel の明示設定を尊重しつつ、設定のないシートの可読性を維持する必要がある。
- **Alternatives Considered**:
  1. すべて Excel 値に置換する。
  2. すべて従来の自動計算を維持する。
  3. 明示値のみ Excel 値を採用し、未指定は自動計算する。
- **Selected Approach**: 3。`customWidth` 列と `customHeight` 行は Excel 値を確定値として採用し、未指定の行・列は内容ベースの自動計算を使う。既定列幅・既定行高は未指定行・列の下限としてのみ使う。
- **Rationale**: Excel で意図的に狭くした列・低くした行を再現でき、設定のないシートは従来どおり読みやすい。
- **Trade-offs**: Excel 側で自動調整された幅（best fit）は明示扱いにならない場合がある。TUI の自動幅と Excel の自動幅は一致しない。
- **Follow-up**: 既存 fixture の `set_column_width` が期待値へ与える影響をテストで確認する。

### Decision: 列幅は切り捨て、行高は基準 15pt で割って四捨五入
- **Context**: Excel の文字数・ポイントを端末セルへ変換する必要がある。
- **Alternatives Considered**:
  1. 列幅・行高とも四捨五入する。
  2. 列幅は 5px パディングを減算して厳密計算し、行高は切り上げる。
  3. 列幅は切り捨て、行高は `ht / 15pt` の四捨五入。
- **Selected Approach**: 3。列幅は `floor(width)`（24.7109375 → 24）、行高は `round(ht / 15.0)`（15pt → 1 行、30pt → 2 行）。
- **Rationale**: 列幅は Excel の文字数指定に対する実表示桁と一致しやすい。行高は端末行数の近似として丸めが自然で、極端な切り上げによる空白行を避けられる。
- **Trade-offs**: フォントの最大数字幅が 7px でないファイルでは 1 桁程度ずれ得る。行高もフォント依存で厳密ではない。
- **Follow-up**: 変換関数を純粋関数として単体テストし、境界値（1 未満、上限超過、欠落）を固定する。

### Decision: `LayoutSnapshot` に Excel 固定列・行の識別を持たせる
- **Context**: 数式再計算後の grow-only 更新で Excel 明示値を拡大しない必要がある。
- **Alternatives Considered**:
  1. `grow_for_cell` を呼ばない。
  2. `LayoutLimits` にフラグを追加する。
  3. `LayoutSnapshot` に固定列・行の集合を持たせる。
- **Selected Approach**: 3。`finish` が Excel 明示値に由来する列・行を記録し、`grow_for_cell` はその列・行をスキップする。
- **Rationale**: レイアウトの状態と更新規則を同じ型に閉じ込められ、呼び出し側の分岐が増えない。
- **Trade-offs**: `LayoutSnapshot` の状態が増え、シリアライズやテスト fixture の更新が必要になる。
- **Follow-up**: 再計算で自動列だけが伸び、固定列は変わらないことをテストする。

### Decision: メタデータ読み取りに統合し、`LayoutMode` で切替える
- **Context**: eager / lazy の両経路で同じ Excel レイアウトを適用し、従来動作への退避手段も必要である。
- **Alternatives Considered**:
  1. `xlsx.rs` で個別に XML を読む。
  2. `SheetMetadataParser` を拡張し、`LoadRequest` にモードを追加する。
  3. 環境変数で切替える。
- **Selected Approach**: 2。`SheetMetadata` に `ExcelLayout` を追加し、`--layout excel|content`（既定 `excel`）を `LoadRequest` 経由で渡す。csv は常に `content`。
- **Rationale**: 既存の 1 パス解析を再利用でき、CLI から明示的に挙動を選べる。
- **Trade-offs**: `LoadRequest` / `Config` / reload 経路に新しい設定が増える。
- **Follow-up**: watch / reload でもモードが維持されることをテストする。

## Risks & Mitigations
- **フォント差による列幅のずれ** — 変換を近似として明記し、floor 規則をテストで固定する。将来 `max_digit_width` を考慮する拡張余地を残す。
- **明示幅が狭いシートで内容が読みにくくなる** — 既存の overflow / 切り詰め / 詳細表示を維持し、利用者が `--layout content` で回避できるようにする。
- **既存 fixture の期待値変更** — `basic_types` などの列幅期待値を更新し、レイアウト差分を明示的にテストする。
- **結合セルと明示幅の競合** — 明示列には幅要求を分配せず、未指定列のみに分配する規則を単体テストする。
- **既定行高の丸めで余白行が出る** — `defaultRowHeight` は下限としてのみ使い、上限・下限クランプと併用する。
- **hidden 行・列の混入** — 本仕様では `hidden` を解釈しない。読み取ってもレイアウトに影響させないことを明記する。

## References
- [ECMA-376 `col` (Column Width & Formatting)](https://c-rex.net/samples/ooxml/e1/Part4/OOXML_P4_DOCX_col_topic_ID0ELFQ4.html) — `width` の文字数単位と 5px パディングの定義。
- [ECMA-376 `sheetFormatPr` (Sheet Format Properties)](https://c-rex.net/samples/ooxml/e1/part4/OOXML_P4_DOCX_sheetFormatPr_topic_ID0EVAG5.html) — `defaultColWidth` / `baseColWidth` / `defaultRowHeight` / `customHeight`。
- [Microsoft Open XML SDK: SheetFormatProperties](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.spreadsheet.sheetformatproperties?view=openxml-3.0.1) — 既定値と `customHeight` の意味。
- [Microsoft Excel: 列幅と行高の変更](https://support.microsoft.com/en-us/excel/change-the-column-width-and-row-height) — ユーザー操作上の位置づけ。
