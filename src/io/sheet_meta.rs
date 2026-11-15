use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::conditional::{
    ConditionalFormat, ConditionalOperator, ConditionalRule, ConditionalStyle,
};
use crate::domain::error::MetadataError;
use crate::domain::filter::{
    CustomCondition, FilterColumnDef, FilterCriterion, FilterDefinition, FilterSource,
};
use crate::domain::freeze::FreezePane;
use crate::domain::layout::ExcelLayout;
use crate::domain::style::RgbColor;

const MAX_EXCEL_COLUMNS: u32 = 16_384;

#[derive(Clone, Debug, Default)]
pub struct SheetMetadata {
    pub filters: Vec<FilterDefinition>,
    pub freeze: FreezePane,
    pub conditional_formats: Vec<ConditionalFormat>,
    pub layout: ExcelLayout,
    pub warnings: Vec<String>,
}

pub struct SheetMetadataParser;

impl SheetMetadataParser {
    pub fn parse(path: &Path) -> Result<HashMap<String, SheetMetadata>, MetadataError> {
        let file = std::fs::File::open(path)?;
        let mut archive =
            ZipArchive::new(file).map_err(|error| MetadataError::Zip(error.to_string()))?;
        let sheets = workbook_sheets(&mut archive)?;
        let dxf_styles = parse_dxf_styles(&mut archive);
        let mut result = HashMap::new();
        for (name, sheet_path) in sheets {
            let mut xml = String::new();
            match archive.by_name(&sheet_path) {
                Ok(mut entry) => {
                    entry
                        .read_to_string(&mut xml)
                        .map_err(|error| MetadataError::Parse {
                            sheet: name.clone(),
                            message: error.to_string(),
                        })?;
                }
                Err(_) => continue,
            }
            let mut metadata = parse_sheet_xml(&name, &xml, &dxf_styles)?;
            let rels_path = Path::new(&sheet_path)
                .parent()
                .map(|parent| {
                    parent.join("_rels").join(format!(
                        "{}.rels",
                        Path::new(&sheet_path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("sheet.xml")
                    ))
                })
                .map(|path| path.to_string_lossy().to_string());
            let mut rels = HashMap::new();
            if let Some(rels_path) = rels_path
                && let Ok(mut entry) = archive.by_name(&rels_path)
            {
                let mut rels_xml = String::new();
                entry
                    .read_to_string(&mut rels_xml)
                    .map_err(|error| MetadataError::Xml(error.to_string()))?;
                rels = parse_relationships(&rels_xml);
            }
            for rid in table_parts(&xml) {
                let Some(target) = rels.get(&rid) else {
                    continue;
                };
                let table_path = normalize_relative(target);
                let Ok(mut entry) = archive.by_name(&table_path) else {
                    continue;
                };
                let mut table_xml = String::new();
                if entry.read_to_string(&mut table_xml).is_err() {
                    continue;
                }
                let table_name =
                    parse_table_name(&table_xml).unwrap_or_else(|| "Table".to_string());
                if let Ok(table_metadata) = parse_sheet_xml(&name, &table_xml, &dxf_styles) {
                    for mut filter in table_metadata.filters {
                        filter.source = FilterSource::Table {
                            name: table_name.clone(),
                        };
                        metadata.filters.push(filter);
                    }
                    metadata.warnings.extend(table_metadata.warnings);
                }
            }
            result.insert(name, metadata);
        }
        Ok(result)
    }
}

fn workbook_sheets<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<(String, String)>, MetadataError> {
    let mut workbook = String::new();
    if let Ok(mut entry) = archive.by_name("xl/workbook.xml") {
        entry
            .read_to_string(&mut workbook)
            .map_err(|error| MetadataError::Xml(error.to_string()))?;
    }
    let mut relationships = String::new();
    if let Ok(mut entry) = archive.by_name("xl/_rels/workbook.xml.rels") {
        entry
            .read_to_string(&mut relationships)
            .map_err(|error| MetadataError::Xml(error.to_string()))?;
    }
    let rels = parse_relationships(&relationships);
    let mut sheets = Vec::new();
    let mut reader = Reader::from_str(&workbook);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "sheet" =>
            {
                let mut name = None;
                let mut rid = None;
                for attr in event.attributes().flatten() {
                    match attr.key.local_name().as_ref() {
                        "name" => name = Some(attr.value.to_string()),
                        "id" => rid = Some(attr.value.to_string()),
                        _ => {}
                    }
                }
                if let (Some(name), Some(rid)) = (name, rid)
                    && let Some(target) = rels.get(&rid)
                {
                    sheets.push((name, normalize_sheet_path(target)));
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(MetadataError::Xml(error.to_string())),
            _ => {}
        }
        buf.clear();
    }
    if sheets.is_empty() {
        let mut fallback = Vec::new();
        for index in 1..=256 {
            let path = format!("xl/worksheets/sheet{index}.xml");
            if archive.by_name(&path).is_ok() {
                fallback.push((format!("Sheet{index}"), path));
            }
        }
        return Ok(fallback);
    }
    Ok(sheets)
}

fn parse_relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let mut id = None;
                let mut target = None;
                for attr in event.attributes().flatten() {
                    match attr.key.local_name().as_ref() {
                        "Id" => id = Some(attr.value.to_string()),
                        "Target" => target = Some(attr.value.to_string()),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) {
                    map.insert(id, target);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

fn normalize_sheet_path(target: &str) -> String {
    if target.starts_with("xl/") {
        target.to_string()
    } else if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("xl/{}", target.trim_start_matches('/'))
    }
}

fn table_parts(xml: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "tablePart" =>
            {
                if let Some(id) = event
                    .attributes()
                    .flatten()
                    .find(|attr| attr.key.local_name().as_ref() == "id")
                    .map(|attr| attr.value.to_string())
                {
                    parts.push(id);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    parts
}

fn parse_table_name(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "table" =>
            {
                return event
                    .attributes()
                    .flatten()
                    .find(|attr| attr.key.local_name().as_ref() == "name")
                    .map(|attr| attr.value.to_string());
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

fn normalize_relative(target: &str) -> String {
    if target.starts_with("xl/") {
        target.to_string()
    } else if let Some(stripped) = target.strip_prefix('/') {
        stripped.to_string()
    } else if let Some(stripped) = target.strip_prefix("../") {
        format!("xl/{stripped}")
    } else {
        format!("xl/{target}")
    }
}

fn parse_sheet_xml(
    sheet: &str,
    xml: &str,
    dxf_styles: &[ConditionalStyle],
) -> Result<SheetMetadata, MetadataError> {
    let mut metadata = SheetMetadata::default();
    let mut layout = ExcelLayout::default();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut freeze_state: Option<String> = None;
    let mut x_split: Option<f64> = None;
    let mut y_split: Option<f64> = None;
    let mut filter_range: Option<CellRange> = None;
    let mut filter_columns: Vec<FilterColumnDef> = Vec::new();
    let mut current_col: Option<u32> = None;
    let mut current_criterion: Option<FilterCriterion> = None;
    let mut cf_ranges: Vec<CellRange> = Vec::new();
    let mut cf_rules: Vec<ConditionalRule> = Vec::new();
    let mut current_rule: Option<(ConditionalOperator, Vec<f64>, ConditionalStyle, u32)> = None;
    let mut current_formula: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(event)) if event.local_name().as_ref() == "autoFilter" => {
                if let Some(range) = event
                    .attributes()
                    .flatten()
                    .find(|attr| attr.key.local_name().as_ref() == "ref")
                    .and_then(|attr| parse_range(&attr.value))
                {
                    let data_range = CellRange::new(
                        CellAddr::new(range.start.row + 1, range.start.col),
                        range.end,
                    );
                    metadata.filters.push(FilterDefinition {
                        source: FilterSource::Worksheet,
                        range,
                        header_row: range.start.row,
                        data_range,
                        columns: Vec::new(),
                    });
                }
            }
            Ok(Event::Start(event)) if event.local_name().as_ref() == "conditionalFormatting" => {
                cf_ranges.clear();
                for attr in event.attributes().flatten() {
                    if attr.key.local_name().as_ref() == "sqref" {
                        cf_ranges.extend(attr.value.split_whitespace().filter_map(parse_range));
                    }
                }
                cf_rules.clear();
            }
            Ok(Event::Start(event)) if event.local_name().as_ref() == "cfRule" => {
                let mut kind = String::new();
                let mut operator = None;
                let mut dxf_id = None;
                let mut priority = 0u32;
                for attr in event.attributes().flatten() {
                    match attr.key.local_name().as_ref() {
                        "type" => kind = attr.value.to_string(),
                        "operator" => operator = parse_conditional_operator(attr.value.as_ref()),
                        "dxfId" => dxf_id = attr.value.parse::<usize>().ok(),
                        "priority" => priority = attr.value.parse().unwrap_or(0),
                        _ => {}
                    }
                }
                if kind == "cellIs"
                    && let Some(operator) = operator
                {
                    let style = dxf_id
                        .and_then(|index| dxf_styles.get(index).copied())
                        .unwrap_or_default();
                    current_rule = Some((operator, Vec::new(), style, priority));
                } else {
                    current_rule = None;
                }
            }
            Ok(Event::Empty(event)) if event.local_name().as_ref() == "cfRule" => {
                // Cell rules always carry their operands in child <formula>
                // elements, so an empty cfRule has nothing we can evaluate.
            }
            Ok(Event::Start(event)) if event.local_name().as_ref() == "formula" => {
                current_formula = Some(String::new());
            }
            Ok(Event::Text(event)) => {
                if let Some(formula) = current_formula.as_mut() {
                    let content = event.xml10_content();
                    if let Ok(text) = quick_xml::escape::unescape(&content) {
                        formula.push_str(&text);
                    }
                }
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                match event.local_name().as_ref() {
                    "sheetFormatPr" => {
                        for attr in event.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "defaultColWidth" => {
                                    layout.default_column_width = parse_f32_positive(&attr.value);
                                }
                                "defaultRowHeight" => {
                                    layout.default_row_height = parse_f32_positive(&attr.value);
                                }
                                _ => {}
                            }
                        }
                    }
                    "col" => {
                        let mut min = None;
                        let mut max = None;
                        let mut width = None;
                        let mut custom = false;
                        for attr in event.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "min" => min = parse_u32(&attr.value),
                                "max" => max = parse_u32(&attr.value),
                                "width" => width = parse_f32_positive(&attr.value),
                                "customWidth" => custom = parse_bool(&attr.value),
                                _ => {}
                            }
                        }
                        if custom && let (Some(min), Some(max), Some(width)) = (min, max, width) {
                            let start = min.max(1);
                            let end = max.max(start).min(MAX_EXCEL_COLUMNS);
                            for col in start..=end {
                                layout.column_widths.insert(col - 1, width);
                            }
                        }
                    }
                    "row" => {
                        let mut row = None;
                        let mut height = None;
                        let mut custom = false;
                        for attr in event.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "r" => row = parse_u32(&attr.value),
                                "ht" => height = parse_f32_positive(&attr.value),
                                "customHeight" => custom = parse_bool(&attr.value),
                                _ => {}
                            }
                        }
                        if custom
                            && let (Some(row), Some(height)) = (row, height)
                            && row > 0
                        {
                            layout.row_heights.insert(row - 1, height);
                        }
                    }
                    "pane" => {
                        for attr in event.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "state" => freeze_state = Some(attr.value.to_string()),
                                "xSplit" => x_split = parse_attr(&attr.value),
                                "ySplit" => y_split = parse_attr(&attr.value),
                                _ => {}
                            }
                        }
                    }
                    "autoFilter" => {
                        for attr in event.attributes().flatten() {
                            if attr.key.local_name().as_ref() == "ref" {
                                filter_range = parse_range(&attr.value);
                            }
                        }
                    }
                    "filterColumn" => {
                        current_col = Some(
                            event
                                .attributes()
                                .flatten()
                                .find(|attr| attr.key.local_name().as_ref() == "colId")
                                .and_then(|attr| attr.value.parse().ok())
                                .unwrap_or(0),
                        );
                    }
                    "filters" => {
                        let blank = event
                            .attributes()
                            .flatten()
                            .find(|attr| attr.key.local_name().as_ref() == "blank")
                            .map(|attr| matches!(attr.value.as_ref(), "1" | "true"))
                            .unwrap_or(false);
                        current_criterion = Some(FilterCriterion::ValueList {
                            values: Vec::new(),
                            include_blank: blank,
                        });
                    }
                    "filter" => {
                        if let (Some(FilterCriterion::ValueList { values, .. }), Some(value)) = (
                            current_criterion.as_mut(),
                            event
                                .attributes()
                                .flatten()
                                .find(|attr| attr.key.local_name().as_ref() == "val")
                                .map(|attr| attr.value.to_string()),
                        ) {
                            values.push(value);
                        }
                    }
                    "customFilters" => {
                        let and = event
                            .attributes()
                            .flatten()
                            .find(|attr| attr.key.local_name().as_ref() == "and")
                            .map(|attr| matches!(attr.value.as_ref(), "1" | "true"))
                            .unwrap_or(false);
                        current_criterion = Some(FilterCriterion::Custom {
                            and,
                            conditions: Vec::new(),
                        });
                    }
                    "customFilter" => {
                        let mut operator = "equal".to_string();
                        let mut value = String::new();
                        for attr in event.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "operator" => operator = attr.value.to_string(),
                                "val" => value = attr.value.to_string(),
                                _ => {}
                            }
                        }
                        if let Some(FilterCriterion::Custom { conditions, .. }) =
                            current_criterion.as_mut()
                        {
                            conditions.push(CustomCondition { operator, value });
                        }
                    }
                    "top10" => {
                        let mut top = true;
                        let mut percent = false;
                        let mut value = 10.0;
                        for attr in event.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "top" => top = matches!(attr.value.as_ref(), "1" | "true"),
                                "percent" => percent = matches!(attr.value.as_ref(), "1" | "true"),
                                "val" => value = attr.value.parse().unwrap_or(10.0),
                                _ => {}
                            }
                        }
                        current_criterion = Some(FilterCriterion::Top10 {
                            top,
                            percent,
                            value,
                        });
                    }
                    "colorFilter" | "iconFilter" | "dynamicFilter" | "dateGroupItem" => {
                        current_criterion = Some(FilterCriterion::Unsupported {
                            kind: event.local_name().as_ref().to_string(),
                        });
                    }
                    _ => {}
                }
                if event.local_name().as_ref() == "filterColumn" {
                    continue;
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "filterColumn" => {
                    if let Some(col_id) = current_col.take()
                        && let Some(criterion) = current_criterion.take()
                    {
                        if let FilterCriterion::Unsupported { kind } = &criterion {
                            metadata
                                .warnings
                                .push(format!("unsupported filter in column {col_id}: {kind}"));
                        }
                        filter_columns.push(FilterColumnDef { col_id, criterion });
                    }
                }
                "autoFilter" => {
                    if let Some(range) = filter_range.take() {
                        let data_range = CellRange::new(
                            CellAddr::new(range.start.row + 1, range.start.col),
                            range.end,
                        );
                        metadata.filters.push(FilterDefinition {
                            source: FilterSource::Worksheet,
                            range,
                            header_row: range.start.row,
                            data_range,
                            columns: std::mem::take(&mut filter_columns),
                        });
                    }
                }
                "formula" => {
                    if let (Some((_, values, _, _)), Some(formula)) =
                        (current_rule.as_mut(), current_formula.take())
                        && let Ok(value) = formula.trim().parse::<f64>()
                    {
                        values.push(value);
                    }
                }
                "cfRule" => {
                    if let Some((operator, values, style, priority)) = current_rule.take() {
                        let required = matches!(
                            operator,
                            ConditionalOperator::Between | ConditionalOperator::NotBetween
                        )
                        .then_some(2)
                        .unwrap_or(1);
                        if values.len() >= required {
                            cf_rules.push(ConditionalRule {
                                operator,
                                values,
                                style,
                                priority,
                            });
                        }
                    }
                    current_formula = None;
                }
                "conditionalFormatting" => {
                    if !cf_rules.is_empty() {
                        for range in cf_ranges.drain(..) {
                            metadata.conditional_formats.push(ConditionalFormat {
                                range,
                                rules: cf_rules.clone(),
                            });
                        }
                    }
                    cf_rules.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(MetadataError::Parse {
                    sheet: sheet.to_string(),
                    message: error.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }
    if let Some(state) = freeze_state {
        if state == "frozen" {
            let rows = y_split.unwrap_or(0.0).max(0.0).floor() as u32;
            let cols = x_split.unwrap_or(0.0).max(0.0).floor() as u32;
            metadata.freeze = FreezePane::new(rows, cols);
        } else {
            metadata
                .warnings
                .push(format!("unsupported pane state: {state}"));
        }
    }
    metadata.layout = layout;
    Ok(metadata)
}

fn parse_dxf_styles<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Vec<ConditionalStyle> {
    let Ok(mut entry) = archive.by_name("xl/styles.xml") else {
        return Vec::new();
    };
    let mut xml = String::new();
    if entry.read_to_string(&mut xml).is_err() {
        return Vec::new();
    }
    parse_dxf_styles_xml(&xml)
}

fn parse_dxf_styles_xml(xml: &str) -> Vec<ConditionalStyle> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut styles = Vec::new();
    let mut current: Option<ConditionalStyle> = None;
    let mut in_dxfs = false;
    let mut in_font = false;
    let mut in_fill = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "dxfs" => in_dxfs = true,
                "dxf" if in_dxfs => current = Some(ConditionalStyle::default()),
                "font" if in_dxfs => in_font = true,
                "fill" if in_dxfs => in_fill = true,
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.local_name().as_ref() {
                "b" if in_font => {
                    let bold = event
                        .attributes()
                        .flatten()
                        .find(|attr| attr.key.local_name().as_ref() == "val")
                        .map(|attr| !matches!(attr.value.as_ref(), "0" | "false"))
                        .unwrap_or(true);
                    if let Some(style) = current.as_mut() {
                        style.bold = Some(bold);
                    }
                }
                "color" if in_font => {
                    if let Some(color) = event
                        .attributes()
                        .flatten()
                        .find(|attr| attr.key.local_name().as_ref() == "rgb")
                        .and_then(|attr| parse_rgb(attr.value.as_ref()))
                        && let Some(style) = current.as_mut()
                    {
                        style.color = Some(color);
                    }
                }
                "fgColor" | "bgColor" if in_fill => {
                    if let Some(color) = event
                        .attributes()
                        .flatten()
                        .find(|attr| attr.key.local_name().as_ref() == "rgb")
                        .and_then(|attr| parse_rgb(attr.value.as_ref()))
                        && let Some(style) = current.as_mut()
                    {
                        style.fill = Some(color);
                    }
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "font" => in_font = false,
                "fill" => in_fill = false,
                "dxf" if in_dxfs => {
                    if let Some(style) = current.take() {
                        styles.push(style);
                    }
                }
                "dxfs" => in_dxfs = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    styles
}

fn parse_rgb(value: &str) -> Option<RgbColor> {
    let text = value.trim();
    let hex = text.strip_prefix('#').unwrap_or(text);
    let (r, g, b) = match hex.len() {
        8 => (
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ),
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ),
        _ => return None,
    };
    Some(RgbColor::new(r, g, b))
}

fn parse_conditional_operator(value: &str) -> Option<ConditionalOperator> {
    match value {
        "equal" => Some(ConditionalOperator::Equal),
        "notEqual" => Some(ConditionalOperator::NotEqual),
        "greaterThan" => Some(ConditionalOperator::GreaterThan),
        "greaterThanOrEqual" => Some(ConditionalOperator::GreaterThanOrEqual),
        "lessThan" => Some(ConditionalOperator::LessThan),
        "lessThanOrEqual" => Some(ConditionalOperator::LessThanOrEqual),
        "between" => Some(ConditionalOperator::Between),
        "notBetween" => Some(ConditionalOperator::NotBetween),
        _ => None,
    }
}

fn parse_attr(value: &str) -> Option<f64> {
    value.parse().ok()
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn parse_f32_positive(value: &str) -> Option<f32> {
    let parsed = value.parse::<f32>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
}

fn parse_bool(value: &str) -> bool {
    matches!(value, "1" | "true" | "TRUE" | "True")
}

fn parse_range(value: &str) -> Option<CellRange> {
    let value = value.trim().replace('$', "");
    let value = value.as_str();
    if value.is_empty() {
        return None;
    }
    if let Some((a, b)) = value.split_once(':') {
        Some(CellRange::new(CellAddr::from_a1(a)?, CellAddr::from_a1(b)?))
    } else {
        let addr = CellAddr::from_a1(value)?;
        Some(CellRange::single(addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_excel_layout() {
        let xml = r#"<worksheet>
            <sheetFormatPr defaultColWidth="12.5" defaultRowHeight="15"/>
            <cols>
                <col min="1" max="2" width="24.7109375" customWidth="1"/>
                <col min="3" max="3" width="5" customWidth="0"/>
            </cols>
            <sheetData>
                <row r="1" ht="30" customHeight="1"><c r="A1"><v>1</v></c></row>
                <row r="2" ht="20" customHeight="0"/>
            </sheetData>
        </worksheet>"#;
        let metadata = parse_sheet_xml("Sheet1", xml, &[]).unwrap();
        assert_eq!(metadata.layout.default_column_width, Some(12.5));
        assert_eq!(metadata.layout.default_row_height, Some(15.0));
        assert_eq!(metadata.layout.column_widths.get(&0), Some(&24.710_938));
        assert_eq!(metadata.layout.column_widths.get(&1), Some(&24.710_938));
        assert!(!metadata.layout.column_widths.contains_key(&2));
        assert_eq!(metadata.layout.row_heights.get(&0), Some(&30.0));
        assert!(!metadata.layout.row_heights.contains_key(&1));
    }

    #[test]
    fn ignores_invalid_excel_layout_values() {
        let xml = r#"<worksheet>
            <sheetFormatPr defaultColWidth="bad" defaultRowHeight="-1"/>
            <cols>
                <col min="1" max="1" width="NaN" customWidth="1"/>
                <col min="2" max="2" width="-3" customWidth="1"/>
            </cols>
            <sheetData>
                <row r="1" ht="NaN" customHeight="1"/>
                <row r="0" ht="30" customHeight="1"/>
            </sheetData>
        </worksheet>"#;
        let metadata = parse_sheet_xml("Sheet1", xml, &[]).unwrap();
        assert!(metadata.layout.default_column_width.is_none());
        assert!(metadata.layout.default_row_height.is_none());
        assert!(metadata.layout.column_widths.is_empty());
        assert!(metadata.layout.row_heights.is_empty());
    }

    #[test]
    fn later_column_definitions_win() {
        let xml = r#"<worksheet>
            <cols>
                <col min="1" max="1" width="10" customWidth="1"/>
                <col min="1" max="1" width="20" customWidth="1"/>
            </cols>
        </worksheet>"#;
        let metadata = parse_sheet_xml("Sheet1", xml, &[]).unwrap();
        assert_eq!(metadata.layout.column_widths.get(&0), Some(&20.0));
    }
}
