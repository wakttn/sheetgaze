# Research & Design Decisions

## Summary

- **Feature**: `cross-sheet-search`
- **Discovery Scope**: Extension（既存検索のシート横断化）
- **Key Findings**:
  - `SearchMatch` はセル座標のみで、シート情報を持たない。
  - `SearchIndex` は単一の `VisibilityMap` を受け取るため、シートごとの可視性を扱えない。
  - `AppState::submit_search` はアクティブシートの `SheetData` を直接走査している。
  - グリッドの検索ハイライトは `SearchIndex::is_match(addr)` のみで、シートを区別しない。
  - 遅延シートは `scan_all` で全行を走査できる。

## Research Log

### 既存検索の構造

- **Context**: 横断検索の最小変更点を特定するため。
- **Sources Consulted**: `src/domain/search.rs`, `src/app/state.rs`, `src/domain/layout.rs`, `tests/view_consistency.rs`
- **Findings**:
  - `SearchIndex` の step は単一の可視性マップを参照する。
  - `submit_search` はアクティブシートだけを走査し、結果を `(row, col)` でソートする。
  - `search_match` はセル座標だけで判定される。
- **Implications**:
  - `SearchMatch` に sheet を追加し、`SearchIndex` は可視性のスライスを受け取る。
  - `active_sheet` を SearchIndex に持たせ、既存の描画経路を大きく変えずにハイライトを絞る。

### 遅延シートと編集オーバーレイ

- **Context**: 全シート検索で遅延行と未保存編集をどう扱うか。
- **Sources Consulted**: `src/domain/row_store.rs`, `src/io/xlsx.rs`, `src/domain/sheet.rs`
- **Findings**:
  - `LazySheet::scan_all` は読み込み済みでない行も走査対象にできる。
  - 編集オーバーレイは `spreadsheet-editing` spec で追加され、`Sheet::cell` 経由で参照できる。
- **Implications**:
  - 検索は `SheetData` の直接走査ではなく、シートのセル走査 API を使う。
  - 編集オーバーレイの行はベース行より優先する。

### ジャンプとビューポート

- **Context**: シート切替時にフォーカスと表示位置をどう更新するか。
- **Sources Consulted**: `src/app/state.rs`, `src/domain/viewport.rs`, `src/domain/layout.rs`
- **Findings**:
  - `switch_sheet` はフォーカスを (0,0) に戻し、ビューポートをリセットする。
  - `ensure_focus_visible` は可視行と固定ペインを考慮する。
- **Implications**:
  - 検索ジャンプ専用の `jump_to` を新設し、シート切替後にフォーカスを設定して行ロードとビューポート調整を行う。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| SearchIndex に sheet を追加 | 既存型を拡張 | 変更が局所的、テストしやすい | active_sheet の更新漏れ | 採用 |
| シートごとに SearchIndex | シート別インデックス | 状態が独立 | 横断 `n` の実装が複雑 | 不採用 |
| バックグラウンド検索 | ワーカーで走査 | UI 応答性 | スレッドとキャンセルが増える | 将来 |

## Design Decisions

### Decision: `SearchIndex.active_sheet` で描画を絞る

- **Context**: `LayoutEngine::plan` のシグネチャを変えずにシート別ハイライトを行う。
- **Alternatives Considered**:
  1. `plan` に sheet index を渡す — 呼び出し箇所とテストが多い。
  2. `SearchIndex` に active_sheet を持たせる — 変更が検索型に閉じる。
- **Selected Approach**: `SearchIndex.active_sheet` をシート切替・ジャンプ・view 生成時に更新し、`is_match` がシートを比較する。
- **Rationale**: 既存の描画経路を維持できる。
- **Trade-offs**: 検索状態と表示状態が少し結合する。
- **Follow-up**: `view()` で必ず active_sheet を同期する。

### Decision: 全シート検索を既定にする

- **Context**: ユーザーがシート横断検索を明示的に求めている。
- **Selected Approach**: `/` は全シートを検索し、結果はシート順で並べる。現在シート限定の切り替えは今回のスコープ外。
- **Rationale**: 発見性が高く、UI の追加操作が不要。
- **Trade-offs**: 巨大ワークブックでは走査コストが増える。将来のチャンク検索で改善する。

## Risks & Mitigations

- 検索結果のシートインデックスが再読み込みでずれる — 再読み込み時に検索インデックスを破棄する既存規則を維持する。
- `is_match` の更新漏れで他シートの同一座標がハイライトされる — `active_sheet` の同期を一箇所（`view` / `jump_to` / `switch_sheet`）に集約し、テストで確認する。
- `n` の移動開始位置が保存済み current のままで、手動移動後の挙動が vim と異なる — 初版は既存挙動を維持し、将来 `seek_from` を追加する。

## References

- 既存仕様: `.kiro/specs/view-consistency/design.md`, `.kiro/specs/spreadsheet-tui/design.md`
- コード: `src/domain/search.rs`, `src/app/state.rs`, `src/domain/viewport.rs`
