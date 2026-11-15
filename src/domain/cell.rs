use crate::domain::formula::{FormulaInfo, FormulaStatus};
use crate::domain::style::CellStyle;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CellValue {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    DateTime(String),
    Error(String),
    ArraySummary {
        rows: u32,
        cols: u32,
        value: Box<CellValue>,
    },
}

impl CellValue {
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn display_text(&self) -> String {
        self.display_text_with_format(None)
    }

    pub fn display_text_with_format(&self, format: Option<&str>) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text(value) => value.clone(),
            Self::Number(value) => crate::domain::number_format::format_number(*value, format)
                .unwrap_or_else(|| format_number(*value)),
            Self::Bool(value) => {
                if *value {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            }
            Self::DateTime(value) => value.clone(),
            Self::Error(value) => value.clone(),
            Self::ArraySummary { value, .. } => value.display_text_with_format(format),
        }
    }

    pub fn numeric(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            Self::Text(value) => value.trim().parse().ok(),
            _ => None,
        }
    }
}

fn format_number(value: f64) -> String {
    if value.is_nan() {
        return "#NUM!".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-∞".to_string()
        } else {
            "∞".to_string()
        };
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let mut text = format!("{value:.10}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub raw: CellValue,
    pub display: String,
    pub number_format: Option<String>,
    pub formula: Option<FormulaInfo>,
    pub style: CellStyle,
    pub wrap_text: bool,
}

impl Cell {
    pub fn new(raw: CellValue, style: CellStyle) -> Self {
        Self::new_with_format(raw, style, None)
    }

    pub fn new_with_format(
        raw: CellValue,
        style: CellStyle,
        number_format: Option<String>,
    ) -> Self {
        let display = raw.display_text_with_format(number_format.as_deref());
        Self {
            raw,
            display,
            number_format,
            formula: None,
            style,
            wrap_text: style.wrap_text,
        }
    }

    /// Whether the cell contains user-visible data. Cells that only carry
    /// borders or formatting are not considered content.
    pub fn has_content(&self) -> bool {
        !self.display.is_empty() || self.formula.is_some()
    }

    pub fn effective(&self, status: FormulaStatus) -> EffectiveCell {
        let value = self
            .formula
            .as_ref()
            .and_then(|formula| formula.evaluated.clone())
            .unwrap_or_else(|| self.raw.clone());
        let display = if matches!(status, FormulaStatus::Calculated) {
            value.display_text_with_format(self.number_format.as_deref())
        } else {
            self.display.clone()
        };
        EffectiveCell {
            value,
            display,
            style: self.style,
            formula: self.formula.clone(),
            formula_status: status,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EffectiveCell {
    pub value: CellValue,
    pub display: String,
    pub style: CellStyle,
    pub formula: Option<FormulaInfo>,
    pub formula_status: FormulaStatus,
}

impl EffectiveCell {
    pub fn is_empty(&self) -> bool {
        self.value.is_empty() && self.display.is_empty()
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum CellRead {
    Loaded(EffectiveCell),
    Pending,
    Missing,
}
