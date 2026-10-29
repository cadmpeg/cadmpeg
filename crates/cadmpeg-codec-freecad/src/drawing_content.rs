// SPDX-License-Identifier: Apache-2.0
//! Physical page content from persisted `TechDraw` properties and SVG assets.

mod dimension;
mod reference;
mod svg;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::{DrawingKind, DrawingSheet};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::units::NonzeroVector;

use crate::loss::FreecadLossCode;
use crate::native::PropertyRecord;

type Properties<'a> = BTreeMap<&'a str, Vec<&'a PropertyRecord>>;

pub(super) fn transfer(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    properties: &[PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<LossNote>, CodecError> {
    if model.drawings.is_empty() {
        return Ok(Vec::new());
    }
    let mut indexed = Properties::new();
    for record in properties {
        indexed
            .entry(record.owner.as_str())
            .or_default()
            .push(record);
    }
    let properties = &indexed;
    let mut losses = Vec::new();
    for drawing in &mut model.drawings {
        let owner = drawing.object.as_str();
        if drawing.kind == DrawingKind::Template {
            let result = template(
                ctx,
                owner,
                properties,
                entries,
                drawing.sheet.as_ref(),
                &mut losses,
            );
            match result {
                Ok(content) => drawing.template_geometry = content,
                Err(CodecError::ResourceLimit(error)) => {
                    return Err(CodecError::ResourceLimit(error))
                }
                Err(error) => losses.push(
                    FreecadLossCode::TemplateNotTransferred
                        .note(format!("{owner}: {error}; SVG remains in its asset")),
                ),
            }
        }
        if matches!(
            drawing.kind,
            DrawingKind::View
                | DrawingKind::Projection
                | DrawingKind::Section
                | DrawingKind::Detail
        ) {
            if let Some(property) = property(properties, owner, "XDirection")? {
                let vector = property.values().first().and_then(|value| {
                    NonzeroVector::new([
                        value.attributes.get("valueX")?.parse().ok()?,
                        value.attributes.get("valueY")?.parse().ok()?,
                        value.attributes.get("valueZ")?.parse().ok()?,
                    ])
                });
                if property.type_name == "App::PropertyVector" && vector.is_some() {
                    drawing.x_direction = vector;
                } else {
                    losses.push(FreecadLossCode::DrawingViewAxisNotTransferred.note(format!(
                        "{owner}: XDirection has no admitted finite nonzero model axis"
                    )));
                }
            }
            if matches!(
                drawing.runtime_type.as_str(),
                "TechDraw::DrawViewPart"
                    | "TechDraw::DrawProjGroup"
                    | "TechDraw::DrawProjGroupItem"
                    | "TechDraw::DrawViewSection"
                    | "TechDraw::DrawViewDetail"
            ) {
                losses.push(FreecadLossCode::ViewGeometryNotStored.note(format!(
                    "{owner}: projected view edges are runtime geometry; source B-rep and view parameters are retained"
                )));
            } else {
                losses.push(FreecadLossCode::ViewGeometryNotTransferred.note(format!(
                    "{owner}: this view family has no normalized view geometry; native parameters and assets are retained"
                )));
            }
        }
        if property(properties, owner, "Caption")?.is_some_and(|property| {
            property.values().iter().any(|value| {
                value
                    .attributes
                    .get("value")
                    .is_some_and(|text| !text.is_empty())
            })
        }) {
            losses.push(FreecadLossCode::CaptionNotTransferred.note(format!(
                "{owner}: caption has no normalized page position or physical style; text remains native"
            )));
        }
        if matches!(
            drawing.kind,
            DrawingKind::Annotation | DrawingKind::Dimension | DrawingKind::Balloon
        ) {
            losses.push(FreecadLossCode::DrawingStyleNotTransferred.note(format!(
                "{owner}: untyped font properties, alignment, colour, line spacing, rich text, captions and dimension presentation remain native"
            )));
        }
    }
    dimension::transfer(model, properties, entries, &mut losses)?;
    for annotation in &mut model.semantic_annotations {
        if !annotation.runtime_type.starts_with("TechDraw::") {
            continue;
        }
        if let Some(height) = scalar(
            properties,
            &annotation.object,
            "TextSize",
            "App::PropertyLength",
        )?
        .and_then(PositiveLength::new)
        {
            annotation.text_style.height = Some(height);
            if let Some(font) = property(properties, &annotation.object, "Font")?
                .filter(|property| property.type_name == "App::PropertyFont")
            {
                annotation.text_style.font_family = font_family(font);
            }
        }
    }
    if model
        .drawings
        .iter()
        .any(|drawing| !drawing.physical_content_is_valid())
    {
        return Err(CodecError::malformed("invalid physical drawing content"));
    }
    Ok(losses)
}

fn template(
    ctx: &DecodeContext<'_>,
    owner: &str,
    properties: &Properties<'_>,
    entries: &BTreeMap<String, View<'_>>,
    size: Option<&DrawingSheet>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<cadmpeg_ir::drawings::TemplateGeometry>, CodecError> {
    let result = property(properties, owner, "PageResult")?
        .or(property(properties, owner, "Template")?)
        .filter(|property| property.type_name == "App::PropertyFileIncluded")
        .ok_or_else(|| CodecError::malformed("no SVG PageResult"))?;
    let names = result.side_entries();
    let [name] = names else {
        return Err(CodecError::malformed("template needs one SVG side entry"));
    };
    let view = entries
        .get(name.as_str())
        .ok_or_else(|| CodecError::malformed("missing template SVG"))?;
    let text = std::str::from_utf8(view.window())
        .map_err(|_| CodecError::malformed("template SVG is not UTF-8"))?;
    let xml = ctx.parse_xml(text, "FreeCAD template SVG")?;
    let size = size.ok_or_else(|| CodecError::malformed("template sheet is absent"))?;
    let editable = size
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.value.as_str()))
        .collect();
    svg::parse(&xml, size, &editable, owner, losses).map(Some)
}

fn font_family(property: &PropertyRecord) -> Option<String> {
    let [value] = property.values() else {
        return None;
    };
    if value.tag != "String"
        || value.attributes.len() != 1
        || value
            .text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        return None;
    }
    value
        .attributes
        .get("value")
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn scalar(
    properties: &Properties<'_>,
    owner: &str,
    name: &str,
    carrier: &str,
) -> Result<Option<f64>, CodecError> {
    let Some(property) = property(properties, owner, name)? else {
        return Ok(None);
    };
    if property.type_name != carrier {
        return Ok(None);
    }
    let [value] = property.values() else {
        return Ok(None);
    };
    if value.tag != "Float"
        || value.attributes.len() != 1
        || value
            .text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        return Ok(None);
    }
    Ok(value
        .attributes
        .get("value")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite()))
}

fn property<'a>(
    properties: &Properties<'a>,
    owner: &str,
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut selected = properties
        .get(owner)
        .into_iter()
        .flatten()
        .copied()
        .filter(|property| property.name == name);
    let result = selected.next();
    if selected.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "duplicate {owner}.{name}"
        )));
    }
    Ok(result)
}
