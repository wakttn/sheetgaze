use crate::domain::cell::{Cell, CellValue};
use crate::domain::copy::CopyEncoder;
use crate::domain::edit::parse_cell_input;
use crate::domain::register::{ClipboardBlock, RegisterError, RegisterKind, RegisterLimits};
use crate::domain::style::CellStyle;

pub struct TsvCodec;

impl TsvCodec {
    pub fn encode(block: &ClipboardBlock) -> Result<String, RegisterError> {
        let mut output = String::new();
        for row in 0..block.height {
            if row > 0 {
                output.push_str("\r\n");
            }
            for col in 0..block.width {
                if col > 0 {
                    output.push('\t');
                }
                let text = block
                    .cell(row, col)
                    .map(|cell| cell.display.as_str())
                    .unwrap_or("");
                output.push_str(&CopyEncoder::quote_tsv_field(text));
            }
        }
        if output.is_empty() {
            return Err(RegisterError::Empty);
        }
        Ok(output)
    }

    pub fn decode(text: &str, limits: &RegisterLimits) -> Result<ClipboardBlock, RegisterError> {
        if text.is_empty() {
            return Err(RegisterError::Empty);
        }
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut records: Vec<Vec<String>> = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| RegisterError::InvalidTsv {
                message: error.to_string(),
            })?;
            records.push(record.iter().map(str::to_string).collect());
        }
        if records.is_empty() {
            return Err(RegisterError::Empty);
        }
        let width = records.iter().map(Vec::len).max().unwrap_or(0) as u32;
        let height = records.len() as u32;
        let cells = (width as usize).saturating_mul(height as usize);
        if cells > limits.max_cells {
            return Err(RegisterError::TooLarge {
                cells,
                limit: limits.max_cells,
            });
        }
        let mut block = ClipboardBlock::new(RegisterKind::Cells, width, height, true);
        for (row, record) in records.iter().enumerate() {
            for col in 0..width as usize {
                let text = record.get(col).map(String::as_str).unwrap_or("");
                let cell = if text.is_empty() {
                    Cell::new(CellValue::Empty, CellStyle::default())
                } else {
                    parse_cell_input(text, CellStyle::default(), None)
                };
                block.insert(row as u32, col as u32, cell);
            }
        }
        Ok(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_quotes_tabs_and_newlines() {
        let mut block = ClipboardBlock::new(RegisterKind::Cells, 2, 2, true);
        block.insert(
            0,
            0,
            Cell::new(CellValue::Text("a\tb".to_string()), CellStyle::default()),
        );
        block.insert(
            0,
            1,
            Cell::new(
                CellValue::Text("line1\nline2".to_string()),
                CellStyle::default(),
            ),
        );
        block.insert(
            1,
            0,
            Cell::new(CellValue::Text("a\"b".to_string()), CellStyle::default()),
        );
        let text = TsvCodec::encode(&block).unwrap();
        let decoded = TsvCodec::decode(&text, &RegisterLimits::default()).unwrap();
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(
            decoded.cell(0, 0).unwrap().raw,
            CellValue::Text("a\tb".to_string())
        );
        assert_eq!(
            decoded.cell(0, 1).unwrap().raw,
            CellValue::Text("line1\nline2".to_string())
        );
        assert_eq!(
            decoded.cell(1, 0).unwrap().raw,
            CellValue::Text("a\"b".to_string())
        );
    }

    #[test]
    fn pads_short_records_and_parses_values() {
        let decoded = TsvCodec::decode("1\tTRUE\ttext\n2", &RegisterLimits::default()).unwrap();
        assert_eq!(decoded.width, 3);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.cell(0, 0).unwrap().raw, CellValue::Number(1.0));
        assert_eq!(decoded.cell(0, 1).unwrap().raw, CellValue::Bool(true));
        assert_eq!(decoded.cell(1, 1).unwrap().raw, CellValue::Empty);
    }

    #[test]
    fn rejects_empty_and_oversized_input() {
        assert_eq!(
            TsvCodec::decode("", &RegisterLimits::default()).unwrap_err(),
            RegisterError::Empty
        );
        let limits = RegisterLimits { max_cells: 1 };
        assert!(matches!(
            TsvCodec::decode("a\tb", &limits).unwrap_err(),
            RegisterError::TooLarge { .. }
        ));
    }
}
