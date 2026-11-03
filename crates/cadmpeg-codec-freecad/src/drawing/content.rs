// SPDX-License-Identifier: Apache-2.0
//! Neutral sheet and persisted view geometry transfer.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::{
    CurveSource, CurveVisibility, DrawingCurve, DrawingCurveGeometry, DrawingKind, DrawingSheet,
    TitleBlockField,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::FiniteVector;

use crate::loss::FreecadLossCode;
use crate::native::PropertyRecord;

pub(crate) fn enrich(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    properties: &[PropertyRecord],
    data: &BTreeMap<String, View<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for drawing in &mut model.drawings {
        let owned: Vec<_> = properties
            .iter()
            .filter(|p| p.owner == drawing.object)
            .collect();
        if drawing.kind == DrawingKind::Template {
            let mut fields = Vec::new();
            if let Some(property) =
                super::typed_property(&owned, "EditableTexts", "App::PropertyMap")?
            {
                let xml = ctx.parse_xml(property.xml.text(), "drawing title block")?;
                for item in xml
                    .root_element()
                    .children()
                    .filter(|n| n.has_tag_name("Map"))
                    .flat_map(|n| n.children().filter(|n| n.has_tag_name("Item")))
                {
                    let name = item.attribute("key").unwrap_or("");
                    if let Some(name) = cadmpeg_core::text::NonBlankString::new(name.to_owned()) {
                        fields.push(TitleBlockField {
                            name,
                            value: item.attribute("value").unwrap_or("").to_owned(),
                        });
                    } else {
                        losses.push(
                            FreecadLossCode::DrawingTitleBlockNotTransferred
                                .note(property.id.clone()),
                        );
                    }
                }
            }
            let carrier = super::sole_named_property("drawing", &owned, "PageResult")?
                .or(super::sole_named_property("drawing", &owned, "Template")?);
            if let Some(property) = carrier {
                let sheet = if let [name] = property.side_entries() {
                    if let Some(bytes) = data.get(name.as_str()) {
                        match std::str::from_utf8(bytes.window()) {
                            Ok(text) => {
                                let xml = ctx.parse_xml(text, "drawing sheet SVG");
                                match xml {
                                    Ok(xml) => sheet(xml.root_element(), fields),
                                    Err(CodecError::ResourceLimit(limit)) => {
                                        return Err(CodecError::ResourceLimit(limit))
                                    }
                                    Err(_) => None,
                                }
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if sheet.as_ref().is_some_and(DrawingSheet::is_valid) {
                    drawing.sheet = sheet;
                } else {
                    losses.push(
                        FreecadLossCode::DrawingSheetNotTransferred.note(property.id.clone()),
                    );
                }
            }
        }
        for property in owned {
            let (record_tag, source, loss) = match property.type_name.as_str() {
                "TechDraw::PropertyCosmeticEdgeList" => (
                    "CosmeticEdge",
                    CurveSource::Cosmetic,
                    FreecadLossCode::DrawingCosmeticEdgeNotTransferred,
                ),
                "TechDraw::PropertyCenterLineList" => (
                    "CenterLine",
                    CurveSource::CenterLine,
                    FreecadLossCode::DrawingCenterLineNotTransferred,
                ),
                "TechDraw::PropertyCosmeticVertexList" => (
                    "CosmeticVertex",
                    CurveSource::Cosmetic,
                    FreecadLossCode::DrawingCosmeticVertexNotTransferred,
                ),
                "TechDraw::PropertyGeomFormatList" => (
                    "GeomFormat",
                    CurveSource::Cosmetic,
                    FreecadLossCode::DrawingGeometryFormatNotTransferred,
                ),
                _ => continue,
            };
            let xml = ctx.parse_xml(property.xml.text(), "drawing curve XML")?;
            for record in xml
                .root_element()
                .children()
                .filter(roxmltree::Node::is_element)
                .flat_map(|n| n.children().filter(|n| n.has_tag_name(record_tag)))
            {
                let curve = match record_tag {
                    "CosmeticEdge" => cosmetic(record, source),
                    "CenterLine" => center_line(record),
                    _ => None,
                };
                if let Some(curve) = curve.filter(DrawingCurve::is_valid) {
                    drawing.curves.push(curve);
                } else {
                    losses.push(loss.note(format!("{}: {}", property.id, record_tag)));
                }
            }
        }
    }
    Ok(())
}

fn sheet(root: roxmltree::Node<'_, '_>, fields: Vec<TitleBlockField>) -> Option<DrawingSheet> {
    if !root.has_tag_name("svg") {
        return None;
    }
    let sheet = DrawingSheet {
        width_mm: svg_length(root.attribute("width")?)?,
        height_mm: svg_length(root.attribute("height")?)?,
        fields,
    };
    sheet.is_valid().then_some(sheet)
}

fn svg_length(text: &str) -> Option<PositiveReal> {
    let text = text.trim();
    for (unit, factor) in [
        ("mm", 1.0),
        ("cm", 10.0),
        ("in", 25.4),
        ("pt", 25.4 / 72.0),
        ("pc", 25.4 / 6.0),
        ("px", 25.4 / 96.0),
    ] {
        if let Some(number) = text.strip_suffix(unit) {
            return PositiveReal::new(number.trim().parse::<f64>().ok()? * factor);
        }
    }
    None
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|n| n.has_tag_name(name))
}

fn scalar(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    child(node, name)?
        .attribute("value")?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn point(node: roxmltree::Node<'_, '_>) -> Option<FiniteVector<2>> {
    let z = node.attribute("Z")?.parse::<f64>().ok()?;
    if z != 0.0 {
        return None;
    }
    FiniteVector::new([
        node.attribute("X")?.parse().ok()?,
        node.attribute("Y")?.parse().ok()?,
    ])
}

fn boolean(node: roxmltree::Node<'_, '_>, name: &str) -> Option<bool> {
    match child(node, name)?.attribute("value")? {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn cosmetic(node: roxmltree::Node<'_, '_>, source: CurveSource) -> Option<DrawingCurve> {
    let displayed = boolean(node, "Visible")?;
    let base = node;
    let expected_source = match source {
        CurveSource::Cosmetic => 1.0,
        CurveSource::CenterLine => 2.0,
        CurveSource::ProjectedModelEdge => return None,
    };
    if scalar(base, "Source")? != expected_source
        || scalar(base, "ExtractType")? != 0.0
        || scalar(base, "GeomType")? != scalar(node, "GeometryType")?
    {
        return None;
    }
    if boolean(base, "Reversed")? {
        return None;
    }
    let visibility = if boolean(base, "HLRVisible")? {
        CurveVisibility::Visible
    } else {
        CurveVisibility::Hidden
    };
    let geometry = match scalar(node, "GeometryType")? {
        1.0 => DrawingCurveGeometry::Circle {
            center: point(child(node, "Center")?)?,
            radius: PositiveReal::new(scalar(node, "Radius")?)?,
        },
        7.0 => {
            let points: Vec<_> = child(node, "Points")?
                .children()
                .filter(|n| n.has_tag_name("Point"))
                .collect();
            let [start, end] = points.as_slice() else {
                return None;
            };
            DrawingCurveGeometry::Line {
                start: point(*start)?,
                end: point(*end)?,
            }
        }
        _ => return None,
    };
    Some(DrawingCurve {
        geometry,
        visibility,
        source,
        displayed: Some(displayed),
    })
}

fn center_line(node: roxmltree::Node<'_, '_>) -> Option<DrawingCurve> {
    for name in ["HShift", "VShift", "Rotate", "Extend"] {
        if scalar(node, name)? != 0.0 {
            return None;
        }
    }
    if boolean(node, "Flip")? {
        return None;
    }
    if ![0.0, 1.0, 2.0].contains(&scalar(node, "Mode")?)
        || ![0.0, 1.0, 2.0].contains(&scalar(node, "Type")?)
    {
        return None;
    }
    let curve = cosmetic(node, CurveSource::CenterLine)?;
    let DrawingCurveGeometry::Line { start, end } = curve.geometry else {
        return None;
    };
    if point(child(node, "Start")?)? != start || point(child(node, "End")?)? != end {
        return None;
    }
    Some(curve)
}
