# Research & Design Decisions

## Summary

- **Feature**: `spreadsheet-editing`
- **Discovery Scope**: Extension（既存の読み取り専用 xlsx TUI に編集・保存を追加）
- **Key Findings**:
  - `calamine` は読み取り専用で、`Sheet` にセル書き込み API がない。
  - eager と lazy でセル格納が異なるため、共通の編集オーバーレイが必要。
  - `FormulaRunner` は保存済みパスを読み直すため、一時 xlsx を介せば未保存編集を再計算できる。
  - `zip` と `quick-xml` は既に依存にあり、シート XML の差分パッチに再利用できる。
  - `ReloadHandle` は自分の保存も変更として検知するため、保存後の基準スタンプ更新が必要。

## Research Log

### セル保存モデル

- **Context**: eager / lazy の両方で編集を扱う方法を決めるため。
- **Sources Consulted**: `src/domain/sheet.rs`, `src/domain/row_store.rs`, `src/io/xlsx.rs`
- **Findings**:
  - eager は `HashMap<CellAddr, Cell>`、lazy は `LazySheet` と `RowWindowCache`。
  - lazy のソースは spill ファイルで、直接書き換える API はない。
  - 表示・コピー・検索・フィルタは `SheetData` を直接読む箇所がある。
- **Implications**:
  - `Sheet.edits` オーバーレイを追加し、`cell` / `read_cell` / `all_cells` と直接走査箇所をオーバーレイ対応にする。
  - 保存は `edits` だけを対象にし、lazy の全行展開を避ける。

### 数式再計算

- **Context**: 未保存編集を数式結果へ反映する方法。
- **Sources Consulted**: `src/io/formula.rs`, `src/app/formula_runner.rs`, formualizer 0.14 source
- **Findings**:
  - 既存 `spawn_recalculation` はパスを開いて全数式を評価する。
  - formualizer には `set_value` / `set_formula` / `evaluate_cells_with_delta` があるが、既存ジョブは毎回ファイルから初期化する。
- **Implications**:
  - 初版は、編集内容を一時 xlsx にパッチして既存ジョブを再利用する。実装が既存表示と一致し、キャンセル・世代管理もそのまま使える。
  - 常駐エンジン化は将来の最適化とする。

### xlsx 差分保存

- **Context**: 既存の書式・フィルタ・グラフを壊さず保存する方法。
- **Sources Consulted**: `src/io/sheet_meta.rs`, `zip` 9.5 API, OOXML SpreadsheetML
- **Findings**:
  - zip の全エントリをコピーし、シート XML だけ差し替えれば未知のパートを保持できる。
  - テキストは `sharedStrings` を更新せず `inlineStr` で書ける。
  - 数式キャッシュは `<v>`、再計算は `calcPr fullCalcOnLoad` で促せる。
  - `calcChain` は削除しても Excel が再生成する。
- **Implications**:
  - ライタークレートで全体再生成せず、シート XML の `<sheetData>` をパッチする。
  - 既存セルの `s` 属性を引き継ぎ、変更セルのみ再構築する。

### キー衝突

- **Context**: 既存の `d`（詳細）、`r`（再読み込み）と Vim の編集キーの衝突。
- **Findings**:
  - 既存 `d` は詳細、`r` は再読み込み、`y` はコピー、`f` はフィルタ。
  - Vim では `d` は削除、`r` は置換、`K` は詳細に近い。
- **Implications**:
  - 詳細を `K` / `Enter`、削除を `d` / `x`、置換を `r{char}`、再読み込みを `:e` / `:e!` に変更する。
  - `f` はフィルタを維持し、文字検索は将来の課題とする。
  - `q` は TUI の慣習を優先して終了のままにする。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| 既存モデルを直接変更 | eager map / lazy cache を書き換え | 実装が単純 | lazy の spill とキャッシュ整合が壊れる | 不採用 |
| 編集オーバーレイ | `Sheet.edits` を優先表示 | lazy も安全、undo が簡単 | 直接走査箇所の更新が必要 | 採用 |
| ライタークレート | umya などで全体保存 | 実装量が少ない | 未知パート・スタイル消失のリスク | 不採用 |

## Design Decisions

### Decision: 編集オーバーレイ + トランザクション

- **Context**: ベースデータを不変に保ち、undo / redo と save を一貫させる。
- **Selected Approach**: `Sheet.edits` に最新値、`EditLog` に before/after を保持する。
- **Rationale**: eager / lazy の差を吸収でき、保存対象も明確になる。
- **Trade-offs**: セル読み取り・走査の一部をオーバーレイ対応に変更する必要がある。
- **Follow-up**: `all_cells`、コピー、検索、フィルタマスクの走査を共通化する。

### Decision: 一時 xlsx による再計算

- **Context**: 未保存編集を含む数式結果を既存エンジンで得る。
- **Selected Approach**: 編集内容を一時ファイルにパッチし、`FormulaRunner` を任意パスから再起動する。
- **Rationale**: 既存の計算経路と一致し、Excel 互換性の検証が容易。
- **Trade-offs**: 編集ごとに zip コピーが発生する。デバウンス/キャンセルで緩和する。
- **Follow-up**: 常駐 formualizer エンジンへの置き換えを将来検討する。

### Decision: `inlineStr` と `calcChain` 削除

- **Context**: sharedStrings と calcChain の整合を保つ。
- **Selected Approach**: 編集テキストは `inlineStr`、`calcChain.xml` は削除し `fullCalcOnLoad` を設定する。
- **Rationale**: 既存の共有文字列テーブルを壊さず、Excel 側で安全に再計算できる。
- **Trade-offs**: 編集セルのリッチテキストはプレーン文字列になる。
- **Follow-up**: リッチテキスト保持は将来課題。

## Risks & Mitigations

- xlsx の未知パートを壊す — 変更したシート XML 以外をバイト単位でコピーし、保存後に zip と XML を検証する。
- 保存と watch の競合 — 保存後の fingerprint を `ReloadHandle` に渡して基準を更新する。dirty 中の外部変更は自動反映しない。
- 数式の再計算遅延 — 世代照合で古い結果を捨て、編集確定ごとにキャンセルして再起動する。
- 入力値の型誤解 — `=` / `'` / TRUE/FALSE / 数値の順で明示的に解釈し、単体テストで固定する。
- lazy シートの編集 — オーバーレイのみを更新し、全行を eager 化しない。

## References

- OOXML SpreadsheetML: worksheet `<sheetData>`, `<c>`, `<f>`, `<v>`, `<is>`, `calcPr`
- 既存実装: `src/io/sheet_meta.rs`, `src/io/xlsx.rs`, `src/domain/sheet.rs`, `src/io/formula.rs`
- 既存仕様: `.kiro/specs/spreadsheet-tui/design.md`, `.kiro/specs/file-reload/design.md`
