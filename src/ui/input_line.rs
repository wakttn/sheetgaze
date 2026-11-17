use unicode_width::UnicodeWidthStr;

use crate::ui::view::InputLineView;

pub fn line(input: &InputLineView) -> String {
    format!("{}{}", input.prompt, input.text)
}

/// Returns the cursor column within the one-line input area, clamped to the
/// available width.
pub fn cursor_column(input: &InputLineView, width: u16) -> u16 {
    let before: String = input.text.chars().take(input.cursor).collect();
    let column = UnicodeWidthStr::width(input.prompt.as_str())
        .saturating_add(UnicodeWidthStr::width(before.as_str()));
    column.min(width.saturating_sub(1) as usize) as u16
}
