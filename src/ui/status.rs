use crate::ui::view::ScreenView;

pub fn status_line(view: &ScreenView) -> String {
    let status = &view.status;
    let mut parts = vec![
        status.file_name.clone(),
        format!("[{}]", status.sheet_name),
        status.cell_label.clone(),
        status.mode_label.clone(),
    ];
    if status.selection_count > 1 {
        parts.push(format!("sel:{}", status.selection_count));
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
