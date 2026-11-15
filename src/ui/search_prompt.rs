use crate::ui::view::SearchPromptView;

pub fn line(prompt: &SearchPromptView) -> String {
    if prompt.searching {
        format!("/{}  searching...", prompt.query)
    } else if prompt.total > 0 {
        format!(
            "/{}  {}/{}  hidden:{}",
            prompt.query,
            prompt.current.min(prompt.total),
            prompt.total,
            prompt.hidden
        )
    } else {
        format!("/{}  no match", prompt.query)
    }
}
