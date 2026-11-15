/// Formats a numeric cell value using an Excel number format code.
///
/// Only the common numeric subset is supported. `None` means the code is
/// `General` or uses a feature we do not render (dates, scientific notation,
/// fractions, text placeholders, ...). Callers should then fall back to the
/// default number rendering.
pub fn format_number(value: f64, code: Option<&str>) -> Option<String> {
    let code = code?.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        return None;
    }
    let sections = split_sections(code);
    if sections.is_empty() {
        return None;
    }
    let negative = value.is_sign_negative() && value != 0.0;
    let section = if negative && sections.len() >= 2 {
        sections[1].as_str()
    } else if value == 0.0 && sections.len() >= 3 {
        sections[2].as_str()
    } else {
        sections[0].as_str()
    };
    if section.is_empty() {
        return Some(String::new());
    }
    let pattern = parse_section(section)?;
    if !value.is_finite() {
        return None;
    }
    let mut magnitude = value.abs();
    if pattern.percent {
        magnitude *= 100.0;
    }
    let mut number = format!("{:.*}", pattern.max_decimals, magnitude);
    if pattern.max_decimals > pattern.min_decimals
        && let Some(dot) = number.find('.')
    {
        while number.len() > dot + 1 + pattern.min_decimals && number.ends_with('0') {
            number.pop();
        }
        if number.ends_with('.') {
            number.pop();
        }
    }
    let (integer, fraction) = match number.split_once('.') {
        Some((integer, fraction)) => (integer.to_string(), Some(fraction.to_string())),
        None => (number, None),
    };
    let mut integer = integer;
    while integer.len() < pattern.min_int {
        integer.insert(0, '0');
    }
    if pattern.grouping {
        integer = group_thousands(&integer);
    }
    let mut rendered = String::new();
    rendered.push_str(&pattern.prefix);
    if negative && sections.len() < 2 {
        rendered.push('-');
    }
    rendered.push_str(&integer);
    if let Some(fraction) = fraction
        && !fraction.is_empty()
    {
        rendered.push('.');
        rendered.push_str(&fraction);
    }
    rendered.push_str(&pattern.suffix);
    Some(rendered)
}

fn split_sections(code: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut brackets = 0u32;
    for ch in code.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !quoted => escaped = true,
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            '[' if !quoted => {
                brackets += 1;
                current.push(ch);
            }
            ']' if !quoted && brackets > 0 => {
                brackets -= 1;
                current.push(ch);
            }
            ';' if !quoted && brackets == 0 => {
                sections.push(current);
                current = String::new();
                if sections.len() == 4 {
                    break;
                }
            }
            _ => current.push(ch),
        }
    }
    if sections.len() < 4 {
        sections.push(current);
    }
    sections
}

fn group_thousands(integer: &str) -> String {
    let mut output = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, ch) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(ch);
    }
    output
}

struct NumberPattern {
    prefix: String,
    suffix: String,
    min_int: usize,
    min_decimals: usize,
    max_decimals: usize,
    grouping: bool,
    percent: bool,
}

fn parse_section(section: &str) -> Option<NumberPattern> {
    let mut prefix = String::new();
    let mut suffix = String::new();
    let mut min_int = 0usize;
    let mut min_decimals = 0usize;
    let mut max_decimals = 0usize;
    let mut grouping = false;
    let mut percentile = false;
    let mut fraction = false;
    let mut saw_number = false;
    let mut literal_after_number = false;
    let mut chars = section.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                let mut literal = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '"' {
                        closed = true;
                        break;
                    }
                    literal.push(next);
                }
                if !closed {
                    return None;
                }
                push_literal(
                    &mut prefix,
                    &mut suffix,
                    &literal,
                    saw_number,
                    &mut literal_after_number,
                );
            }
            '\\' => {
                let next = chars.next()?;
                push_literal(
                    &mut prefix,
                    &mut suffix,
                    &next.to_string(),
                    saw_number,
                    &mut literal_after_number,
                );
            }
            '_' => {
                chars.next()?;
            }
            '*' => {
                chars.next()?;
            }
            '[' => {
                let mut bracket = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == ']' {
                        closed = true;
                        break;
                    }
                    bracket.push(next);
                }
                if !closed {
                    return None;
                }
                let lower = bracket.to_ascii_lowercase();
                if lower == "h" || lower == "m" || lower == "s" {
                    return None;
                }
            }
            '0' | '#' | '?' => {
                saw_number = true;
                if literal_after_number {
                    return None;
                }
                if fraction {
                    max_decimals += 1;
                    if ch == '0' {
                        min_decimals += 1;
                    }
                } else if ch == '0' {
                    min_int += 1;
                }
            }
            '.' if saw_number && !fraction => {
                fraction = true;
            }
            ',' if saw_number && !fraction => {
                grouping = true;
            }
            '%' => {
                percentile = true;
                suffix.push('%');
                literal_after_number = saw_number;
            }
            'E' | 'e' | '/' | '@' => return None,
            'd' | 'D' | 'm' | 'M' | 'y' | 'Y' | 'h' | 'H' | 's' | 'S' if saw_number => {
                return None;
            }
            _ if ch.is_ascii_alphabetic() => return None,
            _ => {
                push_literal(
                    &mut prefix,
                    &mut suffix,
                    &ch.to_string(),
                    saw_number,
                    &mut literal_after_number,
                );
            }
        }
    }
    if !saw_number {
        return None;
    }
    Some(NumberPattern {
        prefix,
        suffix,
        min_int: min_int.max(1),
        min_decimals,
        max_decimals,
        grouping,
        percent: percentile,
    })
}

fn push_literal(
    prefix: &mut String,
    suffix: &mut String,
    literal: &str,
    saw_number: bool,
    literal_after_number: &mut bool,
) {
    if saw_number {
        suffix.push_str(literal);
        *literal_after_number = true;
    } else {
        prefix.push_str(literal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_percentages() {
        assert_eq!(format_number(0.572, Some("0.0%")).as_deref(), Some("57.2%"));
        assert_eq!(format_number(0.3, Some("0.0%")).as_deref(), Some("30.0%"));
        assert_eq!(format_number(1.0, Some("0.0%")).as_deref(), Some("100.0%"));
        assert_eq!(format_number(0.5, Some("0%")).as_deref(), Some("50%"));
    }

    #[test]
    fn formats_grouped_integers_and_units() {
        assert_eq!(
            format_number(2400.0, Some("#,##0")).as_deref(),
            Some("2,400")
        );
        assert_eq!(
            format_number(1042.0, Some("#,##0\"百万円\"")).as_deref(),
            Some("1,042百万円")
        );
        assert_eq!(
            format_number(1234.5, Some("#,##0.00")).as_deref(),
            Some("1,234.50")
        );
    }

    #[test]
    fn keeps_general_and_unsupported_formats_as_fallback() {
        assert_eq!(format_number(1234.0, Some("General")), None);
        assert_eq!(format_number(1234.0, None), None);
        assert_eq!(format_number(1234.0, Some("0.00E+00")), None);
        assert_eq!(format_number(0.5, Some("# ?/?")), None);
    }

    #[test]
    fn formats_negative_sections_and_prefixes() {
        assert_eq!(
            format_number(-1234.0, Some("#,##0")).as_deref(),
            Some("-1,234")
        );
        assert_eq!(
            format_number(-1234.0, Some("#,##0;(#,##0)")).as_deref(),
            Some("(1,234)")
        );
        assert_eq!(
            format_number(1234.0, Some("\"$\"#,##0")).as_deref(),
            Some("$1,234")
        );
    }
}
