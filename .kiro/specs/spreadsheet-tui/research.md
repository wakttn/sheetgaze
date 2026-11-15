# Research & Design Decisions

## Summary

- **Feature**: `spreadsheet-tui`
- **Discovery Scope**: New Feature（グリーンフィールド、包括的調査）
- **Key Findings**:
  - calamine 0.38 は xlsx の値・数式・セルスタイル・罫線・結合範囲を純 Rust で取得でき、表示モデルの主データソースとして適する。
  - formualizer 0.14 は Excel 互換の数式評価を提供し、calamine では不足する再計算を補完できる。読み込みと計算を分離できる。
  - ratatui 0.30.2 と crossterm 0.29 の組み合わせで、表示専用 TUI に必要な描画・入力・端末復元を実装できる。
  - 縦罫線は 1 表示列のガター、横罫線は下線で表現すると、行高を増やさずに罫線と既定グリッド線を描画できる。
  - クリップボードは OSC 52 を基本に、ローカル環境では arboard を補助的に使う二段構えが、SSH とローカルの双方に適合する。
  - 独自のレイアウト層を設け、Excel の列幅・行高ではなく表示内容から TUI 用の幅・行数を計算する必要がある。
  - calamine 0.38 の `worksheet_cells_reader` / `XlsxCellReader` はセルをストリーミングで返すため、xlsx の遅延ロードを行キャッシュ方式で実装できる。
  - calamine はオートフィルタの条件と固定ペインを公開しないため、OOXML の `autoFilter` / `pane` を `zip` + `quick-xml` で別途解析する必要がある。
  - csv の文字コード判定は `chardetng` と `encoding_rs` の組み合わせで、BOM 優先・明示上書き・サンプル判定を実現できる。

## Research Log

### xlsx の読み込みと書式・結合範囲

- **Context**: 要件 1.2、2.2、2.6、3.1 を満たすため、xlsx の値だけでなく数式・スタイル・結合情報を取得できるクレートが必要だった。
- **Sources Consulted**:
  - [calamine 0.38.0 - docs.rs](https://docs.rs/calamine/latest/calamine/)
  - [calamine GitHub README](https://github.com/tafia/calamine)
  - [calamine `Reader` trait](https://docs.rs/calamine/latest/calamine/trait.Reader.html)
  - [calamine `StyleRange` 実装](https://github.com/tafia/calamine/blob/master/src/style.rs)
  - [calamine `Xlsx` 実装](https://github.com/tafia/calamine/blob/master/src/xlsx/mod.rs)
  - [calamine `Range` ドキュメント](https://docs.rs/calamine/latest/calamine/struct.Range.html)
- **Findings**:
  - `Reader::worksheet_range` は `Range<Data>` を返し、`Data` は `Int` / `Float` / `String` / `Bool` / `DateTime` / `DateTimeIso` / `DurationIso` / `Error` / `Empty` を持つ。
  - `Range::start()` / `end()` は A1 を原点とする絶対座標を返す。疎なシートでは先頭の空行・空列がトリムされるため、絶対座標への変換が必須である。
  - `Reader::worksheet_formula` は数式文字列を `Range<String>` として返す。キャッシュ値は `worksheet_range` 側から取得できる。
  - `Reader::worksheet_style` は xlsx のみ対応し、`StyleRange` を返す。`StyleRange::get((row, col))` は絶対座標で `Style` を返し、`Font::is_bold()` と `Font::color` から太字・文字色を取得できる。
  - `Style.borders` は `Borders { left, right, top, bottom, diagonal_down, diagonal_up }` を持ち、各 `Border` は `style: BorderStyle` と任意の `Color` を持つ。空セルでも `StyleRange::cells()` / `next_cell_with_style_id` からスタイルを取得できる。
  - `Xlsx::load_merged_regions()` の後に `merged_regions_by_sheet(name)` を呼ぶと、シートごとの `Dimensions` を取得できる。結合範囲はセル値とは独立に取得する必要がある。
  - calamine 0.38 は `zip` 9 系と `quick-xml` 0.42 系に依存し、純 Rust で完結する。`chrono` feature で日時型を利用できる。
- **Implications**:
  - xlsx ローダーは「値」「数式」「スタイル」「結合」を別 API で読み、絶対座標で 1 つのドメインモデルに統合する。
  - `StyleRange` は明示スタイルのある範囲のみを保持するため、範囲外・スタイルなしは既定スタイルとして扱う。
  - 数式セルのキャッシュ値は calamine から取得し、再計算前の初期表示に使う。

### セル罫線の読み取りと描画

- **Context**: 要件 2.10〜2.12 を満たすため、xlsx の罫線を読み取り、端末セル上で判別できる形で表示する必要があった。
- **Sources Consulted**:
  - [calamine `style.rs`](https://github.com/tafia/calamine/blob/master/src/style.rs)
  - [ratatui `Style` / `Modifier`](https://docs.rs/ratatui/latest/ratatui/style/)
  - [Unicode Box Drawing](https://www.unicode.org/charts/PDF/U2500.pdf)
  - [SGR 58 (underline color)](https://en.wikipedia.org/wiki/ANSI_escape_code)
- **Findings**:
  - calamine は罫線を `BorderStyle`（Thin / Medium / Thick / Double / Dashed / Dotted / Hair など）と `Color` で公開する。
  - Excel は結合範囲の罫線をアンカーセルの書式として保持することが多く、結合範囲の外枠はアンカーと外周セルの両方から集約する必要がある。
  - 端末セルは 1 文字 1 セルなので、縦罫線は専用の 1 表示列、横罫線はセル最下行の下線（SGR 4 / 任意で SGR 58）で表現すると、行高を増やさずに罫線を表示できる。
  - 下線色（SGR 58）は truecolor 対応端末でも未対応のものがあり、未対応時は文字色で下線が描かれる。
- **Implications**:
  - `CellStyle` に `CellBorders` を追加し、eager / lazy の両経路で罫線を保持する。
  - `RenderCell.slot_width` を追加し、はみ出し幅と罫線位置を分離する。列ごとに 1 列のガターを確保する。
  - 既定グリッド線は淡色、明示罫線は線種に応じた box-drawing 文字で描く。横罫線の二重線・太線は下線の色・太さで近似する。

### 数式計算エンジン

- **Context**: calamine は数式を評価しないため、要件 3.1〜3.9 を満たす Excel 互換エンジンが必要だった。
- **Sources Consulted**:
  - [formualizer 0.14.0 - docs.rs](https://docs.rs/formualizer/latest/formualizer/)
  - [formualizer GitHub README](https://github.com/psu3d0/formualizer)
  - [formualizer-workbook API](https://docs.rs/formualizer-workbook/latest/formualizer_workbook/)
  - [formualizer `NativeXlsxAdapter` 実装](https://github.com/psu3d0/formualizer/blob/main/crates/formualizer-workbook/src/xlsx/loader.rs)
  - [formualizer `Workbook` 実装](https://github.com/psu3d0/formualizer/blob/main/crates/formualizer-workbook/src/workbook.rs)
- **Findings**:
  - `formualizer` 0.14 は 400 以上の Excel 関数、動的配列、`LET` / `LAMBDA`、循環参照検出、増分再計算を備える。
  - `NativeXlsxAdapter` と `Workbook::from_reader(reader, LoadStrategy::EagerAll, WorkbookConfig::ephemeral())` でワークブックを読み込み、`evaluate_all()` で全数式を計算できる。
  - `Workbook::get_value(sheet, row, col)` は `Option<LiteralValue>` を返す。`LiteralValue` は `Int` / `Number` / `Text` / `Boolean` / `Date` / `DateTime` / `Time` / `Duration` / `Empty` / `Error` / `Array` / `Pending` を持つ。
  - Workbook API のセル座標は Excel と同じ 1 始まりである（例: B3 は row=3, col=2）。ドメインの 0 始まり座標との変換が境界で必要になる。
  - 外部リンクはキャッシュ値のみを読み、更新しない。マクロや VBA は実行しない。
  - `formualizer` の `xlsx-recalc` feature は workbook / eval / common を有効化する。ネイティブ Rust では追加のランタイムを必要としない。
- **Implications**:
  - calamine で表示モデルを作り、formualizer で数式セルのみを再計算する。両者は同じファイルを別々に読むため、読み込み時のファイル同一性（サイズ・更新時刻）を確認する。
  - 再計算は `std::thread` と `mpsc` でバックグラウンド実行し、完了後に数式セルの値だけを差し替える。UI はキャッシュ値で先に描画する。
  - `Array` / `Pending` / 未対応関数は、キャッシュ値またはエラー表示へフォールバックし、詳細画面で状態を明示する。
  - 外部リンク更新・マクロ実行を行わないことをセキュリティ境界として明記する。

### TUI フレームワークと端末制御

- **Context**: 要件 2、4、5、8.4〜8.6 を満たす描画・入力・端末復元基盤が必要だった。
- **Sources Consulted**:
  - [ratatui 0.30.2](https://ratatui.rs/)
  - [ratatui v0.30 リリースノート](https://ratatui.rs/highlights/v030/)
  - [ratatui `Cargo.toml`](https://github.com/ratatui/ratatui/blob/main/ratatui/Cargo.toml)
  - [crossterm 0.29.0 - crates.io](https://crates.io/crates/crossterm)
- **Findings**:
  - ratatui 0.30.2 は `crossterm` を既定 backend とし、`ratatui-core` / `ratatui-widgets` に分割されている。MSRV は 1.88。
  - crossterm 0.29 はキーイベント、リサイズ、代替画面、raw mode を提供する。ratatui の `Terminal` と組み合わせて差分描画できる。
  - `TestBackend` により、端末なしで描画結果を検証できる。
- **Implications**:
  - `event::poll` + `event::read` の単一スレッドイベントループに、バックグラウンド計算からの `mpsc` メッセージを統合する。非同期ランタイムは導入しない。
  - 端末復元は RAII ガードと panic hook の両方で行う。
  - 画面サイズが小さい場合は、グリッドを描画せず最小サイズメッセージを表示する。

### CSV の読み込み

- **Context**: 要件 1.3、1.6 を満たすため、引用符内改行を含む CSV を正しく解析する必要がある。
- **Sources Consulted**:
  - [csv 1.4.0 - crates.io](https://crates.io/crates/csv)
  - [csv crate ドキュメント](https://docs.rs/csv/latest/csv/)
- **Findings**:
  - `csv` クレートは RFC 4180 に準拠し、引用符、引用符内改行、可変列数を扱える。
  - BOM は自動除去されないため、読み込み前に UTF-8 BOM を除去する必要がある。
  - 列数が行ごとに異なる場合は `flexible(true)` が必要である。
- **Implications**:
  - CSV は 1 シートとしてモデル化し、最大列数に合わせて不足セルを空で埋める。
  - CSV の文字列は数式として評価しない。`=` で始まる値もテキストとして保持する。
  - 文字コードは UTF-8（BOM 含む）を基本としつつ、要件 11 に従って BOM → `--encoding` → `chardetng` の順で判定する。自動判定時の不正バイトは置換文字で継続し、明示指定の失敗はエラーとする。

### クリップボード

- **Context**: 要件 6.1〜6.8 を満たすため、ローカル・SSH・tmux の各環境でコピーを成立させる必要がある。
- **Sources Consulted**:
  - [OSC 52 解説](https://tpoe.dev/blog/osc52)
  - [tmux と OSC 52 の実践](https://medium.com/free-code-camp/tmux-in-practice-integration-with-system-clipboard-bcd72c62ff7b)
  - [arboard 3.7.0 `Cargo.toml`](https://github.com/1Password/arboard/blob/master/Cargo.toml)
  - [base64 0.23.1 - crates.io](https://crates.io/crates/base64)
- **Findings**:
  - OSC 52 は端末へ base64 文字列を送るだけで、SSH 越しでもローカル端末のクリップボードを更新できる。tmux では passthrough 設定が必要な場合がある。
  - OSC 52 のペイロードには端末・tmux 側の上限があり、大きな範囲コピーでは失敗・切り詰めの可能性がある。
  - arboard 3.7 は Linux で x11rb / wl-clipboard-rs を使い、Rust クレートとして完結する。GUI セッションがない場合は失敗する。
  - X11 のクリップボードはアプリ終了時に内容が失われる場合があるため、OSC 52 の方が永続性で優れる。
- **Implications**:
  - `ClipboardSink` trait を設け、`Osc52Sink` と `SystemClipboardSink` を実装する。
  - 既定は `auto`: SSH / tmux 環境では OSC 52、ローカル GUI では arboard を試し、失敗時は OSC 52 へフォールバックする。
  - 範囲コピーが OSC 52 の上限を超える場合は、明示的なエラーと `--clipboard system` の案内を表示する。
  - コピー文字列の生成（TSV 引用符処理）はドメイン側の純粋関数とし、単体テストする。

### 表示幅・折り返し・色

- **Context**: 要件 2.2〜2.9、8.8 を満たすため、全角文字・結合文字・絵文字を含む表示幅と、端末色のフォールバックを設計する必要があった。
- **Sources Consulted**:
  - [unicode-width 0.2.3 - crates.io](https://crates.io/crates/unicode-width)
  - [textwrap 0.16.4 - crates.io](https://crates.io/crates/textwrap)
  - [ratatui スタイル](https://docs.rs/ratatui/latest/ratatui/style/)
- **Findings**:
  - `UnicodeWidthStr::width` は東アジア全角・結合文字を考慮した表示幅を返す。文字数ではなく表示幅で列幅を計算する必要がある。
  - `textwrap` は Unicode の単語境界と表示幅を考慮した折り返しを提供する。
  - ratatui は `Color::Rgb` / `Color::Indexed` / `Color::Reset` と `Modifier::BOLD` を提供する。端末が truecolor でない場合は近似色が必要になる。
  - Excel の既定文字色は黒であり、暗色背景の端末ではそのまま描画すると読めない。
- **Implications**:
  - レイアウトは `unicode-width` の表示幅で計算し、`wrap_text` が有効なセルのみ `textwrap` で折り返す。
  - 文字色は「ほぼ黒」「ほぼ白」を端末既定色へマップし、それ以外は truecolor / 256 色 / 16 色の順で近似する。
  - 太字は `Modifier::BOLD`、色は `Style::fg` に変換する。端末が色をサポートしない場合は装飾を落として文字を優先する。

### 検索の方式

- **Context**: 要件 9 は現在シートの表示値と数式を対象に、一致間を移動できる検索を求める。大規模シートでも UI を止めない必要がある。
- **Sources Consulted**:
  - [regex 1.13.1 - crates.io](https://crates.io/crates/regex)
  - [Vim の検索とスマートケース](https://vimhelp.org/pattern.txt.html)
  - `ratatui` の `TestBackend` による UI テスト
- **Findings**:
  - リテラル部分一致とスマートケースで要件を満たせる。正規表現は初期スコープに対して過剰で、ユーザー入力のコンパイルエラー処理も増える。
  - 一致セルのアドレスだけを保持すれば、メモリは一致数に比例する。全セルの転置インデックスは不要である。
  - 検索は別スレッドで行い、進捗と結果を `mpsc` で送ることで、数万行でも UI を止めずに済む。
  - フィルタ適用中は一致のうち可視行のみを移動対象とし、非表示の一致件数を別に数える。
- **Implications**:
  - `SearchService` は現在シートの `RowSource` / `CellStore` を走査し、`SearchIndex` に一致アドレスを蓄積する。
  - 検索語はリテラル、スマートケース、シート単位。シート切り替えで結果を破棄する。
  - 検索中のキャンセルと、新しい検索による世代管理が必要である。

### Excel オートフィルタの読み取り

- **Context**: 要件 10 は Excel に保存されたオートフィルタを読み取り、表示に適用する。calamine は値・スタイル・結合を提供するが、フィルタ条件の API を持たない。
- **Sources Consulted**:
  - [OOXML AutoFilter の構造](https://c-rex.net/samples/ooxml/e1/Part3/OOXML_P3_Primer_AutoFilter_topic_ID0EYLGK.html)
  - [openpyxl のフィルタ解説](https://openpyxl.readthedocs.io/en/stable/filters.html)
  - [calamine のテーブル API](https://docs.rs/calamine/latest/calamine/struct.Xlsx.html)
  - ローカルの calamine / umya-spreadsheet ソース（`autoFilter` の有無を確認）
- **Findings**:
  - calamine 0.38 は `autoFilter` の条件を公開しない。umya-spreadsheet は `AutoFilter` と `FilterColumn` を持つが、`colId` と `hiddenButton` のみで、条件の子要素は解析しない。
  - OOXML の `autoFilter` は `ref` と `filterColumn` を持ち、`filterColumn` の子に `filters`（値リスト）、`customFilters`、`top10`、`dynamicFilter`、`colorFilter`、`iconFilter`、`dateGroupItem` が入る。
  - テーブルのフィルタは `xl/tables/tableN.xml` の `autoFilter` に保存される。ワークシートの `autoFilter` とは別に読み取る必要がある。
  - 値リストとカスタム条件は、セルの型付き値と表示値の両方を使って比較できる。テキスト比較は大文字小文字を区別せず、`*` / `?` のワイルドカードと `~` エスケープを扱う。
  - Top10 は列ごとのランキングが必要で、全行を走査して上位 N 件のマスクを作る。複数列のフィルタは列ごとのマスクの論理積で合成できる。
- **Implications**:
  - `SheetMetadataParser` を `io/sheet_meta.rs` に置き、`zip` + `quick-xml` でワークシートとテーブルの `autoFilter`、および固定ペインの `pane` を読む。
  - 値リスト・カスタム条件・Top10 を `FilterCriterion` に正規化し、`FilterEngine` が列ごとの可視マスクを生成する。
  - 色・アイコン・動的フィルタ・日付グループの一部は未対応として警告し、該当列を未適用にする。フィルタ条件の編集は行わない。
  - フィルタは読み取り専用のビュー状態であり、元ファイルを書き換えない。

### CSV の文字コード判定とストリーミングデコード

- **Context**: 要件 11 は Shift_JIS / EUC-JP などの csv を文字化けせずに読み込み、大容量ファイルでもメモリを抑えることを求める。
- **Sources Consulted**:
  - [chardetng 1.0.0 - crates.io](https://crates.io/crates/chardetng)
  - [encoding_rs 0.8.44 - crates.io](https://crates.io/crates/encoding_rs)
  - [encoding_rs_io 0.1.8 - crates.io](https://crates.io/crates/encoding_rs_io)
  - [csv 1.4.0 - docs.rs](https://docs.rs/csv/latest/csv/)
- **Findings**:
  - `chardetng` は Firefox 由来の文字コード判定器で、UTF-8、Shift_JIS、EUC-JP、UTF-16 などを扱える。BOM は呼び出し側で優先する必要がある。
  - `encoding_rs` は BOM スニッフィングとストリーミングデコードを提供し、Shift_JIS / CP932 / EUC-JP / UTF-16 をデコードできる。
  - 判定は短いサンプルでは誤る可能性があるため、`--encoding` による上書きと、判定結果の表示が必要である。
  - csv の引用符内改行を扱うには、デコード後の文字列を csv パーサーに渡す必要がある。大容量時は UTF-8 のスピルファイルに行単位で書き出す。
- **Implications**:
  - `EncodingDetector` は BOM → 明示指定 → `chardetng` の順で判定し、`EncodingInfo` を返す。
  - `CsvLoader` はサンプル判定後、ストリーミングデコードしながら UTF-8 のスピルファイルと行オフセットを構築する。
  - 自動判定で不正バイトが出た場合は置換文字で継続し、警告を表示する。明示指定の失敗は致命的エラーとする。

### 大容量 xlsx / csv の遅延ロード

- **Context**: 要件 12 は数万〜数百万行のファイルで、総行数に比例しないメモリ使用量と応答性を求める。
- **Sources Consulted**:
  - [calamine `XlsxCellReader`](https://docs.rs/calamine/latest/calamine/struct.XlsxCellReader.html)
  - [calamine `Xlsx::worksheet_cells_reader`](https://docs.rs/calamine/latest/calamine/struct.Xlsx.html)
  - [tempfile 3.27.0 - crates.io](https://crates.io/crates/tempfile)
  - [postcard 1.1.3 - crates.io](https://crates.io/crates/postcard)
  - ローカルの calamine ソース（`cells_reader.rs`）
- **Findings**:
  - calamine 0.38 は `worksheet_cells_reader(name)` で `XlsxCellReader` を返し、`next_cell_with_style_id()` / `next_cell_with_formula()` / `next_formula()` でセルを XML 順にストリーミングできる。値とスタイル ID を 1 パスで取得できる。
  - ストリーミングリーダーは先頭からの逐次読み取りであり、任意行へシークする API はない。行単位のランダムアクセスには、一度走査して行キャッシュを構築する必要がある。
  - `tempfile` でスピルファイルを作り、行ごとのオフセットを保持すれば、メモリをキャッシュ行数に比例させられる。`postcard` は純 Rust のコンパクトなシリアライザで、行キャッシュの保存に向く。
  - 列幅・行高の統計とフィルタの列マスクは、インデックス構築時の 1 パスで計算できる。行高は 1 行 1 バイト、可視マスクは 1 行 1 ビットで保持できる。
  - csv は UTF-8 スピルファイルとレコードオフセットで同じ行キャッシュ抽象化を利用できる。
  - 数式文字列はストリーミングパスで別途収集し、数式セルのみのマップとして保持する。計算結果はオーバーレイとして未読み込み行にも適用できる。
- **Implications**:
  - `SheetData` を `Eager(CellStore)` と `Lazy(LazySheet)` の enum にし、`RowSource` trait で xlsx / csv の行供給を抽象化する。
  - 遅延モードの閾値はセル数とファイルサイズの両方で判定し、`--lazy auto|always|never` で上書きできる。
  - `RowWindowCache` は LRU で上限行数を管理し、可視範囲の先読みを行う。
  - インデックス構築は進捗イベントを出し、キャンセルトークンで中断できるようにする。
  - メモリ目標は「キャッシュ行数 + 行オフセット + 行高 + 可視マスク + 数式マップ + 検索一致数」に比例させる。

### 行固定（フリーズペイン）の読み取り

- **Context**: 要件 13 は Excel で固定された行（および列）を、スクロール中も表示することを求める。calamine は固定ペイン情報を公開しない。
- **Sources Consulted**:
  - [OOXML の固定ペイン解説](https://www.bennadel.com/blog/467-freezing-row-column-panes-in-excel-xml.htm)
  - [Microsoft: Freeze panes](https://support.microsoft.com/en-us/excel/get-started/freeze-panes-to-lock-rows-and-columns)
  - [Stack Overflow: OOXML freeze panes](https://stackoverflow.com/questions/67718003/office-open-xml-freeze-panes-in-excel)
  - ローカルの calamine / umya-spreadsheet ソース（`pane` の公開 API を確認）
- **Findings**:
  - xlsx の固定ペインは `xl/worksheets/sheetN.xml` の `<sheetView><pane .../></sheetView>` に保存される。
  - `state="frozen"` の場合、`ySplit` が固定行数、`xSplit` が固定列数を表す。`topLeftCell` はスクロール領域の左上セル、`activePane` はアクティブペインを示す。
  - `state="split"` / `frozenSplit` は分割ペインであり、固定表示とは意味が異なる。初期実装では `frozen` のみを対象とし、それ以外は警告して無効化する。
  - 固定境界が結合セルと交差すると、表示の二重化や切り詰めが発生する。Excel でも固定と結合の組み合わせは制限があるため、該当シートの固定を無効化するのが安全である。
  - 固定行はフィルタの非表示対象にしない。フィルタは固定行より下のデータ行に適用する。
  - 固定領域が表示領域を超える場合、固定領域をそのまま描画するとスクロール領域がなくなる。固定領域を上限付きにし、少なくとも数行のスクロール領域を残す必要がある。
- **Implications**:
  - `SheetMetadataParser` が `autoFilter` と `pane` を同じワークシートスキャンで読み取り、`FreezePane` を返す。
  - `FreezePane` は固定行数・固定列数を持ち、結合範囲との交差や範囲外を検証する。
  - `LayoutEngine` は `RenderPlan` を固定領域とスクロール領域に分割し、`GridRenderer` が両者を別々に描画する。
  - ナビゲーションは固定行・スクロール行を区別せず、シートの行順で扱う。スクロール時は固定行を差し引いた領域でフォーカスを可視化する。

### アーキテクチャパターンの評価

- **Context**: 表示・I/O・数式・入力・設定が混在すると、テストと変更が困難になる。
- **Sources Consulted**:
  - プロジェクト内の steering（`product.md` / `tech.md` / `structure.md`）
  - [design-principles.md](../../settings/rules/design-principles.md)
- **Findings**:
  - 単一の `App` にすべてを詰め込む方式は初期実装が速いが、レイアウト計算・コピー整形・ローダーのテストが難しくなる。
  - 完全な Hexagonal はアダプタ層が過剰になり、現時点の規模に対して抽象化が多い。
  - レイヤード + 小さな trait 境界（ローダー、数式計算、クリップボード）が、テスト容易性と実装コストのバランスに優れる。
- **Implications**:
  - `domain` を純粋に保ち、`io` / `ui` / `input` / `config` をアダプタ、`app` をオーケストレータとする。
  - 状態遷移は `Action -> Vec<Effect>` の純粋関数として表現し、イベントループは Effect を実行するだけにする。

## Architecture Pattern Evaluation

| Option | Description | Strengths | Risks / Limitations | Notes |
|--------|-------------|-----------|---------------------|-------|
| 単一構造体 + 直接呼び出し | `App` がローダー・描画・入力をすべて保持 | 実装が最短 | テスト困難、責務過多、依存方向が崩れやすい | 不採用 |
| フル Hexagonal | Ports & Adapters を全機能に適用 | 境界が明確 | 現規模では抽象化過多、ファイル数増加 | 部分採用 |
| レイヤード + 小さな trait 境界 | domain を中心に io/ui/input/config を分離 | テスト容易、依存方向が明確、拡張しやすい | 境界型の変換コードが必要 | 採用 |
| イベントソーシング | 変更履歴をイベントとして保持 | 監査性が高い | 表示専用アプリには過剰 | 不採用 |

## Design Decisions

### Decision: calamine と formualizer の責務分離

- **Context**: calamine は値・スタイル・結合範囲に強いが数式評価をしない。formualizer は数式評価に強いが、表示用の細かいスタイル取得を目的としていない。
- **Alternatives Considered**:
  1. calamine のみを使い、キャッシュ値だけを表示する。
  2. formualizer のみで読み込み・計算・表示モデルを構築する。
  3. calamine で表示モデル、formualizer で数式計算を担当する。
- **Selected Approach**: 3 を採用する。xlsx を calamine で読み込み、数式セルの再計算だけを formualizer の専用スレッドで行う。
- **Rationale**: 表示に必要なスタイル・結合・値の取得が確実で、数式計算を UI から分離できる。計算に失敗してもキャッシュ値で閲覧を継続できる。
- **Trade-offs**: 同じファイルを 2 回読むため、I/O とメモリが増える。読み込み中のファイル変更を検出する必要がある。
- **Follow-up**: 読み込み時のファイルサイズ・更新時刻を記録し、再計算前に一致を確認する。

### Decision: 数式再計算のバックグラウンド実行

- **Context**: 大きなワークブックの再計算は数秒かかる場合があり、UI をブロックすると要件 3.6、8.2 を満たせない。
- **Alternatives Considered**:
  1. 起動時に同期計算し、完了まで待つ。
  2. 別スレッドで計算し、完了後に差分を反映する。
  3. ユーザーが明示的に再計算キーを押したときだけ計算する。
- **Selected Approach**: 2 を採用する。キャッシュ値で即座に描画し、計算結果を `mpsc` で受け取って数式セルだけを更新する。
- **Rationale**: 初期表示を高速に保ち、計算中も閲覧・コピー・終了ができる。
- **Trade-offs**: 表示値が一時的にキャッシュ値と計算値で混在する。状態表示と詳細表示で評価状態を明示する。
- **Follow-up**: 計算中のキャンセル（終了時）と、計算失敗時のフォールバックをテストする。

### Decision: 独自レイアウトと表示上限

- **Context**: Excel の列幅は文字数とフォントに依存し、TUI の等幅セルへ直接マッピングできない。要件 2.2〜2.5 は表示内容に基づく自動調整を求める。
- **Alternatives Considered**:
  1. xlsx の列幅・行高をそのまま使う。
  2. 固定幅・固定行高にする。
  3. 表示内容から列幅・行高を計算し、上限を設ける。
- **Selected Approach**: 3 を採用する。列幅は表示幅の最大値、行高は改行・折り返し行数から計算し、既定上限を 65 桁 / 10 行とする。
- **Rationale**: ターミナル上で読みやすく、要件の上限設定を自然に実現できる。
- **Trade-offs**: Excel の見た目とは一致しない。巨大シートではレイアウト計算のコストがかかる。
- **Follow-up**: eager ではモデル構築時に 1 回計算し、lazy ではインデックス構築時の 1 パスで列幅・行高統計を収集する。10 万セル（eager）と 100 万行（lazy）で性能を計測する。

### Decision: はみ出し表示の規則

- **Context**: 要件 2.4 は「別のデータと干渉しないなら表示」を求めるが、複数行・結合セルとの組み合わせを定義する必要がある。
- **Alternatives Considered**:
  1. 常にセル幅で切り詰める。
  2. 常に隣接セルへはみ出させる。
  3. 単一行セルのみ、隣接セルが空かつ結合範囲でない限りはみ出させる。
- **Selected Approach**: 3 を採用する。複数行セル（改行または `wrap_text`）はセル幅で折り返し・切り詰め、水平方向のはみ出しをしない。
- **Rationale**: 複数行のはみ出しは行高・結合・選択の整合性を壊しやすい。単一行の Excel 風はみ出しに限定すると規則が単純でテストしやすい。
- **Trade-offs**: 長い複数行セルはグリッドでは途中までしか見えない。詳細表示とコピーで全文を提供する。
- **Follow-up**: 結合範囲からの はみ出しは、結合範囲の幅を超えた場合に同じ規則を適用する。

### Decision: セル罫線と既定グリッド線の表現

- **Context**: 要件 2.10〜2.12 は Excel の罫線と、罫線がないセルのグリッド線を端末上で判別できる形で表示することを求める。
- **Alternatives Considered**:
  1. 罫線専用の行・列を確保し、全セルを box-drawing で囲む。
  2. 縦罫線のみ専用列を確保し、横罫線は下線で表現する。
  3. 罫線を描かず、背景色や余白だけで区切る。
- **Selected Approach**: 2 を採用する。列ごとに 1 表示列のガターを確保し、明示的な縦罫線は線種に応じた box-drawing 文字、横罫線は行最下行の下線で描く。明示罫線がない境界には淡色の既定グリッド線を描く。
- **Rationale**: 行高を増やさずに罫線を表示でき、表の閲覧に必要な行数を維持できる。縦線は幅を 1 列消費するが、列の区切りが明確になる。
- **Trade-offs**: 横罫線の太線・二重線は下線では完全再現できない。下線色（SGR 58）非対応端末では下線が文字色になる。罫線の色は端末の色モードに依存する。
- **Follow-up**: 明示罫線がある境界では文字のはみ出しを止める。結合範囲の外枠は結合範囲全体に伸ばし、内部の横グリッド線は抑制する。

### Decision: クリップボード戦略

- **Context**: SSH / tmux とローカル GUI の両方でコピーを成立させる必要がある。
- **Alternatives Considered**:
  1. arboard のみ。
  2. OSC 52 のみ。
  3. 環境に応じて arboard と OSC 52 を切り替える。
- **Selected Approach**: 3 を採用する。`--clipboard auto` を既定とし、SSH / tmux では OSC 52、ローカルでは system clipboard を試して失敗時に OSC 52 へフォールバックする。
- **Rationale**: 各環境の得意な経路を使い、失敗時も操作を継続できる。
- **Trade-offs**: 端末・tmux の設定によっては OSC 52 が無効で、コピー成功を保証できない。
- **Follow-up**: クリップボード trait を注入してテストし、失敗時のメッセージに `--clipboard` の選択肢を含める。

### Decision: CSV を単一シート・テキスト専用として扱う

- **Context**: CSV にはシート・書式・数式の概念がない。xlsx と同じモデルに押し込む際の境界が必要である。
- **Alternatives Considered**:
  1. CSV の `=` 始まりを数式として評価する。
  2. CSV をテキストのみの単一シートとして扱う。
- **Selected Approach**: 2 を採用する。
- **Rationale**: CSV は表計算の計算モデルを持たず、数式評価は予期しない挙動とセキュリティリスクを生む。要件 1.6 とも一致する。
- **Trade-offs**: CSV 内の数式文字列は計算されない。
- **Follow-up**: CSV ローダーのテストで `=1+1` が文字列のまま保持されることを確認する。

### Decision: 検索はリテラル・シート単位・バックグラウンド走査

- **Context**: 要件 9 は検索と一致間移動を求める。正規表現や全シート検索は初期スコープを超える。
- **Alternatives Considered**:
  1. 正規表現検索を最初から提供する。
  2. リテラル部分一致を現在シートに対して提供する。
  3. 全シートを対象にした転置インデックスを構築する。
- **Selected Approach**: 2 を採用する。スマートケースのリテラル部分一致とし、一致アドレスのみを保持する。
- **Rationale**: 要件を満たし、入力エラー処理とメモリ消費を抑えられる。Vim 風の `/`、`n`、`N` と自然に統合できる。
- **Trade-offs**: 正規表現やシート横断検索はできない。
- **Follow-up**: 将来の拡張点として `SearchQuery` にモードを追加できるようにする。

### Decision: Excel フィルタは読み取り専用の可視マスクとして適用

- **Context**: 要件 10 は Excel のフィルタ条件を利用するが、フィルタの編集・保存は表示専用の範囲外である。
- **Alternatives Considered**:
  1. フィルタ条件を TUI で編集し、xlsx に書き戻す。
  2. Excel の条件を読み取り、表示の可視マスクとして適用する。
  3. フィルタを無視し、検索だけを提供する。
- **Selected Approach**: 2 を採用する。列ごとの可視マスクを構築し、全体または列単位で適用・解除できる。
- **Rationale**: 元ファイルを変更せず、Excel の意図した表示を再現できる。フィルタの編集・保存に伴う複雑さを回避できる。
- **Trade-offs**: 色・アイコン・動的フィルタなど一部の条件は未対応となる。
- **Follow-up**: 未対応条件は警告し、該当列を未適用にする。フィルタマスクは行インデックス構築時に計算する。

### Decision: CSV は BOM 優先・自動判定・明示上書き

- **Context**: 日本語環境では Shift_JIS / CP932 / EUC-JP の csv が多く、UTF-8 前提では文字化けする。
- **Alternatives Considered**:
  1. UTF-8 のみ対応する。
  2. `chardetng` で自動判定し、`--encoding` で上書きする。
  3. 常にユーザーが文字コードを指定する。
- **Selected Approach**: 2 を採用する。BOM を最優先し、次に `--encoding` の明示指定、最後に `chardetng` のサンプル判定を使う。
- **Rationale**: 通常のケースを自動化しつつ、誤判定時の逃げ道を提供できる。
- **Trade-offs**: 短いファイルや混在エンコーディングでは誤判定の可能性がある。判定結果の表示と警告が必要である。
- **Follow-up**: 自動判定の結果を `SourceInfo` に保持し、ステータスバーに表示する。不正バイトは置換文字で継続する。

### Decision: 固定ペインは読み取り専用の表示分割として扱う

- **Context**: 要件 13 は Excel の固定行（および列）をスクロール中も表示する。固定ペインの編集・保存は表示専用の範囲外である。
- **Alternatives Considered**:
  1. 固定ペインを無視して全行をスクロールする。
  2. `pane` を読み取り、固定領域とスクロール領域を別々に描画する。
  3. 固定ペインの編集・保存まで提供する。
- **Selected Approach**: 2 を採用する。`state="frozen"` の `ySplit` / `xSplit` を読み、固定行・固定列をグリッドの別領域として描画する。
- **Rationale**: Excel の見出し行を保った閲覧体験を再現でき、元ファイルを変更しない。
- **Trade-offs**: 固定領域が大きい場合にスクロール領域が狭くなる。固定境界と結合セルが交差する場合は固定を無効化する。
- **Follow-up**: 固定領域の上限、リサイズ、フィルタ・検索・コピーとの相互作用をテストする。

### Decision: 行キャッシュ方式の遅延ロード

- **Context**: calamine のストリーミングリーダーは逐次読み取りで、任意行へシークできない。大容量 xlsx の行単位アクセスには中間キャッシュが必要である。
- **Alternatives Considered**:
  1. シート全体をメモリに読み込む。
  2. 行インデックスだけを構築し、毎回先頭から再走査する。
  3. スピルファイルに行キャッシュを構築し、行オフセットで読み出す。
- **Selected Approach**: 3 を採用する。インデックス構築時に値・スタイル・数式・レイアウト統計・フィルタマスクを 1 パスで収集し、行データを `tempfile` に保存する。
- **Rationale**: メモリ使用量をキャッシュ行数に比例させ、任意行へのアクセスを高速化できる。csv も同じ `RowSource` 抽象化で扱える。
- **Trade-offs**: 初回にインデックス構築の一時コストとディスク使用が発生する。スピルファイルの I/O エラー処理が必要である。
- **Follow-up**: 閾値以下は eager モードで従来どおりメモリに保持する。インデックス構築は進捗表示とキャンセルに対応する。

## Risks & Mitigations

- **大規模ファイルのレイアウト計算コスト** — eager は 10 万セルを目標に計測し、lazy はインデックス構築時の 1 パスで列幅・行高統計を収集する。100 万行規模でインデックス構築時間とキャッシュ行数を計測する。
- **数式エンジンの未対応関数・外部リンク** — キャッシュ値へフォールバックし、詳細画面で評価状態を表示する。外部リンクは更新しない。
- **端末による色・OSC 52 の非対応** — 色は truecolor / 256 色 / 16 色 / 無装飾へ段階的にフォールバックする。コピーは system clipboard と OSC 52 の二経路を用意する。
- **xlsx の破損・zip 爆弾** — ローダーのエラーを捕捉し、ユーザーにパスと理由を表示する。ファイルサイズの上限は実装時に再検討する。
- **数式計算スレッドの残留** — 終了時にキャンセルトークンを立て、ワーカーの完了を待つかデタッチする。UI 終了をブロックしない。
- **端末復元漏れ** — RAII ガードと panic hook を併用し、異常終了時も raw mode / alternate screen を戻す。
- **オートフィルタの未対応条件** — 値リスト・カスタム条件・Top10 を実装し、色・アイコン・動的フィルタは警告して列単位で未適用にする。無言で全行表示しない。
- **CSV 文字コードの誤判定** — 判定結果を表示し、`--encoding` で上書き可能にする。不正バイトは置換文字で継続する。
- **遅延モードのインデックス構築コスト** — 進捗表示とキャンセルを提供し、閾値以下は eager モードにする。インデックスは 1 パスでレイアウト・フィルタ・数式マップを兼ねる。
- **スピルファイルのディスク消費 / 漏えい** — `tempfile` の安全な一時領域を使い、Drop で削除する。ディスク不足時はシート単位のエラーとして扱う。
- **検索のメモリ消費** — 一致アドレスのみを保持し、検索語変更・シート切り替えで破棄する。
- **固定ペインと結合セルの交差** — 固定境界が結合範囲と交差する場合は、該当シートの固定を無効化して警告する。
- **固定領域が広すぎる場合** — 固定行・列を上限付きにし、スクロール領域を最低限確保する。制限をステータスに表示する。

## References

- [calamine](https://github.com/tafia/calamine) — xlsx / xls / ods の純 Rust リーダー。値・数式・スタイル・結合範囲を取得する。
- [formualizer](https://github.com/psu3d0/formualizer) — Excel 互換の数式エンジン。バックグラウンド再計算に使用する。
- [ratatui](https://ratatui.rs/) — TUI 描画フレームワーク。`TestBackend` でスナップショットテストが可能。
- [crossterm](https://github.com/crossterm-rs/crossterm) — 端末入力・raw mode・代替画面。
- [csv](https://github.com/BurntSushi/rust-csv) — RFC 4180 準拠の CSV パーサー。
- [unicode-width](https://github.com/unicode-rs/unicode-width) — 表示幅計算。
- [textwrap](https://github.com/mgeisler/textwrap) — Unicode 対応の折り返し。
- [arboard](https://github.com/1Password/arboard) — OS クリップボード。
- [OSC 52](https://tpoe.dev/blog/osc52) — 端末経由のクリップボード更新。
- [chardetng](https://github.com/hsivonen/chardetng) — 文字コード自動判定。
- [encoding_rs](https://github.com/hsivonen/encoding_rs) — 文字コードのデコード / エンコード。
- [OOXML AutoFilter](https://c-rex.net/samples/ooxml/e1/Part3/OOXML_P3_Primer_AutoFilter_topic_ID0EYLGK.html) — オートフィルタの XML 構造。
- [OOXML freeze panes](https://www.bennadel.com/blog/467-freezing-row-column-panes-in-excel-xml.htm) — `pane` の `xSplit` / `ySplit` / `state` の構造。
- [openpyxl filters](https://openpyxl.readthedocs.io/en/stable/filters.html) — 値リスト・カスタム条件・Top10・動的フィルタの整理。
- [tempfile](https://github.com/Stebalien/tempfile) — スピルファイル。
- [postcard](https://github.com/jamesmunns/postcard) — 行キャッシュのシリアライズ。
