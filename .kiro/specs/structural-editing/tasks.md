# Implementation Plan

## 1. Foundation

- [x] 1.1 (P) Extend selection shapes
  - 行・列・矩形の選択モードと `range(rows, cols)` を追加する
  - コピー、レイアウト、AppState の選択範囲呼び出しを新しい契約へ移行する
  - ステータス行に選択種別と選択セル数を表示する
  - 各モードの範囲計算と既存セル範囲選択の回帰テストが通る
  - _Boundary: SelectionModel, GridWidget_
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8_

- [x] 1.2 (P) Add register and TSV codec
  - 座標付き `ClipboardBlock`、名前なし／ヤンクレジスタ、疎／密の契約を実装する
  - タブ・改行・引用符・空フィールドを扱う TSV エンコード／デコードを実装する
  - 空入力、可変長レコード、`--max-copy-cells` 超過時のエラーをテストする
  - `yy` / `dd` / 矩形ヤンクのレジスタ内容が座標を保持する
  - _Boundary: RegisterStore, TsvCodec_
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.9, 2.10, 4.8, 4.10, 4.11_

- [x] 1.3 (P) Add clipboard and structural limits configuration
  - `--clipboard-register unnamedplus|internal` を追加し、既定値を `unnamedplus` にする
  - `--structural-max-cells` を追加し、既定値と有効範囲を定義する
  - 不正値が起動時に拒否されることをテストする
  - `Config::default` と CLI パースが新しい設定を返す
  - _Boundary: Config_
  - _Requirements: 2.11, 8.2_

- [x] 1.4 Extend Ex commands and actions
  - `:irow[!]`、`:icol[!]`、`:drow[!]`、`:dcol[!]`、`:put[!]` をパースする
  - 件数省略時 1、`!` の方向、0 件・範囲外のエラーを定義する
  - `x`、`dd`、`V` + `d`、`C` + `d` のアクションを構造操作へ接続する
  - 不正コマンドと不正件数で状態が変わらない
  - _Boundary: CommandParser, Keymap, Action_
  - _Requirements: 3.5, 3.6, 7.2, 7.3, 7.9, 7.10, 7.11, 7.12_

## 2. Structural Engine

- [x] 2.1 Implement origin tracking and row/column transforms
  - 現在座標から元 xlsx 座標への origin を維持し、挿入・削除で更新する
  - セル、編集オーバーレイ、数式オーバーレイ、行・列の使用範囲を移動する
  - 使用範囲拡張時に origin を `None` で伸ばし、長さ不変条件を維持する
  - 最大行数・最大列数を超える操作が拒否される
  - _Boundary: StructureEngine, Sheet_
  - _Requirements: 3.7, 6.2, 8.1, 8.3, 8.4, 10.6_

- [x] 2.2 Materialize lazy sheets and enforce limits
  - 最初の構造編集で lazy シートを eager 化する
  - lazy / eager の両方で `--structural-max-cells` を検査する
  - 上限超過時にシートが変更されないことをテストする
  - 通常閲覧時は遅延ロードと行キャッシュが維持される
  - _Boundary: StructureEngine, Sheet_
  - _Requirements: 8.1, 8.2, 8.3, 8.4_

- [x] 2.3 Transform merges, filters, layout, freeze, and conditional formats
  - 結合範囲を挿入・削除に合わせて拡張・縮小・削除する
  - フィルタ範囲、フィルタ列、可視性を再評価する
  - 列幅・行高、固定ペイン、条件付き書式、ジャンプ対象を変換する
  - 部分的な調整は警告として返し、致命的な場合だけ操作を拒否する
  - _Boundary: StructureEngine, MergeIndex, FilterDefinition, LayoutSnapshot, FreezePane_
  - _Requirements: 3.7, 9.4, 10.4_

- [x] 2.4 Extend edit transactions for structural undo/redo
  - `EditTransaction` を `ops` 形式へ移行し、構造のみのトランザクションを記録する
  - 削除セル、メタデータスナップショット、数式変更を `StructureEdit` に保持する
  - undo でセル・構造・結合・フィルタ・レイアウト・固定ペイン・数式オーバーレイを復元する
  - redo で再適用し、dirty と保存後リセットが既存挙動と一致する
  - _Boundary: EditLog, StructureEdit_
  - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6_

- [x] 2.5 Update application state after structural edits
  - 検索索引を無効化し、カーソル・選択・ジャンプ履歴をクランプする
  - フィルタ可視性とビューポートを更新する
  - 数式再計算を要求し、dirty 中の外部変更ガードを維持する
  - 構造変更後に選択とカーソルが使用範囲内に収まる
  - _Boundary: AppState_
  - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7_

## 3. Clipboard and Paste

- [x] 3.1 Implement yank, cut, and clipboard writes
  - セル・行・列・矩形のヤンクと切り取りをレジスタへ保存する
  - TSV をシステムクリップボードへ書き込み、レジスタ由来の成功だけを記録する
  - `x` / `dd` / `V` + `d` / `C` + `d` の内容クリアと構造削除を実装する
  - 書き込み失敗時も内部レジスタが残り、警告が表示される
  - _Boundary: AppState, RegisterStore, ClipboardAdapter_
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.9, 2.10, 7.9, 10.2_

- [x] 3.2 Implement clipboard read and unnamedplus paste selection
  - システムクリップボード読み取りと `ClipboardRead` を実装する
  - `p` / `P` の unnamedplus 判定、内部フォールバック、`"+p`、`"0p` を実装する
  - 直前の自アプリ書き込みと一致する場合はリッチレジスタを優先する
  - 読み取り不能時に内部レジスタへフォールバックする
  - _Boundary: ClipboardAdapter, AppState, EventLoop_
  - _Requirements: 2.5, 2.6, 2.7, 2.8, 2.9, 10.2_

- [x] 3.3 Implement paste planning and application
  - 行・列・セル・矩形の `p` / `P` 計画を構築する
  - 構造挿入とセル書き込みを 1 トランザクションで適用する
  - 空セル、結合セル、使用範囲外、空レジスタを扱う
  - 行・列ペースト後に値・書式・数式が保持される
  - _Boundary: PastePlanner, StructureFlow, AppState_
  - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8, 4.9, 4.10, 4.11_

- [x] 3.4 Implement row/column insert keys and commands
  - `o` / `O` / `gI` / `gA` の挿入と Insert モード遷移を実装する
  - `:irow[!]` / `:icol[!]` / `:drow[!]` / `:dcol[!]` を構造操作へ接続する
  - 読み取り専用・CSV・範囲外・不正件数を拒否する
  - 挿入した行・列がセル、数式、結合、フィルタ、レイアウトへ反映される
  - _Boundary: AppState, Keymap, CommandParser, StructureFlow_
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 3.9, 3.10_

## 4. Formula References

- [x] 4.1 (P) Implement A1 reference rewriting
  - 数式 AST の参照スパンだけを差し替える
  - 同一シート・シート修飾・絶対参照・範囲・行全体・列全体を扱う
  - パース不能・構造化参照・外部参照を未対応として返す
  - 更新前後の数式文字列が参照部分だけ異なる
  - _Boundary: FormulaRefEngine_
  - _Requirements: 5.3, 5.4_

- [x] 4.2 Implement structure flow pre-validation and formula updates
  - 構造適用前に eager シートの数式を検証し、未対応時は状態を変更しない
  - 検証済みの数式を更新し、シートごとの書き換えマップに記録する
  - lazy な他シートの数式は保存時更新に委ねる
  - 構造変更と数式更新が 1 つのユースケースとして完了する
  - _Boundary: StructureFlow, WorkbookModel_
  - _Requirements: 5.3, 5.4, 9.5_

## 5. Structural Save

- [x] 5.1 Build workbook save plan and include structural sheets
  - 構造 dirty、セル編集、数式キャッシュを持つシートを保存対象に含める
  - 再計算用一時ワークブックも同じ対象選定を使う
  - 行挿入だけでセル入力がないシートを保存できる
  - 未変更シートは元の zip エントリを保持する
  - _Boundary: OoxmlStructureWriter, WorkbookModel_
  - _Requirements: 5.1, 5.6, 5.7, 5.8_

- [x] 5.2 Remap sheetData rows, cells, dimension, and column definitions
  - 元の行・セル XML とスタイル属性を保持して現在座標へ再配置する
  - 挿入行・列を空として出力し、削除行・列を出力しない
  - 編集セル、数式書き換え、数式キャッシュを反映する
  - 保存→再読み込みで値・書式・数式が保持される
  - _Boundary: OoxmlStructureWriter_
  - _Requirements: 5.2, 5.3, 5.9_

- [x] 5.3 Transform merges, filters, conditional formats, validations, hyperlinks, panes, and breaks
  - 結合セル、オートフィルタ、条件付き書式の範囲を変換する
  - データ検証、ハイパーリンク、固定ペイン、改ページを変換する
  - `dimension` を使用範囲へ更新する
  - 結合・フィルタ・固定ペインを含むラウンドトリップテストが通る
  - _Boundary: OoxmlStructureWriter_
  - _Requirements: 5.2_

- [x] 5.4 Update defined names and cross-sheet formulas; fail closed for unsupported structures
  - 定義名の参照を更新し、`workbook.xml` に反映する
  - lazy シートのクロスシート数式を保存時に更新する
  - 共有数式・配列数式・テーブルパーツ・構造化参照を検出して保存を拒否する
  - 未対応構造のシート・セル・定義名がステータスに表示される
  - _Boundary: OoxmlStructureWriter, FormulaRefEngine_
  - _Requirements: 5.3, 5.4, 5.5, 10.3_

- [x] 5.5 Add stale save guard and post-save normalization
  - 構造 dirty なシートの fingerprint 不一致を検出し、`:w!` でも保存しない
  - 保存成功後に origin を identity に戻し、edits を base に統合する
  - `formula_rewrites` と `structural_dirty` をクリアし、監視基準を更新する
  - 構造変更→保存→構造変更→保存の連続保存で二重変換が起きない
  - _Boundary: EventLoop, AppState, Sheet, OoxmlStructureWriter_
  - _Requirements: 5.1, 5.7, 5.9, 5.10, 6.5_

## 6. UI, Documentation, and Validation

- [x] 6.1 Update keymap, help, status, and README
  - `v` / `V` / `C` / `Ctrl-V`、`yy` / `Y` / `x` / `dd` / `p` / `P` / `o` / `O` / `gI` / `gA` をヘルプに追加する
  - `dd` の挙動変更と内容クリアの代替キーを明記する
  - `--clipboard-register`、`--structural-max-cells`、制限事項を README に記載する
  - ヘルプと README が新しいキー・設定を表示する
  - _Boundary: Keymap, Help, StatusView, README_
  - _Requirements: 1.6, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8_

- [x] 6.2 Add unit, integration, and TUI regression tests
  - 選択、レジスタ、TSV、構造変換、数式参照、undo の単体テストを追加する
  - 切り取り→ペースト→保存→再読み込み、行・列挿入、連続保存、lazy、Stale の統合テストを追加する
  - クリップボード読み取り、リッチ貼り付け、フォールバック、書き込み失敗のテストを追加する
  - 既存の検索・コピー・フィルタ・編集・TUI テストが通る
  - _Boundary: Tests_
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9, 2.10, 2.11, 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 3.9, 3.10, 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8, 4.9, 4.10, 4.11, 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8, 5.9, 5.10, 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, 7.9, 7.10, 7.11, 7.12, 8.1, 8.2, 8.3, 8.4, 8.5, 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 10.1, 10.2, 10.3, 10.4, 10.5, 10.6_

- [x] 6.3 Run formatting, lint, and full test suite
  - `cargo fmt --all` を適用する
  - `cargo clippy --all-targets -- -D warnings` を通す
  - `cargo test --all-targets` を通す
  - 既存機能の回帰がないことを確認する
  - _Boundary: Repository_
  - _Requirements: 1.1, 5.1, 6.1, 8.1, 10.1_
