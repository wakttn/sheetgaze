use ratatui::layout::Rect;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::ui::detail::centered;
use crate::ui::view::HelpView;

pub fn render(frame: &mut ratatui::Frame, help: &HelpView, area: Rect) {
    let popup = centered(area, 70, 70);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(" Key bindings ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = vec!["Navigation  h j k l / arrows, gg G { } 0 ^ $ Ctrl-d Ctrl-u".to_string()];
    lines.push("Search      / then Enter, n / N, Esc".to_string());
    lines.push("Filter      f, Space toggle, a all, c clear, Esc close".to_string());
    lines.push("Cell        Enter / d detail, v / V select, y copy".to_string());
    lines.push("Sheets      Tab / Shift-Tab, ] / [".to_string());
    lines.push("Reload      r manual reload".to_string());
    lines.push("Other       ? help, q quit".to_string());
    lines.push(String::new());
    for (key, description) in &help.bindings {
        lines.push(format!("{key:<12} {description}"));
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false }),
        inner,
    );
}
