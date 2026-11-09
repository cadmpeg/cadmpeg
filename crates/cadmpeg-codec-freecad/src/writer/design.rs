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
pub(super) fn placement(transform: Transform) -> String {
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
            .unwrap_or(0);
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
    // Callers pass rigid placements, whose rotation forms a finite unit
    // quaternion.
    debug_assert!(
        quaternion.iter().all(|value| value.is_finite()),
        "a rigid placement forms a finite quaternion"
    );
    let quaternion = if quaternion.iter().all(|value| value.is_finite()) {
        quaternion
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    xml::property(
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
    )
}

/// Render a spreadsheet cell's content: a string literal carries its
/// `FreeCAD` text quote; every other value renders lossily so an unresolvable
/// reference arrives as text rather than refusing the write (#238).
pub(in crate::writer) fn spreadsheet_expression(
    ir: &CadIr,
    parameter: &cadmpeg_ir::features::DesignParameter,
    names: &HashMap<String, String>,
) -> String {
    let literal = super::parameters::cell_literal_value(parameter);
    if let Some(cadmpeg_ir::features::ParameterValue::String(value)) = &literal {
        return format!("'{value}");
    }
    if let Some(rendered) = super::parameters::render_expression(ir, parameter, names) {
        if literal.is_some() || rendered.starts_with('=') || rendered.is_empty() {
            return rendered;
        }
        return format!("={rendered}");
    }
    let rendered = super::parameters::render_expression_lossy(ir, parameter, names);
    if rendered.starts_with('\'') {
        rendered
    } else {
        format!("'{rendered}")
    }
}

pub(super) fn spreadsheets(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
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
        let mut content = String::new();
        let mut count = 0;
        for cell in sheet.cells() {
            let Some(parameter) = ir
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == cell.parameter)
            else {
                losses.push(
                    crate::loss::FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "spreadsheet {} cell {} references absent parameter {} and is omitted",
                        sheet.id,
                        cell.address.a1(),
                        cell.parameter
                    )),
                );
                continue;
            };
            let cell_content = spreadsheet_expression(ir, parameter, names);
            super::parameters::report_cell_normalization(parameter, &cell_content, losses);
            let mut attributes: Vec<(&str, String)> = parameter
                .properties
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone()))
                .collect();
            attributes.extend([("address", cell.address.a1()), ("content", cell_content)]);
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
            count += 1;
        }
        object.set(
            "cells",
            xml::property(
                "cells",
                "Spreadsheet::PropertySheet",
                &format!("<Cells Count=\"{count}\">{content}</Cells>"),
            ),
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
                        let Some(address) = cadmpeg_ir::CellAddress::new(1, index) else {
                            debug_assert!(false, "a spreadsheet column index is positive");
                            return Ok(String::new());
                        };
                        address.a1().trim_end_matches('1').to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{
        DesignParameter, Expression, ExpressionToken, ParameterId, ParameterValue,
    };

    fn parameter(expression: Expression, value: Option<ParameterValue>) -> DesignParameter {
        DesignParameter {
            product_definition: None,
            id: ParameterId::mint("cadir:model:parameter#text").unwrap(),
            owner: None,
            ordinal: 0,
            name: "Text".into(),
            expression,
            display: None,
            value,
            driving: true,
            properties: std::collections::BTreeMap::default(),
            pmi: None,
            native_ref: None,
        }
    }

    #[test]
    fn spreadsheet_text_literal_uses_its_expression_value() {
        let parameter = parameter(
            Expression::text(r#""\u0061""#),
            Some(ParameterValue::String("old".into())),
        );
        assert_eq!(
            spreadsheet_expression(&CadIr::empty(), &parameter, &HashMap::new()),
            "'a"
        );
    }

    #[test]
    fn spreadsheet_marks_formulas_and_keeps_missing_references_as_text() {
        let mut ir = CadIr::empty();
        let target = ParameterId::mint("cadir:model:parameter#width").unwrap();
        let mut dependency = parameter(Expression::text("2"), None);
        dependency.id = target.clone();
        dependency.name = "Width".into();
        ir.model.parameters.push(dependency);
        let formula = parameter(
            Expression {
                tokens: vec![
                    ExpressionToken::Parameter(target.clone()),
                    ExpressionToken::Text(" * 2".into()),
                ],
            },
            None,
        );
        let names = HashMap::from([
            (format!("parameter-owner:{target}"), "Sheet".to_owned()),
            (format!("parameter-path:{target}"), "Width".to_owned()),
        ]);
        assert_eq!(
            spreadsheet_expression(&ir, &formula, &names),
            "=Sheet.Width * 2"
        );
        let missing = ParameterId::mint("cadir:model:parameter#missing").unwrap();
        let unresolved = parameter(
            Expression {
                tokens: vec![
                    ExpressionToken::Text("2*".into()),
                    ExpressionToken::Parameter(missing.clone()),
                ],
            },
            None,
        );
        assert_eq!(
            spreadsheet_expression(&ir, &unresolved, &names),
            format!("'2*{missing}")
        );
        let literal = parameter(Expression::text("2 mm"), None);
        assert_eq!(spreadsheet_expression(&ir, &literal, &names), "2 mm");
    }
}
