use std::path::PathBuf;

use crate::domain::addr::{CellAddr, CellRange};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubstituteScope {
    CurrentCell,
    Selection,
    Range(CellRange),
    Sheet,
    Workbook,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubstituteCommand {
    pub scope: SubstituteScope,
    pub pattern: String,
    pub replacement: String,
    pub global: bool,
    pub case_sensitive: Option<bool>,
    pub count_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExCommand {
    Noop,
    Write {
        force: bool,
        path: Option<PathBuf>,
    },
    Quit {
        force: bool,
    },
    WriteQuit {
        force: bool,
    },
    Edit {
        force: bool,
    },
    Substitute(SubstituteCommand),
    Goto {
        sheet: Option<String>,
        addr: Option<CellAddr>,
    },
    Sheet(String),
    Help,
    Unknown(String),
}

pub fn parse(input: &str) -> Result<ExCommand, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(ExCommand::Noop);
    }

    if let Some(command) = parse_exact(input) {
        return Ok(command);
    }
    if input == "help" {
        return Ok(ExCommand::Help);
    }
    if let Some(rest) = input
        .strip_prefix("goto")
        .or_else(|| input.strip_prefix("go"))
    {
        return parse_goto(rest);
    }
    if let Some(rest) = input.strip_prefix("sheet") {
        let name = rest.trim();
        if name.is_empty() {
            return Err("sheet name is required".to_string());
        }
        return Ok(ExCommand::Sheet(unquote(name)));
    }
    if let Some(command) = parse_substitute(input)? {
        return Ok(ExCommand::Substitute(command));
    }
    Ok(ExCommand::Unknown(input.to_string()))
}

fn parse_exact(input: &str) -> Option<ExCommand> {
    match input {
        "w" => Some(ExCommand::Write {
            force: false,
            path: None,
        }),
        "w!" => Some(ExCommand::Write {
            force: true,
            path: None,
        }),
        "q" => Some(ExCommand::Quit { force: false }),
        "q!" => Some(ExCommand::Quit { force: true }),
        "wq" | "x" => Some(ExCommand::WriteQuit { force: false }),
        "wq!" | "x!" => Some(ExCommand::WriteQuit { force: true }),
        "e" => Some(ExCommand::Edit { force: false }),
        "e!" => Some(ExCommand::Edit { force: true }),
        _ => None,
    }
}

fn parse_goto(rest: &str) -> Result<ExCommand, String> {
    let target = rest.trim();
    if target.is_empty() {
        return Err("goto requires a cell or sheet address".to_string());
    }
    if let Some((sheet, address)) = target.rsplit_once('!') {
        let sheet = unquote(sheet.trim());
        if sheet.is_empty() {
            return Err("sheet name is empty".to_string());
        }
        let addr = CellAddr::from_a1(address.trim())
            .ok_or_else(|| format!("invalid cell address: {}", address.trim()))?;
        return Ok(ExCommand::Goto {
            sheet: Some(sheet),
            addr: Some(addr),
        });
    }
    if let Some(addr) = CellAddr::from_a1(target) {
        return Ok(ExCommand::Goto {
            sheet: None,
            addr: Some(addr),
        });
    }
    Ok(ExCommand::Goto {
        sheet: Some(unquote(target)),
        addr: None,
    })
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string()
}

fn parse_substitute(input: &str) -> Result<Option<SubstituteCommand>, String> {
    let (scope, rest) = if let Some(rest) = input.strip_prefix("'<,'>s") {
        (SubstituteScope::Selection, rest)
    } else if let Some(rest) = input.strip_prefix("%s") {
        (SubstituteScope::Sheet, rest)
    } else if let Some(rest) = input.strip_prefix("gs") {
        (SubstituteScope::Workbook, rest)
    } else if let Some(rest) = input.strip_prefix('s') {
        (SubstituteScope::CurrentCell, rest)
    } else if let Some(index) = input.find('s') {
        let prefix = &input[..index];
        let Some(range) = parse_range(prefix) else {
            return Ok(None);
        };
        (SubstituteScope::Range(range), &input[index + 1..])
    } else {
        return Ok(None);
    };

    let chars: Vec<char> = rest.chars().collect();
    let Some(&delimiter) = chars.first() else {
        return Err("substitute requires a pattern".to_string());
    };
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut index = 1usize;
    let mut flags_start = chars.len();
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            if ch == delimiter {
                current.push(ch);
            } else {
                current.push('\\');
                current.push(ch);
            }
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if ch == delimiter {
            fields.push(std::mem::take(&mut current));
            index += 1;
            if fields.len() == 2 {
                flags_start = index;
                break;
            }
            continue;
        }
        current.push(ch);
        index += 1;
    }
    if escaped {
        current.push('\\');
    }
    if fields.len() == 1 {
        fields.push(current);
    } else if fields.is_empty() {
        return Err("substitute requires /pattern/replacement/".to_string());
    }
    if fields.len() < 2 {
        return Err("substitute requires /pattern/replacement/".to_string());
    }
    let pattern = unescape_pattern(&fields[0]);
    let replacement = fields[1].clone();
    let flags: String = chars[flags_start..].iter().collect();
    let mut global = false;
    let mut case_sensitive = None;
    let mut count_only = false;
    for flag in flags.chars() {
        match flag {
            'g' => global = true,
            'i' => case_sensitive = Some(false),
            'I' => case_sensitive = Some(true),
            'n' => count_only = true,
            'c' => {
                return Err("confirm substitute is not supported yet".to_string());
            }
            'f' => {
                return Err("formula substitution is not supported yet".to_string());
            }
            _ => {}
        }
    }
    Ok(Some(SubstituteCommand {
        scope,
        pattern,
        replacement,
        global,
        case_sensitive,
        count_only,
    }))
}

fn parse_range(value: &str) -> Option<CellRange> {
    let (left, right) = value.split_once(':')?;
    let start = CellAddr::from_a1(left)?;
    let end = CellAddr::from_a1(right)?;
    Some(CellRange::new(start, end))
}

fn unescape_pattern(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_save_and_quit_commands() {
        assert_eq!(
            parse("w").unwrap(),
            ExCommand::Write {
                force: false,
                path: None
            }
        );
        assert_eq!(parse("q!").unwrap(), ExCommand::Quit { force: true });
        assert_eq!(parse("wq").unwrap(), ExCommand::WriteQuit { force: false });
        assert_eq!(parse("e!").unwrap(), ExCommand::Edit { force: true });
    }

    #[test]
    fn parses_goto_with_and_without_sheet() {
        assert_eq!(
            parse("goto B4").unwrap(),
            ExCommand::Goto {
                sheet: None,
                addr: CellAddr::from_a1("B4")
            }
        );
        assert_eq!(
            parse("goto 'My Sheet'!C3").unwrap(),
            ExCommand::Goto {
                sheet: Some("My Sheet".to_string()),
                addr: CellAddr::from_a1("C3")
            }
        );
    }

    #[test]
    fn parses_substitute_scopes_and_flags() {
        let ExCommand::Substitute(cell) = parse("s/old/new/g").unwrap() else {
            panic!("expected substitute");
        };
        assert_eq!(cell.scope, SubstituteScope::CurrentCell);
        assert!(cell.global);

        let ExCommand::Substitute(sheet) = parse("%s/east/west/i").unwrap() else {
            panic!("expected substitute");
        };
        assert_eq!(sheet.scope, SubstituteScope::Sheet);
        assert_eq!(sheet.case_sensitive, Some(false));

        let ExCommand::Substitute(workbook) = parse("gs/a/b/g").unwrap() else {
            panic!("expected substitute");
        };
        assert_eq!(workbook.scope, SubstituteScope::Workbook);

        let ExCommand::Substitute(range) = parse("A1:C3s/a/b/").unwrap() else {
            panic!("expected substitute");
        };
        assert_eq!(
            range.scope,
            SubstituteScope::Range(CellRange::new(
                CellAddr::from_a1("A1").unwrap(),
                CellAddr::from_a1("C3").unwrap()
            ))
        );
    }

    #[test]
    fn parses_escaped_delimiters() {
        let ExCommand::Substitute(command) = parse(r"s/a\/b/x/").unwrap() else {
            panic!("expected substitute");
        };
        assert_eq!(command.pattern, "a/b");
    }
}
