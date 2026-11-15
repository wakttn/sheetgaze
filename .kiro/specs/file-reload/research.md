# Research & Design Decisions

## Summary

- **Feature**: `file-reload`
- **Discovery Scope**: Extension（既存の単一ファイル読み込み TUI に監視・再読み込みを追加）
- **Key Findings**:
  - 現状は `main.rs` が起動時に一度だけ `load_xlsx` / `load_csv` を呼び、`EventLoop` は数式結果の `mpsc::Receiver` を 80ms 間隔でポーリングしている。再読み込みを足すには、初期読み込みと再読み込みで同じローダー構成を共有する必要がある。
  - `WorkbookModel` は `Box<dyn RowSource>`（`Send` 境界あり）、`NamedTempFile`、`HashMap` などで構成され、ワーカースレッドから UI スレッドへ所有権を移せる。既存の数式再計算も別スレッドで動作している。
  - `FileFingerprint { len, modified_ms }` と `Generation` は既に存在するが、`Generation` は再計算結果の適用時に照合されていない。再読み込みでワークブックを置換すると、旧世代の数式結果が新しい表示へ混入するリスクがある。
  - `notify` 8.2.2 は MSRV 1.88 でプロジェクトの MSRV と一致し、`PollWatcher` は `Config::with_poll_interval` と `with_compare_contents(false)` を提供する。`RecommendedWatcher` は Linux inotify / macOS FSEvents / Windows ReadDirectoryChangesW を抽象化する。
  - WSL2 の drvfs、Box Drive、ネットワーク共有ではネイティブ通知の取りこぼしがあり得るため、既定のポーリングと、ネイティブ選択時の保険的状態確認を併用する設計が妥当である。

## Research Log

### 既存の読み込み・イベントループ構成

- **Context**: 再読み込みを既存のレイヤード構成へどこに統合するかを確認した。
- **Sources Consulted**: `src/main.rs`, `src/app/event_loop.rs`, `src/app/state.rs`, `src/io/xlsx.rs`, `src/io/csv.rs`, `Cargo.toml`
- **Findings**:
  - 形式判定、lazy 設定の上書き、`--no-filter` の後処理、初期シート選択、数式ジョブの起動が `main.rs` に集約されている。
  - 再読み込みで同じ処理を再実装すると、初期読み込みと再読み込みでフィルタ初期状態や lazy 判定がずれる。
  - `EventLoop::run` は `event::poll(80ms)` のたびに描画し、数式結果を受信している。追加のワーカーからのメッセージも同じループで `try_recv` できる。
  - `WorkbookModel` の `source.fingerprint` はロード時点のサイズと mtime を保持している。
- **Implications**: 形式判定と読み込み後処理を `WorkbookLoader` として `io` に抽出し、起動時と再読み込みの双方から利用する。再読み込みのファイル I/O は専用ワーカースレッドに閉じ込め、UI スレッドはメッセージ適用だけを行う。

### スレッド間の所有権と再計算世代

- **Context**: ワーカーで読み込んだ `WorkbookModel` を UI へ渡せるか、旧世代の数式結果をどう排除するかを確認した。
- **Sources Consulted**: `src/domain/workbook.rs`, `src/domain/row_store.rs`, `src/io/row_cache.rs`, `src/io/formula.rs`, `src/domain/task.rs`
- **Findings**:
  - `RowSource: Send` であり、`SpillRowSource` は `NamedTempFile` と `Vec<u64>` を保持する。`WorkbookModel` に `Rc` / `RefCell` は使われていない。
  - `FormulaHandle` は `CancelToken` と `JoinHandle` を持つが、キャンセルは現状使われていない。
  - `FormulaResults.generation` は生成されるが、`AppState::apply_formula_results` は値を見ずに適用する。
  - `WorkbookModel.generation` は初期値 0 のままで、数式ジョブ側だけが `+1` した世代を送る。
- **Implications**: 再読み込み成功時に旧ワークブックを drop し、新しいワークブックへ世代番号を割り当てる。数式ジョブを再起動し、結果と失敗メッセージの双方に世代を持たせ、現在の世代と一致する場合だけ適用する。

### 変更検出バックエンド

- **Context**: 既定ポーリングとネイティブ監視を、WSL2・Box Drive・ネットワーク共有でも破綻しない形で実装する方法を調査した。
- **Sources Consulted**:
  - [crates.io notify](https://crates.io/crates/notify) — 安定版 8.2.2、MSRV 1.88
  - [notify PollWatcher](https://docs.rs/notify/8.2.2/notify/poll/struct.PollWatcher.html) — ポーリング実装と `watch` / `unwatch`
  - [notify Config](https://docs.rs/notify/8.2.2/notify/struct.Config.html) — `with_poll_interval` / `with_compare_contents`
- **Findings**:
  - `PollWatcher` は既定 30 秒間隔。`with_poll_interval` で 1 秒に変更できる。
  - `with_compare_contents(false)` が既定で、mtime とサイズによる判定になる。内容ハッシュはネットワーク FS で高コストになるため無効のままでよい。
  - `watch(path, RecursiveMode::NonRecursive)` はファイルとディレクトリの両方に使える。親ディレクトリを非再帰で監視すると、一時ファイルからの rename を検出できる。
  - 親ディレクトリ監視は対象外の兄弟ファイルの変更も通知するため、通知後に監視対象ファイルのスタンプを再確認してフィルタする必要がある。
  - `RecommendedWatcher` は OS ネイティブ通知を提供するが、WSL2 の drvfs やクラウド同期フォルダではイベントが欠落し得る。
  - `notify` 8.2.2 は CC0-1.0 で、既存の MIT プロジェクトと互換。9.x は MSRV 1.91 のため採用しない。
- **Implications**: 既定は `PollWatcher`、間隔 1 秒、内容比較なしとする。`native` 選択時は `RecommendedWatcher` に加えて 5 秒間隔のスタンプ確認を併用する。`notify` の型は `io::watch` 内に閉じ込め、ドメイン・UI へ漏らさない。

### 監視方式の自動判定

- **Context**: `auto` モードでネイティブ通知が信頼できない環境をどう判定するかを検討した。
- **Sources Consulted**: Linux `/proc/self/mountinfo` の形式、WSL2 の drvfs / 9p、Box Drive の仮想ドライブ構成
- **Findings**:
  - Linux では `/proc/self/mountinfo` のマウントポイントとファイルシステム種別から、9p / drvfs / fuse / cifs / nfs などを判定できる。パスに最も長く一致するマウントポイントを選ぶ必要がある。
  - Windows の Box Drive はドライブ文字やパスだけでは確実に判定できない。
  - macOS の FSEvents はローカルでは信頼できるが、クラウド同期フォルダでは遅延・欠落があり得る。
- **Implications**: Linux の `auto` は mountinfo で仮想・遠隔 FS を検出した場合に poll、それ以外は native + 保険確認とする。Windows の `auto` は Box Drive を判別できないため保守的に poll とする。macOS は native + 保険確認とする。判定不能時は poll に倒す。

### デバウンス、安定確認、再試行

- **Context**: 保存途中のファイル読み込みと、短時間に複数回発生するイベントの扱いを決める必要がある。
- **Sources Consulted**: 要件 1.6、2.1、2.7、3.1〜3.8、4.1〜4.3
- **Findings**:
  - PollWatcher の通知は 1 秒間隔でまとめて届くが、エディタの保存は一時ファイル作成・rename・属性更新など複数イベントを生む。
  - 変更検出時に即読み込みすると、xlsx の zip 中央ディレクトリが未完成な状態を読む可能性がある。
  - 失敗時に旧 `WorkbookModel` を保持すれば、UI はそのまま操作を継続できる。
- **Implications**: イベント検出時のスタンプを記録し、750ms の安定待ち後に再取得したスタンプが一致した場合だけ読み込む。変更が続く間は待機を延長する。手動リロードは待機をバイパスして即時試行し、失敗時は 1 秒間隔で再試行する。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
| ------ | ----------- | --------- | ------------------- | ----- |
| EventLoop 内で直接ファイルを stat / load | 既存ループに処理を追加 | 変更が最小 | ネットワーク FS の stat / load で UI が停止する。要件 7.1 に反する | 不採用 |
| 専用ワーカースレッド + メッセージ | 監視・待機・読み込み・再試行を 1 スレッドに閉じる | UI 非ブロック、単一飛行を保証しやすい | スレッド寿命と終了処理の設計が必要 | 採用 |
| notify の PollWatcher のみ | 既定ポーリングだけを提供 | 実装が最小、環境差が小さい | `native` 要件を満たせない | `poll` の実装として採用 |
| カスタム stat ポーリング | `std::fs::metadata` を自前ループ | 依存なし、完全に制御可能 | 親ディレクトリの rename 検出を自前で実装する必要がある | notify に統一するため不採用 |
| 起動時ローダーの再利用 | `main` の読み込み処理を関数化 | 初期と再読み込みの設定が一致 | 既存 `main` の整理が必要 | 採用 |
| 数式結果の世代照合 | 結果に世代を持たせて適用時に比較 | 旧結果の混入を確実に防ぐ | 既存 `FormulaMessage` の変更が必要 | 採用 |

## Design Decisions

### Decision: 監視と再読み込みを単一のバックグラウンドワーカーに集約する

- **Context**: ポーリング、デバウンス、安定確認、読み込み、再試行、手動リロードを複数スレッドに分けると、重複読み込みと状態競合の調整が複雑になる。
- **Alternatives Considered**:
  1. イベントループ内で stat と読み込みを行う — UI が停止する。
  2. 監視スレッドと読み込みスレッドを分ける — 単一飛行と再試行の調停が複雑になる。
- **Selected Approach**: `ReloadWorker` が 1 スレッドで監視通知・手動コマンド・タイマーを処理し、ファイル I/O と読み込みを直列化する。UI へは `ReloadMessage` を送る。
- **Rationale**: 重複読み込みを構造的に排除でき、旧ワークブックを保持したまま再試行できる。既存の数式バックグラウンド処理と同じ「ワーカー + mpsc」パターンに一致する。
- **Trade-offs**: ワーカーの状態機械と終了処理を設計する必要がある。読み込み中は手動コマンドが待たされるが、要求はキューに残る。
- **Follow-up**: 終了時にワーカーを join しない方針をテストで確認する。

### Decision: 変更検出は notify に統一し、既定を PollWatcher とする

- **Context**: 既定の poll と任意の native を別実装にすると、イベント形式・監視解除・エラー処理が二重化する。
- **Alternatives Considered**:
  1. poll を自前 stat、native のみ notify — 実装が二重化する。
  2. `notify` の `PollWatcher` / `RecommendedWatcher` を切り替える — イベント契約が共通になる。
- **Selected Approach**: 両方を `io::watch::WatchSession` でラップし、`WatchMode` に応じてバックエンドを選ぶ。PollWatcher は 1 秒間隔・内容比較なし、監視対象はファイルと親ディレクトリ（非再帰）。
- **Rationale**: 要件 1.3〜1.5 を最小の実装差分で満たし、環境ごとのバックエンド差をアダプタ内に閉じ込められる。
- **Trade-offs**: 新しい依存 `notify 8.2.2` が増える。PollWatcher の mtime 粒度が粗い FS では検出が遅れる可能性がある。
- **Follow-up**: drvfs / Box Drive 実機での検出遅延を手動確認する。

### Decision: 変更のデバウンスと安定確認を純粋な `ReloadGate` に置く

- **Context**: タイマーとファイル状態の判定をワーカー内に直接書くと、時間依存のテストが難しい。
- **Alternatives Considered**:
  1. ワーカー内で `sleep` と `Instant` を直接扱う — 統合テストしかできない。
  2. 判定を純粋関数へ分離し、ワーカーが現在時刻とスタンプを渡す — 単体テスト可能。
- **Selected Approach**: `ReloadGate` が Idle / Debouncing / Loading / RetryWait の遷移と期限を管理し、`now_ms` と `FileFingerprint` を入力に `GateDecision` を返す。実際の待機と読み込みはワーカーが行う。
- **Rationale**: 要件 1.6、2.1、2.7、3.3、4.3 の時間規則を仮想時刻で検証できる。
- **Trade-offs**: 状態機械の分だけ型が増える。
- **Follow-up**: 境界時刻（期限ちょうど、連続変更、手動割り込み）を単体テストに含める。

### Decision: 再読み込み後の表示状態は AppState で明示的に引き継ぐ

- **Context**: 新しい `WorkbookModel` をそのまま差し替えると、シート・フォーカス・フィルタ・検索が失われる。
- **Alternatives Considered**:
  1. 毎回初期状態へ戻す — 実装は単純だが、更新のたびに目的のセルへ戻る必要がある。
  2. 再読み込み前の状態をスナップショットし、互換性がある範囲で復元する — 利用者の作業を保てる。
- **Selected Approach**: シート名、フォーカス、選択、フィルタの列キー、モードをスナップショットし、新しいワークブックへ適用する。検索結果は破棄し、範囲外はクランプする。
- **Rationale**: 要件 5.1〜5.10 を一箇所で満たし、リロードのたびに UI が初期化される混乱を避ける。
- **Trade-offs**: フィルタ定義が変わった場合の互換判定が必要になる。
- **Follow-up**: 列数変更、シート削除、フィルタ定義変更の組み合わせをテストする。

### Decision: 数式結果に世代照合を導入する

- **Context**: 再読み込み中に旧ワークブックの再計算が完了すると、同じシート名・セル位置へ古い値が適用される可能性がある。
- **Alternatives Considered**:
  1. 旧ジョブの完了を待つ — UI とリロードが遅れる。
  2. 世代番号で結果を照合する — 非同期のまま安全に破棄できる。
- **Selected Approach**: ワークブックごとに単調増加する世代を割り当て、再計算結果と失敗メッセージに同じ世代を持たせる。現在の世代と一致しない結果は破棄する。再読み込み時は旧ジョブをキャンセルする。
- **Rationale**: 要件 2.5、2.6 を満たし、既存の `FormulaResults.generation` を活用できる。
- **Trade-offs**: `FormulaMessage` と `AppState::apply_formula_results` の契約変更が必要。
- **Follow-up**: 遅い再計算中に複数回リロードする統合テストを追加する。

### Decision: 変更検出に内容ハッシュを使わない

- **Context**: サイズと mtime だけでは、同一サイズかつ同一 mtime の書き換えを検出できない。一方、WSL2・Box Drive・ネットワーク共有で 1 秒ごとに全文を読むコストは大きい。
- **Alternatives Considered**:
  1. notify の `compare_contents = true` — 監視対象の全ファイルをポーリング周期ごとに読み込んでハッシュする。
  2. イベント発生時だけハッシュ — メタデータが変わらない変更はイベント自体が発生しないため検出できない。
  3. メタデータのみ — 低負荷だが、同一サイズ・同一 mtime の書き換えは取りこぼす。
- **Selected Approach**: 既定は存在有無・サイズ・mtime のみで判定し、内容ハッシュは行わない。`compare_contents = false` を維持する。
- **Rationale**: 通常の保存・置換保存を実用的なコストで検出でき、既定の 1 秒ポーリングを大容量ファイルや仮想 FS でも維持できる。再読み込み時はローダーが全文を読むため、検出段階での二重読みを避けられる。
- **Trade-offs**: 同一サイズ・同一 mtime の内容変更、および mtime 粒度が粗い環境での同一サイズ高速書き換えは検出しない。この制約は既知の限界として扱い、必要時は手動リロードを使う。
- **Follow-up**: 将来、厳密性を優先する利用者向けに任意の hash モードを追加する余地は残すが、本仕様には含めない。

## Risks & Mitigations

- **Risk**: `notify` 8.2.2 追加によるビルド時間・依存増加 — OS 別バックエンドのみで、非同期ランタイムは追加しない。MSRV 1.88 を確認済み。
- **Risk**: PollWatcher の mtime 粒度が粗い FS で変更を取りこぼす — 既定 1 秒間隔を設定可能にし、手動リロードを常設する。
- **Risk**: 親ディレクトリ監視で無関係な兄弟ファイルのイベントが届く — 通知後に監視対象のスタンプを再確認し、変化がなければ読み込まない。
- **Risk**: ネットワーク FS 上の stat / load が長時間ブロックする — すべてワーカースレッドで実行し、UI はメッセージだけを処理する。
- **Risk**: 再試行中の状態表示が毎秒更新されて煩わしい — 初回、3 回目、以降 3 回ごとに表示し、成功時に消す。
- **Risk**: 自動判定が誤って native を選ぶ — `native` でも 5 秒間隔のスタンプ確認を併用し、登録・実行時エラーは poll へフォールバックする。
- **Risk**: 終了時にワーカーが残る — `ReloadHandle` の Drop で Shutdown を送り、ワーカーは監視を drop して終了する。join はしない。

## References

- [crates.io notify](https://crates.io/crates/notify) — 安定版 8.2.2、MSRV 1.88、CC0-1.0
- [notify PollWatcher](https://docs.rs/notify/8.2.2/notify/poll/struct.PollWatcher.html) — ポーリング監視と `poll()` API
- [notify Config](https://docs.rs/notify/8.2.2/notify/struct.Config.html) — `with_poll_interval`、`with_compare_contents`
- [notify Watcher trait](https://docs.rs/notify/8.2.2/notify/trait.Watcher.html) — `watch` / `unwatch` / `configure`
- [Linux proc filesystem](https://docs.kernel.org/filesystems/proc.html) — `/proc/self/mountinfo` のマウント情報
