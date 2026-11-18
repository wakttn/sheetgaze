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
    lines.push("Search      / then Enter, n / N (works across sheets), * / #".to_string());
    lines.push("Select      v cell, V row, C column, Ctrl-V block, Esc clear".to_string());
    lines.push(
        "Edit        i a I A, s clear+edit, = formula, Enter/Tab commit, Esc commit".to_string(),
    );
    lines.push(
        "Edit ops    r{char} replace, d{motion} clear range, d$ / V+Delete clear row".to_string(),
    );
    lines.push(
        "Clipboard   y yank, yy/Y yank row, x/dd cut, p/P paste, \"0 / \"+ registers".to_string(),
    );
    lines.push(
        "Structure   o/O insert row, gI/gA insert column, :irow :icol :drow :dcol :put".to_string(),
    );
    lines.push("Note        dd cuts the row (structure changes); undo with u".to_string());
    lines.push("History     u undo, Ctrl-r redo, Ctrl-o / Ctrl-i jump back / forward".to_string());
    lines.push("Command     :w :w! :q :q! :wq :x :e :e! :s :%s :gs :goto".to_string());
    lines.push("Filter      f, Space toggle, a all, c clear, Esc close".to_string());
    lines.push("Cell        Enter / K detail, v / V select, y copy".to_string());
    lines.push("View        w fit columns to screen".to_string());
    lines.push("Sheets      Tab / Shift-Tab, ] / [".to_string());
    lines.push("Reload      :e / :e!, discard with :e!".to_string());
    lines.push("Other       ? help, q quit, ZZ save quit, ZQ discard quit".to_string());
    lines.push(String::new());
    for (key, description) in &help.bindings {
        lines.push(format!("{key:<12} {description}"));
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false }),
        inner,
    );
}
