# Requirements Document

## Introduction

`sheetgaze` は xlsx / csv をターミナルで閲覧する TUI である。現在の検索はアクティブシートだけを対象とし、一致の位置にもシート情報がない。本仕様は、ワークブック内の全シートを対象に検索し、一致へシートをまたいでジャンプできるようにすることを目的とする。既存のスマートケース literal 検索、数式表示値の一致、フィルタで隠れた行のスキップは維持する。

## Boundary Context

- **In scope**: 全シート検索、シート情報付き一致、`n` / `N` のシート横断移動、検索からのシート切替ジャンプ、`*` / `#`、`:goto` によるシート指定ジャンプ、検索件数・現在位置・シート名の表示、ジャンプ履歴、既存の検索 UI と TUI テストの更新
- **Out of scope**: 正規表現検索、検索インデックスの永続化、検索結果パネル（quickfix 形式）、非表示シートの表示制御、バックグラウンド検索ワーカー、置換（`spreadsheet-editing` spec が所有）
- **Adjacent expectations**: `spreadsheet-tui` / `view-consistency` の検索ハイライト、フィルタ可視性、遅延ロード、コピー、数式再計算の契約を維持する。`domain` は外部 TUI クレートへ依存しない。

## Requirements

### Requirement 1: 全シート検索

**Objective:** As a ターミナル利用者, I want 開いているワークブックの全シートを一度に検索したい, so that 目的のセルがどのシートにあるかを探し回らずに済む。

#### Acceptance Criteria
1. When 利用者が検索語を確定する, the sheetgaze shall アクティブシートだけでなく、読み込み済みの全シートの表示値と数式を検索対象にする。
2. The sheetgaze shall 各一致にシートの識別情報とセル座標を保持する。
3. The sheetgaze shall 既存どおりスマートケースの literal 一致を行い、大文字を含む検索語は大文字小文字を区別する。
4. Where シートが遅延ロードされている, the sheetgaze shall 遅延行を走査して検索対象に含める。
5. When フィルタによって非表示の行に一致がある, the sheetgaze shall その一致を可視一致として扱わず、非表示件数に数える。

### Requirement 2: 一致間の移動とジャンプ

**Objective:** As a ターミナル利用者, I want 一致を前後にたどってシートをまたいで移動したい, so that 複数シートに散らばるデータを連続して確認できる。

#### Acceptance Criteria
1. When 利用者が `n` を押す, the sheetgaze shall 現在位置より後ろの次の可視一致へ移動し、一致が別シートにある場合はアクティブシートを切り替える。
2. When 利用者が `N` を押す, the sheetgaze shall 現在位置より前の前の可視一致へ移動し、一致が別シートにある場合はアクティブシートを切り替える。
3. When 一致がワークブックの末尾または先頭に達する, the sheetgaze shall シート順にラップして反対端の一致へ移動する。
4. When シートを切り替えて一致へジャンプする, the sheetgaze shall 対象行を読み込み、フォーカスを一致セルに置き、そのセルが表示領域内に来るようにビューポートを調整する。
5. When 利用者が `*` または `#` を押す, the sheetgaze shall フォーカスセルの表示値を検索語として全シートを検索し、次または前の一致へ移動する。
6. When 利用者が `:goto Sheet!A1` または `:goto A1` を実行する, the sheetgaze shall 指定シートとセルへジャンプする。
7. If 指定シート名またはセル座標が不正である, then the sheetgaze shall 状態行に理由を表示し、フォーカスを変更しない。

### Requirement 3: 検索状態の表示

**Objective:** As a ターミナル利用者, I want 検索結果の現在位置と対象シートを確認したい, so that どこを検索しているかを把握できる。

#### Acceptance Criteria
1. While 検索結果が存在する, the sheetgaze shall 現在の一致番号、総一致数、非表示件数を状態行または検索行に表示する。
2. The sheetgaze shall 現在の一致が属するシート名とセル座標を検索行または状態行に表示する。
3. While アクティブシートが検索一致を持つ, the sheetgaze shall そのシート上の一致だけをグリッドでハイライトする。
4. If 一致が 0 件である, then the sheetgaze shall `no match` を表示し、フォーカスを変更しない。

### Requirement 4: ジャンプ履歴

**Objective:** As a ターミナル利用者, I want 検索ジャンプの前後に戻りたい, so that 元の作業位置へ素早く復帰できる。

#### Acceptance Criteria
1. When シートをまたぐ検索ジャンプまたは `:goto` を実行する, the sheetgaze shall ジャンプ前のシートとセルを履歴に記録する。
2. When 利用者が `Ctrl-o` を押す, the sheetgaze shall 直前のジャンプ元へ戻る。
3. When 利用者が `Ctrl-i` を押す, the sheetgaze shall 戻ったジャンプを再度進む。
4. The sheetgaze shall 履歴の上限を設け、上限を超えた古い項目を破棄する。

### Requirement 5: 回帰と性能

**Objective:** As a 保守開発者, I want 既存の検索・フィルタ・遅延ロードを壊さずに横断検索を追加したい, so that 表示の一貫性を維持できる。

#### Acceptance Criteria
1. The sheetgaze shall `view-consistency` の可視一致同期と検索ハイライトの外部契約を維持する。
2. When 検索対象が複数シートである, the sheetgaze shall 一致をシート順、行順、列順で安定して並べる。
3. The sheetgaze shall 検索 UI のテストと横断検索のテストを提供する。
