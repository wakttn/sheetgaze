use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::ui::screen::SnapshotStyle;

/// Serializes a rendered buffer with ANSI SGR escapes.
pub fn buffer_to_ansi(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        let mut current = StyleKey::default();
        let mut x = area.left();
        while x < area.right() {
            let cell = &buffer[(x, y)];
            let symbol = cell.symbol();
            let width = UnicodeWidthStr::width(symbol).max(1) as u16;
            let style = StyleKey {
                fg: cell.fg,
                bg: cell.bg,
                modifier: cell.modifier,
            };
            if style != current {
                style.write_escape(&mut out);
                current = style;
            }
            out.push_str(symbol);
            x = x.saturating_add(width);
        }
        if current != StyleKey::default() {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

/// Serializes a rendered buffer without escapes, trimming trailing spaces.
pub fn buffer_to_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        let mut x = area.left();
        while x < area.right() {
            let symbol = buffer[(x, y)].symbol();
            let width = UnicodeWidthStr::width(symbol).max(1) as u16;
            line.push_str(symbol);
            x = x.saturating_add(width);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Serializes a buffer according to the requested snapshot output style.
pub fn buffer_to_string(buffer: &Buffer, style: SnapshotStyle) -> String {
    match style {
        SnapshotStyle::Ansi => buffer_to_ansi(buffer),
        SnapshotStyle::Plain => buffer_to_text(buffer),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StyleKey {
    fg: Color,
    bg: Color,
    modifier: Modifier,
}

impl StyleKey {
    fn write_escape(self, out: &mut String) {
        out.push_str("\x1b[0");
        for (flag, code) in [
            (Modifier::BOLD, "1"),
            (Modifier::DIM, "2"),
            (Modifier::ITALIC, "3"),
            (Modifier::UNDERLINED, "4"),
            (Modifier::SLOW_BLINK, "5"),
            (Modifier::RAPID_BLINK, "6"),
            (Modifier::REVERSED, "7"),
            (Modifier::HIDDEN, "8"),
            (Modifier::CROSSED_OUT, "9"),
        ] {
            if self.modifier.contains(flag) {
                out.push(';');
                out.push_str(code);
            }
        }
        write_color(out, self.fg, false);
        write_color(out, self.bg, true);
        out.push('m');
    }
}

fn write_color(out: &mut String, color: Color, background: bool) {
    let base = if background { 40 } else { 30 };
    match color {
        Color::Reset => {}
        Color::Black => push_code(out, base),
        Color::Red => push_code(out, base + 1),
        Color::Green => push_code(out, base + 2),
        Color::Yellow => push_code(out, base + 3),
        Color::Blue => push_code(out, base + 4),
        Color::Magenta => push_code(out, base + 5),
        Color::Cyan => push_code(out, base + 6),
        Color::Gray => push_code(out, base + 7),
        Color::DarkGray => push_code(out, base + 60),
        Color::LightRed => push_code(out, base + 61),
        Color::LightGreen => push_code(out, base + 62),
        Color::LightYellow => push_code(out, base + 63),
        Color::LightBlue => push_code(out, base + 64),
        Color::LightMagenta => push_code(out, base + 65),
        Color::LightCyan => push_code(out, base + 66),
        Color::White => push_code(out, base + 67),
        Color::Rgb(r, g, b) => {
            out.push_str(if background { ";48;2;" } else { ";38;2;" });
            out.push_str(&r.to_string());
            out.push(';');
            out.push_str(&g.to_string());
            out.push(';');
            out.push_str(&b.to_string());
        }
        Color::Indexed(index) => {
            out.push_str(if background { ";48;5;" } else { ";38;5;" });
            out.push_str(&index.to_string());
        }
    }
}

fn push_code(out: &mut String, code: u16) {
    out.push(';');
    out.push_str(&code.to_string());
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};

    use super::{buffer_to_ansi, buffer_to_text};

    #[test]
    fn plain_output_trims_trailing_spaces() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer.set_string(0, 0, "ab", Style::default());
        assert_eq!(buffer_to_text(&buffer), "ab\n");
    }

    #[test]
    fn ansi_output_emits_style_changes_and_reset() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer.set_string(
            0,
            0,
            "x",
            Style::default()
                .fg(Color::Red)
                .bg(Color::Indexed(17))
                .add_modifier(Modifier::BOLD),
        );
        let rendered = buffer_to_ansi(&buffer);
        assert!(rendered.contains("\x1b[0;1;31;48;5;17m"));
        assert!(rendered.contains("\x1b[0m"));
        assert!(rendered.ends_with('\n'));
    }

    #[test]
    fn ansi_output_resets_after_a_styled_last_cell() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer.set_string(0, 0, "x", Style::default().fg(Color::Red));
        assert!(buffer_to_ansi(&buffer).ends_with("\x1b[0m\n"));
    }

    #[test]
    fn wide_symbols_skip_continuation_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer.set_string(0, 0, "あい", Style::default());
        assert_eq!(buffer_to_text(&buffer), "あい\n");
    }
}
