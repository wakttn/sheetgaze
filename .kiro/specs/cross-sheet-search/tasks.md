# Implementation Plan

## 1. Search domain foundation

- [x] 1.1 Add sheet identity to search matches
  - `SearchMatch` にシートインデックスを追加する
  - 一致の安定順序（シート、行、列）を定義する
  - 既存の単一シート検索テストが新しい型で通ることを確認する
  - _Requirements: 1.2, 5.2, 5.3_

- [x] 1.2 Make search stepping visibility-aware per sheet
  - シートごとの可視性マップで次/前の一致を選ぶ
  - 全一致が非表示の場合に current を None にし、hidden 件数を数える
  - シート順のラップを単体テストで確認する
  - _Requirements: 1.5, 2.1, 2.2, 2.3_

- [x] 1.3 Scope grid highlighting to the active sheet
  - `SearchIndex` がアクティブシートの一致だけを `is_match` で返す
  - シート切替時にハイライト対象が更新される
  - 他シートの同一座標がハイライトされないことをテストで確認する
  - _Requirements: 3.3, 5.1_

## 2. Workbook-wide search

- [x] 2.1 Scan all sheets and overlays
  - 全シートの eager / lazy データを走査する
  - 未保存編集オーバーレイがあればベース値より優先する
  - 結果をシート順・行順・列順にソートする
  - _Requirements: 1.1, 1.4, 5.2_

- [x] 2.2 Display sheet and cell context
  - 検索行に現在のシート名・セル・件数・非表示件数を表示する
  - 状態行の検索ラベルを横断検索に対応させる
  - 0 件時にフォーカスを変更しないことを確認する
  - _Requirements: 3.1, 3.2, 3.4_

## 3. Jump and history

- [x] 3.1 Implement cross-sheet jump
  - シート切替、行ロード、フォーカス、ビューポートを一括更新する
  - 検索の active_sheet を同期する
  - 別シートの一致へ `n` / `N` で移動できることを統合テストで確認する
  - _Requirements: 2.1, 2.2, 2.4_

- [x] 3.2 Add `:goto`, `*`, and `#`
  - `Sheet!A1` と `A1` を受け付ける
  - フォーカスセルの表示値で横断検索する
  - 不正なシート・座標は状態行に理由を表示する
  - _Requirements: 2.5, 2.6, 2.7_

- [x] 3.3 Add jump history
  - シート横断ジャンプ前に位置を記録する
  - `Ctrl-o` / `Ctrl-i` で戻る/進む
  - 履歴上限と同一位置の重複をテストで確認する
  - _Requirements: 4.1, 4.2, 4.3, 4.4_

## 4. Validation

- [x] 4.1 Regression tests for search and filters
  - 既存の view-consistency テストを更新する
  - 複数シート fixture で検索、移動、ジャンプ、ハイライトを確認する
  - フィルタ非表示行と遅延シートの一致を確認する
  - _Requirements: 1.5, 3.3, 5.1, 5.3_
