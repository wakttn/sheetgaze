# Requirements Document

## Introduction
`sheetgaze` は xlsx / csv をターミナルで閲覧する TUI である。`--layout excel`（既定）では Excel の明示列幅（`customWidth`）を優先するため、狭い列に入りきらない文字列は、隣接セルが空なら横にはみ出し、塞がれている場合は `…` で切り詰められる。本仕様では、xlsx の列幅を優先するモードにおいて、はみ出し先にも収まらない長い文字列をはみ出し可能な範囲で折り返し、行高を広げて全体を表示できるようにする。列幅そのものは変更せず、fit-width モードや content レイアウトの既存挙動は維持する。

## Boundary Context
- **In scope**: xlsx 明示列幅（`customWidth`）の列、明示幅を含む結合範囲、単一行かつ `wrap_text` でないセルの折り返し、はみ出し可能幅の計算、行高の拡張、グリッド・print・セル詳細・スクロール・freeze・フィルタ表示との整合、編集・数式再計算・再読込後の行高更新、テストとドキュメント。
- **Out of scope**: fit-width モードでの動的な行高再計算、content レイアウトの自動列幅の変更、ユーザーによる列幅編集、Excel ファイルへの書き戻し、新しい CLI オプションやキーバインド、`wrap_text` セルおよび明示改行を含むセルの折り返し規則の変更、`--max-row-lines` を超える行の全文表示。
- **Adjacent expectations**: `excel-layout` の明示列幅・明示行高・content レイアウト、`fit-width` の「行高を再計算しない」規則、`view-consistency` の切り詰め・折り返し・freeze 整合、`spreadsheet-tui` の横方向はみ出し規則、`spreadsheet-editing` の grow-only レイアウト更新と整合する。

## Requirements

### Requirement 1: 適用対象とはみ出し可能幅の決定
**Objective:** As a xlsx 閲覧者, I want xlsx の明示列幅を維持したまま長い文字列を読める, so that 列幅を崩さずにセルの内容を確認できる。

#### Acceptance Criteria
1. While `--layout excel` が有効かつ fit-width モードが無効, the sheetgaze shall xlsx の明示列幅（`customWidth`）を持つ列と、その列を含む結合範囲を本仕様の対象とする。
2. The sheetgaze shall content レイアウト、自動幅の列、fit-width モードを本仕様の自動折り返しの対象外とする。
3. When 単一行かつ `wrap_text` でないセルの表示幅が自列幅を超える, the sheetgaze shall 右隣の空セルへ拡張する既存のはみ出し規則に従って、はみ出し可能幅を計算する。
4. The sheetgaze shall はみ出し可能幅の計算で、値を持つセル、結合範囲、表示可能な境界線、使用範囲の終端で拡張を停止する。
5. The sheetgaze shall はみ出し可能幅に列間ガターを含め、既存の横方向はみ出し表示と同じ幅規則を使用する。
6. The sheetgaze shall 本仕様のために列幅を拡大・縮小しない。

### Requirement 2: 長い文字列の折り返し表示
**Objective:** As a xlsx 閲覧者, I want 収まりきらない文字列を折り返して全文表示してほしい, so that `…` で切れた内容を推測せずに読める。

#### Acceptance Criteria
1. When セルの全文がはみ出し可能幅に収まる, the sheetgaze shall 従来どおり1行で隣接する空セルへはみ出して表示する。
2. When セルの全文がはみ出し可能幅に収まらない, the sheetgaze shall そのはみ出し可能幅で文字列を折り返し、行高の範囲内で全文を表示する。
3. When 結合範囲が明示列幅の列を含み、全文が結合範囲の幅に収まらない, the sheetgaze shall 結合範囲の幅で文字列を折り返す。
4. The sheetgaze shall 折り返し表示で隣接セルの値、罫線、選択状態、検索強調を上書きしない。
5. The sheetgaze shall 明示改行を含むセルおよび `wrap_text` セルには既存の折り返し・切り詰め規則を適用し、本仕様の自動折り返しを重ねない。
6. The sheetgaze shall 折り返し後の表示と `truncated` 判定を、グリッド計画とセル詳細で一致させる。

### Requirement 3: 行高の拡張と上限
**Objective:** As a xlsx 閲覧者, I want 折り返した全文が行内に表示されてほしい, so that 縦方向にも内容が欠けない。

#### Acceptance Criteria
1. When 明示列幅の列で自動折り返しが発生する, the sheetgaze shall その行の必要な折り返し行数以上になるように行高を拡張する。
2. The sheetgaze shall 同じ行に複数の対象セルがある場合、必要な折り返し行数の最大値を行高に使用する。
3. Where Excel の明示行高（`customHeight`）が設定されている, the sheetgaze shall 明示行高を下限として扱い、折り返しに必要な行数が上回る場合だけ行高を拡張する。
4. Where シート既定行高または自動計算の行高が設定されている, the sheetgaze shall それらの値も下限として扱う。
5. The sheetgaze shall 折り返し行数を `--max-row-lines` で上限付けし、上限を超える場合は既存の `…` 表示で切り詰める。
6. The sheetgaze shall 拡張した行高をグリッド描画、print スナップショット、セル詳細、スクロール、freeze、フィルタ後の可視行計算で共通に使用する。

### Requirement 4: モードとレイアウトの非干渉
**Objective:** As a メンテナ, I want 新しい折り返しが既存の表示モードを壊さないことを保証したい, so that レイアウト機能の退行を防げる。

#### Acceptance Criteria
1. While fit-width モードが有効, the sheetgaze shall 本仕様の追加折り返し行高を使用せず、既存の fit-width の表示規則を維持する。
2. While `--layout content` が有効, the sheetgaze shall 既存の自動列幅・行高・はみ出し・切り詰めの挙動を維持する。
3. The sheetgaze shall csv の表示挙動を変更しない。
4. The sheetgaze shall 行高の拡張によってセル値、選択範囲、コピー内容、検索結果、フィルタ条件を変更しない。
5. The sheetgaze shall `--max-row-lines` や `--layout` の既存 CLI 契約を変更しない。
6. The sheetgaze shall `wrap_text` セルの既存の切り詰め挙動を維持する。

### Requirement 5: 編集・数式再計算・再読込との整合
**Objective:** As a xlsx 閲覧者, I want 編集や再計算の後も折り返し表示が正しく更新されてほしい, so that 古い行高のまま内容が欠けない。

#### Acceptance Criteria
1. When 明示列幅の列またはその隣接セルが編集された, the sheetgaze shall 影響する行の折り返し行数を再評価し、必要なら行高を grow-only で拡張する。
2. When 数式再計算の結果によって表示文字列が長くなった, the sheetgaze shall 影響する行の折り返し行数を再評価し、必要なら行高を grow-only で拡張する。
3. When ファイルが再読込された, the sheetgaze shall 新しいセル内容と列幅から折り返し行高を再計算する。
4. When 行が挿入または削除された, the sheetgaze shall 折り返し行高を対応する行位置へシフトする。
5. When 列が挿入または削除された, the sheetgaze shall 明示列幅に依存する折り返し行高を無効化または再計算し、古い幅に基づく行高を残さない。
6. The sheetgaze shall 編集・再計算・構造編集の後も、グリッドとセル詳細の折り返し・切り詰め判定を一致させる。

### Requirement 6: テストと互換性
**Objective:** As a メンテナ, I want 折り返し規則と既存機能の非回帰をテストで固定したい, so that レイアウト変更時の退行を早期に検出できる。

#### Acceptance Criteria
1. The sheetgaze shall はみ出し可能幅の計算と折り返し行高の計算を、純粋関数として単体テストできる構造にする。
2. The sheetgaze shall 全長がはみ出し可能幅に収まる場合、収まらない場合、隣接セルで塞がれた場合、結合範囲の場合、明示行高がある場合、`--max-row-lines` に達する場合のテストを持つ。
3. When 既存の `overflow.xlsx` 相当の明示列幅を持たないシートを表示する, the sheetgaze shall 既存のはみ出し・切り詰めテストの期待値を維持する。
4. The sheetgaze shall fit-width、content レイアウト、csv の既存テストが非回帰であることを確認する。
5. The sheetgaze shall print モード、セル詳細、編集後の行高更新を統合テストで確認する。
