use std::collections::HashMap;
use std::io::BufReader;
use std::path::Path;

use calamine::{Data, DataRef, Reader, Xlsx};

use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::{Cell, CellValue};
use crate::domain::error::LoadError;
use crate::domain::filter::{
    FilterEngine, FilterMaskBuilder, FilterMasks, FilterState, FilterValue, VisibilityMap,
    filter_column_indices,
};
use crate::domain::formula::{FormulaInfo, FormulaOverlay, FormulaStatus};
use crate::domain::freeze::FreezePane;
use crate::domain::layout::{LayoutLimits, LayoutMode, LayoutStatsBuilder};
use crate::domain::merge::{MergeIndex, MergeRegion};
use crate::domain::row_store::LazyConfig;
use crate::domain::row_store::{LazySheet, MemoryRowSource, Row, RowSource};
use crate::domain::sheet::{DataRowsBuilder, Sheet, SheetData, SheetMeta};
use crate::domain::style::{BorderLine, BorderStyle, CellBorders, CellStyle, RgbColor};
use crate::domain::workbook::{FileFingerprint, SourceFormat, SourceInfo, WorkbookModel};

use super::row_cache::SpillRowCache;
use super::sheet_meta::SheetMetadataParser;

/// Upper bound for scanning the style range to find borders and fills on
/// empty cells. The style range is usually close to the used range, but a
/// pathological file can style two distant cells and force a huge sparse scan.
const MAX_EMPTY_STYLED_CELLS: u64 = 4_000_000;

pub fn load_xlsx(
    path: &Path,
    layout_limits: &LayoutLimits,
    lazy_config: &LazyConfig,
) -> Result<WorkbookModel, LoadError> {
    load_xlsx_with_mode(path, layout_limits, lazy_config, LayoutMode::Excel)
}

pub fn load_xlsx_with_mode(
    path: &Path,
    layout_limits: &LayoutLimits,
    lazy_config: &LazyConfig,
    layout_mode: LayoutMode,
) -> Result<WorkbookModel, LoadError> {
    let file = std::fs::File::open(path).map_err(|error| LoadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let file_size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let force_lazy = lazy_config.enabled
        && (lazy_config.threshold_bytes == 0
            || file_size > lazy_config.threshold_bytes
            || lazy_config.threshold_cells == 0);
    if force_lazy {
        return load_xlsx_lazy(path, layout_limits, lazy_config, layout_mode);
    }
    let mut workbook = Xlsx::new(BufReader::new(file)).map_err(|error| LoadError::Xlsx {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let sheet_names = workbook.sheet_names();
    let metadata = SheetMetadataParser::parse(path).unwrap_or_default();
    let mut sheets = Vec::new();
    let mut total_cells = 0u64;
    for sheet_name in sheet_names {
        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(_) => continue,
        };
        let formulas = workbook.worksheet_formula(&sheet_name).ok();
        let styles = workbook.worksheet_style(&sheet_name).ok();
        let merge_dimensions: Vec<(u32, u32, u32, u32)> = workbook
            .merge_cells_by_sheet_name(&sheet_name)
            .unwrap_or_default()
            .into_iter()
            .map(|dimensions| {
                (
                    dimensions.start.0,
                    dimensions.start.1,
                    dimensions.end.0,
                    dimensions.end.1,
                )
            })
            .collect();
        let merges = MergeIndex::new(
            merge_dimensions
                .into_iter()
                .map(|(r1, c1, r2, c2)| MergeRegion {
                    anchor: CellAddr::new(r1, c1),
                    range: CellRange::new(CellAddr::new(r1, c1), CellAddr::new(r2, c2)),
                })
                .collect(),
        );
        let mut cells = HashMap::new();
        let mut data_rows = DataRowsBuilder::default();
        let mut stats = LayoutStatsBuilder::new(layout_limits);
        let start = range.start().unwrap_or((0, 0));
        let mut max_row = 0u32;
        let mut max_col = 0u32;
        for (rel_row, rel_col, data) in range.cells() {
            if matches!(data, Data::Empty) {
                continue;
            }
            let row = start.0 + rel_row as u32;
            let col = start.1 + rel_col as u32;
            let value = convert_data(data);
            let source_style = styles.as_ref().and_then(|styles| styles.get((row, col)));
            let style = source_style.map(convert_style).unwrap_or_default();
            let number_format = source_style.and_then(convert_number_format);
            let mut cell = Cell::new_with_format(value.clone(), style, number_format);
            if let Some(formulas) = &formulas
                && let Some(formula) = formulas.get_value((row, col))
                && !formula.trim().is_empty()
            {
                let expression = if formula.starts_with('=') {
                    formula.to_string()
                } else {
                    format!("={formula}")
                };
                cell.formula = Some(FormulaInfo::new(expression, Some(value.clone())));
            }
            let addr = CellAddr::new(row, col);
            if merges.region_at(addr).is_none() {
                stats.observe(row, col, &cell.display, cell.wrap_text);
            }
            data_rows.observe_row(row, cell.has_content());
            cells.insert(addr, cell);
            max_row = max_row.max(row);
            max_col = max_col.max(col);
            total_cells += 1;
        }
        if let Some(styles) = &styles {
            let style_cells = (styles.width() as u64).saturating_mul(styles.height() as u64);
            if style_cells <= MAX_EMPTY_STYLED_CELLS {
                for (row, col, style) in styles.cells() {
                    let addr = CellAddr::new(row, col);
                    if cells.contains_key(&addr) || merges.is_covered(addr) {
                        continue;
                    }
                    let cell_style = convert_style(style);
                    if !cell_style.borders.any_visible() && cell_style.fill.is_none() {
                        continue;
                    }
                    max_row = max_row.max(row);
                    max_col = max_col.max(col);
                    total_cells += 1;
                    cells.insert(addr, Cell::new(CellValue::Empty, cell_style));
                }
            }
        }
        let merge_borders = collect_merge_borders(
            &merges,
            cells.iter().map(|(addr, cell)| (*addr, cell.style.borders)),
        );
        // Merged ranges can extend beyond the last cell with a value. Include
        // the full range so covered rows/columns participate in layout,
        // scrolling, and rendering.
        for region in merges.regions() {
            max_row = max_row.max(region.range.end.row);
            max_col = max_col.max(region.range.end.col);
        }
        for region in merges.regions() {
            if let Some(cell) = cells.get(&region.anchor) {
                stats.observe_merge(region.range, &cell.display, cell.wrap_text);
            }
        }
        let used_range = CellRange::new(CellAddr::new(0, 0), CellAddr::new(max_row, max_col));
        let excel_layout = (layout_mode == LayoutMode::Excel)
            .then(|| metadata.get(&sheet_name).map(|meta| &meta.layout))
            .flatten()
            .filter(|layout| !layout.is_empty());
        let layout = stats.finish(&merges, excel_layout, used_range);
        let mut warnings = Vec::new();
        let mut freeze = metadata
            .get(&sheet_name)
            .map(|meta| meta.freeze)
            .unwrap_or_else(FreezePane::none);
        if let Err(warning) = freeze.validate(used_range, &merges, &sheet_name) {
            warnings.push(warning.to_string());
            freeze.disabled = true;
        }
        let mut filters = metadata
            .get(&sheet_name)
            .map(|meta| meta.filters.clone())
            .unwrap_or_default();
        filters.retain(|filter| {
            if let Err(warning) = filter.validate(used_range, &merges) {
                warnings.push(warning.to_string());
                false
            } else {
                true
            }
        });
        if let Some(meta) = metadata.get(&sheet_name) {
            warnings.extend(meta.warnings.iter().cloned());
        }
        let filter_state = FilterState::from_definitions(&filters, false);
        let filter_masks = if filters.is_empty() {
            FilterMasks::default()
        } else {
            let columns = filter_column_indices(&filters);
            let mut builder = FilterMaskBuilder::new(&filters, max_row + 1);
            for row in 0..=max_row {
                let values = columns
                    .iter()
                    .map(|col| {
                        let addr = CellAddr::new(row, *col);
                        cells
                            .get(&addr)
                            .map(|cell| {
                                FilterValue::from_effective(
                                    &cell.effective(FormulaStatus::CachedOnly),
                                )
                            })
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>();
                builder.observe_row(row, &values);
            }
            builder.finish()
        };
        let mut visibility = if filters.is_empty() {
            VisibilityMap::all(max_row + 1)
        } else {
            FilterEngine::combine(&filters, &filter_masks, &filter_state, max_row + 1)
        };
        visibility.force_visible(0, freeze.active_rows().min(max_row + 1));
        let lazy = lazy_config.enabled
            && (file_size > lazy_config.threshold_bytes
                || total_cells > lazy_config.threshold_cells);
        let formula_cells: Vec<CellAddr> = cells
            .iter()
            .filter_map(|(addr, cell)| cell.formula.is_some().then_some(*addr))
            .collect();
        let data = if lazy {
            let rows: Vec<Row> = cells
                .into_iter()
                .fold(
                    HashMap::<u32, Vec<(u32, Cell)>>::new(),
                    |mut rows, (addr, cell)| {
                        rows.entry(addr.row).or_default().push((addr.col, cell));
                        rows
                    },
                )
                .into_iter()
                .map(|(row, mut cells)| {
                    cells.sort_by_key(|(col, _)| *col);
                    Row { row, cells }
                })
                .collect();
            let row_count = max_row + 1;
            let mut rows_by_number = vec![Row::default(); row_count as usize];
            for row in rows {
                if let Some(slot) = rows_by_number.get_mut(row.row as usize) {
                    *slot = row;
                }
            }
            let source = MemoryRowSource::new(rows_by_number);
            SheetData::Lazy(LazySheet::new(
                Box::new(source) as Box<dyn RowSource>,
                lazy_config.row_cache_rows,
                lazy_config.prefetch_rows,
            ))
        } else {
            SheetData::Eager(cells)
        };
        let conditional_formats = metadata
            .get(&sheet_name)
            .map(|meta| meta.conditional_formats.clone())
            .unwrap_or_default();
        sheets.push(Sheet {
            name: sheet_name,
            meta: SheetMeta {
                row_count: max_row + 1,
                column_count: max_col + 1,
            },
            used_range,
            data_rows: data_rows.finish(),
            data,
            edits: HashMap::new(),
            merges,
            merge_borders,
            layout,
            layout_limits: *layout_limits,
            freeze,
            conditional_formats,
            filters,
            filter_masks,
            filter_masks_dirty: false,
            filter_state,
            visibility,
            formula_overlay: FormulaOverlay::default(),
            formula_cells,
            warnings,
        });
    }
    WorkbookModel::new(
        SourceInfo {
            path: path.to_path_buf(),
            format: SourceFormat::Xlsx,
            fingerprint: fingerprint(path),
            encoding: None,
            lazy: sheets
                .iter()
                .any(|sheet| matches!(sheet.data, SheetData::Lazy(_))),
        },
        sheets,
    )
}

fn load_xlsx_lazy(
    path: &Path,
    layout_limits: &LayoutLimits,
    lazy_config: &LazyConfig,
    layout_mode: LayoutMode,
) -> Result<WorkbookModel, LoadError> {
    let file = std::fs::File::open(path).map_err(|error| LoadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut workbook = Xlsx::new(BufReader::new(file)).map_err(|error| LoadError::Xlsx {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let sheet_names = workbook.sheet_names();
    let metadata = SheetMetadataParser::parse(path).unwrap_or_default();
    let mut sheets = Vec::new();
    for sheet_name in sheet_names {
        let formulas = workbook.worksheet_formula(&sheet_name).ok();
        let merges = MergeIndex::new(
            workbook
                .merge_cells_by_sheet_name(&sheet_name)
                .unwrap_or_default()
                .into_iter()
                .map(|dimensions| MergeRegion {
                    anchor: CellAddr::new(dimensions.start.0, dimensions.start.1),
                    range: CellRange::new(
                        CellAddr::new(dimensions.start.0, dimensions.start.1),
                        CellAddr::new(dimensions.end.0, dimensions.end.1),
                    ),
                })
                .collect(),
        );
        let mut reader = match workbook.worksheet_cells_reader(&sheet_name) {
            Ok(reader) => reader,
            Err(_) => continue,
        };
        let styles = reader.styles().to_vec();
        let dimensions = reader.dimensions();
        let mut cache = SpillRowCache::new()?;
        let mut stats = LayoutStatsBuilder::new(layout_limits);
        let mut current_row: Option<u32> = None;
        let mut row_cells: Vec<(u32, Cell)> = Vec::new();
        let mut next_row_to_write = 0u32;
        let merge_max_row = merges
            .regions()
            .iter()
            .map(|region| region.range.end.row)
            .max()
            .unwrap_or(0);
        let merge_max_col = merges
            .regions()
            .iter()
            .map(|region| region.range.end.col)
            .max()
            .unwrap_or(0);
        let mut max_row = dimensions.end.0.max(merge_max_row);
        let mut max_col = dimensions.end.1.max(merge_max_col);
        let mut formula_cells = Vec::new();
        let mut data_rows = DataRowsBuilder::default();
        let mut merge_borders: HashMap<CellAddr, CellBorders> = HashMap::new();
        while let Some((cell, style_id)) =
            reader
                .next_cell_with_style_id()
                .map_err(|error| LoadError::Xlsx {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                })?
        {
            let (row, col) = cell.get_position();
            max_row = max_row.max(row);
            max_col = max_col.max(col);
            let value = convert_data_ref(cell.get_value());
            let source_style = styles.get(style_id);
            let mut domain_cell = Cell::new_with_format(
                value.clone(),
                source_style.map(convert_style).unwrap_or_default(),
                source_style.and_then(convert_number_format),
            );
            if domain_cell.style.borders.any_visible()
                && let Some(region) = merges.region_at(CellAddr::new(row, col))
            {
                accumulate_merge_borders(
                    &mut merge_borders,
                    region,
                    CellAddr::new(row, col),
                    domain_cell.style.borders,
                );
            }
            if let Some(formulas) = &formulas
                && let Some(formula) = formulas.get_value((row, col))
                && !formula.trim().is_empty()
            {
                let expression = if formula.starts_with('=') {
                    formula.to_string()
                } else {
                    format!("={formula}")
                };
                domain_cell.formula = Some(FormulaInfo::new(expression, Some(value)));
                formula_cells.push(CellAddr::new(row, col));
            }
            data_rows.observe_row(row, domain_cell.has_content());
            if !domain_cell.raw.is_empty()
                || domain_cell.formula.is_some()
                || domain_cell.style.borders.any_visible()
                || domain_cell.style.fill.is_some()
            {
                let addr = CellAddr::new(row, col);
                if merges.region_at(addr).is_none() {
                    stats.observe(row, col, &domain_cell.display, domain_cell.wrap_text);
                }
                if let Some(region) = merges.region_at(addr)
                    && region.anchor == addr
                {
                    stats.observe_merge(region.range, &domain_cell.display, domain_cell.wrap_text);
                }
                if current_row != Some(row) {
                    if let Some(previous) = current_row {
                        cache.write_row(&Row {
                            row: previous,
                            cells: std::mem::take(&mut row_cells),
                        })?;
                        next_row_to_write = previous + 1;
                    }
                    while next_row_to_write < row {
                        cache.write_row(&Row {
                            row: next_row_to_write,
                            cells: Vec::new(),
                        })?;
                        next_row_to_write += 1;
                    }
                    current_row = Some(row);
                }
                row_cells.push((col, domain_cell));
            }
        }
        if let Some(previous) = current_row {
            cache.write_row(&Row {
                row: previous,
                cells: std::mem::take(&mut row_cells),
            })?;
            next_row_to_write = previous + 1;
        }
        while next_row_to_write <= dimensions.end.0.max(max_row) {
            cache.write_row(&Row {
                row: next_row_to_write,
                cells: Vec::new(),
            })?;
            next_row_to_write += 1;
        }
        drop(reader);
        let used_range = CellRange::new(CellAddr::new(0, 0), CellAddr::new(max_row, max_col));
        let excel_layout = (layout_mode == LayoutMode::Excel)
            .then(|| metadata.get(&sheet_name).map(|meta| &meta.layout))
            .flatten()
            .filter(|layout| !layout.is_empty());
        let layout = stats.finish(&merges, excel_layout, used_range);
        let mut warnings = Vec::new();
        let mut freeze = metadata
            .get(&sheet_name)
            .map(|meta| meta.freeze)
            .unwrap_or_else(FreezePane::none);
        if let Err(warning) = freeze.validate(used_range, &merges, &sheet_name) {
            warnings.push(warning.to_string());
            freeze.disabled = true;
        }
        let mut filters = metadata
            .get(&sheet_name)
            .map(|meta| meta.filters.clone())
            .unwrap_or_default();
        filters.retain(|filter| {
            if let Err(warning) = filter.validate(used_range, &merges) {
                warnings.push(warning.to_string());
                false
            } else {
                true
            }
        });
        if let Some(meta) = metadata.get(&sheet_name) {
            warnings.extend(meta.warnings.iter().cloned());
        }
        let filter_state = FilterState::from_definitions(&filters, false);
        let mut source = cache.finish()?;
        let filter_masks = if filters.is_empty() {
            FilterMasks::default()
        } else {
            let columns = filter_column_indices(&filters);
            let mut builder = FilterMaskBuilder::new(&filters, max_row + 1);
            let _ = source.scan_all(&mut |row| {
                let values = columns
                    .iter()
                    .map(|col| {
                        row.cell(*col)
                            .map(|cell| {
                                FilterValue::from_effective(
                                    &cell.effective(FormulaStatus::CachedOnly),
                                )
                            })
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>();
                builder.observe_row(row.row, &values);
                std::ops::ControlFlow::Continue(())
            });
            builder.finish()
        };
        let mut visibility = if filters.is_empty() {
            VisibilityMap::all(max_row + 1)
        } else {
            FilterEngine::combine(&filters, &filter_masks, &filter_state, max_row + 1)
        };
        visibility.force_visible(0, freeze.active_rows().min(max_row + 1));
        let lazy = LazySheet::new(
            Box::new(source) as Box<dyn RowSource>,
            lazy_config.row_cache_rows,
            lazy_config.prefetch_rows,
        );
        let conditional_formats = metadata
            .get(&sheet_name)
            .map(|meta| meta.conditional_formats.clone())
            .unwrap_or_default();
        sheets.push(Sheet {
            name: sheet_name,
            meta: SheetMeta {
                row_count: max_row + 1,
                column_count: max_col + 1,
            },
            used_range,
            data_rows: data_rows.finish(),
            data: SheetData::Lazy(lazy),
            edits: HashMap::new(),
            merges,
            merge_borders,
            layout,
            layout_limits: *layout_limits,
            freeze,
            conditional_formats,
            filters,
            filter_masks,
            filter_masks_dirty: false,
            filter_state,
            visibility,
            formula_overlay: FormulaOverlay::default(),
            formula_cells,
            warnings,
        });
    }
    WorkbookModel::new(
        SourceInfo {
            path: path.to_path_buf(),
            format: SourceFormat::Xlsx,
            fingerprint: fingerprint(path),
            encoding: None,
            lazy: true,
        },
        sheets,
    )
}

fn convert_data_ref(data: &DataRef<'_>) -> CellValue {
    match data {
        DataRef::Empty => CellValue::Empty,
        DataRef::String(value) => CellValue::Text(value.clone()),
        DataRef::SharedString(value) => CellValue::Text((*value).to_string()),
        DataRef::Float(value) => CellValue::Number(*value),
        DataRef::Int(value) => CellValue::Number(*value as f64),
        DataRef::Bool(value) => CellValue::Bool(*value),
        DataRef::DateTime(value) => CellValue::DateTime(
            value
                .as_datetime()
                .map(|datetime| datetime.to_string())
                .unwrap_or_else(|| value.to_string()),
        ),
        DataRef::DateTimeIso(value) | DataRef::DurationIso(value) => CellValue::Text(value.clone()),
        DataRef::Error(value) => CellValue::Error(value.to_string()),
    }
}

fn convert_data(data: &Data) -> CellValue {
    match data {
        Data::Empty => CellValue::Empty,
        Data::String(value) => CellValue::Text(value.clone()),
        Data::Float(value) => CellValue::Number(*value),
        Data::Int(value) => CellValue::Number(*value as f64),
        Data::Bool(value) => CellValue::Bool(*value),
        Data::DateTime(value) => CellValue::DateTime(
            value
                .as_datetime()
                .map(|datetime| datetime.to_string())
                .unwrap_or_else(|| value.to_string()),
        ),
        Data::DateTimeIso(value) | Data::DurationIso(value) => CellValue::Text(value.clone()),
        Data::Error(value) => CellValue::Error(value.to_string()),
    }
}

fn convert_style(style: &calamine::Style) -> CellStyle {
    let font = style.font.as_ref();
    let bold = font.is_some_and(|font| font.is_bold());
    let color = font
        .and_then(|font| font.color)
        .map(|color| RgbColor::new(color.red, color.green, color.blue));
    let fill = style.fill.as_ref().and_then(|fill| {
        if matches!(fill.pattern, calamine::FillPattern::Solid) {
            fill.get_color()
                .map(|color| RgbColor::new(color.red, color.green, color.blue))
        } else {
            None
        }
    });
    let wrap_text = style
        .alignment
        .as_ref()
        .is_some_and(|alignment| alignment.wrap_text);
    CellStyle {
        bold,
        color,
        fill,
        wrap_text,
        borders: style
            .borders
            .as_ref()
            .map(convert_borders)
            .unwrap_or_default(),
    }
}

fn convert_number_format(style: &calamine::Style) -> Option<String> {
    style
        .number_format
        .as_ref()
        .map(|format| format.format_code.clone())
        .filter(|code| !code.trim().is_empty())
}

fn convert_borders(borders: &calamine::Borders) -> CellBorders {
    CellBorders {
        left: convert_border(&borders.left),
        right: convert_border(&borders.right),
        top: convert_border(&borders.top),
        bottom: convert_border(&borders.bottom),
    }
}

fn convert_border(border: &calamine::Border) -> BorderLine {
    let style = match border.style {
        calamine::BorderStyle::None => BorderStyle::None,
        calamine::BorderStyle::Hair => BorderStyle::Hair,
        calamine::BorderStyle::Dotted => BorderStyle::Dotted,
        calamine::BorderStyle::Dashed
        | calamine::BorderStyle::MediumDashed
        | calamine::BorderStyle::DashDot
        | calamine::BorderStyle::MediumDashDot
        | calamine::BorderStyle::DashDotDot
        | calamine::BorderStyle::MediumDashDotDot
        | calamine::BorderStyle::SlantDashDot => BorderStyle::Dashed,
        calamine::BorderStyle::Thin => BorderStyle::Thin,
        calamine::BorderStyle::Medium => BorderStyle::Medium,
        calamine::BorderStyle::Thick => BorderStyle::Thick,
        calamine::BorderStyle::Double => BorderStyle::Double,
        _ => BorderStyle::Thin,
    };
    let color = border
        .color
        .map(|color| RgbColor::new(color.red, color.green, color.blue));
    BorderLine::new(style, color)
}

fn accumulate_merge_borders(
    result: &mut HashMap<CellAddr, CellBorders>,
    region: &MergeRegion,
    addr: CellAddr,
    borders: CellBorders,
) {
    let entry = result.entry(region.anchor).or_default();
    if addr == region.anchor {
        // Excel stores the format of a merged range on its anchor cell. The
        // anchor's four sides describe the outer border of the whole range.
        *entry = entry.merge(borders);
        return;
    }
    if addr.col == region.range.start.col {
        entry.left = entry.left.merge(borders.left);
    }
    if addr.col == region.range.end.col {
        entry.right = entry.right.merge(borders.right);
    }
    if addr.row == region.range.start.row {
        entry.top = entry.top.merge(borders.top);
    }
    if addr.row == region.range.end.row {
        entry.bottom = entry.bottom.merge(borders.bottom);
    }
}

fn collect_merge_borders(
    merges: &MergeIndex,
    borders: impl Iterator<Item = (CellAddr, CellBorders)>,
) -> HashMap<CellAddr, CellBorders> {
    let mut result = HashMap::new();
    for (addr, borders) in borders {
        if !borders.any_visible() {
            continue;
        }
        if let Some(region) = merges.region_at(addr) {
            accumulate_merge_borders(&mut result, region, addr, borders);
        }
    }
    result
}

fn fingerprint(path: &Path) -> FileFingerprint {
    match std::fs::metadata(path) {
        Ok(meta) => FileFingerprint {
            len: meta.len(),
            modified_ms: meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or(0),
            present: true,
        },
        Err(_) => FileFingerprint::default(),
    }
}
