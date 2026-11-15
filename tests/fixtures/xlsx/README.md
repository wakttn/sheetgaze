# xlsx fixtures

`sheetgaze` の xlsx 読み込み・表示機能を検証するための fixture 集です。
すべて `tests/generate_fixtures.rs` から `rust_xlsxwriter 0.102` で生成しており、
Python や外部プロセスには依存しません。

## 再生成

```bash
cargo test --test generate_fixtures -- --ignored --nocapture
```

- 出力先: `tests/fixtures/xlsx/`
- 文書の作成日時を `2026-01-01` に固定しているため、再実行しても同じバイト列になります。
- 通常の `cargo test` では再生成されません。生成済みのバイナリをテストが読みます。

## 一覧

| ファイル | 目的 | 含まれる機能 | 期待される確認ポイント |
|---|---|---|---|
| `basic_types.xlsx` | 基本値型の読み込み | 文字列、整数、小数、真偽値、日付、空セル | 各値型が `CellValue` に変換される。日付は `DateTime`。空セルは空表示になる。 |
| `formulas.xlsx` | 数式とキャッシュ値 | 四則演算、`SUM`、クロスシート参照、`IF`、`=1/0`、循環参照、未対応関数、キャッシュ値 | `formula_cells` に 7 セル。数式文字列とキャッシュ値が読める。エラー式は `#DIV/0!`、未対応関数は `#NAME?`。 |
| `styles.xlsx` | セル書式 | 太字、赤文字、青文字、太字+赤、通常セルとの混在 | `CellStyle.bold` と `CellStyle.color` がセルごとに反映される。 |
| `borders.xlsx` | セル罫線 | 全辺の細線、太線、二重線、破線、色付き罫線、空セルの罫線、結合範囲の外枠、罫線によるはみ出し停止 | `CellStyle.borders` と `Sheet.merge_borders` が反映される。空セル `C2` も罫線を持つ。 |
| `merged_newlines.xlsx` | 結合セルと改行 | 単一行結合、複数行×複数列結合、複数列結合、改行入り文字列、全角、絵文字 | `MergeIndex` の範囲とアンカーが正しい。アンカー以外は結合として解決される。改行が保持される。 |
| `merged_tail.xlsx` | シート末尾の複数行結合 | 最終データ行より下まで伸びる結合範囲、折り返し長文 | `used_range` が結合範囲の終端まで拡張される。結合範囲の全行に文章が分配され、アンカー行が画面外でも表示中の行に文章が残る。 |
| `overflow.xlsx` | 長い文字列のはみ出し | 80 文字（はみ出しのみ）と 100 文字（切り詰め）の文字列、隣接セルが空/非空/結合のケース、折り返しセル | 隣接が空なら横にはみ出し、非空または結合なら切り詰めて `…` を表示する。列幅上限 65 の確認にも使える。 |
| `excel_layout.xlsx` | Excel の列幅・行高設定 | 明示列幅（24 / 6 / 12）、明示行高（30pt / 45pt / 18pt）、折り返し、未指定列への結合、シート既定行高 30pt | 明示列幅・行高が採用され、未指定列は自動計算される。明示行高 1 行の折り返しセルは切り詰められる。 |
| `freeze_rows.xlsx` | 行固定 | 先頭 2 行の frozen pane、100 行 × 100 列のデータ | `freeze.frozen_rows == 2`、`frozen_cols == 0`、固定が有効。下方向にスクロールしても先頭 2 行が残る。 |
| `freeze_both.xlsx` | 行と列の同時固定 | 1 行 + 1 列の frozen pane、100 行 × 100 列のデータ | `frozen_rows == 1`、`frozen_cols == 1`、固定が有効。上下左右にスクロールしても `row` 列と `C1` 行が残る。 |
| `freeze_merge_conflict.xlsx` | 固定の警告ケース | 固定境界（2 行目）と交差する結合セル `A2:B3`、100 行 × 100 列のデータ | 読み込みは成功する。固定は無効化され、`warnings` に結合境界の警告が入る。 |
| `filters_values.xlsx` | 値リストフィルタ | `Region` 列の East/West のみ表示、21 行（ヘッダー含む） | フィルタが読み込まれ、North/South の行が非表示になる。 |
| `filters_custom.xlsx` | カスタムフィルタ | `Sales > 10` の数値条件 | フィルタが読み込まれ、10 以下の行が非表示になる。 |
| `filters_table.xlsx` | テーブルの autoFilter | `A1:C16` のテーブル、既定の autoFilter | テーブル由来の autoFilter が `FilterSource::Table` として読めること。 |
| `filter_merge_conflict.xlsx` | フィルタの警告ケース | フィルタ範囲内に結合セル `A4:B5` | 読み込みは成功する。矛盾するフィルタは無効化され、`warnings` に警告が入る。 |
| `multi_sheet.xlsx` | 複数シート | `概要` / `データ` / `空シート`、日本語シート名、クロスシート数式 | 3 シートが順番どおり読める。日本語シート名が化けない。クロスシート参照のキャッシュ値が読める。 |
| `sparse.xlsx` | 疎なセル配置 | `A1`、`Z50`、`AA100` のみ | `used_range` が `A1:AA100` になり、離れたセルも読める。 |
| `special_chars.xlsx` | 特殊文字 | タブ、引用符、改行、日本語、絵文字、ゼロ幅スペース、複合文字列 | 値がそのまま保持される。コピー時の TSV 引用規則や表示幅計算の入力に使える。 |
| `large_lazy.xlsx` | 遅延ロード | 6,001 行 × 12 列（約 72,000 セル）、数値と文字列 | `--lazy always` で `SheetData::Lazy` になり、末尾の行も `ensure_rows` で読める。行キャッシュ上限の確認に使える。 |
| `empty_sheet.xlsx` | 空シート | セルを 1 つも書かないシート | シート数は 1。セルは空値として扱われ、クラッシュしない。 |
| `single_cell.xlsx` | 最小構成 | `A1` に `"only"` のみ | 1 セルだけでも読み込め、`used_range` が `A1` になる。 |
| `hidden_or_very_hidden.xlsx` | 非表示行・列 | 2 行目と 4 行目を非表示、B 列を非表示 | API で生成できることの確認用。現状の loader は行・列の非表示状態を保持しない。 |

## 補足

- freeze 系 fixture は 100 行 × 100 列のデータを持ち、1 行目は `C1`〜`C100` の列ラベル、1 列目は `R1`〜`R100` の行ラベル、それ以外は `R{row}C{col}` の値になっています。どの端末サイズでもスクロール後に固定行・固定列が残ることを確認できます。
- `filters_top10.xlsx` は生成していません。`rust_xlsxwriter 0.102` の
  `FilterCondition` に Top10 を表現する API がなく、生成後の XML 手加工は
  「rust_xlsxwriter のみを使う」制約に反するためです。Top10 の解析自体は
  アプリ側の `SheetMetadataParser` / `FilterEngine` に実装があります。
- `hidden_or_very_hidden.xlsx` の非表示状態は Excel の表示属性であり、
  現在の `sheetgaze` は読み込み時に使用しません。fixture は API 表現の確認用です。
- `excel_layout.xlsx` の明示行高はポイント単位です。端末行数への変換は
  15pt を基準に四捨五入するため、30pt は 2 行、45pt は 3 行、18pt は 1 行になります。
- `large_lazy.xlsx` の末尾行は、`prefetch_rows` が EOF を越えても
  `RowWindowCache` が行数でクランプするため読み込めます。
- `filters_table.xlsx` のテーブル autoFilter は自己終了タグも解析します。
- 警告ケース（`freeze_merge_conflict.xlsx`、`filter_merge_conflict.xlsx`）は
  どちらも「壊れた xlsx」ではなく、Excel の整合性検査に引っかかる組み合わせを
  意図的に作っています。読み込み自体は成功し、警告のみが出ることを想定しています。
