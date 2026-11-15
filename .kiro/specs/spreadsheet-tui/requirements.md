# Requirements Document

## Introduction

`sheetgaze` は、xlsx / csv ファイルをターミナル上で閲覧し、xlsx の数式計算結果を確認するための TUI ツールである。オフィススイートを起動できない環境でも、表の構造・値・書式・結合セルを素早く把握し、Excel に保存されたフィルタ、行固定、検索を使って目的の行へ到達し、必要なセルをコピーできることを目的とする。大容量ファイルでは行単位の遅延ロードを行い、メモリ使用量を抑える。本仕様は表示専用の閲覧機能を対象とし、セル編集・フィルタ条件の変更・ファイル保存は対象外とする。

## Boundary Context

- **In scope**: xlsx / csv の読み込み、csv の文字コード自動判定、xlsx 数式の再計算と表示、グリッド表示、改行・結合セル・文字色・太字・セル罫線の表示、罫線のないセルへのグリッド線表示、Excel の行固定（列固定を含む）の反映、列幅・行高の自動調整と上限設定、セル詳細表示、単一セル・範囲コピー、Vim 風キー操作と矢印キー操作、セル検索、Excel に保存されたオートフィルタの適用、大容量ファイルの行単位遅延ロード。
- **Out of scope**: セル編集・保存、フィルタ条件の新規作成・変更・並べ替え、行固定・列固定の新規作成・変更、分割ペイン（split pane）の再現、xls / xlsb / ods の読み込み、グラフ・画像・ピボットテーブル・マクロの実行、外部リンクの更新、クリップボードからの貼り付け、正規表現検索、シートをまたぐ検索、検索インデックスの永続化、Excel の書式を完全に再現する印刷・帳票出力。
- **Adjacent expectations**: 端末は ANSI カラーと UTF-8 を扱えるものとする。OS クリップボードが利用できない環境では OSC 52 を代替手段として用いる。数式計算は外部プロセスやネットワークに依存しない。Excel フィルタの条件と固定ペインは読み取り専用で利用し、元ファイルを変更しない。

## Requirements

### Requirement 1: ファイル読み込みと形式対応

**Objective:** As a ターミナル利用者, I want xlsx / csv ファイルを直接開ける, so that オフィススイートを起動せずに表の内容を確認できる。

#### Acceptance Criteria
1. When 利用者がファイルパスを指定して sheetgaze を起動する, the sheetgaze shall ファイル形式を判定し、対応形式であればワークブックを読み込んで初期画面を表示する。
2. Where 入力ファイルが .xlsx である, the sheetgaze shall シート名、セル値、数式、キャッシュ値、セルの太字・文字色・罫線、結合範囲を読み込む。罫線は値を持たない空セルと結合範囲の外枠についても読み込む。
3. Where 入力ファイルが .csv である, the sheetgaze shall RFC 4180 に従って引用符、引用符内の改行、UTF-8 BOM を処理し、単一シートとして読み込む。
4. If ファイルが存在しない、読み取り不能、未対応形式、または破損している, then the sheetgaze shall 対象パスと理由を含むエラーを表示し、端末状態を復元して非ゼロの終了コードで終了する。
5. When ワークブックが複数のシートを持つ, the sheetgaze shall シート一覧と現在のシートを表示し、利用者がシートを切り替えられるようにする。
6. The sheetgaze shall csv 内の数式に見える文字列を数式として評価せず、文字列として扱う。

### Requirement 2: グリッド表示とレイアウト

**Objective:** As a ターミナル利用者, I want 表の内容を崩れずに把握できる, so that セルの値や書式を素早く比較できる。

#### Acceptance Criteria
1. The sheetgaze shall 行番号・列番号のヘッダーを持つグリッドにセル値を表示し、画面外のセルを描画しない。
2. When セル値に改行が含まれる, the sheetgaze shall 改行を反映した複数行表示を行い、行高を設定された上限（既定 10 行）まで自動調整する。
3. The sheetgaze shall セル内容に応じて列幅を設定された上限（既定 65 表示桁）まで自動調整する。
4. When 単一行の内容がセル幅を超え、隣接セルが空かつ結合範囲でない, the sheetgaze shall 内容を隣接セルへはみ出して表示し、他のデータを上書きしない。ただし、はみ出し先の境界に明示的な罫線がある場合は、その境界で表示を止める。
5. If はみ出し先に値または結合範囲がある, then the sheetgaze shall 内容をセル幅で切り詰め、切り詰めを示す記号を表示する。
6. When セルが結合範囲に属する, the sheetgaze shall 左上セルの内容を結合範囲全体に表示し、結合範囲内の他セルを独立したセルとして描画・選択しない。
7. When セルに太字または文字色が設定されている, the sheetgaze shall 端末が対応する範囲で ANSI の太字・色属性として表示する。
8. While 端末のサイズが変更されている, the sheetgaze shall 表示領域を再計算し、フォーカス中のセルを画面内に保つ。
9. If 列幅・行高・結合範囲の制約により内容が表示しきれない, then the sheetgaze shall グリッドまたはセル詳細で切り詰めを判別できる表示を行う。
10. When セルに Excel の罫線が設定されている, the sheetgaze shall 罫線の辺と線種（細線・太線・二重線・破線など）を判別できる記号で表示する。縦罫線は専用の1表示列、横罫線はセル最下行の下線として表現する。
11. The sheetgaze shall 明示的な罫線を持たないセル境界にも、罫線と区別できる淡いグリッド線を表示する。
12. When 結合範囲に罫線が設定されている, the sheetgaze shall 結合範囲の外枠に罫線を表示し、結合範囲の内部には横方向のグリッド線を表示しない。

### Requirement 3: 数式計算

**Objective:** As a 表計算の利用者, I want 数式の計算結果を確認できる, so that オフィススイートなしで値の整合性を検証できる。

#### Acceptance Criteria
1. When xlsx に数式セルが含まれる, the sheetgaze shall Excel 互換の計算エンジンで数式を評価し、計算結果を表示する。
2. While 数式の再計算が完了していない, the sheetgaze shall 読み込んだキャッシュ値があればそれを表示し、計算中であることを状態表示する。
3. When 数式が他のシート、セル範囲、または計算エンジンが対応する関数を参照する, the sheetgaze shall 参照を解決して計算する。
4. If 数式の評価が失敗する、未対応関数を含む、循環参照になる、または外部リンクを参照する, then the sheetgaze shall Excel のエラー値またはキャッシュ値を表示し、評価状態を判別できるようにする。
5. When 利用者が数式セルの詳細を開く, the sheetgaze shall 数式文字列、キャッシュ値、計算値、評価状態を表示する。
6. While 再計算が実行されている, the sheetgaze shall 閲覧・移動・コピー・終了の操作を妨げない。
7. When 再計算が完了する, the sheetgaze shall フォーカス位置と選択範囲を維持したまま表示値を更新する。
8. If 計算エンジンがワークブックを読み込めない, then the sheetgaze shall キャッシュ値による表示を継続し、再計算に失敗したことを状態表示する。
9. Where 再計算ポリシーが auto であり、lazy シートのサイズが設定上限を超える, the sheetgaze shall 再計算をスキップしてキャッシュ値を表示し、スキップ理由を状態表示する。

### Requirement 4: ナビゲーションと選択

**Objective:** As a Vim に慣れた利用者, I want キーボードだけで表を巡回・選択できる, so that マウスなしで効率的に目的のセルへ到達できる。

#### Acceptance Criteria
1. The sheetgaze shall h / j / k / l キーおよび上下左右の矢印キーでフォーカスを 1 セル単位で移動する。
2. When フォーカスが結合範囲にある, the sheetgaze shall 結合範囲を 1 つの論理セルとして移動し、範囲外への移動時は結合範囲の外側の隣接セルへ移動する。
3. The sheetgaze shall gg / G / 0 / $ / Ctrl-d / Ctrl-u / PageUp / PageDown のキー操作を提供する。
4. When 利用者が v キーを押す, the sheetgaze shall 矩形の範囲選択を開始し、移動キーで選択範囲を拡張する。
5. When 利用者が V キーを押す, the sheetgaze shall 現在行を含む行範囲の選択を開始し、上下移動で行範囲を拡張する。
6. When 利用者が Esc キーを押す, the sheetgaze shall 範囲選択を解除し、通常モードへ戻る。
7. While フォーカスが表示領域の外へ移動する, the sheetgaze shall 必要最小限のスクロールでフォーカスを画面内に戻す。
8. When 利用者が ? キーを押す, the sheetgaze shall キーバインド一覧を表示する。
9. When 利用者が q キーを押す, the sheetgaze shall 端末状態を復元して終了する。

### Requirement 5: セル詳細表示

**Objective:** As a 表計算の利用者, I want セルの完全な内容を確認できる, so that 切り詰められた値や数式の詳細を正確に把握できる。

#### Acceptance Criteria
1. When 利用者がフォーカス中のセルの詳細表示を要求する, the sheetgaze shall セルアドレス、値の種類、生の値、表示値、数式、キャッシュ値、計算値、結合範囲、太字・文字色、切り詰めの有無を表示する。
2. When セル値が長い、または改行を含む, the sheetgaze shall 詳細表示内でスクロールでき、改行を保持したまま全文を表示する。
3. When 利用者が詳細表示を閉じる, the sheetgaze shall 元のグリッド位置とモードへ戻る。
4. While 詳細表示が開いている, the sheetgaze shall 上下キーおよび j / k キーで内容をスクロールできる。

### Requirement 6: コピー

**Objective:** As a ターミナル利用者, I want セルの内容を他のアプリケーションへ渡せる, so that 表の値を再利用できる。

#### Acceptance Criteria
1. When 利用者が単一セルのコピーを要求する, the sheetgaze shall 表示値を改行を含めてそのままクリップボードへ書き込む。
2. When 利用者が範囲選択のコピーを要求する, the sheetgaze shall 選択範囲を行・列構造を保った TSV としてクリップボードへ書き込む。
3. When コピー対象のフィールドにタブ、改行、またはダブルクォートが含まれる, the sheetgaze shall TSV のフィールドをダブルクォートで囲み、内部のダブルクォートを二重化する。
4. When 選択範囲に結合セルが含まれる, the sheetgaze shall 左上セルの値を 1 回だけ出力し、結合範囲内の他のセルは空フィールドとして出力する。
5. If クリップボードへの書き込みに失敗する, then the sheetgaze shall 原因と代替手段を含むエラーを表示し、アプリケーションの操作を継続可能にする。
6. When コピーが成功する, the sheetgaze shall コピーしたセル数または文字数を状態表示する。
7. While セル詳細表示が開いている, the sheetgaze shall コピー要求に対して詳細表示中の全文をクリップボードへ書き込む。
8. If コピー対象が設定されたセル数上限または OSC 52 のペイロード上限を超える, then the sheetgaze shall コピーを中止し、範囲を狭めるか別のクリップボード手段を使うよう案内する。

### Requirement 7: 設定と起動

**Objective:** As a ターミナル利用者, I want 表示上限や動作を起動時に調整できる, so that 端末や用途に合わせて表示を最適化できる。

#### Acceptance Criteria
1. The sheetgaze shall ファイルパスに加えて、最大列幅（既定 65）、最大行数（既定 10）、初期シート、カラーモード、クリップボードモード、文字コード、遅延ロード設定、再計算ポリシー、コピー上限を CLI オプションで指定できる。
2. When 利用者が --help または --version を指定する, the sheetgaze shall 使用方法またはバージョンを表示して終了する。
3. If 不正なオプション値が指定される, then the sheetgaze shall 該当オプションと許容範囲を含む使用法エラーを表示し、非ゼロの終了コードで終了する。
4. The sheetgaze shall 設定ファイルなしでも既定値で動作する。
5. Where --no-recalc が指定される, the sheetgaze shall 数式の再計算を実行せず、読み込んだキャッシュ値のみを表示する。
6. The sheetgaze shall --recalc auto|always|never を提供し、lazy シートの再計算可否を制御できる。

### Requirement 8: 非機能品質

**Objective:** As a ターミナル利用者, I want 安全で予測可能な動作を得る, so that 大きなファイルや特殊な端末環境でも安心して使える。

#### Acceptance Criteria
1. The sheetgaze shall Rust で実装され、コア機能の実行に外部オフィススイート、外部プロセス、非 Rust ランタイムを必要としない。
2. When 10 MiB 以下かつ 10 万セル以下の xlsx を読み込む, the sheetgaze shall 参照環境で 2 秒以内に初期画面を表示し、数式再計算をバックグラウンドで開始する。
3. The sheetgaze shall 画面に表示される範囲のセルのみを描画し、スクロール時の再描画コストを表示領域に比例させる。
4. When 端末サイズが 40 桁 × 10 行以上である, the sheetgaze shall パニックせずに表示する。
5. If 端末サイズが最小サイズ未満である, then the sheetgaze shall 最小サイズを求めるメッセージを表示する。
6. The sheetgaze shall 正常終了、エラー終了、パニックのいずれの場合も端末状態を復元する。
7. The sheetgaze shall 読み込み・数式計算中にマクロ、VBA、外部リンクの更新、ネットワークアクセスを実行しない。
8. The sheetgaze shall UTF-8 の全角文字、結合文字、絵文字を含むセル値の表示幅を正しく計算し、列境界を破壊しない。
9. If セル値や数式に不正なデータが含まれる, then the sheetgaze shall 該当セルをエラー表示または空値として扱い、アプリケーション全体をクラッシュさせない。

### Requirement 9: 検索

**Objective:** As a 表の利用者, I want セルの内容を検索して一致間を移動できる, so that 大きな表から目的のデータを素早く見つけられる。

#### Acceptance Criteria
1. When 利用者が / キーを押す, the sheetgaze shall 検索語入力を受け付ける検索プロンプトを表示する。
2. When 検索語が確定される, the sheetgaze shall 現在のシートの表示値と数式文字列を対象にリテラル部分一致検索を実行する。
3. The sheetgaze shall 既定で大文字小文字を区別せず、検索語に大文字が含まれる場合は区別するスマートケース検索を行う。
4. When 検索が完了する, the sheetgaze shall 最初の一致セルへ移動し、一致件数と現在位置を状態表示する。
5. When 利用者が n または N を押す, the sheetgaze shall 次の一致または前の一致へ移動し、末尾・先頭で折り返す。
6. While 検索が実行中, the sheetgaze shall 進捗を表示し、移動・コピー・終了の操作を妨げない。
7. When フィルタが適用されている, the sheetgaze shall 検索結果の移動時に非表示行の一致をスキップし、非表示の一致件数を状態表示する。
8. When 検索結果の一致セルが表示領域内にある, the sheetgaze shall 一致セルを視覚的に強調表示する。
9. If 一致がない, then the sheetgaze shall その旨を状態表示し、フォーカスを変更しない。
10. When 利用者が Esc を押す、またはシートを切り替える, the sheetgaze shall 検索プロンプトまたは検索結果を解除する。

### Requirement 10: Excel フィルタの利用

**Objective:** As a 表の利用者, I want Excel で設定済みのフィルタをそのまま使える, so that 元ファイルを変更せずに必要な行だけを表示できる。

#### Acceptance Criteria
1. When xlsx にワークシートまたはテーブルのオートフィルタが保存されている, the sheetgaze shall フィルタ範囲、列、条件、適用状態を読み込む。
2. The sheetgaze shall 保存された条件を既定で適用し、非表示行を除外した表示と元の行番号を提供する。
3. The sheetgaze shall 値リスト、カスタム条件（AND / OR、比較演算子、ワイルドカード）、Top10 の条件を適用する。
4. If 色・アイコン・動的フィルタなど未対応の条件が含まれる, then the sheetgaze shall 該当列を未適用として扱い、警告を表示する。
5. When 利用者が f キーを押す, the sheetgaze shall フィルタパネルを表示し、列ごとの条件と適用状態、表示行数を確認できるようにする。
6. When 利用者がフィルタパネルで切り替えを行う, the sheetgaze shall 全体または列単位の適用・解除を反映し、ファイルを変更しない。
7. Where --no-filter が指定される, the sheetgaze shall 保存された条件を適用せず全行を表示する。
8. While フィルタが適用されている, the sheetgaze shall ナビゲーション、検索、コピーを可視行のみに適用する。
9. If フィルタ範囲が結合セルやシート範囲と矛盾する, then the sheetgaze shall 該当フィルタを無効化し、警告を表示して読み込みを継続する。
10. The sheetgaze shall csv に対して Excel フィルタを適用せず、フィルタパネルに定義がないことを表示する。

### Requirement 11: CSV の文字コード判定

**Objective:** As a 日本語環境の利用者, I want csv の文字コードを自動判定できる, so that Shift_JIS などのファイルも文字化けせずに閲覧できる。

#### Acceptance Criteria
1. When csv を開く, the sheetgaze shall BOM を優先し、次にファイル先頭のサンプルから文字コードを自動判定する。
2. The sheetgaze shall UTF-8、UTF-16LE、UTF-16BE、Shift_JIS、CP932、EUC-JP を判定・デコード対象とする。
3. When 自動判定結果を採用する, the sheetgaze shall 判定した文字コードを状態表示またはファイル情報に表示する。
4. Where --encoding が指定される, the sheetgaze shall 自動判定を上書きし、指定された文字コードでデコードする。
5. If 指定された文字コードが不正、またはデコードに失敗する, then the sheetgaze shall エラーと --encoding の案内を表示し、非ゼロの終了コードで終了する。
6. If 自動判定が曖昧、または不正バイトを検出する, then the sheetgaze shall 警告を表示し、置換文字で読み込みを継続するか、利用者に --encoding の指定を促す。
7. While 大容量 csv を読み込む, the sheetgaze shall ストリーミングデコードを行い、ファイル全体の UTF-8 文字列をメモリに保持しない。

### Requirement 12: 大容量データの遅延ロード

**Objective:** As a 大規模データの利用者, I want 必要な行だけを読み込める, so that 数万〜数百万行のファイルでもメモリを抑えて閲覧できる。

#### Acceptance Criteria
1. When ファイルのサイズまたは使用セル数が設定された閾値を超える, the sheetgaze shall 遅延モードを選択する。
2. While 遅延モードである, the sheetgaze shall シートのメタデータと行インデックスを先に構築し、セル本体は表示に必要な行だけ読み込む。
3. The sheetgaze shall 読み込み済み行を上限付きキャッシュで管理し、上限を超えた行を解放する。
4. When 利用者が未読み込みの行へ移動する, the sheetgaze shall 対象行を読み込み、読み込み中も UI をブロックしない。
5. The sheetgaze shall 遅延モードでも列幅・行高の自動調整、結合セル、検索、フィルタ、コピー、詳細表示を提供する。
6. The sheetgaze shall 遅延モードの閾値、行キャッシュ上限、先読み行数を CLI で設定できる。
7. While 遅延インデックスを構築中, the sheetgaze shall 進捗を表示し、中断・終了要求に応答する。
8. If 遅延モードのインデックス構築または行読み込みに失敗する, then the sheetgaze shall 該当シートを読み込みエラーとして表示し、他シートの閲覧を継続する。
9. When 数式再計算が遅延モードで完了する, the sheetgaze shall 計算結果をオーバーレイとして保持し、未読み込み行でも表示時に反映する。
10. The sheetgaze shall 遅延モードのセル本体のメモリ使用量をキャッシュ済み行数に比例させ、行オフセット・行高・可視マスクなどのインデックスを最小限のメタデータに留める。

### Requirement 13: 行固定（フリーズペイン）

**Objective:** As a 表の利用者, I want Excel で固定された行をスクロール中も表示できる, so that 見出し行を参照しながらデータを閲覧できる。

#### Acceptance Criteria
1. When xlsx のワークシートに固定ペイン（state=frozen、ySplit > 0）が保存されている, the sheetgaze shall 固定行数と固定列数を読み込む。
2. While 利用者が縦方向にスクロールする, the sheetgaze shall 固定行をグリッド上部に残し、固定行より下の行だけをスクロールさせる。
3. Where 行と列の両方が固定されている, the sheetgaze shall 固定行と固定列を同時に表示し、交差部分を二重に描画しない。
4. The sheetgaze shall 固定行・固定列を通常のセルと同じようにナビゲーション、検索、選択、コピー、詳細表示の対象にする。
5. If 固定領域が表示領域を超える, then the sheetgaze shall 固定領域をスクロール可能な行・列が残る大きさに制限し、制限されたことを状態表示する。
6. If 固定境界が結合セルと交差する、ペインが split 状態である、または固定行数・列数が不正である, then the sheetgaze shall 該当シートの固定を無効化して警告を表示し、読み込みを継続する。
7. When 端末サイズが変更される, the sheetgaze shall 固定領域とスクロール領域を再計算し、フォーカス中のセルを画面内に保つ。
8. While フィルタが適用されている, the sheetgaze shall 固定行を常に表示し、フィルタによる非表示の対象を固定行より下のデータ行に限定する。
9. The sheetgaze shall 固定行数・固定列数をステータスバーまたはシート情報に表示する。
10. The sheetgaze shall csv に対して行固定・列固定を適用しない。
11. When 固定行とスクロール行にまたがる範囲をコピーする, the sheetgaze shall シート上の行順で重複なく出力する。
