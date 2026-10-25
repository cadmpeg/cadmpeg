// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet admission uses the fields returned to each actual producer.

use super::{offset_cell_address, CellAddress, MAX_SKETCH_RECORDS};
use crate::native::{PropertyRecord, ValueRecord};
use crate::property_reader::PropertyReader;
use cadmpeg_core::CodecError;
use cadmpeg_ir::spreadsheets::{SpreadsheetDimension, SpreadsheetRange};
use std::borrow::Cow;
use std::collections::BTreeMap;

pub(super) struct Cell {
    pub(super) order: usize,
    pub(super) address: String,
    pub(super) parsed_address: CellAddress,
}

pub(super) struct SpreadsheetFacts<'a> {
    pub(super) property: &'a PropertyRecord,
    pub(super) cells: Vec<Cell>,
    pub(super) column_widths: Vec<SpreadsheetDimension>,
    pub(super) row_heights: Vec<SpreadsheetDimension>,
    pub(super) merged_ranges: Vec<SpreadsheetRange>,
}

pub(super) fn facts<'a>(
    reader: &PropertyReader<'_, '_>,
    properties: &[&'a PropertyRecord],
) -> Result<SpreadsheetFacts<'a>, CodecError> {
    let property = crate::native::unique_property(properties.iter().copied(), |property| {
        property.name == "cells" && property.type_name == "Spreadsheet::PropertySheet"
    })
    .map_err(|_| CodecError::malformed("spreadsheet has multiple cells properties"))?;
    let Some(property) = property else {
        reader.unavailable_named("cells")?;
        return Err(CodecError::malformed("spreadsheet has no cells property"));
    };
    let root = root(reader, property, "Cells")?;
    let attributes = fields(reader, property, root, &["Count"])?;
    let count = count(attributes.get("Count"), "spreadsheet Cells Count")?;
    let found = property
        .values()
        .iter()
        .filter(|value| value.parent == Some(root.order) && value.tag == "Cell")
        .count();
    if count != found {
        reader.unavailable(property)?;
        return Err(CodecError::malformed("spreadsheet cell count differs"));
    }
    retain_index::<Cell>(reader, count, "spreadsheet admission cells")?;
    retain_index::<SpreadsheetRange>(reader, count, "spreadsheet admission ranges")?;
    let mut cells = Vec::with_capacity(count);
    let mut merged_ranges: Vec<SpreadsheetRange> = Vec::with_capacity(count);
    for value in property
        .values()
        .iter()
        .filter(|value| value.parent == Some(root.order) && value.tag == "Cell")
    {
        let attributes = fields(reader, property, value, &["address", "rowSpan", "colSpan"])?;
        let address = attributes
            .get("address")
            .ok_or_else(|| CodecError::malformed("spreadsheet cell has no address"))?;
        let parsed_address = CellAddress::parse(address)
            .ok_or_else(|| CodecError::malformed("spreadsheet cell has invalid address"))?;
        if cells
            .iter()
            .any(|cell: &Cell| cell.parsed_address == parsed_address)
        {
            return Err(CodecError::malformed(
                "spreadsheet cell address is repeated",
            ));
        }
        if let Some(range) = merged_range(
            Some(address),
            attributes.get("rowSpan").map(String::as_str),
            attributes.get("colSpan").map(String::as_str),
        )? {
            if !merged_ranges
                .iter()
                .any(|existing| existing.contains(range.start()))
            {
                if merged_ranges
                    .iter()
                    .any(|existing| overlaps(existing, &range))
                {
                    return Err(CodecError::malformed("spreadsheet merged ranges overlap"));
                }
                merged_ranges.push(range);
            }
        }
        reader.retain_bytes(
            cadmpeg_core::decode::u64_from_index(address.len()),
            "spreadsheet admission address",
        )?;
        cells.push(Cell {
            order: value.order,
            address: address.clone(),
            parsed_address,
        });
    }
    Ok(SpreadsheetFacts {
        property,
        cells,
        column_widths: dimensions(
            reader,
            properties,
            "Spreadsheet::PropertyColumnWidths",
            "columnWidths",
            "ColumnInfo",
            "Column",
            "width",
        )?,
        row_heights: dimensions(
            reader,
            properties,
            "Spreadsheet::PropertyRowHeights",
            "rowHeights",
            "RowInfo",
            "Row",
            "height",
        )?,
        merged_ranges,
    })
}

fn fields<'a>(
    reader: &PropertyReader<'_, '_>,
    property: &PropertyRecord,
    value: &'a ValueRecord,
    names: &[&str],
) -> Result<Cow<'a, BTreeMap<String, String>>, CodecError> {
    reader
        .feature_fields(property, value.order, names)
        .map(|fields| {
            fields.map_or(Cow::Borrowed(&value.attributes), |fields| {
                Cow::Owned(fields.attributes)
            })
        })
}

fn root<'a>(
    reader: &PropertyReader<'_, '_>,
    property: &'a PropertyRecord,
    tag: &str,
) -> Result<&'a ValueRecord, CodecError> {
    if property.body.is_none() {
        reader.unavailable(property)?;
        return Err(CodecError::malformed("spreadsheet value is not held"));
    }
    let mut roots = property
        .values()
        .iter()
        .filter(|value| value.parent.is_none() && value.tag == tag);
    let Some(first) = roots.next() else {
        reader.unavailable(property)?;
        return Err(CodecError::malformed("spreadsheet root is missing"));
    };
    if roots.next().is_some() {
        return Err(CodecError::malformed("spreadsheet root is ambiguous"));
    }
    Ok(first)
}

fn count(value: Option<&String>, role: &str) -> Result<usize, CodecError> {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count <= MAX_SKETCH_RECORDS)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("{role} is invalid or exceeds its limit"))
        })
}

fn dimensions(
    reader: &PropertyReader<'_, '_>,
    properties: &[&PropertyRecord],
    kind: &str,
    name: &str,
    container: &str,
    tag: &str,
    size: &str,
) -> Result<Vec<SpreadsheetDimension>, CodecError> {
    let property = crate::native::unique_property(properties.iter().copied(), |property| {
        property.name == name && property.type_name == kind
    })
    .map_err(|_| CodecError::malformed("spreadsheet dimensions are ambiguous"))?;
    let Some(property) = property else {
        if reader.unavailable_named(name)? {
            return Err(CodecError::malformed("spreadsheet dimension is not held"));
        }
        return Ok(Vec::new());
    };
    let root = root(reader, property, container)?;
    let attributes = fields(reader, property, root, &["Count"])?;
    let count = count(attributes.get("Count"), "spreadsheet dimension Count")?;
    if count
        != property
            .values()
            .iter()
            .filter(|value| value.parent == Some(root.order) && value.tag == tag)
            .count()
    {
        reader.unavailable(property)?;
        return Err(CodecError::malformed("spreadsheet dimension count differs"));
    }
    retain_index::<SpreadsheetDimension>(reader, count, "spreadsheet admission dimensions")?;
    let mut dimensions: Vec<SpreadsheetDimension> = Vec::with_capacity(count);
    for value in property
        .values()
        .iter()
        .filter(|value| value.parent == Some(root.order) && value.tag == tag)
    {
        let attributes = fields(reader, property, value, &["name", size])?;
        let name = attributes
            .get("name")
            .ok_or_else(|| CodecError::malformed("spreadsheet dimension has no name"))?;
        let pixels = attributes
            .get(size)
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| CodecError::malformed("spreadsheet dimension has invalid size"))?;
        let index = if tag == "Column" {
            CellAddress::parse(&format!("{name}1")).map(CellAddress::col)
        } else {
            name.parse::<u32>().ok().filter(|index| *index > 0)
        }
        .and_then(std::num::NonZeroU32::new)
        .ok_or_else(|| CodecError::malformed("spreadsheet dimension has invalid index"))?;
        if dimensions.iter().any(|dimension| dimension.index == index) {
            return Err(CodecError::malformed(
                "spreadsheet dimension repeats an index",
            ));
        }
        dimensions.push(SpreadsheetDimension { index, pixels });
    }
    Ok(dimensions)
}

pub(super) fn merged_range(
    address: Option<&str>,
    rows: Option<&str>,
    columns: Option<&str>,
) -> Result<Option<SpreadsheetRange>, CodecError> {
    let rows = rows
        .map_or(Ok(1_i32), str::parse::<i32>)
        .map_err(|_| CodecError::malformed("spreadsheet cell has invalid row span"))?;
    let columns = columns
        .map_or(Ok(1_i32), str::parse::<i32>)
        .map_err(|_| CodecError::malformed("spreadsheet cell has invalid column span"))?;
    if rows < 1 || columns < 1 || rows == 1 && columns == 1 {
        return Ok(None);
    }
    let address =
        address.ok_or_else(|| CodecError::malformed("spreadsheet cell has no address"))?;
    let end = offset_cell_address(
        address,
        u32::try_from(rows - 1).map_err(CodecError::malformed)?,
        u32::try_from(columns - 1).map_err(CodecError::malformed)?,
    )
    .ok_or_else(|| CodecError::malformed("spreadsheet cell span is out of range"))?;
    let start = CellAddress::parse(address)
        .ok_or_else(|| CodecError::malformed("spreadsheet cell has invalid address"))?;
    let end = CellAddress::parse(&end)
        .ok_or_else(|| CodecError::malformed("spreadsheet cell span is out of range"))?;
    SpreadsheetRange::new(start, end)
        .map(Some)
        .ok_or_else(|| CodecError::malformed("spreadsheet cell span is out of range"))
}

fn overlaps(left: &SpreadsheetRange, right: &SpreadsheetRange) -> bool {
    left.start().row() <= right.end().row()
        && right.start().row() <= left.end().row()
        && left.start().col() <= right.end().col()
        && right.start().col() <= left.end().col()
}

fn retain_index<T>(
    reader: &PropertyReader<'_, '_>,
    count: usize,
    role: &'static str,
) -> Result<(), CodecError> {
    let bytes = cadmpeg_core::decode::u64_from_index(count)
        .checked_mul(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<T>(),
        ))
        .ok_or_else(|| CodecError::malformed("spreadsheet admission storage overflows"))?;
    reader.retain_bytes(bytes, role)
}
