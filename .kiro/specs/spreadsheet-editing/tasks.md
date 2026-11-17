# Implementation Plan

## 1. Editing domain foundation

- [x] 1.1 Add edit overlay to sheets
  - eager / lazy 共通で編集セルを保持する
  - セル読み取り、全セル走査、コピー、検索、フィルタマスクがオーバーレイを優先する
  - used range / meta / data rows / layout / formula cells を編集時に更新する
  - _Requirements: 2.6, 2.8, 3.1_

- [x] 1.2 Add input parsing and edit transactions
  - `=` / `'` / TRUE/FALSE / 数値 / テキスト / 空 を解釈する
  - before/after を持つトランザクションと undo / redo / dirty を実装する
  - undo / redo の上限と保存後の dirty 解除をテストする
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.7, 3.2, 3.3, 3.4, 3.5, 3.6_

- [x] 1.3 Add Ex command parser
  - `:w`, `:w!`, `:q`, `:q!`, `:wq`, `:x`, `:e`, `:e!`, `:goto`, `:sheet`, `:help` を解釈する
  - `:s`, `:%s`, `:gs`, 範囲指定と `g` / `i` / `I` / `n` フラグを解釈する
  - 不正なコマンドは状態行に理由を返す
  - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.8, 7.1_

## 2. Vim editing UX

- [x] 2.1 Add Insert and Command modes with an input line
  - `i` / `a` / `I` / `A` / `F2` / `s` / `=` で Insert に入る
  - `Enter` / `Tab` / `Shift-Tab` / `Esc` / `Ctrl-c` の確定・移動・破棄を実装する
  - `:` で Command に入り、履歴とバックスペースを扱う
  - 下部入力行にテキストとカーソルを表示する
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9_

- [x] 2.2 Add normal-mode editing commands
  - `x`, `r{char}`, `d{motion}`, `dd`, `K` を実装する
  - `u` / `Ctrl-r` / `ZZ` / `ZQ` を実装する
  - 詳細表示を `K` / `Enter` に移す
  - _Requirements: 1.10, 1.11, 1.12, 1.13, 1.14, 3.2, 3.3, 7.4_

## 3. Save and recalculation

- [x] 3.1 Implement xlsx diff writer
  - シート名から XML パスを解決し、変更セルだけを `<sheetData>` に反映する
  - 既存セルのスタイル属性、数式、inlineStr、dimension、calcPr、calcChain を扱う
  - 他の zip エントリを保持し、一時ファイル + atomic rename で保存する
  - zip/XML と再読み込みで既存機能が保持されることをテストする
  - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.8_

- [x] 3.2 Wire save commands and safety guards
  - `:w`, `:w!`, `:wq`, `:x`, `ZZ`, `:q`, `:q!`, `:e`, `:e!` を AppState に接続する
  - CSV / read-only / 共有数式 / 外部変更を保存前に検出する
  - 保存成功時に dirty を解除し、watch の基準スタンプを更新する
  - `Effect::Save` と保存後の終了をイベントループへ追加する
  - _Requirements: 4.5, 4.6, 4.7, 4.9, 7.1, 7.2, 7.3, 7.5, 7.6_

- [x] 3.3 Recalculate formulas from unsaved edits
  - 編集内容を一時 xlsx に書き、任意パスから再計算ジョブを起動する
  - 世代照合、キャンセル、サイズポリシー、失敗時の表示を実装する
  - 編集確定後に依存数式の表示が更新されることをテストする
  - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

## 4. Replace

- [x] 4.1 Implement literal replace planning
  - 表示値に対して literal / スマートケースで一致する
  - 数式セル、非表示行、結合非アンカーをスキップする
  - `g` / `i` / `I` / `n` と置換件数・スキップ件数を返す
  - _Requirements: 5.4, 5.5, 5.6, 5.8_

- [x] 4.2 Apply replace commands transactionally
  - 現在セル、Visual 範囲、シート、全シートの範囲を扱う
  - 置換全体を 1 トランザクションにして undo 可能にする
  - 置換後のレイアウト、可視性、検索、数式再計算を更新する
  - `:%s` と `:gs` の統合テストを追加する
  - _Requirements: 5.1, 5.2, 5.3, 5.7, 5.9_

## 5. Protection and validation

- [x] 5.1 Add read-only mode and external-change guards
  - `--read-only` を CLI に追加する
  - CSV と読み取り専用ファイルで編集・置換・保存を拒否する
  - dirty 中の外部変更を自動反映せず、`:e!` で破棄できるようにする
  - _Requirements: 7.5, 7.6, 4.6_

- [x] 5.2 Add regression and TUI tests
  - Insert / Command の入力行とカーソル、dirty 表示、`:q` 拒否をテストする
  - 編集→保存→再読み込みで値・スタイル・結合・フィルタ・固定ペインを確認する
  - 既存の検索・コピー・フィルタ・watch のテストが通ることを確認する
  - _Requirements: 1.1, 3.4, 4.1, 4.4, 7.2, 7.3_
