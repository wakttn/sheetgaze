# Implementation Plan

- [x] 1. ドメインの Excel レイアウトモデルと変換規則を実装する
- [x] 1.1 `ExcelLayout` と `LayoutMode`、列幅・行高の変換関数を追加する
  - Excel の列幅（文字数）を切り捨てで端末桁へ変換し、最小・最大でクランプする
  - Excel の行高（ポイント）を基準行高 15pt で除算して四捨五入し、1 行以上・最大行数以下にクランプする
  - 既定列幅・既定行高を保持できるようにする
  - _Requirements: 3.1, 3.2, 3.3, 4.1, 4.2, 4.3, 4.4_

- [x] 1.2 `LayoutStatsBuilder::finish` に Excel 明示値の優先規則を組み込む
  - 自動列幅 → Excel 明示列幅 → 結合幅の未指定列分配 → wrap 行高 → Excel 明示行高の順に確定する
  - 明示列幅・行高は自動計算で上書きせず、未指定のみ自動計算する
  - 既定列幅・行高は未指定列・行の下限として適用する
  - 結合セルの幅要求を Excel 明示列へ分配しない
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 2.1, 2.2, 2.3, 2.4, 2.5, 3.4, 3.5, 5.1, 5.2_

- [x] 1.3 `LayoutSnapshot` に固定列・行を保持し、`grow_for_cell` を更新する
  - Excel 明示値に由来する列・行を `fixed_columns` / `fixed_rows` に記録する
  - 数式再計算後の grow-only 更新で固定列・行を拡大しない
  - 自動計算対象の列・行だけが必要に応じて拡大する
  - _Requirements: 7.1, 7.2, 7.3, 7.4_

- [x] 2. OOXML から Excel レイアウト情報を読み取る
- [x] 2.1 `SheetMetadataParser` で `<sheetFormatPr>` / `<cols>` / `<col>` / `<row>` を解析する
  - `defaultColWidth` / `defaultRowHeight` を取得する
  - `customWidth` が真の `<col width>` を 0 始まり列インデックスへ展開する
  - `customHeight` が真の `<row ht>` を 0 始まり行インデックスへ変換する
  - 同一列・行の重複定義は後勝ちにする
  - _Requirements: 1.1, 1.2, 2.1, 2.2, 3.1, 3.2, 8.2_

- [x] 2.2 不正なレイアウト属性を安全に無視する
  - 数値変換できない値、負値、非有限値を除外する
  - 範囲指定の列数に上限を設け、異常に広い指定で過大なメモリを使わない
  - 解析失敗時も既存のメタデータフォールバックを維持する
  - _Requirements: 4.6, 8.1_

- [x] 3. 読み込み経路と CLI にレイアウトモードを配線する
- [x] 3.1 xlsx の eager / lazy に Excel レイアウトを適用する
  - `load_xlsx` は既定で Excel 準拠、モード指定付きの読み込み関数を追加する
  - eager / lazy の両方で同じ `finish` 呼び出しと使用範囲を渡す
  - `--layout content` では Excel レイアウトを渡さない
  - _Requirements: 4.5, 6.1, 6.2, 6.5_

- [x] 3.2 `Config` / `LoadRequest` / reload 経路へレイアウトモードを伝播する
  - `--layout excel|content` を追加し、既定を `excel` にする
  - `LoadRequest` にモードを追加し、reload / watch でも同じ値を使う
  - csv はモードにかかわらず完全自動レイアウトにする
  - _Requirements: 6.1, 6.2, 6.3, 6.4_

- [x] 4. fixture とテストを追加・更新する
- [x] 4.1 Excel レイアウト用 fixture を追加する
  - 明示列幅、狭い明示列幅 + 長文、明示行高、wrap、結合セル、シート既定値を含める
  - fixture 生成テストから決定的に再生成できるようにする
  - _Requirements: 8.5_

- [x] 4.2 変換関数と確定順序の単体テストを追加する
  - 列幅の切り捨て・クランプ、行高の丸め・クランプ、既定値の下限適用を検証する
  - 明示列幅が自動幅を置き換え、未指定列は自動のままであることを検証する
  - 結合幅が明示列に分配されないことを検証する
  - 固定列・行が `grow_for_cell` で拡大されないことを検証する
  - _Requirements: 8.3_

- [x] 4.3 eager / lazy / モード切替の統合テストを追加する
  - eager と lazy で同一の列幅・行高になることを検証する
  - `--layout content` で従来の自動幅に戻ることを検証する
  - csv が常に自動レイアウトになることを検証する
  - _Requirements: 4.5, 6.2, 6.3, 6.5_

- [x] 4.4 既存 fixture・スナップショットの期待値を更新する
  - `set_column_width` を持つ fixture の期待列幅を新しい規則に合わせる
  - 明示列幅による切り詰めがグリッドと詳細で一致することを検証する
  - freeze / filter / copy / search の既存テストが通ることを確認する
  - _Requirements: 2.3, 5.3, 5.4, 8.1, 8.4_

- [x] 5. ドキュメントと steering を整合させる
  - README のレイアウト説明と `--layout` の既定値を更新する
  - fixture 一覧に新しい fixture を追加する
  - steering の「独自レイアウト」記述を Excel 準拠ハイブリッドへ更新する
  - _Requirements: 8.1, 8.4_

- [x] 6. 全体検証
  - `cargo fmt --all` を実行する
  - `cargo clippy --all-targets -- -D warnings` を実行する
  - `cargo test --all-targets` を実行する
  - `large_lazy.xlsx` の読み込み時間に有意な増加がないことを確認する
  - _Requirements: 8.2, 8.5_
