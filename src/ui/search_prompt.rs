use crate::ui::view::SearchPromptView;

pub fn line(prompt: &SearchPromptView) -> String {
    if prompt.searching {
        format!("/{}  searching...", prompt.query)
    } else if prompt.total > 0 {
        let context = match (&prompt.sheet, &prompt.cell) {
            (Some(sheet), Some(cell)) => format!("  {sheet}!{cell}"),
            _ => String::new(),
        };
        format!(
            "/{}  {}/{}  hidden:{}{}",
            prompt.query,
            prompt.current.min(prompt.total),
            prompt.total,
            prompt.hidden,
            context
        )
    } else {
        format!("/{}  no match", prompt.query)
    }
}
