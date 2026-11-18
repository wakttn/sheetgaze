use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::domain::formula::{RecalcConfig, RecalcPolicy};
use crate::domain::layout::{LayoutLimits, LayoutMode};
use crate::domain::reload::{WatchConfig, WatchMode};
use crate::domain::row_store::LazyConfig;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    #[default]
    Auto,
    Truecolor,
    Ansi256,
    Ansi16,
    Mono,
}

/// How Excel cell fills are represented in the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum FillMode {
    /// Paint the cell background.
    #[default]
    Background,
    /// Use the fill color for the cell outline instead of the background.
    Border,
    /// Ignore fills.
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ClipboardMode {
    #[default]
    Auto,
    Osc52,
    System,
    None,
}

/// Which register `p` / `P` read by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ClipboardRegisterMode {
    /// Vim's `clipboard=unnamedplus`: read the system clipboard and fall back
    /// to the internal register when reading is unavailable.
    #[default]
    UnnamedPlus,
    /// Use only the in-app unnamed register by default.
    Internal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum LazyMode {
    #[default]
    Auto,
    Always,
    Never,
}

/// How column widths and row heights are chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum LayoutModeArg {
    /// Use explicit Excel column widths and row heights when present.
    #[default]
    Excel,
    /// Ignore Excel sizes and auto-fit every column and row from content.
    Content,
}

impl From<LayoutModeArg> for LayoutMode {
    fn from(value: LayoutModeArg) -> Self {
        match value {
            LayoutModeArg::Excel => Self::Excel,
            LayoutModeArg::Content => Self::Content,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum WatchModeArg {
    #[default]
    Poll,
    Native,
    Auto,
    Off,
}

#[derive(Debug, Parser, Clone)]
#[command(name = "sheetgaze", version, about = "Terminal spreadsheet viewer")]
pub struct Cli {
    pub file: PathBuf,
    /// Render one screen snapshot to stdout and exit instead of starting the TUI.
    #[arg(long)]
    pub print: bool,
    /// Snapshot width in columns. Requires --print.
    #[arg(long, value_name = "COLS", requires = "print")]
    pub width: Option<u16>,
    /// Snapshot height in rows. Requires --print.
    #[arg(long, value_name = "ROWS", requires = "print")]
    pub height: Option<u16>,
    /// Omit ANSI escape sequences from the snapshot. Requires --print.
    #[arg(long, requires = "print")]
    pub no_ansi: bool,
    #[arg(long, default_value_t = 65)]
    pub max_col_width: u16,
    #[arg(long, default_value_t = 10)]
    pub max_row_lines: u16,
    /// How column widths and row heights are chosen.
    #[arg(long, value_enum, default_value_t = LayoutModeArg::Excel)]
    pub layout: LayoutModeArg,
    /// Shrink all used columns proportionally so the table fits the terminal width.
    #[arg(long)]
    pub fit_width: bool,
    #[arg(long)]
    pub sheet: Option<String>,
    #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
    pub color: ColorMode,
    /// How to render Excel cell fills.
    #[arg(long, value_enum, default_value_t = FillMode::Background)]
    pub fill_mode: FillMode,
    #[arg(long, value_enum, default_value_t = ClipboardMode::Auto)]
    pub clipboard: ClipboardMode,
    /// Register used by p / P: unnamedplus reads the system clipboard first.
    #[arg(long, value_enum, default_value_t = ClipboardRegisterMode::UnnamedPlus)]
    pub clipboard_register: ClipboardRegisterMode,
    #[arg(long)]
    pub encoding: Option<String>,
    #[arg(long, value_enum, default_value_t = LazyMode::Auto)]
    pub lazy: LazyMode,
    #[arg(long, default_value_t = 200_000)]
    pub lazy_threshold_cells: u64,
    #[arg(long, default_value_t = 16 * 1024 * 1024)]
    pub lazy_threshold_bytes: u64,
    #[arg(long, default_value_t = 2_000)]
    pub row_cache_rows: usize,
    #[arg(long, default_value_t = 200)]
    pub prefetch_rows: usize,
    #[arg(long, value_enum, default_value_t = RecalcPolicyArg::Auto)]
    pub recalc: RecalcPolicyArg,
    #[arg(long)]
    pub no_recalc: bool,
    #[arg(long, default_value_t = 500_000)]
    pub recalc_max_cells: u64,
    #[arg(long, default_value_t = 64 * 1024 * 1024)]
    pub recalc_max_bytes: u64,
    #[arg(long)]
    pub no_filter: bool,
    /// Open the workbook read-only and disable cell editing.
    #[arg(long)]
    pub read_only: bool,
    #[arg(long, value_enum, default_value_t = WatchModeArg::Poll)]
    pub watch: WatchModeArg,
    #[arg(long, default_value_t = 1_000)]
    pub watch_interval_ms: u64,
    #[arg(long, default_value_t = 200_000)]
    pub max_copy_cells: usize,
    /// Maximum number of cells that may be materialized for structural editing.
    #[arg(long, default_value_t = 500_000)]
    pub structural_max_cells: u64,
    #[arg(long, default_value_t = 262_144)]
    pub osc52_max_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum RecalcPolicyArg {
    #[default]
    Auto,
    Always,
    Never,
}

impl From<WatchModeArg> for WatchMode {
    fn from(value: WatchModeArg) -> Self {
        match value {
            WatchModeArg::Poll => Self::Poll,
            WatchModeArg::Native => Self::Native,
            WatchModeArg::Auto => Self::Auto,
            WatchModeArg::Off => Self::Off,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let cli = Cli {
            file: PathBuf::from("."),
            print: false,
            width: None,
            height: None,
            no_ansi: false,
            max_col_width: 65,
            max_row_lines: 10,
            layout: LayoutModeArg::Excel,
            fit_width: false,
            sheet: None,
            color: ColorMode::Auto,
            fill_mode: FillMode::Background,
            clipboard: ClipboardMode::Auto,
            clipboard_register: ClipboardRegisterMode::UnnamedPlus,
            encoding: None,
            lazy: LazyMode::Auto,
            lazy_threshold_cells: 200_000,
            lazy_threshold_bytes: 16 * 1024 * 1024,
            row_cache_rows: 2_000,
            prefetch_rows: 200,
            recalc: RecalcPolicyArg::Auto,
            no_recalc: false,
            recalc_max_cells: 500_000,
            recalc_max_bytes: 64 * 1024 * 1024,
            no_filter: false,
            read_only: false,
            watch: WatchModeArg::Poll,
            watch_interval_ms: 1_000,
            max_copy_cells: 200_000,
            structural_max_cells: 500_000,
            osc52_max_bytes: 262_144,
        };
        Self {
            layout: LayoutLimits::default(),
            layout_mode: LayoutMode::Excel,
            lazy: LazyConfig::default(),
            recalc: RecalcConfig::default(),
            watch: WatchConfig::default(),
            cli,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub cli: Cli,
    pub layout: LayoutLimits,
    pub layout_mode: LayoutMode,
    pub lazy: LazyConfig,
    pub recalc: RecalcConfig,
    pub watch: WatchConfig,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self, String> {
        if cli.max_col_width < 3 || cli.max_col_width > 500 {
            return Err("--max-col-width must be between 3 and 500".to_string());
        }
        if cli.max_row_lines == 0 || cli.max_row_lines > 100 {
            return Err("--max-row-lines must be between 1 and 100".to_string());
        }
        if !(100..=60_000).contains(&cli.watch_interval_ms) {
            return Err("--watch-interval-ms must be between 100 and 60000".to_string());
        }
        if !(1_000..=10_000_000).contains(&cli.structural_max_cells) {
            return Err("--structural-max-cells must be between 1000 and 10000000".to_string());
        }
        if let Some(width) = cli.width
            && !(40..=1000).contains(&width)
        {
            return Err("--width must be between 40 and 1000".to_string());
        }
        if let Some(height) = cli.height
            && !(10..=500).contains(&height)
        {
            return Err("--height must be between 10 and 500".to_string());
        }
        let policy = if cli.no_recalc {
            RecalcPolicy::Never
        } else {
            match cli.recalc {
                RecalcPolicyArg::Auto => RecalcPolicy::Auto,
                RecalcPolicyArg::Always => RecalcPolicy::Always,
                RecalcPolicyArg::Never => RecalcPolicy::Never,
            }
        };
        Ok(Self {
            layout: LayoutLimits {
                max_col_width: cli.max_col_width,
                max_row_lines: cli.max_row_lines,
                min_col_width: 3,
                default_col_width: 8,
            },
            layout_mode: cli.layout.into(),
            lazy: LazyConfig {
                enabled: !matches!(cli.lazy, LazyMode::Never),
                threshold_cells: cli.lazy_threshold_cells,
                threshold_bytes: cli.lazy_threshold_bytes,
                row_cache_rows: cli.row_cache_rows.max(1),
                prefetch_rows: cli.prefetch_rows,
            },
            recalc: RecalcConfig {
                policy,
                max_cells: cli.recalc_max_cells,
                max_bytes: cli.recalc_max_bytes,
            },
            watch: WatchConfig {
                mode: cli.watch.into(),
                poll_interval: Duration::from_millis(cli.watch_interval_ms),
                ..WatchConfig::default()
            },
            cli,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_flag_parses() {
        let cli = Cli::try_parse_from(["sheetgaze", "--read-only", "book.xlsx"]).unwrap();
        assert!(cli.read_only);
        assert!(!Config::default().cli.read_only);
    }

    #[test]
    fn fill_mode_defaults_to_background_and_parses_border() {
        assert_eq!(Config::default().cli.fill_mode, FillMode::Background);
        let cli = Cli::try_parse_from(["sheetgaze", "--fill-mode", "border", "book.xlsx"]).unwrap();
        assert_eq!(cli.fill_mode, FillMode::Border);
    }

    #[test]
    fn watch_defaults_to_poll_every_second() {
        let config = Config::default();
        assert_eq!(config.watch.mode, WatchMode::Poll);
        assert_eq!(config.watch.poll_interval, Duration::from_millis(1_000));
        assert_eq!(config.watch.debounce, Duration::from_millis(750));
        assert_eq!(config.watch.retry, Duration::from_millis(1_000));
        assert_eq!(config.watch.native_safety, Duration::from_millis(5_000));
    }

    #[test]
    fn watch_interval_must_be_in_range() {
        let mut cli = Config::default().cli;
        cli.watch_interval_ms = 99;
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.watch_interval_ms = 60_001;
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.watch_interval_ms = 100;
        assert_eq!(
            Config::from_cli(cli).unwrap().watch.poll_interval,
            Duration::from_millis(100)
        );
    }

    #[test]
    fn watch_mode_argument_maps_to_domain_mode() {
        assert_eq!(WatchMode::from(WatchModeArg::Poll), WatchMode::Poll);
        assert_eq!(WatchMode::from(WatchModeArg::Native), WatchMode::Native);
        assert_eq!(WatchMode::from(WatchModeArg::Auto), WatchMode::Auto);
        assert_eq!(WatchMode::from(WatchModeArg::Off), WatchMode::Off);
    }

    #[test]
    fn layout_mode_defaults_to_excel_and_parses_content() {
        assert_eq!(Config::default().layout_mode, LayoutMode::Excel);
        let cli = Cli::try_parse_from(["sheetgaze", "--layout", "content", "book.xlsx"]).unwrap();
        assert_eq!(cli.layout, LayoutModeArg::Content);
        assert_eq!(
            Config::from_cli(cli).unwrap().layout_mode,
            LayoutMode::Content
        );
    }

    #[test]
    fn fit_width_defaults_off_and_parses_the_flag() {
        assert!(!Config::default().cli.fit_width);
        let cli = Cli::try_parse_from(["sheetgaze", "--fit-width", "book.xlsx"]).unwrap();
        assert!(cli.fit_width);
        assert!(Config::from_cli(cli).unwrap().cli.fit_width);

        let cli = Cli::try_parse_from([
            "sheetgaze",
            "--fit-width",
            "--layout",
            "content",
            "book.xlsx",
        ])
        .unwrap();
        assert!(cli.fit_width);
        assert_eq!(cli.layout, LayoutModeArg::Content);
        let config = Config::from_cli(cli).unwrap();
        assert!(config.cli.fit_width);
        assert_eq!(config.layout_mode, LayoutMode::Content);
    }

    #[test]
    fn clipboard_register_defaults_to_unnamedplus() {
        assert_eq!(
            Config::default().cli.clipboard_register,
            ClipboardRegisterMode::UnnamedPlus
        );
        let cli =
            Cli::try_parse_from(["sheetgaze", "--clipboard-register", "internal", "book.xlsx"])
                .unwrap();
        assert_eq!(cli.clipboard_register, ClipboardRegisterMode::Internal);
    }

    #[test]
    fn structural_max_cells_must_be_in_range() {
        let mut cli = Config::default().cli;
        cli.structural_max_cells = 999;
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.structural_max_cells = 10_000_001;
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.structural_max_cells = 1_000;
        assert!(Config::from_cli(cli).is_ok());
    }

    #[test]
    fn print_dimensions_must_be_in_range() {
        let mut cli = Config::default().cli;
        cli.print = true;
        cli.width = Some(39);
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.print = true;
        cli.width = Some(1001);
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.print = true;
        cli.height = Some(9);
        assert!(Config::from_cli(cli).is_err());

        let mut cli = Config::default().cli;
        cli.print = true;
        cli.width = Some(40);
        cli.height = Some(10);
        assert!(Config::from_cli(cli).is_ok());
    }
}
