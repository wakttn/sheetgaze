# Research & Design Decisions

---
**Purpose**: 非 Mono モードの既定配色を全 UI に適用するための調査と設計判断を記録する。
---

## Summary

- **Feature**: `non-mono-theme`
- **Discovery Scope**: 既存 TUI への小規模な表示拡張
- **Key Findings**:
  - `Theme` は選択・検索・ヘッダなどの個別 `Style` を持つが、画面全体の基底背景色・文字色を持たない。
  - ratatui の `Buffer` はセルごとに `fg` / `bg` を保持し、`Color::Reset` が端末既定色を表す。
  - 各ウィジェットは `Style::default()` を基底にパッチするため、描画後に `Reset` のチャネルだけを補完すれば全 UI に一貫して適用できる。
  - ポップアップは `Clear` でセルを初期化するため、描画後の後処理が必要。
- **Implications**:
  - `Theme` に基底スタイルを追加し、`Screen` の描画後にチャネル単位で補完する。
  - `Mono` では後処理をスキップする。

## Research Log

### 既存テーマと描画

- **Context**: 非 Mono モードで全 UI を白背景・黒文字にする方法を検討した。
- **Sources Consulted**: `src/ui/theme.rs`, `src/ui/screen.rs`, `src/ui/grid.rs`, `src/ui/status.rs`, `src/ui/help.rs`, `src/ui/detail.rs`, `src/ui/filter_panel.rs`, `src/ui/input_line.rs`, `src/ui/search_prompt.rs`, ratatui 0.30 の `Buffer` API
- **Findings**:
  - `Theme::cell` は Excel のセル色と塗りつぶしを個別に処理する。
  - `GridWidget` のグリッド線は `fg` のみを設定し、`bg` は `Reset` のままになる。
  - `Clear` はセルを `Cell::default()` に戻す。
  - 画面全体の `Reset` セルを一括で補完すれば、各ウィジェットの個別修正を避けられる。
- **Implications**:
  - `fg` と `bg` を独立に補完する。両方が `Reset` のときだけ適用すると、グリッド線の背景が端末既定色のまま残る。
  - 画面サイズ不足時の早期 return も補完対象に含める。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| ウィジェットごとに基底色を設定 | 各ウィジェットの `Style` に白背景・黒文字を明示 | 挙動が明確 | 変更箇所が多く、ポップアップや将来のウィジェットで漏れやすい | 不採用 |
| 描画後のバッファ補完 | `Color::Reset` のチャネルを基底色で埋める | 一箇所で全 UI に適用できる | バッファ走査コストが加わる | 採用 |
| ターミナル全体の背景色を変更 | ANSI で端末背景を設定 | 実装が単純 | 端末状態を変更し、終了後の復元が必要 | 不採用 |

## Design Decisions

### Decision: 描画後の基底色補完

- **Context**: 非 Mono モードで全 UI を白背景・黒文字にする。
- **Alternatives Considered**:
  1. すべてのウィジェットの `Style` に基底色を明示する。
  2. 描画後に `Color::Reset` のセルへ基底色を適用する。
- **Selected Approach**: 2 を採用し、`fg` と `bg` をチャネル単位で補完する。
- **Rationale**: 既存のウィジェット実装を変更せず、ポップアップや入力行を含む全領域へ一貫して適用できる。
- **Trade-offs**: 描画後のバッファ走査コストが加わる。スナップショットの ANSI 出力が変わる。
- **Follow-up**: `Mono` では後処理をスキップし、`--no-ansi` の文字内容が変わらないことをテストする。

## Risks & Mitigations

- **Risk**: スナップショットの ANSI 出力が広範囲に変わる — 既存テストを更新し、Mono の回帰テストを追加する。
- **Risk**: 明示的な `Color::Reset` を意図するセルまで白・黒になる — 非 Mono モードでは `Reset` を端末既定色ではなく基底色として扱う仕様とし、Mono で従来動作を保つ。
- **Risk**: 画面サイズ不足時の早期 return で補完が漏れる — 早期 return 経路でも補完を適用する。

## References

- `src/ui/theme.rs` — 既存のテーマ定義。
- `src/ui/screen.rs` — 描画パイプライン。
- ratatui 0.30 `Buffer` / `Cell` — `Color::Reset` とスタイルパッチ。
