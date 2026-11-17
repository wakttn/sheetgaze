# Requirements Document

## Introduction

`sheetgaze` は xlsx / csv の閲覧を目的とした TUI だが、調査中にセル値を修正したい場面がある。本仕様は、Vim 風のモーダル操作で xlsx のセル値・数式を編集し、明示的な保存操作で元ファイルを安全に更新する機能を追加する。置換（`:s`）と、シート横断検索（`cross-sheet-search`）を組み合わせた一括変更も対象とする。CSV の保存、書式編集、行・列の構造編集は対象外とする。

## Boundary Context

- **In scope**: Insert / Command モード、セル値・数式の入力、編集オーバーレイ、undo / redo、dirty 表示、`:w` による xlsx の差分保存、未保存編集からの数式再計算、`:s` / `:%s` / `:gs` による置換、編集後のレイアウト・フィルタ・検索・数式更新、保存と watch / reload の整合、read-only 保護
- **Out of scope**: CSV の編集・保存、セル書式・結合・行高・列幅の編集、行/列の挿入・削除、共有数式・配列数式の編集、グラフ・画像・ピボット・マクロの変更、正規表現置換、確認付き置換（`c` フラグ）、マクロ記録、保存履歴
- **Adjacent expectations**: `cross-sheet-search` の検索型とジャンプ、`spreadsheet-tui` の表示・レイアウト・コピー、`file-reload` の監視・再読み込み契約を維持する。`domain` は `zip` / `quick-xml` / `calamine` の型を公開しない。

## Requirements

### Requirement 1: Vim 風の編集モード

**Objective:** As a Vim に慣れたターミナル利用者, I want 閲覧時と同じモーダル操作でセルを編集したい, so that マウスや別のエディタなしで修正できる。

#### Acceptance Criteria
1. When 利用者が通常モードで `i`、`a`、`I`、`A`、`F2` のいずれかを押す, the sheetgaze shall 現在のセルを対象に Insert モードへ入り、セル内容を下部入力行に表示する。
2. When 利用者が Insert モードで文字を入力する, the sheetgaze shall 入力行のカーソル位置に文字を挿入する。
3. When 利用者が Insert モードで `Enter` を押す, the sheetgaze shall 入力を確定して下のセルへ移動する。
4. When 利用者が Insert モードで `Tab` または `Shift-Tab` を押す, the sheetgaze shall 入力を確定して右または左のセルへ移動する。
5. When 利用者が Insert モードで `Esc` を押す, the sheetgaze shall 入力を確定して通常モードへ戻る。
6. When 利用者が Insert モードで `Ctrl-c` を押す, the sheetgaze shall 入力を破棄して通常モードへ戻る。
7. When 利用者が通常モードで `s` を押す, the sheetgaze shall 現在のセル内容を空にした状態で Insert モードへ入る。
8. When 利用者が通常モードで `=` を押す, the sheetgaze shall `=` を前置した状態で Insert モードへ入る。
9. When 利用者が通常モードで `:` を押す, the sheetgaze shall Command モードへ入り、下部入力行に `:` を表示する。
10. When 利用者が通常モードで `x` を押す, the sheetgaze shall 現在のセルまたは選択範囲の内容をクリアする。
11. When 利用者が通常モードで `r` に続けて文字を押す, the sheetgaze shall 現在のセルまたは選択範囲をその文字で置き換える。
12. When 利用者が通常モードで `d` に続けて `d` を押す, the sheetgaze shall 現在行の内容をクリアする（行構造は削除しない）。
13. When 利用者が通常モードで `d` に続けて移動キーを押す, the sheetgaze shall フォーカスから移動先までの矩形範囲の内容をクリアする。
14. The sheetgaze shall セル詳細表示を `K` または `Enter` で開けるようにし、`d` を削除操作に割り当てる。

### Requirement 2: 入力値の解釈

**Objective:** As a 表計算の利用者, I want 入力した文字列が Excel と同様の型で保存される, so that 数値・真偽値・数式をそのまま扱える。

#### Acceptance Criteria
1. When 入力が `=` で始まる, the sheetgaze shall 数式として扱い、先頭の `=` を保持した式をセルに設定する。
2. When 入力が `'` で始まる, the sheetgaze shall 先頭の `'` を除去し、残りをテキストとして扱う。
3. When 入力が `TRUE` または `FALSE`（大文字小文字を問わない）である, the sheetgaze shall 真偽値として扱う。
4. When 入力が有限の数値として解釈できる, the sheetgaze shall 数値として扱う。
5. When 入力が上記以外である, the sheetgaze shall テキストとして扱う。
6. The sheetgaze shall 編集対象セルの既存スタイルと数値書式を保持する。
7. When 入力が空である, the sheetgaze shall セル内容を空にし、スタイルと数値書式を保持する。
8. When 編集対象が結合セルの非アンカーである, the sheetgaze shall 結合範囲のアンカーを編集対象にする。

### Requirement 3: 編集トランザクションと undo / redo

**Objective:** As a 利用者, I want 編集をまとめて取り消したい, so that 誤った変更を安全に戻せる。

#### Acceptance Criteria
1. The sheetgaze shall 1 回の Insert 確定、1 回の範囲クリア、1 回の置換をそれぞれ 1 トランザクションとして記録する。
2. When 利用者が通常モードで `u` を押す, the sheetgaze shall 直前のトランザクションを逆順に適用して元の値へ戻す。
3. When 利用者が通常モードで `Ctrl-r` を押す, the sheetgaze shall 取り消したトランザクションを再適用する。
4. While 未保存の変更がある, the sheetgaze shall ステータス行に `[+]` を表示する。
5. The sheetgaze shall undo / redo の履歴上限を設け、上限を超えた古い履歴を破棄する。
6. When 保存が成功する, the sheetgaze shall 未保存表示を消し、保存前の undo / redo 履歴を破棄する。編集後のセル値はメモリ上に保持する。

### Requirement 4: 安全な xlsx 保存

**Objective:** As a 利用者, I want 編集したセルだけを既存の xlsx に反映したい, so that 書式・フィルタ・グラフなどを壊さずに保存できる。

#### Acceptance Criteria
1. When 利用者が `:w` を実行する, the sheetgaze shall 変更されたセルだけを対象シートの XML に反映し、他の zip エントリを保持する。
2. The sheetgaze shall 変更したテキストセルを `inlineStr` として書き、既存の `sharedStrings` の内容を変更しない。
3. The sheetgaze shall 数式セルに式と利用可能なキャッシュ値を書き、`fullCalcOnLoad` を設定する。
4. When 変更セルが既存のセルである, the sheetgaze shall 元のスタイル属性を保持する。
5. The sheetgaze shall 保存を一時ファイルへ行い、成功後にatomic renameで元ファイルを置き換える。
6. If ファイルが読み取り専用、CSV、共有数式・配列数式、または保存対象外である, then the sheetgaze shall 保存せず、状態行に理由を表示する。
7. When 保存中に新しい編集が発生する, the sheetgaze shall 保存対象のスナップショットと現在の編集を区別し、保存後に未保存状態を維持する。
8. If 保存に失敗する, then the sheetgaze shall 元ファイルを変更せず、未保存表示を維持する。
9. When 保存が成功する, the sheetgaze shall ファイル監視の基準スタンプを更新し、自分の保存による自動再読み込みを発生させない。

### Requirement 5: 置換

**Objective:** As a 利用者, I want 複数セルの文字列を一括で置換したい, so that 表全体の表記揺れを素早く修正できる。

#### Acceptance Criteria
1. When 利用者が `:s/old/new/[flags]` を実行する, the sheetgaze shall 現在セル（Visual 中は選択範囲）の表示値を対象に置換する。
2. When 利用者が `:%s/old/new/[flags]` を実行する, the sheetgaze shall 現在シートの可視セルを対象に置換する。
3. When 利用者が `:gs/old/new/[flags]` を実行する, the sheetgaze shall 全シートの可視セルを対象に置換する。
4. The sheetgaze shall 既定で literal・スマートケースで一致し、`g` でセル内の全出現、`i` / `I` で大文字小文字を上書きする。
5. The sheetgaze shall 既定で数式セルを置換対象から除外し、スキップ件数を表示する。
6. The sheetgaze shall フィルタで非表示の行を置換対象から除外する。
7. When 置換が 1 件以上適用される, the sheetgaze shall 置換全体を 1 トランザクションとして undo 可能にする。
8. The sheetgaze shall `n` フラグで件数のみを表示し、変更を適用しない。
9. The sheetgaze shall 置換後にレイアウト、フィルタ可視性、検索結果、数式再計算を更新する。

### Requirement 6: 数式再計算

**Objective:** As a 利用者, I want 編集した値が依存する数式へ反映されてほしい, so that 保存前に結果を確認できる。

#### Acceptance Criteria
1. When 編集確定後に数式セルが存在する, the sheetgaze shall 編集内容を含む一時 xlsx を作成し、既存の計算エンジンで再計算する。
2. When 再計算結果を受信する, the sheetgaze shall 現在の世代と一致する結果だけを表示へ反映する。
3. If 再計算ポリシーが無効、またはサイズ上限を超える, then the sheetgaze shall 再計算をスキップし、その理由を状態行に表示する。
4. When 新しい編集が再計算中に発生する, the sheetgaze shall 古い計算をキャンセルし、新しい編集内容で再計算する。
5. The sheetgaze shall 再計算結果を保存時の数式キャッシュ値として利用できる。

### Requirement 7: コマンドと保護

**Objective:** As a 利用者, I want 編集と保存を Vim 風のコマンドで制御したい, so that 操作を一貫させられる。

#### Acceptance Criteria
1. The sheetgaze shall `:w`、`:w!`、`:q`、`:q!`、`:wq`、`:x`、`:e`、`:e!`、`:goto`、`:sheet`、`:help` を解釈する。
2. When 未保存変更がある状態で `:q` を実行する, the sheetgaze shall 終了せず、未保存であることを表示する。
3. When 利用者が `:q!` を実行する, the sheetgaze shall 変更を破棄して終了する。
4. When 利用者が `ZZ` を押す, the sheetgaze shall 保存して終了する。`ZQ` は破棄して終了する。
5. If 起動時に `--read-only` が指定されている、またはファイル形式が CSV である, then the sheetgaze shall 編集・置換・保存を拒否し、理由を表示する。
6. When ファイルが外部で変更された後に保存しようとする, the sheetgaze shall `:w!` がない限り保存を拒否する。
