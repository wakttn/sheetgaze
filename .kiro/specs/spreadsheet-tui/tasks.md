# Implementation Plan

## 1. Foundation and Contracts

- [x] 1.1 Create the Rust project skeleton and build configuration
  - Cargo manifest, toolchain, module tree, lint/format/test commands
  - Dependencies pinned to the versions in the design; system clipboard optional
  - `cargo check` succeeds on an empty binary/library
  - _Requirements: 8.1_

- [x] 1.2 Implement shared domain value types and task tokens
  - Cell address/range/A1 conversion, cell values, effective cells, styles, modes, cancellation tokens
  - Unit tests for coordinate conversion, width-sensitive text, and empty/error values
  - Domain types compile without TUI or reader crates in public signatures
  - _Boundary: Domain value types_
  - _Requirements: 1.2, 2.2, 3.4, 8.8, 8.9_

- [x] 1.3 Implement CLI configuration and error model
  - File path, width/height limits, sheet, color, clipboard, encoding, lazy, recalc, filter, copy limits
  - Validation errors include option names and ranges; `--help`/`--version` work
  - Invalid options exit non-zero with a readable message
  - _Boundary: CliConfig, error types_
  - _Requirements: 1.4, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 10.7, 11.4, 12.1, 12.6_

- [x] 1.4 Create fixture generation and baseline test harness
  - Generate small xlsx/csv fixtures for values, styles, merges, formulas, filters, freeze panes, encodings, and large data
  - Fixture generation is ignored by default and documented as a test command
  - `cargo test` runs without regenerating fixtures
  - _Requirements: 1.2, 1.3, 10.1, 13.1_

## 2. Data Ingestion and Calculation

- [x] 2.1 Implement CSV encoding detection and loader
  - BOM priority, explicit override, sample detection, streaming UTF-8 spill for large files
  - RFC 4180 quoting, embedded newlines, variable columns, BOM removal
  - CSV values remain text; `=1+1` is not evaluated
  - Encoding label and warnings are exposed to the app
  - _Boundary: EncodingDetector, CsvLoader_
  - _Requirements: 1.3, 1.6, 8.9, 11.1, 11.2, 11.3, 11.4, 11.5, 11.6, 11.7, 12.2_

- [x] 2.2 (P) Implement the eager xlsx loader
  - Stream values and styles with calamine, collect formulas and cached values, read merged regions
  - Build a workbook with sheet order, used range, and effective display strings
  - Invalid sheets/cells degrade to errors rather than panicking
  - _Boundary: XlsxLoader_
  - _Requirements: 1.1, 1.2, 2.6, 2.7, 3.1, 8.1, 8.2, 8.7, 8.9, 13.1_

- [x] 2.3 (P) Implement sheet metadata parsing for filters and freeze panes
  - Parse worksheet and table autoFilter conditions; parse frozen pane split values
  - Validate filter ranges against used range/merges and freeze boundaries against merges
  - Unsupported conditions and split panes produce warnings and safe degradation
  - _Boundary: SheetMetadataParser, FilterDefinition, FreezePane_
  - _Requirements: 10.1, 10.3, 10.4, 10.9, 13.1, 13.6_

- [x] 2.4 Implement background formula calculation
  - Run formualizer in a worker; map 0-based addresses to 1-based engine coordinates
  - Apply results as an overlay; preserve cached values on failure/unsupported/circular/external links
  - Auto policy skips recalculation above configured lazy limits and reports the reason
  - _Boundary: FormulaService, RecalcConfig_
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 3.9, 7.5, 7.6, 8.2, 8.7, 8.9, 12.9_

## 3. Layout, Selection, and Copy

- [x] 3.1 Implement cell formatting and merged-cell resolution
  - General-style number/text/date/error display; effective formula values
  - Merge anchor lookup, covered-cell detection, used-range tracking
  - Long/multiline values and wide characters preserve full content for detail/copy
  - _Boundary: CellFormatter, MergeIndex_
  - _Requirements: 1.2, 2.2, 2.6, 2.7, 3.4, 5.1, 8.8, 8.9_

- [x] 3.2 (P) Implement the layout engine and render plan
  - Column widths and row heights with limits, wrapping, multiline, merge distribution
  - Single-line overflow into empty unmerged neighbors; clipping/ellipsis otherwise
  - Frozen rows/columns split into fixed and scrollable render regions
  - Observable: a viewport produces a render plan with no duplicate cell draws
  - _Boundary: LayoutEngine, LayoutStatsBuilder_
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.8, 2.9, 3.7, 4.7, 8.2, 8.3, 8.8, 12.5, 13.2, 13.3, 13.5, 13.7_

- [x] 3.3 (P) Implement selection and streaming copy
  - Rectangular/row selection normalization, visible-row handling, combined-cell output
  - TSV quoting/CRLF, single-cell copy, copy limits, streaming CopySession
  - Copy of a range larger than the row cache completes without LRU thrashing
  - _Boundary: Selection, CopyEncoder, CopySession_
  - _Requirements: 4.4, 4.5, 4.6, 6.1, 6.2, 6.3, 6.4, 6.7, 6.8, 10.8, 13.11_

## 4. Lazy Loading

- [x] 4.1 Implement row sources, row index, and LRU window cache
  - Range loading, scan-all for search/filter, capacity and prefetch controls
  - Pending rows are distinguishable from empty rows
  - Observable: resident rows never exceed the configured cache size
  - _Boundary: RowSource, RowWindowCache, LazyConfig_
  - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.6, 12.10_

- [x] 4.2 Implement xlsx lazy indexing and row cache
  - One streaming pass for offsets, layout stats, formula cells, filter masks
  - Spill rows with postcard; first window loads after index; progress/cancel supported
  - Index failure is reported per sheet while other sheets remain available
  - _Boundary: XlsxLoader lazy path, SpillRowCache_
  - _Requirements: 2.5, 9.7, 12.2, 12.3, 12.7, 12.8, 12.10_

- [x] 4.3 Implement CSV lazy path and row-loading effects
  - UTF-8 spill and record offsets; row window loading from the spill
  - UI shows a loading placeholder and remains responsive while rows load
  - _Boundary: CsvLoader lazy path, AppState row loading_
  - _Requirements: 12.1, 12.2, 12.4, 12.5, 12.7, 12.8, 11.7_

## 5. Search and Filters

- [x] 5.1 Implement search index and background search service
  - Smart-case literal search over display and formula text using effective values
  - Filter-aware next/previous navigation and hidden-match count
  - Search results can be cancelled and are invalidated on sheet/formula changes
  - _Boundary: SearchIndex, SearchService, SearchSourceFactory_
  - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 9.8, 9.9, 9.10, 12.5_

- [x] 5.2 Implement filter evaluation and visibility
  - Value lists, custom AND/OR conditions, wildcards, Top10, unsupported warnings
  - Column masks combine into a visibility map; frozen rows remain visible
  - Saved filter conditions apply by default; `--no-filter` shows all rows
  - _Boundary: FilterEngine, VisibilityMap, FilterState_
  - _Requirements: 9.7, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7, 10.8, 10.9, 10.10, 13.8_

## 6. TUI and Application

- [x] 6.1 Implement view models, screen layout, grid, and theme
  - Owned ScreenView/StatusView/DetailView/HelpView; no AppState dependency in UI
  - Headers, sheet tabs, selection, cursor, search highlight, frozen regions, min-size message
  - TestBackend snapshot renders a normal viewport and a frozen viewport
  - _Boundary: ScreenView, Screen, GridRenderer, Theme_
  - _Requirements: 1.5, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9, 3.2, 3.5, 3.7, 3.8, 4.7, 8.3, 8.4, 8.5, 8.8, 9.8, 10.2, 11.3, 12.4, 12.7, 13.2, 13.3, 13.4, 13.5, 13.6, 13.7, 13.9, 13.10_

- [x] 6.2 Implement detail, help, status, search prompt, and filter panel
  - Full cell detail with formula/cache/calculated/status, scrollable multiline text
  - Filter panel shows columns, criteria, apply state, and visible rows
  - Search prompt shows query, progress, match count, and current position
  - _Boundary: DetailRenderer, HelpRenderer, StatusBar, SearchPrompt, FilterPanel_
  - _Requirements: 3.5, 4.8, 5.1, 5.2, 5.3, 5.4, 6.7, 9.1, 9.4, 9.6, 10.5, 11.3, 12.7, 13.9_

- [x] 6.3 Implement keymap, app state, and event loop
  - Normal/Visual/Detail/Help/Search/Filter modes; Vim and arrow navigation; selection
  - Copy/detail row-loading effects, formula/search/row-load result integration, resize/quit
  - Observable: keyboard-only flow opens detail, copies, searches, filters, and quits
  - _Boundary: Keymap, AppState, EventLoop, TerminalGuard_
  - _Requirements: 1.4, 1.5, 2.8, 3.2, 3.6, 3.7, 3.8, 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8, 4.9, 5.3, 6.6, 9.4, 9.5, 9.6, 9.7, 10.5, 10.6, 10.7, 10.8, 12.4, 12.7, 12.9, 13.4_

- [x] 6.4 Implement clipboard sinks and limits
  - OSC 52 base64/tmux passthrough; optional system clipboard; fallback and payload limit
  - Copy success/failure is reported without leaving the app
  - _Boundary: ClipboardService, Osc52Sink, SystemClipboardSink_
  - _Requirements: 6.1, 6.2, 6.5, 6.6, 6.8, 10.8_

## 7. Integration

- [x] 7.1 Wire startup, format detection, encoding, eager/lazy choice, and first render
  - CSV encoding detection, xlsx metadata, freeze/filter setup, formula job, initial view
  - Errors restore the terminal and exit non-zero; runtime errors stay in the status bar
  - Observable: `sheetgaze file.xlsx` and `sheetgaze file.csv` open to a usable grid
  - _Boundary: main, EventLoop, loaders, AppState_
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 8.1, 8.2, 8.6, 8.7, 11.1, 11.2, 11.3, 11.4, 11.5, 11.6, 11.7, 12.1, 12.9, 13.1_

- [x] 7.2 Integrate search, filter, freeze, lazy rows, copy, and detail interactions
  - Filter changes update visibility and search navigation; formula results refresh masks/search
  - Frozen rows and scrollable rows copy in sheet order; lazy copy uses CopySession
  - Observable: filter/search/copy/detail work together on a large frozen sheet
  - _Boundary: AppState, FilterEngine, SearchIndex, CopyService, RowWindowCache_
  - _Requirements: 9.7, 10.8, 10.9, 12.5, 12.9, 13.4, 13.8, 13.11_

- [x] 7.3 Harden terminal lifecycle, small terminals, and error reporting
  - RAII restore, panic hook, minimum-size message, resize handling, actionable errors
  - Observable: normal quit, load error, and panic all restore the terminal
  - _Boundary: TerminalGuard, Screen, StatusBar_
  - _Requirements: 1.4, 4.9, 8.4, 8.5, 8.6_

## 8. Validation

- [x] 8.1 Add domain unit tests
  - Address/range, layout limits, overflow/merge, selection, TSV, filters, search, freeze, recalculating
  - Observable: `cargo test` covers all domain rules with fixtures and no terminal required
  - _Requirements: 2.2, 2.3, 2.4, 2.5, 2.6, 4.4, 4.5, 4.6, 6.1, 6.2, 6.3, 6.4, 8.8, 9.3, 10.3, 10.4, 13.5, 13.6_

- [x] 8.2 Add loader, formula, encoding, filter, lazy, and freeze integration tests
  - xlsx/csv fixtures, Shift_JIS/EUC-JP/UTF-16, formulas, autoFilter, freeze panes, large data
  - Observable: loaders produce expected effective values, visibility, and freeze metadata
  - _Requirements: 1.2, 1.3, 3.1, 3.4, 10.1, 10.9, 11.2, 11.5, 12.2, 12.8, 13.1, 13.6_

- [x] 8.3 Add TUI snapshots and CLI tests
  - Grid/detail/help/search/filter views, frozen regions, minimum size, invalid CLI options
  - Observable: snapshots are stable and `--help`/bad input exit codes are asserted
  - _Requirements: 2.1, 4.8, 5.1, 5.2, 7.2, 7.3, 8.4, 8.5, 9.8, 10.5, 13.2_

- [x] 8.4 Run formatting, linting, and performance checks
  - `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`
  - 100k-cell eager load, large lazy cache, 80x24 render, search/filter background progress
  - Observable: all checks pass; benchmark notes are recorded in the spec
  - _Requirements: 8.1, 8.2, 8.3, 8.9, 12.10_

## 9. Cell Borders

- [x] 9.1 Add border value types and xlsx parsing
  - `BorderStyle` / `BorderLine` / `CellBorders` on `CellStyle`
  - Read `style.borders` in eager and lazy modes, including empty bordered cells
  - Aggregate the outer borders of merged ranges
  - _Requirements: 1.2, 2.10, 2.12_

- [x] 9.2 Render explicit borders and default gridlines
  - Reserve one gutter column per cell and draw box-drawing vertical borders
  - Draw horizontal borders as row-bottom underlines without consuming rows
  - Draw faint default gridlines for unbordered boundaries and suppress interior gridlines inside merges
  - Stop text overflow at explicit borders
  - _Requirements: 2.4, 2.10-2.12_

- [x] 9.3 Add fixture and regression coverage
  - `borders.xlsx` covers thin, medium, double, dashed, colored, empty-cell, merged, and overflow cases
  - TUI tests assert default gridlines and explicit border glyphs
  - _Requirements: 2.10-2.12_
