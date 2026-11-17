use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use zip::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::domain::addr::CellAddr;
use crate::domain::cell::{Cell, CellValue};
use crate::domain::error::SaveError;
use crate::domain::formula::FormulaInfo;
use crate::domain::workbook::{SourceFormat, WorkbookModel};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveOutcome {
    pub cells: usize,
}

/// Writes all unsaved cell edits back to the workbook's source path using an
/// atomic temporary file.
pub fn save_workbook(workbook: &WorkbookModel) -> Result<SaveOutcome, SaveError> {
    if workbook.source.format != SourceFormat::Xlsx {
        return Err(SaveError::UnsupportedFormat);
    }
    let edited: Vec<&crate::domain::sheet::Sheet> = workbook
        .sheets
        .iter()
        .filter(|sheet| !sheet.edits.is_empty())
        .collect();
    if edited.is_empty() {
        return Err(SaveError::NoChanges);
    }
    let path = workbook.source.path.clone();
    let metadata = std::fs::metadata(&path).map_err(|error| SaveError::Io {
        path: path.clone(),
        message: error.to_string(),
    })?;
    if metadata.permissions().readonly() {
        return Err(SaveError::ReadOnly { path });
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".sheetgaze-")
        .suffix(".xlsx")
        .tempfile_in(parent)
        .map_err(|error| SaveError::Io {
            path: path.clone(),
            message: error.to_string(),
        })?;
    write_patched_zip(workbook, temp.as_file_mut())?;
    let cells = edited.iter().map(|sheet| sheet.edits.len()).sum();
    std::fs::set_permissions(temp.path(), metadata.permissions()).map_err(|error| {
        SaveError::Io {
            path: path.clone(),
            message: error.to_string(),
        }
    })?;
    temp.persist(&path).map_err(|error| SaveError::Io {
        path: path.clone(),
        message: error.error.to_string(),
    })?;
    Ok(SaveOutcome { cells })
}

/// Creates a temporary patched workbook for formula recalculation. The caller
/// owns the returned file and should keep it alive until the calculation job
/// finishes.
pub fn write_temp_workbook(workbook: &WorkbookModel) -> Result<NamedTempFile, SaveError> {
    if workbook.source.format != SourceFormat::Xlsx {
        return Err(SaveError::UnsupportedFormat);
    }
    let mut temp = tempfile::Builder::new()
        .prefix("sheetgaze-calc-")
        .suffix(".xlsx")
        .tempfile()
        .map_err(|error| SaveError::Io {
            path: PathBuf::from("(temporary file)"),
            message: error.to_string(),
        })?;
    write_patched_zip(workbook, temp.as_file_mut())?;
    Ok(temp)
}

/// Removes the temporary workbook at drop time. Kept as a small wrapper so
/// callers do not need to depend on `tempfile` directly.
fn write_patched_zip(workbook: &WorkbookModel, output: &mut File) -> Result<(), SaveError> {
    let source_path = &workbook.source.path;
    let source = File::open(source_path).map_err(|error| SaveError::Io {
        path: source_path.clone(),
        message: error.to_string(),
    })?;
    let mut archive = ZipArchive::new(source).map_err(|error| SaveError::Zip(error.to_string()))?;
    let sheet_paths = sheet_paths(&mut archive)?;
    let mut replacements: HashMap<String, Vec<u8>> = HashMap::new();

    for sheet in &workbook.sheets {
        let cache: HashMap<CellAddr, CellValue> = sheet
            .formula_overlay
            .values
            .iter()
            .map(|(addr, result)| (*addr, result.value.clone()))
            .collect();
        if sheet.edits.is_empty() && cache.is_empty() {
            continue;
        }
        let Some(path) = sheet_paths.get(&sheet.name).cloned() else {
            return Err(SaveError::Xml(format!(
                "could not find worksheet XML for {}",
                sheet.name
            )));
        };
        let xml = read_entry(&mut archive, &path)?;
        let xml = String::from_utf8(xml)
            .map_err(|error| SaveError::Xml(format!("worksheet XML is not UTF-8: {error}")))?;
        let patched = patch_sheet_xml(
            &xml,
            &sheet.edits,
            &cache,
            sheet.name.as_str(),
            sheet.used_range.end,
        )?;
        replacements.insert(path, patched.into_bytes());
    }

    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let workbook_xml = String::from_utf8(workbook_xml)
        .map_err(|error| SaveError::Xml(format!("workbook XML is not UTF-8: {error}")))?;
    replacements.insert(
        "xl/workbook.xml".to_string(),
        patch_calc_pr(&workbook_xml).into_bytes(),
    );

    if let Ok(content_types) = read_entry(&mut archive, "[Content_Types].xml") {
        let text = String::from_utf8_lossy(&content_types);
        let patched = remove_tags_containing(&text, "calcChain");
        if patched != text {
            replacements.insert("[Content_Types].xml".to_string(), patched.into_bytes());
        }
    }
    if let Ok(rels) = read_entry(&mut archive, "xl/_rels/workbook.xml.rels") {
        let text = String::from_utf8_lossy(&rels);
        let patched = remove_tags_containing(&text, "calcChain");
        if patched != text {
            replacements.insert(
                "xl/_rels/workbook.xml.rels".to_string(),
                patched.into_bytes(),
            );
        }
    }

    output.set_len(0).map_err(|error| SaveError::Io {
        path: source_path.clone(),
        message: error.to_string(),
    })?;
    output
        .seek(std::io::SeekFrom::Start(0))
        .map_err(|error| SaveError::Io {
            path: source_path.clone(),
            message: error.to_string(),
        })?;
    let mut writer = ZipWriter::new(output);
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| SaveError::Zip(error.to_string()))?;
        let name = entry
            .name()
            .map_err(|error| SaveError::Zip(error.to_string()))?
            .to_string();
        if name == "xl/calcChain.xml" {
            continue;
        }
        let options = file_options(&entry);
        if name.ends_with('/') {
            writer
                .add_directory(name.clone(), options)
                .map_err(|error| SaveError::Zip(error.to_string()))?;
            continue;
        }
        if let Some(data) = replacements.get(&name) {
            writer
                .start_file(name.clone(), options)
                .map_err(|error| SaveError::Zip(error.to_string()))?;
            writer.write_all(data).map_err(|error| SaveError::Io {
                path: source_path.clone(),
                message: error.to_string(),
            })?;
        } else {
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|error| SaveError::Zip(error.to_string()))?;
            writer
                .start_file(name.clone(), options)
                .map_err(|error| SaveError::Zip(error.to_string()))?;
            writer.write_all(&data).map_err(|error| SaveError::Io {
                path: source_path.clone(),
                message: error.to_string(),
            })?;
        }
    }
    let file = writer
        .finish()
        .map_err(|error| SaveError::Zip(error.to_string()))?;
    file.sync_all().map_err(|error| SaveError::Io {
        path: source_path.clone(),
        message: error.to_string(),
    })?;
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|error| SaveError::Io {
            path: source_path.clone(),
            message: error.to_string(),
        })?;
    validate_archive(file, &replacements)?;
    Ok(())
}

fn validate_archive(
    file: &mut File,
    replacements: &HashMap<String, Vec<u8>>,
) -> Result<(), SaveError> {
    let mut archive = ZipArchive::new(file).map_err(|error| SaveError::Zip(error.to_string()))?;
    for name in replacements.keys() {
        let mut entry = archive
            .by_name(name)
            .map_err(|error| SaveError::Zip(format!("{name}: {error}")))?;
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|error| SaveError::Xml(format!("{name}: {error}")))?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(error) => {
                    return Err(SaveError::Xml(format!("{name}: {error}")));
                }
            }
        }
    }
    Ok(())
}

fn file_options<R: Read + ?Sized>(entry: &zip::read::ZipFile<'_, R>) -> SimpleFileOptions {
    let mut options = SimpleFileOptions::default()
        .compression_method(entry.compression())
        .unix_permissions(entry.unix_mode().unwrap_or(0o644));
    if let Some(modified) = entry.last_modified() {
        options = options.last_modified_time(modified);
    }
    options
}

fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, SaveError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| SaveError::Zip(format!("{name}: {error}")))?;
    let mut data = Vec::new();
    entry
        .read_to_end(&mut data)
        .map_err(|error| SaveError::Zip(format!("{name}: {error}")))?;
    Ok(data)
}

fn sheet_paths<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<HashMap<String, String>, SaveError> {
    let workbook = read_entry(archive, "xl/workbook.xml")?;
    let workbook = String::from_utf8(workbook)
        .map_err(|error| SaveError::Xml(format!("workbook XML is not UTF-8: {error}")))?;
    let rels = read_entry(archive, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let rels = String::from_utf8_lossy(&rels);
    let mut relationships = HashMap::new();
    let mut pos = 0;
    while let Some(tag) = find_open_tag(&rels, pos, "Relationship") {
        let raw = &rels[tag.start..tag.end];
        if let (Some(id), Some(target)) = (attribute(raw, "Id"), attribute(raw, "Target")) {
            relationships.insert(id, normalize_target(&target));
        }
        pos = tag.end;
    }
    let mut paths = HashMap::new();
    let mut pos = 0;
    while let Some(tag) = find_open_tag(&workbook, pos, "sheet") {
        let raw = &workbook[tag.start..tag.end];
        let name = attribute(raw, "name");
        let rid = attribute(raw, "r:id").or_else(|| attribute(raw, "id"));
        if let (Some(name), Some(rid)) = (name, rid)
            && let Some(target) = relationships.get(&rid)
        {
            paths.insert(name, target.clone());
        }
        pos = tag.end;
    }
    Ok(paths)
}

fn normalize_target(target: &str) -> String {
    let target = target.trim_start_matches('/');
    let raw = if target.starts_with("xl/") {
        target.to_string()
    } else {
        format!("xl/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn patch_sheet_xml(
    xml: &str,
    edits: &HashMap<CellAddr, Cell>,
    cache: &HashMap<CellAddr, CellValue>,
    sheet_name: &str,
    used_end: CellAddr,
) -> Result<String, SaveError> {
    let Some(sheet_data) = find_open_tag(xml, 0, "sheetData") else {
        return Err(SaveError::Xml(format!("sheetData missing in {sheet_name}")));
    };
    let (content_start, content_end, close_tag, close_end) = if sheet_data.self_closing {
        (
            sheet_data.end,
            sheet_data.end,
            "</sheetData>",
            sheet_data.end,
        )
    } else {
        let Some(close) = find_close_tag(xml, sheet_data.end, "sheetData") else {
            return Err(SaveError::Xml(format!(
                "sheetData is not closed in {sheet_name}"
            )));
        };
        (
            sheet_data.end,
            close.start,
            &xml[close.start..close.end],
            close.end,
        )
    };
    let content = &xml[content_start..content_end];
    let rows = parse_rows(content, edits, sheet_name)?;
    let mut by_row: BTreeMap<u32, RowSpan> = BTreeMap::new();
    for row in rows {
        by_row.insert(row.row, row);
    }
    for addr in edits.keys() {
        by_row.entry(addr.row).or_insert_with(|| RowSpan {
            row: addr.row,
            open_tag: format!("<row r=\"{}\">", addr.row + 1),
            inner: String::new(),
            close_tag: "</row>".to_string(),
            cells: Vec::new(),
        });
    }

    let mut new_content = String::new();
    for (row_number, row) in by_row {
        let row_edits: BTreeMap<u32, &Cell> = edits
            .iter()
            .filter(|(addr, _)| addr.row == row_number)
            .map(|(addr, cell)| (addr.col, cell))
            .collect();
        let row_cache: BTreeMap<u32, &CellValue> = cache
            .iter()
            .filter(|(addr, _)| addr.row == row_number)
            .map(|(addr, value)| (addr.col, value))
            .collect();
        new_content.push_str(&patch_row(&row, &row_edits, &row_cache, sheet_name)?);
        new_content.push('\n');
    }

    let mut open_tag = xml[sheet_data.start..sheet_data.end].to_string();
    if sheet_data.self_closing
        && let Some(pos) = open_tag.rfind('/')
    {
        open_tag.remove(pos);
    }
    let mut result = String::new();
    result.push_str(&xml[..sheet_data.start]);
    result.push_str(&open_tag);
    result.push_str(&new_content);
    result.push_str(close_tag);
    result.push_str(&xml[close_end..]);
    result = patch_dimension(result, used_end);
    Ok(result)
}

#[derive(Clone, Debug)]
struct RowSpan {
    row: u32,
    open_tag: String,
    inner: String,
    close_tag: String,
    cells: Vec<CellSpan>,
}

#[derive(Clone, Debug)]
struct CellSpan {
    addr: CellAddr,
    col: u32,
    open_tag: String,
    raw: String,
}

fn parse_rows(
    content: &str,
    edits: &HashMap<CellAddr, Cell>,
    sheet_name: &str,
) -> Result<Vec<RowSpan>, SaveError> {
    let mut rows = Vec::new();
    let mut pos = 0usize;
    let mut previous_row = 0u32;
    while let Some(tag) = find_open_tag(content, pos, "row") {
        let raw = &content[tag.start..tag.end];
        let row = attribute(raw, "r")
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| value.saturating_sub(1))
            .unwrap_or(previous_row + 1);
        previous_row = row;
        let (inner, close_tag, end) = if tag.self_closing {
            (String::new(), "</row>".to_string(), tag.end)
        } else {
            let Some(close) = find_close_tag(content, tag.end, "row") else {
                return Err(SaveError::Xml(format!(
                    "row {row} is not closed in {sheet_name}"
                )));
            };
            (
                content[tag.end..close.start].to_string(),
                content[close.start..close.end].to_string(),
                close.end,
            )
        };
        let cells = parse_cells(&inner, row);
        for addr in edits.keys().filter(|addr| addr.row == row) {
            if let Some(cell) = cells.iter().find(|cell| cell.addr == *addr) {
                ensure_editable_cell(sheet_name, *addr, &cell.raw)?;
            }
        }
        let open_tag = if tag.self_closing {
            let mut raw = raw.to_string();
            if let Some(position) = raw.rfind('/') {
                raw.remove(position);
            }
            raw
        } else {
            raw.to_string()
        };
        rows.push(RowSpan {
            row,
            open_tag,
            inner,
            close_tag,
            cells,
        });
        pos = end;
    }
    Ok(rows)
}

fn parse_cells(inner: &str, row: u32) -> Vec<CellSpan> {
    let mut cells = Vec::new();
    let mut pos = 0usize;
    let mut previous_col = 0u32;
    while let Some(tag) = find_open_tag(inner, pos, "c") {
        let raw = &inner[tag.start..tag.end];
        let addr = attribute(raw, "r")
            .and_then(|value| CellAddr::from_a1(&value))
            .unwrap_or(CellAddr::new(row, previous_col.saturating_add(1)));
        previous_col = addr.col;
        let end = if tag.self_closing {
            tag.end
        } else {
            find_close_tag(inner, tag.end, "c")
                .map(|close| close.end)
                .unwrap_or(tag.end)
        };
        cells.push(CellSpan {
            addr,
            col: addr.col,
            open_tag: raw.to_string(),
            raw: inner[tag.start..end].to_string(),
        });
        pos = end;
    }
    cells
}

fn patch_row(
    row: &RowSpan,
    edits: &BTreeMap<u32, &Cell>,
    cache: &BTreeMap<u32, &CellValue>,
    sheet_name: &str,
) -> Result<String, SaveError> {
    if edits.is_empty() && cache.is_empty() {
        return Ok(format!("{}{}{}", row.open_tag, row.inner, row.close_tag));
    }
    let mut cells: BTreeMap<u32, String> = row
        .cells
        .iter()
        .map(|cell| (cell.col, cell.raw.clone()))
        .collect();
    for (col, edited) in edits {
        let (addr, original_tag) = row
            .cells
            .iter()
            .find(|cell| cell.col == *col)
            .map(|cell| (cell.addr, Some(cell.open_tag.as_str())))
            .unwrap_or((CellAddr::new(row.row, *col), None));
        if let Some(cell) = row.cells.iter().find(|cell| cell.col == *col) {
            ensure_editable_cell(sheet_name, cell.addr, &cell.raw)?;
        }
        cells.insert(*col, render_cell(addr, edited, original_tag));
    }
    for (col, value) in cache {
        if edits.contains_key(col) {
            continue;
        }
        if let Some(raw) = cells.get(col).cloned()
            && let Some(patched) = patch_cached_cell(&raw, value)
        {
            cells.insert(*col, patched);
        }
    }
    let mut output = String::new();
    output.push_str(&row.open_tag);
    for cell in cells.values() {
        output.push_str(cell);
    }
    output.push_str(&row.close_tag);
    Ok(output)
}

fn ensure_editable_cell(sheet_name: &str, addr: CellAddr, raw: &str) -> Result<(), SaveError> {
    if raw.contains("t=\"shared\"") || raw.contains("t='shared'") {
        return Err(SaveError::UnsupportedFormula {
            sheet: sheet_name.to_string(),
            addr: addr.a1(),
            message: "shared formula cells cannot be edited yet".to_string(),
        });
    }
    if raw.contains("t=\"array\"") || raw.contains("t='array'") {
        return Err(SaveError::UnsupportedFormula {
            sheet: sheet_name.to_string(),
            addr: addr.a1(),
            message: "array formula cells cannot be edited yet".to_string(),
        });
    }
    Ok(())
}

fn patch_cached_cell(raw: &str, value: &CellValue) -> Option<String> {
    find_open_tag(raw, 0, "f")?;
    let tag_end = find_tag_end(raw, 0)?;
    let type_attr = match value {
        CellValue::Text(_) => Some("str"),
        CellValue::Bool(_) => Some("b"),
        CellValue::Error(_) => Some("e"),
        CellValue::Number(_) => None,
        CellValue::Empty | CellValue::DateTime(_) | CellValue::ArraySummary { .. } => return None,
    };
    let cached = match value {
        CellValue::Text(value) => xml_escape(value),
        CellValue::Bool(value) => {
            if *value {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        CellValue::Error(value) => xml_escape(value),
        CellValue::Number(value) => value.to_string(),
        CellValue::Empty | CellValue::DateTime(_) | CellValue::ArraySummary { .. } => return None,
    };
    let mut output = String::new();
    output.push_str(&set_cell_type(&raw[..tag_end], type_attr));
    let body = &raw[tag_end..];
    if let Some(value_tag) = find_open_tag(body, 0, "v") {
        let value_end = find_close_tag(body, value_tag.end, "v")?.end;
        output.push_str(&body[..value_tag.start]);
        output.push_str("<v>");
        output.push_str(&cached);
        output.push_str("</v>");
        output.push_str(&body[value_end..]);
    } else {
        let close = body.rfind("</c>")?;
        output.push_str(&body[..close]);
        output.push_str("<v>");
        output.push_str(&cached);
        output.push_str("</v>");
        output.push_str(&body[close..]);
    }
    Some(output)
}

fn set_cell_type(tag: &str, value: Option<&str>) -> String {
    let mut output = tag.to_string();
    if let Some(span) = find_attribute_span(&output, "t") {
        output.replace_range(span.0..span.1, "");
    }
    if let Some(value) = value {
        let position = output
            .rfind("/>")
            .or_else(|| output.rfind('>'))
            .unwrap_or(output.len());
        output.insert_str(position, &format!(" t=\"{value}\""));
    }
    output
}

fn find_attribute_span(tag: &str, name: &str) -> Option<(usize, usize)> {
    let bytes = tag.as_bytes();
    let mut index = 1usize;
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && bytes[index] != b'>'
        && bytes[index] != b'/'
    {
        index += 1;
    }
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'>' || bytes[index] == b'/' {
            break;
        }
        let start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        let key = &tag[start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || (bytes[index] != b'"' && bytes[index] != b'\'') {
            break;
        }
        let quote = bytes[index];
        index += 1;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        if index < bytes.len() {
            index += 1;
        }
        if key == name {
            return Some((start, index));
        }
    }
    None
}

fn render_cell(addr: CellAddr, cell: &Cell, original_tag: Option<&str>) -> String {
    let mut attrs = format!(" r=\"{}\"", addr.a1());
    if let Some(tag) = original_tag {
        for name in ["s", "cm", "vm", "ph"] {
            if let Some(value) = attribute(tag, name) {
                attrs.push_str(&format!(" {name}=\"{value}\""));
            }
        }
    }
    if let Some(formula) = &cell.formula {
        return render_formula(&attrs, formula);
    }
    match &cell.raw {
        CellValue::Empty => format!("<c{attrs}/>"),
        CellValue::Text(value) => render_text(&attrs, value),
        CellValue::Number(value) => format!("<c{attrs}><v>{value}</v></c>"),
        CellValue::Bool(value) => {
            format!(
                "<c{attrs} t=\"b\"><v>{}</v></c>",
                if *value { 1 } else { 0 }
            )
        }
        CellValue::DateTime(value) => render_text(&attrs, value),
        CellValue::Error(value) => format!("<c{attrs} t=\"e\"><v>{}</v></c>", xml_escape(value)),
        CellValue::ArraySummary { value, .. } => match value.as_ref() {
            CellValue::Number(value) => format!("<c{attrs}><v>{value}</v></c>"),
            CellValue::Text(value) => render_text(&attrs, value),
            CellValue::Bool(value) => {
                format!(
                    "<c{attrs} t=\"b\"><v>{}</v></c>",
                    if *value { 1 } else { 0 }
                )
            }
            CellValue::Error(value) => {
                format!("<c{attrs} t=\"e\"><v>{}</v></c>", xml_escape(value))
            }
            _ => format!("<c{attrs}/>"),
        },
    }
}

fn render_text(attrs: &str, value: &str) -> String {
    format!(
        "<c{attrs} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
        xml_escape(value)
    )
}

fn render_formula(attrs: &str, formula: &FormulaInfo) -> String {
    let expression = formula.expression.trim_start_matches('=');
    let value = formula.evaluated.clone().or_else(|| formula.cached.clone());
    let (type_attr, cached) = match value {
        Some(CellValue::Text(value)) => (" t=\"str\"", Some(xml_escape(&value))),
        Some(CellValue::Bool(value)) => (
            " t=\"b\"",
            Some(if value {
                "1".to_string()
            } else {
                "0".to_string()
            }),
        ),
        Some(CellValue::Number(value)) => ("", Some(value.to_string())),
        Some(CellValue::Error(value)) => (" t=\"e\"", Some(xml_escape(&value))),
        Some(CellValue::DateTime(_)) | Some(CellValue::ArraySummary { .. }) | None => ("", None),
        Some(CellValue::Empty) => ("", None),
    };
    let formula = format!("<f>{}</f>", xml_escape(expression));
    match cached {
        Some(value) => format!("<c{attrs}{type_attr}>{formula}<v>{value}</v></c>"),
        None => format!("<c{attrs}{type_attr}>{formula}</c>"),
    }
}

fn patch_dimension(xml: String, used_end: CellAddr) -> String {
    if let Some(tag) = find_open_tag(&xml, 0, "dimension") {
        let raw = &xml[tag.start..tag.end];
        let current = attribute(raw, "ref")
            .and_then(|value| {
                let (start, end) = value.split_once(':')?;
                Some((CellAddr::from_a1(start)?, CellAddr::from_a1(end)?))
            })
            .unwrap_or((CellAddr::new(0, 0), CellAddr::new(0, 0)));
        let end = CellAddr::new(
            used_end.row.max(current.1.row),
            used_end.col.max(current.1.col),
        );
        let start = current.0;
        let replacement = format!("<dimension ref=\"{}:{}\"/>", start.a1(), end.a1());
        let mut result = String::new();
        result.push_str(&xml[..tag.start]);
        result.push_str(&replacement);
        result.push_str(&xml[tag.end..]);
        return result;
    }
    if let Some(sheet_data) = find_open_tag(&xml, 0, "sheetData") {
        let end = used_end;
        let insertion = format!("<dimension ref=\"A1:{}\"/>", end.a1());
        let mut result = String::new();
        result.push_str(&xml[..sheet_data.start]);
        result.push_str(&insertion);
        result.push_str(&xml[sheet_data.start..]);
        return result;
    }
    xml
}

fn patch_calc_pr(xml: &str) -> String {
    let replacement = "<calcPr calcId=\"191029\" fullCalcOnLoad=\"1\" forceFullCalc=\"1\"/>";
    if let Some(tag) = find_open_tag(xml, 0, "calcPr") {
        let end = if tag.self_closing {
            tag.end
        } else {
            find_close_tag(xml, tag.end, "calcPr")
                .map(|close| close.end)
                .unwrap_or(tag.end)
        };
        let mut result = String::new();
        result.push_str(&xml[..tag.start]);
        result.push_str(replacement);
        result.push_str(&xml[end..]);
        return result;
    }
    if let Some(end) = xml.find("</workbook>") {
        let mut result = String::new();
        result.push_str(&xml[..end]);
        result.push_str(replacement);
        result.push_str(&xml[end..]);
        return result;
    }
    xml.to_string()
}

fn remove_tags_containing(xml: &str, needle: &str) -> String {
    let mut result = String::new();
    let mut position = 0usize;
    while let Some(relative) = xml[position..].find('<') {
        let start = position + relative;
        let Some(end) = find_tag_end(xml, start) else {
            break;
        };
        let tag = &xml[start..end];
        if tag.contains(needle) {
            position = end;
            continue;
        }
        result.push_str(&xml[position..end]);
        position = end;
    }
    result.push_str(&xml[position..]);
    result
}

#[derive(Clone, Copy, Debug)]
struct TagSpan {
    start: usize,
    end: usize,
    self_closing: bool,
}

fn find_open_tag(xml: &str, from: usize, name: &str) -> Option<TagSpan> {
    let mut position = from;
    while position < xml.len() {
        let relative = xml[position..].find('<')?;
        let start = position + relative;
        let rest = &xml[start..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->")?;
            position = start + end + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>")?;
            position = start + end + 3;
            continue;
        }
        if rest.starts_with("<?") {
            let end = rest.find("?>")?;
            position = start + end + 2;
            continue;
        }
        if rest.starts_with("</") || rest.starts_with("<!") {
            let end = find_tag_end(xml, start)?;
            position = end;
            continue;
        }
        let name_start = start + 1;
        let mut index = name_start;
        while index < xml.len() {
            let ch = xml.as_bytes()[index];
            if ch.is_ascii_whitespace() || ch == b'/' || ch == b'>' {
                break;
            }
            index += 1;
        }
        let tag_name = &xml[name_start..index];
        let end = find_tag_end(xml, start)?;
        if tag_name == name {
            let self_closing = xml[start..end]
                .trim_end_matches('>')
                .trim_end()
                .ends_with('/');
            return Some(TagSpan {
                start,
                end,
                self_closing,
            });
        }
        position = end;
    }
    None
}

fn find_close_tag(xml: &str, from: usize, name: &str) -> Option<TagSpan> {
    let needle = format!("</{name}");
    let mut position = from;
    while let Some(relative) = xml[position..].find(&needle) {
        let start = position + relative;
        let after = start + needle.len();
        if after >= xml.len() {
            return None;
        }
        let next = xml.as_bytes()[after];
        if !(next.is_ascii_whitespace() || next == b'>') {
            position = after;
            continue;
        }
        let end = find_tag_end(xml, start)?;
        return Some(TagSpan {
            start,
            end,
            self_closing: false,
        });
    }
    None
}

fn find_tag_end(xml: &str, start: usize) -> Option<usize> {
    let bytes = xml.as_bytes();
    let mut quote = None;
    let mut index = start + 1;
    while index < bytes.len() {
        let ch = bytes[index];
        match quote {
            Some(expected) if ch == expected => quote = None,
            Some(_) => {}
            None if ch == b'"' || ch == b'\'' => quote = Some(ch),
            None if ch == b'>' => return Some(index + 1),
            None => {}
        }
        index += 1;
    }
    None
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut index = 1usize;
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && bytes[index] != b'>'
        && bytes[index] != b'/'
    {
        index += 1;
    }
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'>' || bytes[index] == b'/' {
            break;
        }
        let key_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        let key = &tag[key_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || (bytes[index] != b'"' && bytes[index] != b'\'') {
            break;
        }
        let quote = bytes[index];
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        let value = &tag[value_start..index];
        if index < bytes.len() {
            index += 1;
        }
        if key == name {
            return Some(unescape_xml(value));
        }
    }
    None
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tags_and_attributes() {
        let xml = r#"<c r="B2" s="4" t="s"><v>0</v></c>"#;
        let tag = find_open_tag(xml, 0, "c").unwrap();
        assert_eq!(
            attribute(&xml[tag.start..tag.end], "r").as_deref(),
            Some("B2")
        );
        assert_eq!(
            attribute(&xml[tag.start..tag.end], "s").as_deref(),
            Some("4")
        );

        let close = find_close_tag(xml, tag.end, "c").unwrap();
        assert_eq!(close.end, xml.len());
    }

    #[test]
    fn renders_text_with_inline_string() {
        let cell =
            Cell::new_with_format(CellValue::Text("a&b".to_string()), Default::default(), None);
        let xml = render_cell(CellAddr::new(1, 1), &cell, None);
        assert!(xml.contains("r=\"B2\""));
        assert!(xml.contains("t=\"inlineStr\""));
        assert!(xml.contains("a&amp;b"));
    }

    #[test]
    fn updates_calc_pr() {
        let xml = r#"<workbook><sheets/><calcPr calcId="1"/></workbook>"#;
        let patched = patch_calc_pr(xml);
        assert!(patched.contains("fullCalcOnLoad=\"1\""));
        assert!(!patched.contains("calcId=\"1\""));
    }

    #[test]
    fn removes_calc_chain_tags() {
        let xml = r#"<Types><Override PartName="/xl/calcChain.xml" ContentType="x"/></Types>"#;
        let patched = remove_tags_containing(xml, "calcChain");
        assert!(!patched.contains("calcChain"));
    }

    #[test]
    fn patches_a_cell_in_sheet_data() {
        let xml = r#"<worksheet><dimension ref="A1:B2"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;
        let mut edits = HashMap::new();
        edits.insert(
            CellAddr::new(0, 0),
            Cell::new_with_format(CellValue::Number(42.0), Default::default(), None),
        );
        let patched =
            patch_sheet_xml(xml, &edits, &HashMap::new(), "Sheet1", CellAddr::new(0, 1)).unwrap();
        assert!(patched.contains("<v>42</v>"));
        assert!(patched.contains("dimension"));
    }

    #[test]
    fn inserts_new_cells_in_column_order() {
        let xml = r#"<worksheet><sheetData><row r="1"><c r="B1"><v>1</v></c></row></sheetData></worksheet>"#;
        let mut edits = HashMap::new();
        edits.insert(
            CellAddr::new(0, 0),
            Cell::new_with_format(
                CellValue::Text("left".to_string()),
                Default::default(),
                None,
            ),
        );
        let patched =
            patch_sheet_xml(xml, &edits, &HashMap::new(), "Sheet1", CellAddr::new(0, 1)).unwrap();
        let a = patched.find("r=\"A1\"").unwrap();
        let b = patched.find("r=\"B1\"").unwrap();
        assert!(a < b);
        assert!(patched.contains("left"));
    }

    #[test]
    fn patches_formula_cache_values() {
        let raw = r#"<c r="B1"><f>A1*3</f><v>0</v></c>"#;
        let patched = patch_cached_cell(raw, &CellValue::Number(12.0)).unwrap();
        assert!(patched.contains("<v>12</v>"));
        assert!(!patched.contains("t=\"str\""));

        let raw = r#"<c r="B1" t="str"><f>\"x\"</f><v>old</v></c>"#;
        let patched = patch_cached_cell(raw, &CellValue::Text("new".to_string())).unwrap();
        assert!(patched.contains("t=\"str\""));
        assert!(patched.contains("<v>new</v>"));
    }

    #[test]
    fn inserts_a_cache_value_when_missing() {
        let raw = r#"<c r="B1"><f>A1*3</f></c>"#;
        let patched = patch_cached_cell(raw, &CellValue::Number(12.0)).unwrap();
        assert!(patched.contains("</f><v>12</v></c>"));
    }

    #[test]
    fn patches_self_closing_rows_and_sheet_data() {
        let xml = r#"<worksheet><sheetData/></worksheet>"#;
        let mut edits = HashMap::new();
        edits.insert(
            CellAddr::new(2, 0),
            Cell::new_with_format(CellValue::Number(3.0), Default::default(), None),
        );
        let patched =
            patch_sheet_xml(xml, &edits, &HashMap::new(), "Sheet1", CellAddr::new(2, 0)).unwrap();
        assert!(patched.contains("<sheetData>"));
        assert!(patched.contains("<row r=\"3\">"));
        assert!(patched.contains("</row>"));
        assert!(patched.contains("</sheetData>"));
    }

    #[test]
    fn refuses_shared_formula_cells() {
        assert!(
            ensure_editable_cell(
                "S",
                CellAddr::new(0, 0),
                r#"<c r="A1"><f t="shared" si="0"/></c>"#
            )
            .is_err()
        );
    }

    #[test]
    fn normalizes_relationship_targets() {
        assert_eq!(
            normalize_target("worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            normalize_target("/xl/worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
    }

    #[test]
    fn parses_dimension_end() {
        let xml = r#"<worksheet><dimension ref="A1:B2"/><sheetData/></worksheet>"#;
        let patched = patch_dimension(xml.to_string(), CellAddr::new(4, 4));
        assert!(patched.contains("A1:E5"));
    }
}
