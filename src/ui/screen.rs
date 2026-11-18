use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;
use ratatui::{Terminal, backend::TestBackend};

use crate::domain::mode::Mode;
use crate::ui::ansi;
use crate::ui::detail;
use crate::ui::filter_panel;
use crate::ui::grid::GridWidget;
use crate::ui::help;
use crate::ui::input_line;
use crate::ui::search_prompt;
use crate::ui::status;
use crate::ui::theme::Theme;
use crate::ui::view::ScreenView;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapshotStyle {
    #[default]
    Ansi,
    Plain,
}

pub struct Screen;

impl Screen {
    /// Renders one screen snapshot to an in-memory buffer and serializes it.
    pub fn snapshot(
        view: &ScreenView,
        theme: &Theme,
        width: u16,
        height: u16,
        style: SnapshotStyle,
    ) -> Result<String, String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(never) => match never {},
        };
        match terminal.draw(|frame| Self::render_impl(frame, view, theme, false)) {
            Ok(_) => {}
            Err(never) => match never {},
        };
        Ok(ansi::buffer_to_string(terminal.backend().buffer(), style))
    }

    pub fn render(frame: &mut ratatui::Frame, view: &ScreenView, theme: &Theme) {
        Self::render_impl(frame, view, theme, true);
    }

    fn render_impl(frame: &mut ratatui::Frame, view: &ScreenView, theme: &Theme, show_tabs: bool) {
        let area = frame.area();
        if area.width < 40 || area.height < 10 {
            let message = format!(
                "Terminal too small: {}x{} (minimum 40x10)",
                area.width, area.height
            );
            frame.render_widget(Paragraph::new(message), area);
            theme.apply_base(frame.buffer_mut(), area);
            return;
        }
        let chunks = if show_tabs {
            Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area)
        } else {
            Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(area)
        };
        frame.render_widget(Paragraph::new(status::status_line(view)), chunks[0]);
        frame.render_widget(GridWidget { view, theme }, chunks[1]);
        if show_tabs {
            let bottom = if let Some(input) = &view.input_line {
                input_line::line(input)
            } else if let Some(prompt) = &view.search_prompt {
                search_prompt::line(prompt)
            } else {
                status::tabs_line(view)
            };
            frame.render_widget(Paragraph::new(bottom), chunks[2]);
            if let Some(input) = &view.input_line {
                let x = area
                    .x
                    .saturating_add(input_line::cursor_column(input, area.width));
                let y = chunks[2].y;
                frame.set_cursor_position((x, y));
            }
        }
        match view.mode {
            Mode::Detail => {
                if let Some(detail) = &view.detail {
                    detail::render(frame, detail, area);
                }
            }
            Mode::Help => {
                if let Some(help) = &view.help {
                    help::render(frame, help, area);
                }
            }
            Mode::Filter => {
                if let Some(panel) = &view.filter_panel {
                    filter_panel::render(frame, panel, area);
                }
            }
            _ => {}
        }
        theme.apply_base(frame.buffer_mut(), area);
    }
}
