// SPDX-License-Identifier: Apache-2.0
//! Registered spreadsheet value roots and independently owned cell/dimension leaves.

use crate::native::ValueRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::CellAddress;
use std::collections::BTreeMap;

type Attributes = BTreeMap<String, String>;

pub(crate) fn root_tag(kind: &str) -> Option<&'static str> {
    match kind {
        "Spreadsheet::PropertySheet" => Some("Cells"),
        "Spreadsheet::PropertyColumnWidths" => Some("ColumnInfo"),
        "Spreadsheet::PropertyRowHeights" => Some("RowInfo"),
        _ => None,
    }
}

pub(crate) fn validate_root(
    kind: &str,
    tag: &str,
    attributes: &Attributes,
    _children: usize,
) -> Result<(), CodecError> {
    if root_tag(kind) != Some(tag) {
        return Err(CodecError::malformed(
            "spreadsheet value has an unexpected root",
        ));
    }
    let count = required(attributes, "Count")?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("spreadsheet Count is not an unsigned integer"))?;
    if count > 1_000_000 {
        return Err(CodecError::malformed(
            "spreadsheet Count differs from its bounded records",
        ));
    }
    Ok(())
}

pub(crate) fn validate_child(
    kind: &str,
    tag: &str,
    parent: &str,
    attributes: &Attributes,
) -> Result<(), CodecError> {
    if super::links::expression_container(kind) {
        if tag == "XLinks" && matches!(parent, "Cells" | "ExpressionEngine") {
            unsigned(attributes, "count")?;
            if attributes.contains_key("docs") {
                unsigned(attributes, "docs")?;
            }
            return Ok(());
        }
        if parent == "XLinks" {
            if tag == "DocMap" {
                required(attributes, "name")?;
                required(attributes, "label")?;
                unsigned(attributes, "index")?;
                return Ok(());
            }
            if tag == "XLink" {
                required(attributes, "name")?;
                return Ok(());
            }
        }
        if parent == "XLink" && tag == "Sub" {
            required(attributes, "value")?;
            return Ok(());
        }
    }
    let child_tag = match kind {
        "Spreadsheet::PropertySheet" => "Cell",
        "Spreadsheet::PropertyColumnWidths" => "Column",
        "Spreadsheet::PropertyRowHeights" => "Row",
        _ => return Ok(()),
    };
    if root_tag(kind) != Some(parent) || tag != child_tag {
        return Err(CodecError::malformed(
            "spreadsheet record has an unexpected nesting or tag",
        ));
    }
    if tag == "Cell" {
        cell_address(attributes)?;
        cell_rectangle(attributes)?;
    } else {
        dimension_index(kind, attributes)?;
        let key = if tag == "Column" { "width" } else { "height" };
        required(attributes, key)?.parse::<u32>().map_err(|_| {
            CodecError::malformed("spreadsheet dimension size is not an unsigned integer")
        })?;
    }
    Ok(())
}

pub(crate) fn validate_layout(
    kind: &str,
    tag: &str,
    attributes: &Attributes,
    children: &[&str],
) -> Result<(), CodecError> {
    if super::links::expression_container(kind) && tag == "XLinks" {
        let count = unsigned(attributes, "count")?;
        if count != children.iter().filter(|tag| **tag == "XLink").count()
            || attributes
                .get("docs")
                .map(|_| unsigned(attributes, "docs"))
                .transpose()?
                .unwrap_or(0)
                != children.iter().filter(|tag| **tag == "DocMap").count()
        {
            return Err(CodecError::malformed(
                "expression XLinks counts differ from their records",
            ));
        }
        if let Some(hidden) = attributes.get("hidden") {
            for index in hidden.split_whitespace() {
                if !index.parse::<usize>().is_ok_and(|index| index < count) {
                    return Err(CodecError::malformed(
                        "expression hidden index is outside XLinks",
                    ));
                }
            }
        }
        return Ok(());
    }
    let (child_tag, count_key) = match (kind, tag) {
        ("Spreadsheet::PropertySheet", "Cells") => ("Cell", "Count"),
        ("Spreadsheet::PropertyColumnWidths", "ColumnInfo") => ("Column", "Count"),
        ("Spreadsheet::PropertyRowHeights", "RowInfo") => ("Row", "Count"),
        ("App::PropertyExpressionEngine", "ExpressionEngine") => ("Expression", "count"),
        _ => return Ok(()),
    };
    let count = unsigned(attributes, count_key)?;
    if count > 1_000_000 || count != children.iter().filter(|tag| **tag == child_tag).count() {
        return Err(CodecError::malformed(
            "expression/spreadsheet count differs from bounded direct records",
        ));
    }
    Ok(())
}

pub(crate) fn validate_previous(
    kind: &str,
    tag: &str,
    parent: Option<usize>,
    attributes: &Attributes,
    previous: &[ValueRecord],
) -> Result<(), CodecError> {
    if super::links::expression_container(kind) && parent.is_some() {
        if tag == "XLinks"
            && previous
                .iter()
                .any(|peer| peer.parent == parent && peer.tag == tag)
        {
            return Err(CodecError::malformed(
                "expression container has an extra XLinks carrier",
            ));
        }
        if tag == "DocMap" {
            let index = unsigned(attributes, "index")?;
            let container = previous
                .iter()
                .find(|peer| Some(peer.order) == parent)
                .ok_or_else(|| {
                    CodecError::malformed("document map has no retained XLinks parent")
                })?;
            if index >= unsigned(&container.attributes, "count")? {
                return Err(CodecError::malformed(
                    "document map index is outside XLinks",
                ));
            }
        }
        if tag == "DocMap"
            && previous.iter().any(|peer| {
                peer.parent == parent
                    && peer.tag == tag
                    && peer.attributes.get("index") == attributes.get("index")
            })
        {
            return Err(CodecError::malformed(
                "expression document map index is duplicated",
            ));
        }
    }
    if parent.is_none() || root_tag(kind).is_none() || !matches!(tag, "Cell" | "Column" | "Row") {
        return Ok(());
    }
    for peer in previous
        .iter()
        .filter(|peer| peer.parent == parent && peer.tag == tag)
    {
        if tag == "Cell" {
            if cell_address(attributes)? == cell_address(&peer.attributes)? {
                return Err(CodecError::malformed(
                    "spreadsheet cell address is duplicated",
                ));
            }
            if let (Some(current), Some(prior)) = (
                cell_rectangle(attributes)?,
                cell_rectangle(&peer.attributes)?,
            ) {
                if current.0 <= prior.1
                    && prior.0 <= current.1
                    && current.2 <= prior.3
                    && prior.2 <= current.3
                {
                    return Err(CodecError::malformed("spreadsheet merged ranges overlap"));
                }
            }
        } else if dimension_index(kind, attributes)? == dimension_index(kind, &peer.attributes)? {
            return Err(CodecError::malformed("spreadsheet dimension is duplicated"));
        }
    }
    Ok(())
}

/// Actual ordinal among the expression container's XLink siblings.
pub(crate) fn link_index(
    source: &crate::xml_framing::XmlSource<'_>,
    index: usize,
) -> Result<Option<usize>, CodecError> {
    if source.name(index)? != "XLink" {
        return Ok(None);
    }
    let Some(parent) = source.elements()[index].parent() else {
        return Ok(None);
    };
    if source.name(parent)? != "XLinks" {
        return Ok(None);
    }
    Ok(Some(
        source
            .children(parent)
            .take_while(|child| *child != index)
            .filter(|child| source.name(*child).is_ok_and(|name| name == "XLink"))
            .count(),
    ))
}

fn required<'a>(attributes: &'a Attributes, key: &str) -> Result<&'a str, CodecError> {
    attributes
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed(format_args!("spreadsheet record has no {key}")))
}

fn unsigned(attributes: &Attributes, key: &str) -> Result<usize, CodecError> {
    required(attributes, key)?.parse::<usize>().map_err(|_| {
        CodecError::malformed(format_args!(
            "expression/spreadsheet {key} is not an unsigned integer"
        ))
    })
}

fn cell_address(attributes: &Attributes) -> Result<CellAddress, CodecError> {
    CellAddress::parse(required(attributes, "address")?)
        .ok_or_else(|| CodecError::malformed("spreadsheet cell address is invalid"))
}

fn dimension_index(kind: &str, attributes: &Attributes) -> Result<u32, CodecError> {
    let name = required(attributes, "name")?;
    if kind == "Spreadsheet::PropertyColumnWidths" {
        CellAddress::parse(&format!("{name}1")).map(CellAddress::col)
    } else {
        name.parse::<u32>().ok().filter(|index| *index > 0)
    }
    .ok_or_else(|| CodecError::malformed("spreadsheet dimension name is invalid"))
}

fn cell_rectangle(attributes: &Attributes) -> Result<Option<(u32, u32, u32, u32)>, CodecError> {
    let span = |key| -> Result<i32, CodecError> {
        attributes.get(key).map_or(Ok(1), |value| {
            value
                .parse::<i32>()
                .map_err(|_| CodecError::malformed("spreadsheet cell span is not an integer"))
        })
    };
    let rows = span("rowSpan")?;
    let columns = span("colSpan")?;
    // FreeCAD retains nonpositive spans as cell metadata without a merged range.
    if rows < 1 || columns < 1 || rows == 1 && columns == 1 {
        return Ok(None);
    }
    let start = cell_address(attributes)?;
    let end_row = start
        .row()
        .checked_add(
            u32::try_from(rows - 1)
                .map_err(|_| CodecError::malformed("spreadsheet row span is invalid"))?,
        )
        .ok_or_else(|| CodecError::malformed("spreadsheet row span overflows"))?;
    let end_col = start
        .col()
        .checked_add(
            u32::try_from(columns - 1)
                .map_err(|_| CodecError::malformed("spreadsheet column span is invalid"))?,
        )
        .ok_or_else(|| CodecError::malformed("spreadsheet column span overflows"))?;
    Ok(Some((start.row(), end_row, start.col(), end_col)))
}
