use crate::ui::view::ScreenView;

pub fn status_line(view: &ScreenView) -> String {
    let status = &view.status;
    let mut parts = vec![
        status.file_name.clone(),
        format!("[{}]", status.sheet_name),
        status.cell_label.clone(),
        status.mode_label.clone(),
    ];
    if status.dirty {
        parts.push("[+]".to_string());
    }
    if !status.fit_label.is_empty() {
        parts.push(status.fit_label.clone());
    }
    if status.selection_count > 1 {
        let kind = match status.selection_kind {
            Some(crate::domain::selection::SelectionMode::Row) => "line",
            Some(crate::domain::selection::SelectionMode::Column) => "col",
            Some(crate::domain::selection::SelectionMode::Block) => "block",
            _ => "sel",
        };
        parts.push(format!("{kind}:{}", status.selection_count));
    }
    if !status.formula_label.is_empty() {
        parts.push(status.formula_label.clone());
    }
    if !status.search_label.is_empty() {
        parts.push(status.search_label.clone());
    }
    if !status.filter_label.is_empty() {
        parts.push(status.filter_label.clone());
    }
    if !status.freeze_label.is_empty() {
        parts.push(status.freeze_label.clone());
    }
    if !status.load_label.is_empty() {
        parts.push(status.load_label.clone());
    }
    if !status.reload_label.is_empty() {
        parts.push(status.reload_label.clone());
    }
    if let Some(encoding) = &status.encoding_label {
        parts.push(format!("enc:{encoding}"));
    }
    if let Some(message) = &status.message {
        parts.push(message.clone());
    }
    parts.join("  ")
}

pub fn tabs_line(view: &ScreenView) -> String {
    view.tabs
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if index == view.active_sheet {
                format!("[{name}]")
            } else {
                format!(" {name} ")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
