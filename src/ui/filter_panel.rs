use ratatui::layout::Rect;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::ui::detail::centered;
use crate::ui::view::FilterPanelView;

pub fn render(frame: &mut ratatui::Frame, panel: &FilterPanelView, area: Rect) {
    let popup = centered(area, 70, 60);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(" Filters ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let lines = if panel.lines.is_empty() {
        "No saved filters".to_string()
    } else {
        panel
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                if index == panel.selected {
                    format!("> {line}")
                } else {
                    format!("  {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    frame.render_widget(Paragraph::new(lines), inner);
}
