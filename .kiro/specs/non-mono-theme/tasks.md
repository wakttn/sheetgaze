# Implementation Plan

## 1. Default Palette

- [x] 1.1 Implement base style and screen post-pass
  - `Mono` 以外では背景白・文字黒、`Mono` では `Reset` を返す基底スタイルを追加する
  - 描画後に `fg` / `bg` をチャネル単位で補完する
  - 画面サイズ不足時の早期 return とポップアップの `Clear` 後にも適用する
  - 非 Mono の全 UI が白背景・黒文字になり、Mono は端末既定色のままになる
  - _Boundary: Theme, Screen_
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.6_

- [x] 1.2 Add palette regression tests and documentation
  - 非 Mono / Mono / `--no-ansi` のスナップショットテストを追加・更新する
  - ヘルプ、詳細、フィルタパネル、入力行の基底色を確認する
  - README に非 Mono の既定配色を記載する
  - スナップショットの文字内容が配色変更で変わらない
  - _Boundary: Tests, README_
  - _Requirements: 1.3, 1.4, 1.5_
