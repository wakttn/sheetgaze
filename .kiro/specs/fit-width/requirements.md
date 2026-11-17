# Requirements Document

## Introduction
`sheetgaze` は xlsx / csv をターミナルで閲覧する読み取り専用 TUI である。列幅は Excel の明示幅または内容から決まるため、列数の多いシートでは画面に収まりきらず横スクロールが必要になる。本仕様では、閲覧中に全列を端末幅へ比例縮小して一望できる「フィット幅モード」と、元の列幅に戻せるトグル操作を追加する。元のレイアウトは変更せず、表示時だけフィット幅を派生させる。

## Boundary Context
- **In scope**: 使用範囲全列の比例縮小、フィット幅モードのトグル、`w` キーバインド、`--fit-width` CLI 引数、print モードへの適用、ステータス表示、リサイズ・再読込・数式再計算時の再計算、グリッド・横スクロール・固定列・セル詳細との整合、テスト。
- **Out of scope**: 行高の再計算、列の非表示制御、ユーザーによる列幅の直接編集、Excel ファイルへの書き戻し、フィット幅の永続化（設定ファイル保存）、列ごとの個別フィット。
- **Adjacent expectations**: `excel-layout` の Excel 明示幅・content レイアウト、`view-consistency` の切り詰め・折り返し・freeze、`spreadsheet-tui` の横スクロールと描画規則と整合する。フィット幅は既存の `Sheet.layout` を変更しない。

## Requirements

### Requirement 1: フィット幅の計算
**Objective:** As a ターミナル利用者, I want 使用範囲の全列を画面幅に比例縮小して一望したい, so that 横スクロールせずに表全体の構成を把握できる。

#### Acceptance Criteria
1. When フィット幅モードが有効になった, the sheetgaze shall 使用範囲の全列を対象に表示用の列幅を計算する。
2. When 全列の元の幅と列間ガターの合計が表示領域に収まる, the sheetgaze shall 元の列幅を変更しない。
3. When 全列の合計が表示領域に収まらない, the sheetgaze shall 各列に最低 1 桁を確保した上で、残りの表示幅を元の列幅の比率で配分する。
4. The sheetgaze shall フィット幅を元の列幅より大きくしない。
5. When 1 桁幅にしても全列が表示領域に収まらない, the sheetgaze shall 全列を 1 桁に圧縮し、横スクロールを許可する。
6. The sheetgaze shall フィット幅の計算を決定的にし、端数配分が同点の場合は列インデックス順で解決する。
7. The sheetgaze shall フィット幅の計算を列数に比例する計算量で行い、セル数に依存させない。

### Requirement 2: モード切替と状態
**Objective:** As a ターミナル利用者, I want フィット幅と元の列幅をキーで切り替えたい, so that 一覧性と元のレイアウトを状況に応じて使い分けられる。

#### Acceptance Criteria
1. When 通常モードまたは範囲選択モードで `w` が押された, the sheetgaze shall フィット幅モードの有効・無効を反転する。
2. When フィット幅モードが有効になった, the sheetgaze shall 横スクロール位置を先頭列へ戻す。
3. When フィット幅モードが無効になった, the sheetgaze shall 元の列幅でフォーカスセルが見えるように横スクロール位置を補正する。
4. While フィット幅モードが有効, the sheetgaze shall 端末リサイズ後に新しい表示幅でフィット幅を再計算する。
5. While フィット幅モードが有効, the sheetgaze shall シート切替・ファイル再読込・watch 再読込の後も有効状態を維持する。
6. The sheetgaze shall フィット幅モードの状態をステータス行に表示する。
7. When 1 桁幅にしても収まらない, the sheetgaze shall ステータス行に `fit:limited` を表示する。
8. The sheetgaze shall 検索・フィルタ・詳細・ヘルプモードでは `w` をフィット幅の切替として扱わない。

### Requirement 3: CLI と起動時の適用
**Objective:** As a ターミナル利用者, I want 起動時からフィット幅で表示したい, so that スクリプトや print スナップショットでも同じ見た目を再現できる。

#### Acceptance Criteria
1. When `--fit-width` が指定された, the sheetgaze shall TUI の起動時からフィット幅モードを有効にする。
2. When `--fit-width` と `--print` が指定された, the sheetgaze shall print スナップショットにもフィット幅を適用する。
3. Where `--fit-width` が指定されない, the sheetgaze shall フィット幅モードを無効で開始する。
4. The sheetgaze shall `--fit-width` を既存の `--layout excel|content` と独立に扱う。
5. While フィット幅モードが無効, the sheetgaze shall 既存の Excel 準拠または content レイアウトをそのまま表示する。

### Requirement 4: 表示整合
**Objective:** As a ターミナル利用者, I want フィット幅でも選択・詳細・固定列が一貫して動いてほしい, so that 表示を縮めても操作結果を信頼できる。

#### Acceptance Criteria
1. While フィット幅モードが有効, the sheetgaze shall グリッド描画・横スクロール・固定列・セル詳細の切り詰め判定で同じ表示用列幅を使用する。
2. When 結合セルがフィット幅の対象になる, the sheetgaze shall 構成する列の合計幅を縮小し、既存の切り詰め規則を適用する。
3. When `wrap_text` セルが縮小後の幅に収まらない, the sheetgaze shall 既存の切り詰め表示（`…`）を適用する。
4. The sheetgaze shall フィット幅のために行高を再計算しない。
5. While フィット幅モードが無効, the sheetgaze shall 既存の freeze・filter・copy・search・detail の挙動を維持する。
6. The sheetgaze shall フィット幅の変更でセル値・選択範囲・コピー内容を変更しない。

### Requirement 5: 再計算・再読込との整合
**Objective:** As a ターミナル利用者, I want 数式再計算や再読込の後もフィット幅が正しく更新されてほしい, so that 古い幅のまま表示が崩れない。

#### Acceptance Criteria
1. When 数式再計算によって元の列幅が拡大した, the sheetgaze shall 次回表示時にフィット幅を再計算する。
2. When ファイルが再読込された, the sheetgaze shall 新しい元の列幅からフィット幅を再計算する。
3. The sheetgaze shall フィット幅を元のレイアウト（`Sheet.layout`）へ書き戻さない。
4. When フィット幅モードが無効になった, the sheetgaze shall 数式再計算後の元の列幅を表示する。

### Requirement 6: テストと互換性
**Objective:** As a メンテナ, I want フィット幅の規則と既存機能の非回帰をテストで固定したい, so that レイアウト変更時の退行を早期に検出できる。

#### Acceptance Criteria
1. The sheetgaze shall フィット幅の計算を純粋関数として単体テストできる構造にする。
2. When 既存のレイアウト・freeze・スクロールのテストがフィット幅無効で実行される, the sheetgaze shall 既存の期待値を維持する。
3. The sheetgaze shall ちょうど収まる場合、1 列のみの場合、1 桁でも収まらない場合、列数が過多な場合の境界テストを持つ。
4. The sheetgaze shall `w` のキーマップ、`--fit-width` の解析、print モードでのフィット適用をテストする。
