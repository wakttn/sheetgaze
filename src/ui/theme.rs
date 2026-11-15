use ratatui::style::{Color, Modifier, Style};

use crate::config::{ColorMode, FillMode};
use crate::domain::style::{BorderLine, CellStyle, RgbColor};

#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: ColorMode,
    pub fill_mode: FillMode,
    pub selection: Style,
    pub search: Style,
    pub current: Style,
    pub header: Style,
    /// Explicit borders stored in the spreadsheet.
    pub border: Style,
    /// Faint separators drawn between all cells.
    pub grid: Style,
}

impl Theme {
    pub fn new(mode: ColorMode) -> Self {
        Self::new_with_fill(mode, FillMode::Background)
    }

    pub fn new_with_fill(mode: ColorMode, fill_mode: FillMode) -> Self {
        Self {
            mode,
            fill_mode,
            selection: Style::default().bg(Color::DarkGray).fg(Color::White),
            search: Style::default().bg(Color::Yellow).fg(Color::Black),
            current: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            header: Style::default().add_modifier(Modifier::BOLD),
            border: Style::default().fg(Color::Gray),
            grid: Style::default().fg(Color::DarkGray),
        }
    }

    pub fn cell(&self, style: CellStyle) -> Style {
        let mut result = Style::default();
        if style.bold {
            result = result.add_modifier(Modifier::BOLD);
        }
        if !matches!(self.mode, ColorMode::Mono) {
            let filled_as_border = self.fill_mode == FillMode::Border && style.fill.is_some();
            let mut foreground = if filled_as_border {
                // The outline carries the Excel color. Keep the terminal's
                // normal foreground for text so dark Excel fonts do not become
                // unreadable against an unknown terminal background.
                None
            } else {
                style
                    .color
                    .filter(|color| !color.is_near_black() && !color.is_near_white())
                    .map(|color| Color::Rgb(color.r, color.g, color.b))
            };
            if self.fill_mode == FillMode::Background
                && let Some(fill) = style.fill
            {
                result = result.bg(Color::Rgb(fill.r, fill.g, fill.b));
                if foreground.is_none() {
                    // Excel's zebra fills and light status fills are common.
                    // On a dark terminal the default foreground is light, so
                    // keep the fill visible by choosing a contrasting color.
                    foreground = Some(if fill.is_light() {
                        Color::Black
                    } else {
                        Color::White
                    });
                }
            }
            if let Some(color) = foreground {
                result = result.fg(color);
            }
        }
        result
    }

    /// Terminal color used to draw a fill as a cell outline.
    pub fn fill_color(&self, fill: RgbColor) -> Option<Color> {
        if matches!(self.mode, ColorMode::Mono) || self.fill_mode != FillMode::Border {
            return None;
        }
        Some(Color::Rgb(fill.r, fill.g, fill.b))
    }

    /// Color used for an explicit border. Excel border colors are honored in
    /// color modes when they are distinguishable from the terminal background.
    pub fn border_color(&self, line: BorderLine) -> Color {
        if !matches!(self.mode, ColorMode::Mono)
            && let Some(color) = line.color
            && !color.is_near_black()
            && !color.is_near_white()
        {
            return Color::Rgb(color.r, color.g, color.b);
        }
        self.border.fg.unwrap_or(Color::Gray)
    }

    pub fn grid_color(&self) -> Color {
        self.grid.fg.unwrap_or(Color::DarkGray)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::style::{CellBorders, RgbColor};

    fn style(fill: Option<RgbColor>, color: Option<RgbColor>) -> CellStyle {
        CellStyle {
            bold: false,
            color,
            fill,
            wrap_text: false,
            borders: CellBorders::default(),
        }
    }

    #[test]
    fn light_fill_uses_dark_foreground() {
        let theme = Theme::new(ColorMode::Truecolor);
        let rendered = theme.cell(style(Some(RgbColor::new(0xF7, 0xF9, 0xFC)), None));
        assert_eq!(rendered.fg, Some(Color::Black));
        assert_eq!(rendered.bg, Some(Color::Rgb(0xF7, 0xF9, 0xFC)));
    }

    #[test]
    fn dark_fill_uses_light_foreground() {
        let theme = Theme::new(ColorMode::Truecolor);
        let rendered = theme.cell(style(Some(RgbColor::new(0x2F, 0x55, 0x97)), None));
        assert_eq!(rendered.fg, Some(Color::White));
    }

    #[test]
    fn explicit_conditional_color_wins_over_contrast_fallback() {
        let theme = Theme::new(ColorMode::Truecolor);
        let rendered = theme.cell(style(
            Some(RgbColor::new(0xFF, 0xC7, 0xCE)),
            Some(RgbColor::new(0x9C, 0x00, 0x06)),
        ));
        assert_eq!(rendered.fg, Some(Color::Rgb(0x9C, 0x00, 0x06)));
    }

    #[test]
    fn mono_ignores_fills_and_contrast_fallback() {
        let theme = Theme::new(ColorMode::Mono);
        let rendered = theme.cell(style(Some(RgbColor::new(0xF7, 0xF9, 0xFC)), None));
        assert_eq!(rendered.fg, None);
        assert_eq!(rendered.bg, None);
    }

    #[test]
    fn selection_has_a_readable_foreground() {
        let theme = Theme::new(ColorMode::Truecolor);
        assert_eq!(theme.selection.fg, Some(Color::White));
    }

    #[test]
    fn border_fill_mode_does_not_paint_background() {
        let theme = Theme::new_with_fill(ColorMode::Truecolor, FillMode::Border);
        let fill = RgbColor::new(0xFF, 0xC7, 0xCE);
        let rendered = theme.cell(style(Some(fill), None));
        assert_eq!(rendered.bg, None);
        assert_eq!(rendered.fg, None);
        assert_eq!(theme.fill_color(fill), Some(Color::Rgb(0xFF, 0xC7, 0xCE)));
    }

    #[test]
    fn border_fill_mode_keeps_text_readable_on_unknown_backgrounds() {
        let theme = Theme::new_with_fill(ColorMode::Truecolor, FillMode::Border);
        let fill = RgbColor::new(0xFF, 0xC7, 0xCE);
        let rendered = theme.cell(style(Some(fill), Some(RgbColor::new(0x9C, 0x00, 0x06))));
        assert_eq!(rendered.bg, None);
        assert_eq!(rendered.fg, None);
    }

    #[test]
    fn none_fill_mode_ignores_fills() {
        let theme = Theme::new_with_fill(ColorMode::Truecolor, FillMode::None);
        let fill = RgbColor::new(0xF7, 0xF9, 0xFC);
        let rendered = theme.cell(style(Some(fill), None));
        assert_eq!(rendered.bg, None);
        assert_eq!(rendered.fg, None);
        assert_eq!(theme.fill_color(fill), None);
    }
}
