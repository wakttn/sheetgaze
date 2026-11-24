# sheetgaze

`sheetgaze` is a terminal spreadsheet viewer for `.xlsx` and `.csv` files, implemented in Rust.

## Features

- Read xlsx values, formulas, cached values, number formats, bold/color/fill styles, cell borders, merged cells, and frozen panes
- Apply cell conditional formatting (numeric cellIs rules, font color, fill, and bold)
- Render Excel fills as backgrounds or as colored cell outlines (`--fill-mode`)
- Render Excel borders (vertical glyphs and horizontal underlines) plus faint gridlines between all cells
- Read csv with BOM / Shift_JIS / CP932 / EUC-JP / UTF-16 detection
- Recalculate formulas with formualizer in a background thread
- Excel autoFilter support (worksheet and table definitions) with value lists, custom conditions, and Top10
- Search all sheets with smart-case literal matching, `n` / `N` cross-sheet navigation, `*` / `#` word search, and filter-aware skipping
- Replace cell text with `:s`, `:%s`, or `:gs` (current cell / sheet / all sheets), with literal smart-case matching and `g` / `i` / `I` / `n` flags
- Edit xlsx cell values and formulas with Vim-style modal keys, undo / redo, and atomic OOXML save that preserves styles, merges, filters, and other workbook parts
- Recalculate formulas from unsaved edits using a temporary patched workbook
- Prefer explicit Excel column widths and row heights, with content-based auto-fit for unspecified cells and configurable caps (`--layout`)
- Wrap long single-line values inside explicit Excel column widths when they cannot fully spill, expanding the row up to `--max-row-lines`
- Toggle fit-to-width mode with `w` or start in it with `--fit-width`, proportionally shrinking all used columns into the terminal width
- Lazy row loading with an LRU row cache for large files
- Copy a cell or range as TSV via OSC 52 (system clipboard is an optional feature)
- Watch the opened file and reload it automatically, with manual `:e` reload
- Vim-style navigation and editing plus arrow keys

## Build and run

```bash
cargo run --release -- path/to/book.xlsx
cargo run --release -- path/to/data.csv
```

### Print mode

Render one screen snapshot to stdout and exit without entering the alternate
screen or raw mode:

```bash
cargo run --release -- --print path/to/book.xlsx
cargo run --release -- --print --no-ansi --width 120 --height 40 path/to/book.xlsx
```

`--print` renders the same status line and grid as the TUI (sheet tabs are
omitted). It works when stdout is not a terminal; `--width` / `--height` default
to the terminal size and fall back to 100x30. ANSI colors are enabled by
default; `--no-ansi` emits plain text. File watching is disabled, and pending
formula recalculation is awaited before the snapshot is rendered.

Useful options:

```text
--print
--width 120
--height 40
--no-ansi
--fill-mode background|border|none
--max-col-width 65
--max-row-lines 10
--layout excel|content
--fit-width
--sheet Sheet1
--encoding shift_jis
--lazy auto|always|never
--row-cache-rows 2000
--recalc auto|always|never
--no-filter
--read-only
--watch poll|native|auto|off
--watch-interval-ms 1000
--max-copy-cells 200000
--clipboard-register unnamedplus|internal
--structural-max-cells 500000
--osc52-max-bytes 262144
```

`--clipboard-register unnamedplus` is the default and follows Vim's
`clipboard=unnamedplus`: `p` reads the system clipboard and falls back to the
in-app register when reading is unavailable. The optional `system-clipboard`
feature is required to read the native OS clipboard; OSC 52 remains write-only.

`--layout excel` (the default) uses explicit Excel column widths (`customWidth`) and row heights (`customHeight`) when present, and auto-fits only the unspecified columns and rows. `--layout content` ignores Excel sizes and auto-fits everything from cell content. csv files always use content-based layout. Column widths are converted from Excel character units by truncating the fraction, and row heights are converted from points by dividing by 15pt and rounding to terminal lines.

Build with the optional OS clipboard backend:

```bash
cargo run --release --features system-clipboard -- path/to/book.xlsx
```

## Keys

- `h j k l` / arrows: move
- `gg` / `G`: first / last row with data
- `{` / `}`: first / last row of the used range
- `0` / `^` / `$`: line start / first non-blank / line end
- `Ctrl-d` / `Ctrl-u`, `PageUp` / `PageDown`: page movement
- `v` / `V` / `C` / `Ctrl-V`: cell / row / column / rectangular block selection
- `w`: toggle fit-to-width mode
- `y` / `yy` / `Y`: yank selection / current row
- `p` / `P`: paste after / before; `"+p` reads the system clipboard, `"0p` reads the yank register
- `x` / `d{motion}`: cut cell or range contents; `dd` cuts the current row and shifts rows up
- `o` / `O`: insert a row below / above and start editing
- `gI` / `gA`: insert a column left / right and start editing
- `i` / `a` / `I` / `A` / `F2`: edit cell (start / end cursor)
- `s`: clear the cell and start editing
- `=`: start editing a formula
- `Enter` / `Tab` / `Shift-Tab`: commit and move down / right / left
- `Esc`: commit edit; `Ctrl-c`: cancel edit
- `r{char}`: replace cell or selection contents
- `d$` / `V` + `Delete`: clear contents without changing row structure
- `u` / `Ctrl-r`: undo / redo
- `K` or `Enter`: cell detail
- `/`, `n`, `N`: workbook search / next / previous
- `*` / `#`: search the current cell value
- `Ctrl-o` / `Ctrl-i`: jump back / forward
- `:`: command line (`:w`, `:w!`, `:q`, `:q!`, `:wq`, `:x`, `:e`, `:e!`, `:s`, `:%s`, `:gs`, `:goto Sheet!A1`, `:irow`, `:icol`, `:drow`, `:dcol`, `:put`)
- `ZZ` / `ZQ`: save and quit / discard and quit
- `f`: filter panel
- `Tab` / `Shift-Tab`: switch sheet
- `?`: help
- `q`: quit

Editing is available for xlsx files. CSV and `--read-only` workbooks remain read-only. Saving is in-place; the first save after editing also resets the in-memory undo history because the OOXML file is updated atomically.

## Fixtures

`tests/fixtures/xlsx/` contains 22 generated workbooks covering values, formulas, styles, borders, merges, newlines, overflow, explicit Excel layouts, explicit-width wrapping, 100x100 freeze panes, worksheet/table filters, warning cases, multiple sheets, sparse data, special characters, and a 6,001-row lazy-load file.

```bash
cargo test --test generate_fixtures -- --ignored --nocapture
```

See `tests/fixtures/xlsx/README.md` for the coverage table.

## Tests

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo test --release --test performance -- --ignored
```

`formualizer` 0.14.0 does not build with `zip` 7.3 or newer, because
`ZipFile::data_start()` became `Option<u64>`. Cargo.toml patches `formualizer`
to the upstream revision that contains the fix
(`f88f74ffc811b4005c988f02ebe420f5d465fff4`); the patch can be removed once a
release includes it.

## Releases

[`.github/workflows/release.yml`](.github/workflows/release.yml) builds and
publishes a GitHub Release when a version tag is pushed. The tag must match the
package version in `Cargo.toml`:

```bash
# 1. Bump `version` in Cargo.toml and refresh Cargo.lock.
cargo check
git add Cargo.toml Cargo.lock
git commit -m "Release v0.2.1"
git push origin main

# 2. Push the matching tag.
git tag v0.2.1
git push origin v0.2.1
```

The workflow runs `cargo fmt --check`, Clippy, and the test suite before
building. It publishes prebuilt archives for Linux (x86_64, aarch64), macOS
(Intel, Apple Silicon), and Windows (x86_64), with SHA-256 checksums and
automatically generated release notes. Prerelease tags such as
`v0.2.0-rc.1` are marked as prereleases.

The release is created as a draft first. Assets are uploaded to the draft, and
it is published only after every build succeeds, so a failed build does not
produce a half-populated public release.

## Current limitations

- Automatic reload compares the file's existence, size, and modification time only; a rewrite that preserves both size and mtime is not detected. Use `r` in that case.
- Search matching runs synchronously; formula recalculation runs in a background thread.
- Fit-to-width mode shrinks columns only; row heights are not recalculated, so wrapped cells may be truncated with `…` until the mode is turned off. The extra row heights computed for explicit Excel column widths are ignored in fit-to-width mode.
- Text wrapped inside explicit Excel column widths is capped at `--max-row-lines` (default 10); longer values end with `…` and can be read in full in the cell detail view.
- If even one-character columns cannot fit, fit-to-width stays at maximum compression, reports `fit:limited`, and keeps horizontal scrolling.
- CSV decoding currently reads the source file into memory before writing the UTF-8 spill file; row access is lazy after that.
- Multi-row merged cells distribute wrapped text across the merged rows; a merged range that starts left of the viewport is not partially clipped yet.
- Horizontal borders use terminal underline attributes; double/thick horizontal styles are approximated, and underline color depends on terminal support.
- Empty cells that only carry borders or fills are rendered within the sheet's styled range; extremely large style ranges may be skipped for performance.
- Color/icon/dynamic filters are reported as unsupported; value-list, custom, and Top10 filters are evaluated.
- Conditional formatting supports numeric `cellIs` rules (equal, comparison, between, and not-between); color scales, data bars, icon sets, and formula rules are not evaluated.
- In `--fill-mode border`, fills are drawn as thin outlines using the fill color. Explicit Excel borders are recolored to the fill color, and text on filled cells uses the terminal foreground for readability.
- The optional `system-clipboard` feature is needed for native OS clipboard use; the default uses OSC 52. OSC 52 is write-only, so `p` falls back to the in-app register over SSH unless the OS clipboard is readable.
- Structural editing materializes the active sheet in memory. `--structural-max-cells` limits this; larger sheets require `--lazy never`-friendly sizes or a smaller sheet.
- Structural save rejects shared formulas, array formulas, table parts, and structured references rather than risking workbook corruption.
- `dd` now cuts the whole row and shifts rows up; use `d$` or `V` then `Delete` to clear contents without changing row structure.

## License

MIT. See [LICENSE](LICENSE).
