# Requirements Document

## Introduction

sheetgaze は xlsx / csv をターミナルで閲覧し、xlsx のセル値と数式を Vim 風のキー操作で編集できる TUI ツールである。本機能は、行・列の構造編集、列選択と矩形選択、Vim の `clipboard=unnamedplus` に相当するヤンク／切り取り／ペースト、システムクリップボード連携を追加する。

既存のセル編集、検索、フィルタ、遅延ロード、安全な xlsx 保存を壊さず、構造変更を xlsx へ保存できることを目的とする。Excel の全機能を完全再現することは目的とせず、安全に変換できない構造は保存を拒否してデータ破損を防ぐ。

## Boundary Context

- **In scope**: 行・列の挿入と構造削除、行・列・矩形選択、ヤンク／切り取り／ペースト、内部レジスタとシステムクリップボードの連携、構造変更の xlsx 保存、構造変更の undo / redo、関連する Ex コマンドとヘルプ。
- **Out of scope**: CSV への書き戻し、xlsx 以外の形式変換、マクロ・画像・グラフ・ピボット・VBA の編集、OSC 52 によるクリップボード読み取り、クリップボード画像、永続レジスタ、複数範囲選択、行・列の非表示やサイズ変更そのもの、非 Mono モードの既定配色。
- **Adjacent expectations**: 既存のセル編集・検索・フィルタ・固定ペイン・フィット幅・ファイル監視・再読み込みの挙動を維持する。クリップボードの転送方式は既存の `--clipboard` 設定に従う。非 Mono モードの既定配色は別 spec `non-mono-theme` が所有する。

## Requirements

### Requirement 1: 選択モード

**Objective:** As a terminal spreadsheet user, I want 行・列・矩形を選択できる, so that 構造編集やヤンクの対象を正確に指定できる。

#### Acceptance Criteria

1. When ユーザーが `V` を押した, the sheetgaze shall 現在行を起点とし、使用範囲の全列を対象とする行選択を開始する。
2. When ユーザーが `C` を押した, the sheetgaze shall 現在列を起点とし、使用範囲の全行を対象とする列選択を開始する。
3. When ユーザーが `Ctrl-V` を押した, the sheetgaze shall 現在セルを起点とする矩形選択を開始する。
4. While 行・列・矩形選択のいずれかが有効である, when ユーザーが移動キーを押した, the sheetgaze shall 選択形状を維持したまま選択範囲を拡張する。
5. When ユーザーが `Esc` を押した, the sheetgaze shall 選択を解除して通常モードへ戻る。
6. While 選択が有効である, the sheetgaze shall ステータス行に選択種別と選択セル数を表示する。
7. When ユーザーが `v` を押した, the sheetgaze shall 既存のセル範囲選択を開始する。
8. While 列選択が有効である, the sheetgaze shall 対象列を使用範囲の全行にわたって強調表示する。

### Requirement 2: ヤンク・切り取り・レジスタ

**Objective:** As a Vim user, I want `clipboard=unnamedplus` に近いクリップボード操作を行う, so that 表の一部を別の場所や外部アプリへ素早く移動できる。

#### Acceptance Criteria

1. When ユーザーが選択範囲をヤンクした, the sheetgaze shall 対象セルを内部の名前なしレジスタへ保存し、システムクリップボードが利用可能なら TSV テキストを書き込む。
2. When ユーザーがセルまたは矩形選択を切り取った, the sheetgaze shall 対象セルを名前なしレジスタへ保存し、行・列構造を変えずに内容をクリアする。
3. When ユーザーが行選択または列選択を切り取った, the sheetgaze shall 対象行または対象列を名前なしレジスタへ保存し、対象を構造ごと削除する。
4. The sheetgaze shall `yy` と `Y` で現在行をヤンクし、`dd` で現在行を切り取り、`V` と `d` で選択行を切り取り、`C` と `d` で選択列を切り取る。
5. When `--clipboard-register unnamedplus` が有効な状態でユーザーが `p` または `P` を押した, the sheetgaze shall システムクリップボードの読み取りを試み、読み取り不能な場合は内部の名前なしレジスタへフォールバックする。
6. When システムクリップボードの内容が直前の sheetgaze の書き込みと一致する, the sheetgaze shall 値・数式・書式を保持するため内部レジスタのリッチなセル情報を優先して貼り付ける。
7. When ユーザーが `"+p` を押した, the sheetgaze shall システムクリップボードのみから貼り付け、読み取り不能時はエラーを表示する。
8. When ユーザーが `"0p` を押した, the sheetgaze shall 内部のヤンク専用レジスタから貼り付ける。
9. If システムクリップボードへの書き込みに失敗した, then the sheetgaze shall 内部レジスタを保持したままステータス行に警告を表示する。
10. The sheetgaze shall TSV のタブ・改行・引用符・空フィールドを正しく読み書きする。
11. The sheetgaze shall `--clipboard-register` で `unnamedplus` と `internal` を選択でき、既定値を `unnamedplus` とする。

### Requirement 3: 行・列の挿入

**Objective:** As a spreadsheet user, I want 任意の位置に行や列を挿入できる, so that 既存データを壊さずに表を拡張できる。

#### Acceptance Criteria

1. When ユーザーが通常モードで `o` を押した, the sheetgaze shall 現在行の下に空行を挿入し、現在列で Insert モードを開始する。
2. When ユーザーが通常モードで `O` を押した, the sheetgaze shall 現在行の上に空行を挿入し、現在列で Insert モードを開始する。
3. When ユーザーが通常モードで `gA` を押した, the sheetgaze shall 現在列の右に空列を挿入し、現在行で Insert モードを開始する。
4. When ユーザーが通常モードで `gI` を押した, the sheetgaze shall 現在列の左に空列を挿入し、現在行で Insert モードを開始する。
5. When ユーザーが `:irow [count]` または `:irow! [count]` を実行した, the sheetgaze shall 指定件数（省略時 1）の行を現在行の後または前に挿入する。
6. When ユーザーが `:icol [count]` または `:icol! [count]` を実行した, the sheetgaze shall 指定件数（省略時 1）の列を現在列の右または左に挿入する。
7. When 行または列が挿入された, the sheetgaze shall 既存セル、数式参照、結合セル、フィルタ範囲、列幅・行高、固定ペイン、条件付き書式を挿入位置に合わせて移動または拡張する。
8. If 挿入件数が 1 以上 1000 以下でない、または挿入位置がシート範囲外である, then the sheetgaze shall 操作を実行せず、ステータス行に有効範囲を表示する。
9. If ワークブックが読み取り専用または CSV である, then the sheetgaze shall 行・列の挿入を拒否し、理由を表示する。
10. When 構造変更を含むワークブックを保存した, the sheetgaze shall 挿入された行・列を xlsx に反映する。

### Requirement 4: ペースト

**Objective:** As a spreadsheet user, I want ヤンクまたは切り取った内容を指定位置へ貼り付ける, so that 行・列・セル範囲を移動または複製できる。

#### Acceptance Criteria

1. When 名前なしレジスタが行データを保持している状態でユーザーが `p` を押した, the sheetgaze shall 現在行の下にレジスタの行数を挿入し、レジスタ内容を書き込む。
2. When 名前なしレジスタが行データを保持している状態でユーザーが `P` を押した, the sheetgaze shall 現在行の上にレジスタの行数を挿入し、レジスタ内容を書き込む。
3. When 名前なしレジスタが列データを保持している状態でユーザーが `p` を押した, the sheetgaze shall 現在列の右にレジスタの列数を挿入し、レジスタ内容を書き込む。
4. When 名前なしレジスタが列データを保持している状態でユーザーが `P` を押した, the sheetgaze shall 現在列の左にレジスタの列数を挿入し、レジスタ内容を書き込む。
5. When 名前なしレジスタがセルまたは矩形データを保持している状態でユーザーが `p` または `P` を押した, the sheetgaze shall 現在セルを左上として上書きし、必要なら使用範囲を拡張する。
6. While 矩形データを貼り付けている, the sheetgaze shall レジスタ内の空セルで貼り付け先のセル内容をクリアする。
7. When ペーストが実行された, the sheetgaze shall 構造挿入とセル書き込みを 1 回の undo トランザクションとして記録する。
8. If レジスタが空である, then the sheetgaze shall 何も変更せず、ステータス行に「貼り付ける内容がない」旨を表示する。
9. If 貼り付け先が結合セルの被覆セルに重なる, then the sheetgaze shall 結合アンカー以外への書き込みをスキップし、ステータス行に警告を表示する。
10. When リッチなレジスタを貼り付けた, the sheetgaze shall セル値、数式、書式、表示形式を保持する。
11. When システムクリップボードの TSV テキストを貼り付けた, the sheetgaze shall 各フィールドをセル入力規則に従って値・数値・真偽値・数式として解釈する。

### Requirement 5: 構造変更の xlsx 保存

**Objective:** As a spreadsheet user, I want 行・列の構造変更を xlsx に保存できる, so that 編集結果を Excel でも利用できる。

#### Acceptance Criteria

1. When 構造変更を含むワークブックを保存した, the sheetgaze shall 変更内容を xlsx に反映し、失敗時には元ファイルを部分的に破損させない。
2. When 構造変更を保存した, the sheetgaze shall 値、数式、セル書式、表示形式、結合セル、オートフィルタ、固定ペイン、列幅、行高、条件付き書式、使用範囲を保持または変換する。
3. When 行または列の挿入・削除を保存した, the sheetgaze shall 同一シート内の数式参照を移動後のセルへ更新し、絶対参照・相対参照・範囲参照・行全体・列全体の参照を扱う。
4. When 行または列の挿入・削除を保存した, the sheetgaze shall 他シートから対象シートへの数式参照と、対象シートを参照する定義名を更新する。
5. If 共有数式、配列数式、テーブル列数式など安全に変換できない構造が対象範囲に含まれる, then the sheetgaze shall 保存を拒否し、未保存変更を保持したまま対象と理由を表示する。
6. If ワークブックが xlsx 以外または読み取り専用である, then the sheetgaze shall 構造変更の保存を拒否する。
7. When 保存が成功した, the sheetgaze shall ファイル監視の基準情報を更新し、未保存状態を解除する。
8. When 未保存の構造変更から数式を再計算した, the sheetgaze shall 現在の構造を反映した数式結果を表示する。
9. If 保存処理が途中で失敗した, then the sheetgaze shall 元ファイルを変更せず、未保存状態を維持する。
10. If 構造変更の保存前に外部でファイルが変更されている, then the sheetgaze shall 保存を拒否し、再読み込みまたは破棄を促す。

### Requirement 6: Undo / Redo と Dirty 管理

**Objective:** As a spreadsheet user, I want 構造編集をやり直せる, so that 誤操作から安全に復帰できる。

#### Acceptance Criteria

1. The sheetgaze shall 行・列の挿入、構造削除、切り取り、ペーストをそれぞれ 1 回の undo トランザクションとして記録する。
2. When ユーザーが構造変更を undo した, the sheetgaze shall セル、行・列構造、結合セル、フィルタ、列幅・行高、固定ペイン、条件付き書式、数式参照を変更前の状態へ戻す。
3. When ユーザーが構造変更を redo した, the sheetgaze shall undo 前の状態を再適用する。
4. While 未保存の構造変更がある, the sheetgaze shall dirty 状態を表示し、`q` や `:q` で終了しようとした場合は保存または破棄を促す。
5. When 保存が成功した, the sheetgaze shall 既存のセル編集と同様に undo / redo 履歴をリセットする。
6. When undo または redo が実行された, the sheetgaze shall ステータス行に操作種別と対象件数を表示する。

### Requirement 7: コマンド・キーバインド・ドキュメント

**Objective:** As a Vim user, I want 一貫したキーと Ex コマンドを使う, so that 操作を覚えやすく再現しやすい。

#### Acceptance Criteria

1. The sheetgaze shall `v`、`V`、`C`、`Ctrl-V` でセル・行・列・矩形選択を開始できる。
2. The sheetgaze shall `y`、`yy`、`Y`、`x`、`dd`、`p`、`P`、`o`、`O`、`gI`、`gA` を本要件の操作に割り当てる。
3. The sheetgaze shall `:irow`、`:irow!`、`:icol`、`:icol!`、`:drow`、`:drow!`、`:dcol`、`:dcol!`、`:put`、`:put!` を提供する。
4. When ユーザーが `?` を押した, the sheetgaze shall ヘルプに新しい選択・切り取り・挿入・ペースト・レジスタのキーを表示する。
5. The sheetgaze shall README に新しいキー、`--clipboard-register`、`--structural-max-cells`、既知の制限を記載する。
6. When 不明な Ex コマンドまたは不正な件数が入力された, the sheetgaze shall 状態を変更せず、ステータス行にエラーを表示する。
7. The sheetgaze shall `dd` が行内容のクリアから行の構造切り取りへ変わったことを README とヘルプに明記する。
8. The sheetgaze shall `"` プレフィックスで `"0` と `"+` のレジスタを指定できる。
9. When ユーザーが `x` を押した, the sheetgaze shall セル・矩形選択では内容をクリアして名前なしレジスタへ保存し、行・列選択では対象を構造ごと切り取る。
10. The sheetgaze shall `:drow [count]` と `:dcol [count]` で現在行・列を含めて後方へ指定件数（省略時 1）を構造削除し、`:drow! [count]` と `:dcol! [count]` で現在行・列の前方へ指定件数（省略時 1）を構造削除する。
11. The sheetgaze shall `:put [register]` で現在位置の後ろへ、`:put! [register]` で現在位置の前へ貼り付け、レジスタ省略時は名前なしレジスタ、`0` はヤンク専用、`+` はシステムクリップボードを使用する。
12. If `:drow` / `:dcol` の対象範囲がシート範囲を超える、または `:put` のレジスタが空もしくは不正である, then the sheetgaze shall ワークブックを変更せず、ステータス行にエラーを表示する。

### Requirement 8: 遅延ロードとサイズ制限

**Objective:** As a user of large workbooks, I want 構造編集時だけ必要な範囲を読み込む, so that メモリ使用量を制御できる。

#### Acceptance Criteria

1. When 遅延ロード中のシートに最初の構造編集を適用した, the sheetgaze shall 対象シートの全セルに対して構造編集を一貫して適用する。
2. The sheetgaze shall `--structural-max-cells` で構造編集時に許容する最大セル数を設定できる。
3. If 対象シートのセル数が上限を超える, then the sheetgaze shall 構造編集を実行せず、シートサイズと上限を表示する。
4. While 構造編集を行っていない, the sheetgaze shall 既存の遅延ロードと行キャッシュを維持する。
5. If ヤンクまたはコピー対象が `--max-copy-cells` を超える, then the sheetgaze shall クリップボード書き込みを中断し、上限を表示する。

### Requirement 9: 表示・検索・フィルタの一貫性

**Objective:** As a spreadsheet user, I want 構造編集後も表示と検索が矛盾しない, so that 編集後も同じ操作感で調査を続けられる。

#### Acceptance Criteria

1. When 構造変更が適用された, the sheetgaze shall 既存の検索索引を無効化し、次回検索時に再構築する。
2. When 構造変更が適用された, the sheetgaze shall カーソルと選択範囲を新しい使用範囲へクランプする。
3. When 構造変更が適用された, the sheetgaze shall カーソルが見える位置までビューポートを調整する。
4. When 構造変更が適用された, the sheetgaze shall フィルタマスクと可視行を再評価する。
5. When 構造変更が適用された, the sheetgaze shall 数式の再計算を要求する。
6. While 未保存変更がある, the sheetgaze shall 外部ファイル変更の自動再読み込みを抑止し、既存の警告を維持する。
7. When 構造変更が適用された, the sheetgaze shall ジャンプ履歴のうち無効になったセル参照を破棄またはクランプする。

### Requirement 10: エラー処理と保護

**Objective:** As a user, I want 危険な操作が data loss を起こさない, so that 安心して編集できる。

#### Acceptance Criteria

1. If 読み取り専用または CSV のワークブックで構造編集を要求した, then the sheetgaze shall 操作を拒否し、理由を表示する。
2. If システムクリップボードの読み取りが利用できない, then the sheetgaze shall `unnamedplus` では内部レジスタへフォールバックし、`"+p` ではエラーを表示する。
3. If 構造変更の保存で未対応の OOXML 構造を検出した, then the sheetgaze shall 保存を拒否し、未保存変更を保持する。
4. If 結合セルまたはフィルタ範囲と構造操作が衝突した, then the sheetgaze shall 可能な範囲で変換し、スキップまたは削除した項目をステータス行に警告する。
5. If 保存処理のいずれかの段階でエラーが発生した, then the sheetgaze shall 元ファイルへ部分的な書き込みを行わない。
6. If 空のレジスタ、範囲外のセル、0 件の削除など無効な操作が要求された, then the sheetgaze shall ワークブックを変更せず、状態を維持する。
