use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::CellValue;
use crate::domain::style::{CellStyle, RgbColor};

/// Style fragment applied by a matching conditional-formatting rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConditionalStyle {
    pub bold: Option<bool>,
    pub color: Option<RgbColor>,
    pub fill: Option<RgbColor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionalOperator {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Between,
    NotBetween,
}

#[derive(Clone, Debug)]
pub struct ConditionalRule {
    pub operator: ConditionalOperator,
    pub values: Vec<f64>,
    pub style: ConditionalStyle,
    pub priority: u32,
}

#[derive(Clone, Debug)]
pub struct ConditionalFormat {
    pub range: CellRange,
    pub rules: Vec<ConditionalRule>,
}

impl ConditionalRule {
    fn matches(&self, value: f64) -> bool {
        let Some(first) = self.values.first().copied() else {
            return false;
        };
        match self.operator {
            ConditionalOperator::Equal => nearly_equal(value, first),
            ConditionalOperator::NotEqual => !nearly_equal(value, first),
            ConditionalOperator::GreaterThan => value > first,
            ConditionalOperator::GreaterThanOrEqual => value >= first,
            ConditionalOperator::LessThan => value < first,
            ConditionalOperator::LessThanOrEqual => value <= first,
            ConditionalOperator::Between => self
                .values
                .get(1)
                .is_some_and(|second| value >= first.min(*second) && value <= first.max(*second)),
            ConditionalOperator::NotBetween => self
                .values
                .get(1)
                .is_some_and(|second| value < first.min(*second) || value > first.max(*second)),
        }
    }
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON * left.abs().max(right.abs()).max(1.0)
}

/// Returns the style of the first matching rule, in XML order/priority order.
pub fn style_for(
    formats: &[ConditionalFormat],
    addr: CellAddr,
    value: &CellValue,
) -> Option<ConditionalStyle> {
    let numeric = value.numeric()?;
    for format in formats {
        if !format.range.contains(addr) {
            continue;
        }
        let mut rules: Vec<&ConditionalRule> = format.rules.iter().collect();
        rules.sort_by_key(|rule| rule.priority);
        if let Some(rule) = rules.into_iter().find(|rule| rule.matches(numeric)) {
            return Some(rule.style);
        }
    }
    None
}

impl CellStyle {
    pub fn apply_conditional(&mut self, style: ConditionalStyle) {
        if let Some(bold) = style.bold {
            self.bold = bold;
        }
        if let Some(color) = style.color {
            self.color = Some(color);
        }
        if let Some(fill) = style.fill {
            self.fill = Some(fill);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(rule: ConditionalRule) -> ConditionalFormat {
        ConditionalFormat {
            range: CellRange::new(CellAddr::new(0, 0), CellAddr::new(9, 0)),
            rules: vec![rule],
        }
    }

    #[test]
    fn matches_cell_is_operators() {
        let red = ConditionalStyle {
            color: Some(RgbColor::new(156, 0, 6)),
            ..ConditionalStyle::default()
        };
        let formats = vec![format(ConditionalRule {
            operator: ConditionalOperator::GreaterThanOrEqual,
            values: vec![0.8],
            style: red,
            priority: 1,
        })];
        assert_eq!(
            style_for(&formats, CellAddr::new(0, 0), &CellValue::Number(0.8)),
            Some(red)
        );
        assert_eq!(
            style_for(&formats, CellAddr::new(0, 0), &CellValue::Number(0.79)),
            None
        );
    }

    #[test]
    fn between_is_inclusive_and_ordered() {
        let style = ConditionalStyle {
            fill: Some(RgbColor::new(255, 235, 156)),
            ..ConditionalStyle::default()
        };
        let formats = vec![format(ConditionalRule {
            operator: ConditionalOperator::Between,
            values: vec![0.8, 0.65],
            style,
            priority: 1,
        })];
        for value in [0.65, 0.7, 0.8] {
            assert_eq!(
                style_for(&formats, CellAddr::new(0, 0), &CellValue::Number(value)),
                Some(style)
            );
        }
        assert_eq!(
            style_for(&formats, CellAddr::new(0, 0), &CellValue::Number(0.81)),
            None
        );
    }
}
