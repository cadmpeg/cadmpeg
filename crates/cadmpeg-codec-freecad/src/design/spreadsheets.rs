// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet cell, dimension, and merged-range transfer.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use cadmpeg_ir::spreadsheets::{
    CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetDimension, SpreadsheetId,
    SpreadsheetRange,
};

use super::{design_identity_text, feature_id, malformed_design, MAX_SKETCH_RECORDS};
use crate::native::{malformed, ObjectRecord, PropertyRecord};

fn direct_spreadsheet_value<'a, 'input: 'a>(
    ctx: &DecodeContext<'_>,
    xml: &'a roxmltree::Document<'input>,
    tag: &str,
    property_id: &str,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let wrapper = ctx.xml_root_element(xml, "FreeCAD spreadsheet property root")?;
    let mut found = None;
    let mut nodes = xml.descendants();
    while let Some(node) = ctx.next_charged(&mut nodes, "FreeCAD spreadsheet values")? {
        if !ctx.xml_has_tag_name(node, tag, "FreeCAD spreadsheet value tag")? {
            continue;
        }
        if found.is_some() {
            return Err(malformed_design(
                ctx,
                format_args!("{property_id} has multiple {tag} values"),
            ));
        }
        found = Some(node);
    }
    let Some(node) = found else {
        return Err(malformed_design(
            ctx,
            format_args!("{property_id} has no {tag} value"),
        ));
    };
    if node.parent() != Some(wrapper) {
        return Err(malformed_design(
            ctx,
            format_args!("{property_id} has no direct {tag} value"),
        ));
    }
    Ok(node)
}

pub(super) fn append_spreadsheet(
    ctx: &DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<Spreadsheet, CodecError> {
    let property = match crate::native::sole_property_matching(ctx, properties, |property| {
        property.name == "cells" && property.type_name == "Spreadsheet::PropertySheet"
    })? {
        Ok(Some(property)) => property,
        Ok(None) => {
            return Err(malformed_design(
                ctx,
                format_args!("spreadsheet {} has no cells property", object.id()),
            ));
        }
        Err(_) => return Err(malformed("spreadsheet has multiple cells properties")),
    };
    let admitted_xml = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            malformed_design(
                ctx,
                format_args!("invalid spreadsheet {}: {error}", property.id),
            )
        })?;
    let xml = admitted_xml.document();
    let cells = direct_spreadsheet_value(ctx, xml, "Cells", &property.id)?;
    let count_text = ctx
        .xml_attribute(cells, "Count", "FreeCAD design XML attribute")?
        .ok_or_else(|| {
            malformed_design(ctx, format_args!("{} has invalid Cells Count", property.id))
        })?;
    let declared = ctx
        .parse_text::<usize>(count_text, "fcstd spreadsheet cell count")?
        .map_err(|_| {
            malformed_design(ctx, format_args!("{} has invalid Cells Count", property.id))
        })?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(malformed_design(
            ctx,
            format_args!("{} cell count exceeds {MAX_SKETCH_RECORDS}", property.id),
        ));
    }
    let mut found = 0_usize;
    let mut xml_nodes_3 = cells.children();
    while let Some(cell) = ctx.next_charged(&mut xml_nodes_3, "FreeCAD design XML traversal")? {
        if ctx.xml_has_tag_name(cell, "Cell", "FreeCAD design XML tag")? {
            found = found.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("FreeCAD spreadsheet cell count", u64::MAX, u64::MAX)
            })?;
        }
    }
    if declared != found {
        return Err(malformed_design(
            ctx,
            format_args!(
                "{} declares {declared} cells but contains {}",
                property.id, found
            ),
        ));
    }
    let mut cell_ids = ctx.vector_storage(found, "FreeCAD spreadsheet cells")?;
    let mut merged_ranges: Vec<SpreadsheetRange> = Vec::new();
    let mut index = 0_usize;
    let mut xml_nodes_4 = cells.children();
    while let Some(cell) = ctx.next_charged(&mut xml_nodes_4, "FreeCAD design XML traversal")? {
        if !ctx.xml_has_tag_name(cell, "Cell", "FreeCAD design XML tag")? {
            continue;
        }
        let address = ctx
            .xml_attribute(cell, "address", "FreeCAD design XML attribute")?
            .ok_or_else(|| {
                malformed_design(ctx, format_args!("{} cell has no address", property.id))
            })?;
        let content = ctx
            .xml_attribute(cell, "content", "FreeCAD design XML attribute")?
            .unwrap_or_default();
        let name = ctx
            .xml_attribute(cell, "alias", "FreeCAD design XML attribute")?
            .unwrap_or(address);
        let mut retained = BTreeMap::new();
        ctx.insert_btree_map(
            &mut retained,
            cadmpeg_core::nonblank_literal!("address"),
            ctx.copy_retained_text(address, "fcstd spreadsheet address")?,
            "fcstd spreadsheet cell properties",
        )?;
        for attribute in [
            cadmpeg_core::nonblank_literal!("alias"),
            cadmpeg_core::nonblank_literal!("alignment"),
            cadmpeg_core::nonblank_literal!("style"),
            cadmpeg_core::nonblank_literal!("foregroundColor"),
            cadmpeg_core::nonblank_literal!("backgroundColor"),
            cadmpeg_core::nonblank_literal!("displayUnit"),
            cadmpeg_core::nonblank_literal!("rowSpan"),
            cadmpeg_core::nonblank_literal!("colSpan"),
        ] {
            if let Some(value) =
                ctx.xml_attribute(cell, attribute.as_str(), "FreeCAD design XML attribute")?
            {
                ctx.insert_btree_map(
                    &mut retained,
                    attribute,
                    ctx.copy_retained_text(value, "fcstd spreadsheet cell attribute")?,
                    "fcstd spreadsheet cell properties",
                )?;
            }
        }
        let (row, column) = cell_address(ctx, address)?.ok_or_else(|| {
            malformed_design(
                ctx,
                format_args!("{} cell has invalid address", property.id),
            )
        })?;
        let cell_address = CellAddress::new(row, column).ok_or_else(|| {
            malformed_design(
                ctx,
                format_args!("{} cell has invalid address", property.id),
            )
        })?;
        let id = ParameterId::mint(design_identity_text(
            ctx,
            "parameter",
            object,
            format_args!(":cell:{address}"),
            "fcstd spreadsheet cell identity",
        )?)
        .map_err(CodecError::malformed)?;
        ctx.push_vec(
            &mut cell_ids,
            SpreadsheetCell {
                address: cell_address,
                parameter: id.try_clone_for_decode(ctx, "fcstd spreadsheet cell parameter")?,
            },
            "FreeCAD spreadsheet cells",
        )?;
        if let Some(range) = merged_range(ctx, cell)? {
            if !ctx.any_by(
                &merged_ranges,
                |existing| Ok(existing.contains(range.start())),
                "fcstd spreadsheet merged range lookup",
            )? {
                ctx.push_vec(&mut merged_ranges, range, "fcstd spreadsheet merged ranges")?;
            }
        }
        let value = if content.starts_with('=') {
            None
        } else {
            match ctx.parse_text::<f64>(content, "fcstd spreadsheet cell value")? {
                Ok(value) => cadmpeg_ir::scalar::FiniteReal::new(value).map(ParameterValue::Real),
                Err(_) => None,
            }
        };
        ctx.push_vec(
            parameters,
            DesignParameter {
                id,
                owner: Some(feature_id(ctx, object)?),
                ordinal: u32::try_from(index).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "FreeCAD ordinal",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(index),
                    )
                })?,
                name: ctx.copy_retained_text(name, "fcstd spreadsheet cell name")?,
                expression: ctx.copy_retained_text(content, "fcstd spreadsheet cell expression")?,
                display: None,
                value,
                dependencies: DistinctMembers::default(),
                properties: retained,
                pmi: None,
                native_ref: Some(ctx.copy_retained_text(
                    &property.id,
                    "fcstd spreadsheet parameter native reference",
                )?),
            },
            "fcstd spreadsheet parameters",
        )?;
        index = index
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("FreeCAD ordinal", u64::MAX, u64::MAX))?;
    }
    let column_widths = spreadsheet_dimensions(
        ctx,
        properties,
        "Spreadsheet::PropertyColumnWidths",
        "columnWidths",
        "ColumnInfo",
        "Column",
        "width",
    )?;
    let row_heights = spreadsheet_dimensions(
        ctx,
        properties,
        "Spreadsheet::PropertyRowHeights",
        "rowHeights",
        "RowInfo",
        "Row",
        "height",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(cell_ids.len()),
        "fcstd spreadsheet distinct parameter IDs",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(cell_ids.len()),
        "fcstd spreadsheet distinct addresses",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(column_widths.len()),
        "fcstd spreadsheet distinct column widths",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(row_heights.len()),
        "fcstd spreadsheet distinct row heights",
    )?;
    Spreadsheet::new(
        SpreadsheetId::mint(design_identity_text(
            ctx,
            "spreadsheet",
            object,
            format_args!(""),
            "fcstd spreadsheet identity",
        )?)
        .map_err(CodecError::malformed)?,
        feature_id(ctx, object)?,
        cell_ids,
        column_widths,
        row_heights,
        merged_ranges,
        Some(ctx.copy_retained_text(object.id(), "fcstd spreadsheet native reference")?),
    )
    .map_err(CodecError::malformed)
}

fn spreadsheet_dimensions(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    type_name: &str,
    property_name: &str,
    container: &str,
    element: &str,
    value_name: &str,
) -> Result<Vec<SpreadsheetDimension>, CodecError> {
    let property = match crate::native::sole_property_matching(ctx, properties, |property| {
        property.name == property_name && property.type_name == type_name
    })? {
        Ok(Some(property)) => property,
        Ok(None) => return Ok(Vec::new()),
        Err(_) => {
            return Err(malformed_design(
                ctx,
                format_args!("spreadsheet has multiple {property_name} properties"),
            ));
        }
    };
    let admitted_xml = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            malformed_design(
                ctx,
                format_args!("invalid spreadsheet dimension {}: {error}", property.id),
            )
        })?;
    let xml = admitted_xml.document();
    let root = direct_spreadsheet_value(ctx, xml, container, &property.id)?;
    let mut found = 0_usize;
    let mut xml_nodes_5 = root.children();
    while let Some(record) = ctx.next_charged(&mut xml_nodes_5, "FreeCAD design XML traversal")? {
        if ctx.xml_has_tag_name(record, element, "FreeCAD design XML tag")? {
            found = found.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("FreeCAD spreadsheet dimension count", u64::MAX, u64::MAX)
            })?;
        }
    }
    let count_text = ctx
        .xml_attribute(root, "Count", "FreeCAD design XML attribute")?
        .ok_or_else(|| {
            malformed_design(
                ctx,
                format_args!("{} has invalid dimension count", property.id),
            )
        })?;
    let declared = ctx
        .parse_text::<usize>(count_text, "fcstd spreadsheet dimension count")?
        .map_err(|_| {
            malformed_design(
                ctx,
                format_args!("{} has invalid dimension count", property.id),
            )
        })?;
    if declared != found || declared > MAX_SKETCH_RECORDS {
        return Err(malformed_design(
            ctx,
            format_args!("{} dimension count does not match its records", property.id),
        ));
    }
    let mut dimensions = ctx.vector_storage(found, "fcstd spreadsheet dimensions")?;
    let mut xml_nodes_6 = root.children();
    while let Some(record) = ctx.next_charged(&mut xml_nodes_6, "FreeCAD design XML traversal")? {
        if !ctx.xml_has_tag_name(record, element, "FreeCAD design XML tag")? {
            continue;
        }
        let name = ctx
            .xml_attribute(record, "name", "FreeCAD design XML attribute")?
            .ok_or_else(|| {
                malformed_design(ctx, format_args!("{} dimension has no name", property.id))
            })?;
        let pixels_text = ctx
            .xml_attribute(record, value_name, "FreeCAD design XML attribute")?
            .ok_or_else(|| {
                malformed_design(
                    ctx,
                    format_args!("{} dimension has invalid size", property.id),
                )
            })?;
        let pixels = ctx
            .parse_text::<u32>(pixels_text, "fcstd spreadsheet dimension size")?
            .map_err(|_| {
                malformed_design(
                    ctx,
                    format_args!("{} dimension has invalid size", property.id),
                )
            })?;
        let index = if element == "Column" {
            let (address, _address_storage) =
                ctx.format_scoped(format_args!("{name}1"), "fcstd spreadsheet column address")?;
            let (_, column) = cell_address(ctx, &address)?.ok_or_else(|| {
                malformed_design(
                    ctx,
                    format_args!("{} dimension has invalid column {name}", property.id),
                )
            })?;
            column
        } else {
            ctx.parse_text::<u32>(name, "fcstd spreadsheet row address")?
                .ok()
                .filter(|row| *row > 0)
                .ok_or_else(|| {
                    malformed_design(
                        ctx,
                        format_args!("{} dimension has invalid row {name}", property.id),
                    )
                })?
        };
        let index = std::num::NonZeroU32::new(index).ok_or_else(|| {
            malformed_design(
                ctx,
                format_args!("{} dimension index must be nonzero", property.id),
            )
        })?;
        ctx.push_vec(
            &mut dimensions,
            SpreadsheetDimension { index, pixels },
            "fcstd spreadsheet dimensions",
        )?;
    }
    Ok(dimensions)
}

fn merged_range(
    ctx: &DecodeContext<'_>,
    cell: roxmltree::Node<'_, '_>,
) -> Result<Option<SpreadsheetRange>, CodecError> {
    let rows = match ctx.xml_attribute(cell, "rowSpan", "FreeCAD design XML attribute")? {
        Some(value) => ctx
            .parse_text::<i32>(value, "fcstd spreadsheet row span")?
            .map_err(|_| {
                malformed_design(ctx, format_args!("spreadsheet cell has invalid row span"))
            })?,
        None => 1_i32,
    };
    let columns = match ctx.xml_attribute(cell, "colSpan", "FreeCAD design XML attribute")? {
        Some(value) => ctx
            .parse_text::<i32>(value, "fcstd spreadsheet column span")?
            .map_err(|_| {
                malformed_design(
                    ctx,
                    format_args!("spreadsheet cell has invalid column span"),
                )
            })?,
        None => 1_i32,
    };
    if rows < 1 || columns < 1 {
        return Ok(None);
    }
    if rows == 1 && columns == 1 {
        return Ok(None);
    }
    let start = ctx
        .xml_attribute(cell, "address", "FreeCAD design XML attribute")?
        .ok_or_else(|| malformed_design(ctx, format_args!("spreadsheet cell has no address")))?;
    let (end, _end_storage) =
        ctx.with_scoped_storage("fcstd spreadsheet range endpoint", || {
            offset_cell_address(
                ctx,
                start,
                u32::try_from(rows - 1).map_err(|_| {
                    CodecError::Malformed("spreadsheet cell span is out of range".into())
                })?,
                u32::try_from(columns - 1).map_err(|_| {
                    CodecError::Malformed("spreadsheet cell span is out of range".into())
                })?,
            )
        })?;
    let end = end.ok_or_else(|| {
        malformed_design(ctx, format_args!("spreadsheet cell span is out of range"))
    })?;
    let (start_row, start_column) = cell_address(ctx, start)?.ok_or_else(|| {
        malformed_design(ctx, format_args!("spreadsheet cell has invalid address"))
    })?;
    let start = CellAddress::new(start_row, start_column).ok_or_else(|| {
        malformed_design(ctx, format_args!("spreadsheet cell has invalid address"))
    })?;
    let (end_row, end_column) = cell_address(ctx, &end)?.ok_or_else(|| {
        malformed_design(ctx, format_args!("spreadsheet cell span is out of range"))
    })?;
    let end = CellAddress::new(end_row, end_column).ok_or_else(|| {
        malformed_design(ctx, format_args!("spreadsheet cell span is out of range"))
    })?;
    SpreadsheetRange::new(start, end)
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell span is out of range".into()))
        .map(Some)
}

fn offset_cell_address(
    ctx: &DecodeContext<'_>,
    address: &str,
    rows: u32,
    columns: u32,
) -> Result<Option<String>, CodecError> {
    Ok((|| -> Result<Option<String>, CodecError> {
        let (row, mut column) = required!(cell_address(ctx, address)?);
        let row = required!(row.checked_add(rows));
        column = required!(column.checked_add(columns));
        // Seven base-26 letters hold every nonzero u32 column index.
        let mut label = [0_u8; 7];
        let mut start = label.len();
        while column > 0 {
            start = required!(start.checked_sub(1));
            column -= 1;
            label[start] = b'A' + required!(u8::try_from(column % 26).ok());
            column /= 26;
        }
        let letters = required!(std::str::from_utf8(&label[start..]).ok());
        Ok(Some(ctx.format_retained(
            format_args!("{letters}{row}"),
            "fcstd spreadsheet cell address",
        )?))
    })()?)
}

fn cell_address(ctx: &DecodeContext<'_>, address: &str) -> Result<Option<(u32, u32)>, CodecError> {
    let split = required!(ctx.position_by(
        address.as_bytes(),
        |byte| Ok(byte.is_ascii_digit()),
        "fcstd spreadsheet cell address"
    )?);
    let mut column = 0_u32;
    let mut bytes = address[..split].bytes();
    while let Some(byte) = ctx.next_charged(&mut bytes, "fcstd spreadsheet column address")? {
        if !byte.is_ascii_uppercase() {
            return Ok(None);
        }
        column = required!(column
            .checked_mul(26)
            .and_then(|column| column.checked_add(u32::from(byte - b'A' + 1))));
    }
    let row = required!(ctx
        .parse_text::<u32>(&address[split..], "fcstd spreadsheet row address")?
        .ok());
    Ok((row != 0 && column != 0).then_some((row, column)))
}

#[cfg(test)]
fn range_contains_address(range: &SpreadsheetRange, address: &str) -> bool {
    CellAddress::parse(address).is_some_and(|address| range.contains(address))
}

#[cfg(test)]
mod tests;
