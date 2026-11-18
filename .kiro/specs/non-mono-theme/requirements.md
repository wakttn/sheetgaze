# Requirements Document

## Introduction

sheetgaze のカラーモードには `Auto`、`Truecolor`、`Ansi256`、`Ansi16`、`Mono` がある。本機能は、`Mono` 以外のすべてのカラーモードで、明示的な色指定がない UI 領域の背景色を白、文字色を黒とする既定配色を提供する。

Excel のセル色・塗りつぶし・選択強調・検索強調などの明示的なスタイルは維持し、端末既定色に依存した読みにくい表示を改善する。`Mono` モードは端末既定色を尊重し、白背景・黒文字を強制しない。

## Boundary Context

- **In scope**: 非 `Mono` カラーモードの基底背景色・文字色、グリッド・ヘッダ・ステータス・タブ・入力行・ヘルプ・詳細・フィルタパネルへの適用、`--no-ansi` スナップショットとの整合。
- **Out of scope**: カラーモードの追加、テーマファイル、ユーザー定義配色、シンタックスハイライト、Excel 側の色情報の変更。
- **Adjacent expectations**: 構造編集・選択・検索の明示的な強調色は既存のコントラスト規則を維持する。`--no-ansi` は ANSI を出力しない既存動作を維持する。

## Requirements

### Requirement 1: 非 Mono モードの既定配色

**Objective:** As a terminal user, I want 明るい背景でも読みやすい既定配色を得る, so that 表と UI を快適に閲覧できる。

#### Acceptance Criteria

1. While カラーモードが `Mono` ではない, the sheetgaze shall 明示的な色指定がない領域の背景色を白、文字色を黒とする。
2. While カラーモードが `Mono` である, the sheetgaze shall 端末既定の背景色と文字色を使用し、白背景・黒文字を強制しない。
3. While 非 Mono モードである, the sheetgaze shall Excel のセル色・塗りつぶし・選択強調・検索強調を判読可能なコントラストで表示する。
4. The sheetgaze shall グリッド、行番号・列ヘッダ、ステータス行、タブ行、入力行、ヘルプ、詳細表示、フィルタパネルに同じ既定配色を適用する。
5. Where `--no-ansi` のスナップショットを出力する, the sheetgaze shall 配色変更によって ANSI 以外の文字内容を変えない。
6. The sheetgaze shall `Auto`、`Truecolor`、`Ansi256`、`Ansi16` のすべてで同じ既定配色を適用する。
