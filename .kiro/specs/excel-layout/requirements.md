# Requirements Document

## Introduction
`sheetgaze` は xlsx / csv をターミナルで閲覧する読み取り専用 TUI である。現在はセル内容の表示幅と改行・折り返し行数から列幅・行高を自動計算しているため、Excel で列幅や行高を明示的に設定したシートでも、その意図したレイアウトが再現されない。本仕様では、xlsx に保存された Excel の列幅・行高（`customWidth` / `customHeight`）を優先して表示へ反映し、未指定の行・列だけ従来の自動計算にフォールバックする。既定動作は Excel 準拠とし、`--layout content` で従来の完全自動レイアウトに切り替えられるようにする。

## Boundary Context
- **In scope**: xlsx の `<cols>` / `<col>` / `<row>` / `<sheetFormatPr>` の読み取り、Excel 明示値の `LayoutSnapshot` への反映、明示値と自動計算値の優先規則、文字数・ポイントから端末桁・行数への変換、eager / lazy 両経路での一致、レイアウトモードの CLI 切替、数式再計算時の固定列・行の扱い、既存 fixture とテストの更新。
- **Out of scope**: 非表示行・列（`hidden`）の表示制御、フォント・DPI・ピクセル単位の完全再現、印刷レイアウト、csv に対する Excel 相当の幅・高さ設定、セル編集・保存、Excel 以外の表計算形式。
- **Adjacent expectations**: `view-consistency` の切り詰め・折り返し・grow-only 更新、`spreadsheet-tui` の Requirement 2（グリッド表示とレイアウト）および Requirement 10（遅延ロード）と整合する。既存の freeze / filter / copy / search / detail は新しい列幅・行高の下でも従来どおり動作する。

## Requirements

### Requirement 1: Excel 列幅の読み取りと適用
**Objective:** As a ターミナル利用者, I want Excel で設定した列幅どおりに表を表示したい, so that 列の切り詰め位置や表の見た目を Excel と一致させて内容を比較できる。

#### Acceptance Criteria
1. When xlsx のシートを読み込む, the sheetgaze shall シート XML の `<cols>` / `<col>` から `customWidth` が真の列幅を取得する。
2. When 複数の `<col>` が同じ列に該当する, the sheetgaze shall XML 内で後に現れた定義をその列の列幅として採用する。
3. When Excel の明示列幅がセル内容の表示幅より狭い, the sheetgaze shall 明示列幅を維持し、内容を既存の切り詰め・はみ出し規則で表示する。
4. When `<col>` の `customWidth` が偽または `width` が欠落している, the sheetgaze shall その列を内容からの自動計算対象とする。
5. Where 列幅の上限・下限が設定されている, the sheetgaze shall 変換後の明示列幅をその範囲にクランプする。

### Requirement 2: Excel 行高の読み取りと適用
**Objective:** As a ターミナル利用者, I want Excel で設定した行高どおりに表を表示したい, so that 意図的に高く・低く設定された行を Excel と同じ見た目で確認できる。

#### Acceptance Criteria
1. When xlsx のシートを読み込む, the sheetgaze shall シート XML の `<row>` から `customHeight` が真の行高（`ht`）を取得する。
2. When `<row>` の `customHeight` が偽または `ht` が欠落している, the sheetgaze shall その行を内容からの自動計算対象とする。
3. When 明示行高が折り返し後のテキスト行数より少ない, the sheetgaze shall 明示行高を維持し、表示しきれない内容を切り詰め状態として示す。
4. Where 行高の上限・下限が設定されている, the sheetgaze shall 変換後の明示行高をその範囲にクランプする。
5. When 結合セルのアンカー行に明示行高がある, the sheetgaze shall 結合範囲のうち明示行高を持つ行にはその値を使用する。

### Requirement 3: 既定値と未指定時のフォールバック
**Objective:** As a ターミナル利用者, I want Excel の既定列幅・既定行高も尊重しつつ、未設定の列・行は読みやすく自動調整してほしい, so that Excel 側の設定と TUI の可読性を両立できる。

#### Acceptance Criteria
1. When `<sheetFormatPr>` に `defaultColWidth` がある, the sheetgaze shall 明示列幅を持たない列の最小幅としてその値を使用する。
2. When `<sheetFormatPr>` に `defaultRowHeight` がある, the sheetgaze shall 明示行高を持たない行の最小高としてその値を使用する。
3. When `<sheetFormatPr>` に既定値がない, the sheetgaze shall 既存の既定値（列幅 8 桁、行高 1 行）を最小値として使用する。
4. The sheetgaze shall 内容から算出した列幅・行高が Excel の既定値を上回る場合、その自動計算値を採用する。
5. The sheetgaze shall Excel の既定値で明示列幅・明示行高を上書きしない。

### Requirement 4: 単位変換と上限
**Objective:** As a ターミナル利用者, I want Excel の文字数・ポイント単位の設定が端末表示に一貫して変換されてほしい, so that 同じファイルを eager / lazy のどちらで開いても同じレイアウトになる。

#### Acceptance Criteria
1. When 列幅を端末表示桁へ変換する, the sheetgaze shall Excel の文字数単位の小数部を切り捨て、1 以上の整数桁にする（例: 24.7109375 → 24）。
2. When 行高を端末行数へ変換する, the sheetgaze shall `ht` を基準行高（既定 15pt）で除算し、四捨五入して 1 以上の整数行にする（例: 30pt → 2 行）。
3. The sheetgaze shall 変換後の列幅を設定された最小列幅以上・最大列幅以下にクランプする。
4. The sheetgaze shall 変換後の行高を 1 行以上・設定された最大行数以下にクランプする。
5. When 同じ xlsx を eager と lazy で読み込む, the sheetgaze shall 同一の列幅・行高を生成する。
6. If `width` または `ht` が数値として解釈できない、または負の値である, then the sheetgaze shall その属性を無視して自動計算へフォールバックし、読み込み自体は失敗させない。

### Requirement 5: 表示整合と結合セル
**Objective:** As a ターミナル利用者, I want 列幅・行高の変更後も折り返し・はみ出し・切り詰めが一貫してほしい, so that グリッドとセル詳細で同じ内容判定を信頼できる。

#### Acceptance Criteria
1. When `wrap_text` セルの行高を自動計算する, the sheetgaze shall Excel 明示列幅を反映した最終列幅で折り返し行数を計算する。
2. When 結合セルが明示列幅の列と未指定列にまたがる, the sheetgaze shall 明示列幅を変更せず、幅の要求を未指定列にだけ分配する。
3. When 明示列幅または明示行高によって内容が収まらない, the sheetgaze shall 既存の「はみ出し」または「切り詰め」規則を適用する。
4. The sheetgaze shall グリッド表示とセル詳細表示で切り詰め判定を一致させる。
5. The sheetgaze shall Excel のフォント・ピクセル差を再現せず、変換後の端末桁・行数をレイアウトの確定値として扱う。

### Requirement 6: レイアウトモードと読み込み経路
**Objective:** As a ターミナル利用者, I want 既定では Excel 準拠で表示し、必要なら従来の自動レイアウトに戻せる, so that 状況に応じて見た目を選択できる。

#### Acceptance Criteria
1. The sheetgaze shall 既定のレイアウトモードを Excel 準拠とする。
2. When `--layout content` が指定される, the sheetgaze shall Excel の列幅・行高を無視して従来の完全自動レイアウトを使用する。
3. When csv を読み込む, the sheetgaze shall レイアウトモードにかかわらず完全自動レイアウトを使用する。
4. When ファイルを再読み込みまたは watch で再読込する, the sheetgaze shall 設定されたレイアウトモードを再適用する。
5. The sheetgaze shall eager / lazy の両読み込み経路で同じレイアウトモードの規則を適用する。

### Requirement 7: 数式再計算後のレイアウト更新
**Objective:** As a ターミナル利用者, I want 数式の再計算結果が Excel 設定を壊さずに表示されてほしい, so that 計算後もシートの意図した列幅・行高を維持できる。

#### Acceptance Criteria
1. When 数式再計算によってセルの表示値が長くなる, the sheetgaze shall Excel 明示列幅・明示行高を拡大しない。
2. When 数式再計算によって自動計算対象の列・行の表示値が長くなる, the sheetgaze shall 既存の grow-only 規則で必要な分だけ拡大する。
3. The sheetgaze shall Excel 明示値に由来する列・行の識別を再計算後も保持する。
4. When 再計算結果が明示列幅・明示行高に収まらない, the sheetgaze shall 切り詰め状態を詳細表示に反映する。

### Requirement 8: 互換性と性能
**Objective:** As a メンテナが, I want 既存機能を壊さず低コストで Excel レイアウトを導入したい, so that 回帰と性能劣化を抑えながらリリースできる。

#### Acceptance Criteria
1. The sheetgaze shall 新しい列幅・行高の下でも freeze、filter、copy、search、detail、overflow の既存挙動を維持する。
2. The sheetgaze shall Excel レイアウトの取得を既存のシートメタデータ読み取りパスに統合し、シート XML の追加全走査を行わない。
3. The sheetgaze shall 列幅・行高の変換と優先規則を純粋関数として単体テストできる構造にする。
4. When 既存 fixture が Excel 列幅を持つ, the sheetgaze shall 新しい期待値に合わせてテストを更新し、意図しないレイアウト回帰を検出できるようにする。
5. The sheetgaze shall 明示列幅・行高、未指定フォールバック、結合セル、wrap、狭い明示幅、既定値を検証する fixture を提供する。
