// SPDX-License-Identifier: Apache-2.0
//! Neutral feature, parameter, and spreadsheet output.

pub(super) mod features;
mod sketches;
pub(super) use sketches::write as sketches;

use super::xml::{self, Object};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::transform::Transform;
use std::collections::HashMap;

/// Placement quaternion comes from the neutral matrix, never source XML.
/// General affine placements cannot be stored by `App::PropertyPlacement`.
pub(super) fn placement(transform: Transform) -> Result<String, CodecError> {
    let rows = transform.affine_rows();
    let matrix = [
        [rows[0][0], rows[0][1], rows[0][2]],
        [rows[1][0], rows[1][1], rows[1][2]],
        [rows[2][0], rows[2][1], rows[2][2]],
    ];
    let trace = matrix[0][0] + matrix[1][1] + matrix[2][2];
    let quaternion = if trace > 0.0 {
        let scale = (trace + 1.0).sqrt() * 2.0;
        [
            (matrix[2][1] - matrix[1][2]) / scale,
            (matrix[0][2] - matrix[2][0]) / scale,
            (matrix[1][0] - matrix[0][1]) / scale,
            scale / 4.0,
        ]
    } else {
        let i = (0..3)
            .max_by(|left, right| matrix[*left][*left].total_cmp(&matrix[*right][*right]))
            .ok_or_else(|| CodecError::malformed("placement matrix is empty"))?;
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let scale = (1.0 + matrix[i][i] - matrix[j][j] - matrix[k][k]).sqrt() * 2.0;
        let mut result = [0.0; 4];
        result[i] = scale / 4.0;
        result[j] = (matrix[j][i] + matrix[i][j]) / scale;
        result[k] = (matrix[k][i] + matrix[i][k]) / scale;
        result[3] = (matrix[k][j] - matrix[j][k]) / scale;
        result
    };
    if quaternion.iter().any(|value| !value.is_finite()) {
        return Err(CodecError::malformed(
            "placement cannot form a finite quaternion",
        ));
    }
    Ok(xml::property(
        "Placement",
        "App::PropertyPlacement",
        &xml::element(
            "PropertyPlacement",
            &[
                ("Px", rows[0][3].to_string()),
                ("Py", rows[1][3].to_string()),
                ("Pz", rows[2][3].to_string()),
                ("Q0", quaternion[0].to_string()),
                ("Q1", quaternion[1].to_string()),
                ("Q2", quaternion[2].to_string()),
                ("Q3", quaternion[3].to_string()),
            ],
        ),
    ))
}

fn spreadsheet_expression(
    ir: &CadIr,
    parameter: &cadmpeg_ir::features::DesignParameter,
    names: &HashMap<String, String>,
) -> Option<String> {
    if let Some(cadmpeg_ir::features::ParameterValue::String(value)) = &parameter.value {
        if parameter.expression
            == cadmpeg_ir::features::Expression::literal(parameter.value.as_ref()?)
        {
            return Some(value.clone());
        }
    }
    super::parameters::render_expression(ir, parameter, names)
}

pub(super) fn spreadsheets(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
) -> Result<(), CodecError> {
    for sheet in &ir.model.spreadsheets {
        let name = names
            .get(sheet.feature.as_str())
            .cloned()
            .unwrap_or_else(|| format!("Sheet{}", objects.len()));
        let index = if let Some(index) = objects.iter().position(|object| object.name == name) {
            index
        } else {
            objects.push(Object::new(
                name.clone(),
                "Spreadsheet::Sheet",
                sheet.id.as_str(),
            ));
            objects.len() - 1
        };
        names.insert(sheet.id.as_str().to_owned(), name);
        let object = &mut objects[index];
        object.kind = "Spreadsheet::Sheet".into();
        let mut content = format!("<Cells Count=\"{}\">", sheet.cells().len());
        for cell in sheet.cells() {
            let parameter = ir
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == cell.parameter)
                .ok_or_else(|| {
                    CodecError::malformed("spreadsheet cell references absent parameter")
                })?;
            let mut attributes: Vec<(&str, String)> = parameter
                .properties
                .iter()
                .filter(|(key, _)| {
                    !matches!(key.as_str(), "address" | "alias" | "rowSpan" | "colSpan")
                })
                .map(|(key, value)| (key.as_str(), value.clone()))
                .collect();
            attributes.extend([
                ("address", cell.address.a1()),
                (
                    "content",
                    spreadsheet_expression(ir, parameter, names).ok_or_else(|| {
                        CodecError::UnsupportedContent(format!(
                            "spreadsheet parameter {} has an unresolved expression reference",
                            parameter.id
                        ))
                    })?,
                ),
            ]);
            let symbol = names
                .get(&format!("parameter-path:{}", parameter.id))
                .unwrap_or(&parameter.name);
            if *symbol != cell.address.a1() {
                attributes.push(("alias", symbol.clone()));
            }
            if let Some(range) = sheet
                .merged_ranges()
                .iter()
                .find(|range| range.start() == cell.address)
            {
                attributes.push((
                    "rowSpan",
                    (range.end().row() - range.start().row() + 1).to_string(),
                ));
                attributes.push((
                    "colSpan",
                    (range.end().col() - range.start().col() + 1).to_string(),
                ));
            }
            content.push_str(&xml::element("Cell", &attributes));
        }
        content.push_str("</Cells>");
        object.set(
            "cells",
            xml::property("cells", "Spreadsheet::PropertySheet", &content),
        );
        for (name, kind, container, element, dimension, pixels) in [
            (
                "columnWidths",
                "Spreadsheet::PropertyColumnWidths",
                "ColumnInfo",
                "Column",
                sheet.column_widths(),
                "width",
            ),
            (
                "rowHeights",
                "Spreadsheet::PropertyRowHeights",
                "RowInfo",
                "Row",
                sheet.row_heights(),
                "height",
            ),
        ] {
            let rows = dimension
                .iter()
                .map(|dimension| {
                    let index = dimension.index.get();
                    let name = if element == "Column" {
                        let address = cadmpeg_ir::CellAddress::new(1, index)
                            .ok_or_else(|| {
                                CodecError::malformed("spreadsheet column must be positive")
                            })?
                            .a1();
                        address.trim_end_matches('1').to_owned()
                    } else {
                        index.to_string()
                    };
                    Ok(xml::element(
                        element,
                        &[("name", name), (pixels, dimension.pixels.to_string())],
                    ))
                })
                .collect::<Result<String, CodecError>>()?;
            object.set(
                name,
                xml::property(
                    name,
                    kind,
                    &format!(
                        "<{container} Count=\"{}\">{rows}</{container}>",
                        dimension.len()
                    ),
                ),
            );
        }
    }
    Ok(())
}
