use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::ui::view::DetailView;

pub fn render(frame: &mut ratatui::Frame, detail: &DetailView, area: Rect) {
    let popup = centered(area, 90, 80);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(format!(" {} ", detail.address));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = Vec::new();
    lines.push(format!("type: {}", detail.kind));
    lines.push(format!("raw: {}", detail.raw));
    lines.push(format!("display: {}", detail.display));
    if let Some(formula) = &detail.formula {
        lines.push(format!("formula: {formula}"));
    }
    if let Some(cached) = &detail.cached {
        lines.push(format!("cached: {cached}"));
    }
    if let Some(evaluated) = &detail.evaluated {
        lines.push(format!("calculated: {evaluated}"));
    }
    lines.push(format!("status: {}", detail.status));
    lines.push(format!("style: {}", detail.style));
    if let Some(merge) = &detail.merge {
        lines.push(format!("merged: {merge}"));
    }
    if detail.truncated {
        lines.push("truncated: yes".to_string());
    }
    lines.push(String::new());
    lines.extend(detail.lines.iter().cloned());
    let paragraph = Paragraph::new(lines.join("\n"))
        .wrap(Wrap { trim: false })
        .scroll((detail.scroll as u16, 0))
        .style(Style::default().fg(Color::Reset));
    frame.render_widget(paragraph, inner);
}

pub fn centered(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let width = area.width.saturating_mul(width_percent) / 100;
    let height = area.height.saturating_mul(height_percent) / 100;
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width: width.max(20).min(area.width),
        height: height.max(8).min(area.height),
    }
}
