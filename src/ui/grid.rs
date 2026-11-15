use std::collections::BTreeMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use crate::config::FillMode;
use crate::domain::addr::column_label;
use crate::domain::layout::{RenderColumn, RenderRow};
use crate::domain::style::{BorderLine, BorderStyle, RgbColor};
use crate::ui::theme::Theme;
use crate::ui::view::ScreenView;

const ROW_HEADER_WIDTH: u16 = 6;

pub struct GridWidget<'a> {
    pub view: &'a ScreenView,
    pub theme: &'a Theme,
}

impl Widget for GridWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < ROW_HEADER_WIDTH + 4 || area.height < 3 {
            return;
        }
        let data_x = area.x + ROW_HEADER_WIDTH;
        let grid_left = data_x.saturating_sub(1);
        let header_y = area.y;
        let mut rows: Vec<&RenderRow> = Vec::new();
        rows.extend(self.view.plan.frozen_rows.iter());
        rows.extend(self.view.plan.scroll_rows.iter());

        let mut tops = Vec::with_capacity(rows.len());
        let mut bottoms = Vec::with_capacity(rows.len());
        let mut y = header_y + 1;
        for row in &rows {
            let height = row.height.max(1);
            if y.saturating_add(height) > area.y + area.height {
                break;
            }
            tops.push(y);
            bottoms.push(y + height - 1);
            y += height;
        }
        let visible_rows = &rows[..tops.len()];
        let grid_right = visible_rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .map(|cell| data_x + cell.x + cell.slot_width + 1)
            .max()
            .unwrap_or(data_x)
            .min(area.x + area.width);
        if grid_right <= grid_left {
            return;
        }

        draw_row_numbers(area, buf, visible_rows, &tops, self.theme);
        draw_headers(
            area,
            buf,
            &self.view.plan.columns,
            data_x,
            header_y,
            grid_left,
            grid_right,
            self.theme,
        );
        draw_default_vertical_lines(
            buf,
            visible_rows,
            data_x,
            &tops,
            &bottoms,
            grid_left,
            grid_right,
            self.theme,
        );
        draw_cells(area, buf, visible_rows, data_x, &tops, self.theme);
        draw_default_horizontal_lines(
            buf,
            visible_rows,
            data_x,
            header_y,
            &bottoms,
            grid_left,
            grid_right,
            self.theme,
        );
        draw_fill_borders(
            buf,
            visible_rows,
            data_x,
            header_y,
            &tops,
            &bottoms,
            grid_left,
            grid_right,
            self.theme,
        );
        draw_explicit_borders(
            buf,
            visible_rows,
            data_x,
            header_y,
            &tops,
            &bottoms,
            grid_left,
            grid_right,
            self.theme,
        );
    }
}

fn draw_row_numbers(
    area: Rect,
    buf: &mut Buffer,
    rows: &[&RenderRow],
    tops: &[u16],
    theme: &Theme,
) {
    for (row, y) in rows.iter().zip(tops) {
        buf.set_stringn(
            area.x,
            *y,
            format!("{:>5} ", row.row + 1),
            ROW_HEADER_WIDTH as usize,
            theme.header,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_headers(
    area: Rect,
    buf: &mut Buffer,
    columns: &[RenderColumn],
    data_x: u16,
    y: u16,
    grid_left: u16,
    grid_right: u16,
    theme: &Theme,
) {
    let header_style = Style::default()
        .add_modifier(Modifier::BOLD)
        .patch(theme.header);
    let style = Style::default()
        .fg(theme.grid_color())
        .add_modifier(Modifier::BOLD);
    for column in columns {
        let x = data_x.saturating_add(column.x);
        if x >= area.x + area.width {
            break;
        }
        let width = (area.x + area.width - x).min(column.width.max(1));
        buf.set_stringn(x, y, column_label(column.col), width as usize, header_style);
        let boundary = x.saturating_add(column.width.max(1));
        if boundary < grid_right {
            draw_vertical(buf, boundary, y, y, "│", style);
        }
    }
    buf.set_stringn(area.x, y, " row  ", ROW_HEADER_WIDTH as usize, header_style);

    draw_vertical(buf, grid_left, y, y, "│", style);
}

#[allow(clippy::too_many_arguments)]
fn draw_default_vertical_lines(
    buf: &mut Buffer,
    rows: &[&RenderRow],
    data_x: u16,
    tops: &[u16],
    bottoms: &[u16],
    grid_left: u16,
    grid_right: u16,
    theme: &Theme,
) {
    let style = Style::default().fg(theme.grid_color());
    for (index, (row, top)) in rows.iter().zip(tops).enumerate() {
        if grid_left < grid_right {
            draw_vertical(buf, grid_left, *top, bottoms[index], "│", style);
        }
        for cell in &row.cells {
            let left = (data_x + cell.x).saturating_sub(1);
            if left >= grid_left && left < grid_right {
                draw_vertical(buf, left, *top, bottoms[index], "│", style);
            }
            let right = data_x + cell.x + cell.slot_width;
            if right >= grid_left && right < grid_right {
                draw_vertical(buf, right, *top, bottoms[index], "│", style);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cells(
    area: Rect,
    buf: &mut Buffer,
    rows: &[&RenderRow],
    data_x: u16,
    tops: &[u16],
    theme: &Theme,
) {
    for (row, top) in rows.iter().zip(tops) {
        for cell in &row.cells {
            let x = data_x + cell.x;
            if x >= area.x + area.width {
                continue;
            }
            let mut style = theme.cell(cell.style);
            if cell.selected {
                style = style.patch(theme.selection);
            }
            if cell.search_match {
                style = style.patch(theme.search);
            }
            if cell.cursor {
                style = style.patch(theme.current);
            }
            if cell.selected || cell.cursor || cell.search_match || style.bg.is_some() {
                // Style the whole slot so fills, empty cells, and the padding
                // after short values are visible.
                let fill_width = cell
                    .slot_width
                    .max(1)
                    .min((area.x + area.width).saturating_sub(x));
                for line_index in 0..row.height {
                    let y = top.saturating_add(line_index);
                    if y >= area.y + area.height {
                        break;
                    }
                    for offset in 0..fill_width {
                        buf[(x + offset, y)].set_symbol(" ").set_style(style);
                    }
                }
            }
            for (line_index, line) in cell.lines.iter().enumerate() {
                if line_index as u16 >= row.height {
                    break;
                }
                let text = line
                    .spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>();
                let max = (area.x + area.width).saturating_sub(x) as usize;
                if max == 0 {
                    break;
                }
                buf.set_stringn(
                    x,
                    top + line_index as u16,
                    text,
                    max.min(cell.width as usize),
                    style,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_default_horizontal_lines(
    buf: &mut Buffer,
    rows: &[&RenderRow],
    data_x: u16,
    header_y: u16,
    bottoms: &[u16],
    grid_left: u16,
    grid_right: u16,
    theme: &Theme,
) {
    underline_row(
        buf,
        grid_left,
        grid_right,
        header_y,
        theme.border.fg.unwrap_or(ratatui::style::Color::Gray),
    );
    for (index, bottom) in bottoms.iter().enumerate() {
        let boundary_row = rows[index].row;
        let mut exclusions = Vec::new();
        for row in rows {
            for cell in &row.cells {
                let Some(span) = cell.merge_span else {
                    continue;
                };
                if boundary_row >= span.start.row && boundary_row < span.end.row {
                    let left = data_x + cell.x;
                    let right = data_x + cell.x + cell.slot_width + 1;
                    exclusions.push((left, right));
                }
            }
        }
        underline_row_excluding(
            buf,
            grid_left,
            grid_right,
            *bottom,
            theme.grid_color(),
            &mut exclusions,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_fill_borders(
    buf: &mut Buffer,
    rows: &[&RenderRow],
    data_x: u16,
    header_y: u16,
    tops: &[u16],
    bottoms: &[u16],
    grid_left: u16,
    grid_right: u16,
    theme: &Theme,
) {
    if theme.fill_mode != FillMode::Border {
        return;
    }
    for (index, row) in rows.iter().enumerate() {
        let top_boundary = if index == 0 {
            header_y
        } else {
            bottoms[index - 1]
        };
        for cell in &row.cells {
            let Some(fill) = cell.style.fill else {
                continue;
            };
            let Some(color) = theme.fill_color(fill) else {
                continue;
            };
            let style = Style::default().fg(color);
            let left_x = (data_x + cell.x).saturating_sub(1);
            if left_x >= grid_left && left_x < grid_right {
                draw_vertical(buf, left_x, tops[index], bottoms[index], "│", style);
            }
            let right_x = data_x + cell.x + cell.slot_width;
            if right_x >= grid_left && right_x < grid_right {
                draw_vertical(buf, right_x, tops[index], bottoms[index], "│", style);
            }

            let left = (data_x + cell.x).saturating_sub(1).max(grid_left);
            let right = (data_x + cell.x + cell.slot_width + 1).min(grid_right);
            if right <= left {
                continue;
            }
            let top_visible = cell.merge_span.is_none_or(|span| row.row == span.start.row);
            if top_visible {
                underline_row(buf, left, right, top_boundary, color);
            }
            let bottom_visible = cell.merge_span.is_none_or(|span| row.row == span.end.row);
            if bottom_visible {
                underline_row(buf, left, right, bottoms[index], color);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_explicit_borders(
    buf: &mut Buffer,
    rows: &[&RenderRow],
    data_x: u16,
    header_y: u16,
    tops: &[u16],
    bottoms: &[u16],
    grid_left: u16,
    grid_right: u16,
    theme: &Theme,
) {
    for (index, row) in rows.iter().enumerate() {
        let mut vertical: BTreeMap<u16, (BorderLine, Option<RgbColor>)> = BTreeMap::new();
        for cell in &row.cells {
            if cell.style.borders.left.is_visible() {
                let x = (data_x + cell.x).saturating_sub(1);
                insert_border(&mut vertical, x, cell.style.borders.left, cell.style.fill);
            }
            if cell.style.borders.right.is_visible() {
                let x = data_x + cell.x + cell.slot_width;
                insert_border(&mut vertical, x, cell.style.borders.right, cell.style.fill);
            }
        }
        for (x, (line, fill)) in vertical {
            if x < grid_left || x >= grid_right {
                continue;
            }
            let color = fill
                .and_then(|fill| theme.fill_color(fill))
                .unwrap_or_else(|| theme.border_color(line));
            draw_vertical(
                buf,
                x,
                tops[index],
                bottoms[index],
                vertical_glyph(line.style),
                Style::default().fg(color),
            );
        }

        let top_boundary = if index == 0 {
            header_y
        } else {
            bottoms[index - 1]
        };
        for cell in &row.cells {
            let left = (data_x + cell.x).saturating_sub(1).max(grid_left);
            let right = (data_x + cell.x + cell.slot_width + 1).min(grid_right);
            if right <= left {
                continue;
            }
            let top_visible = cell.merge_span.is_none_or(|span| row.row == span.start.row);
            if top_visible && cell.style.borders.top.is_visible() {
                let color = cell
                    .style
                    .fill
                    .and_then(|fill| theme.fill_color(fill))
                    .unwrap_or_else(|| theme.border_color(cell.style.borders.top));
                underline_row(buf, left, right, top_boundary, color);
            }
            let bottom_visible = cell.merge_span.is_none_or(|span| row.row == span.end.row);
            if bottom_visible && cell.style.borders.bottom.is_visible() {
                let color = cell
                    .style
                    .fill
                    .and_then(|fill| theme.fill_color(fill))
                    .unwrap_or_else(|| theme.border_color(cell.style.borders.bottom));
                underline_row(buf, left, right, bottoms[index], color);
            }
        }
    }
}

fn insert_border(
    map: &mut BTreeMap<u16, (BorderLine, Option<RgbColor>)>,
    x: u16,
    line: BorderLine,
    fill: Option<RgbColor>,
) {
    map.entry(x)
        .and_modify(|(existing, existing_fill)| {
            *existing = existing.merge(line);
            if existing_fill.is_none() {
                *existing_fill = fill;
            }
        })
        .or_insert((line, fill));
}

fn draw_vertical(buf: &mut Buffer, x: u16, top: u16, bottom: u16, glyph: &str, style: Style) {
    for y in top..=bottom {
        buf[(x, y)].set_symbol(glyph).set_style(style);
    }
}

fn underline_row(buf: &mut Buffer, left: u16, right: u16, y: u16, color: ratatui::style::Color) {
    if right <= left {
        return;
    }
    let style = Style::default()
        .underline_color(color)
        .add_modifier(Modifier::UNDERLINED);
    for x in left..right {
        buf[(x, y)].set_style(style);
    }
}

fn underline_row_excluding(
    buf: &mut Buffer,
    left: u16,
    right: u16,
    y: u16,
    color: ratatui::style::Color,
    exclusions: &mut [(u16, u16)],
) {
    if exclusions.is_empty() {
        underline_row(buf, left, right, y, color);
        return;
    }
    exclusions.sort_unstable();
    let mut cursor = left;
    for (start, end) in exclusions.iter().copied() {
        if end <= cursor || start >= right {
            continue;
        }
        let start = start.max(left);
        if start > cursor {
            underline_row(buf, cursor, start, y, color);
        }
        cursor = cursor.max(end.min(right));
        if cursor >= right {
            break;
        }
    }
    if cursor < right {
        underline_row(buf, cursor, right, y, color);
    }
}

fn vertical_glyph(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::None | BorderStyle::Hair | BorderStyle::Thin => "│",
        BorderStyle::Dotted | BorderStyle::Dashed => "┆",
        BorderStyle::Medium | BorderStyle::Thick => "┃",
        BorderStyle::Double => "║",
    }
}
