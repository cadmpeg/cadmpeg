// SPDX-License-Identifier: Apache-2.0
//! Exact dimension measurements from persisted reference geometry.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::Drawing;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::semantic_annotations::DimensionKind;
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;

use super::{property, Properties};
use crate::brep::{
    parse_reference_payload, Tables, TextCurve, TextEdgeRepresentation, TextTShapeGeometry,
};
use crate::loss::FreecadLossCode;

pub(super) fn transfer(
    model: &mut Model,
    properties: &Properties<'_>,
    entries: &BTreeMap<String, View<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if !model.semantic_annotations.iter().any(|annotation| {
        annotation.kind == cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Dimension
    }) {
        return Ok(());
    }
    let drawings = model
        .drawings
        .iter()
        .map(|drawing| (drawing.object.as_str(), drawing))
        .collect::<BTreeMap<_, _>>();
    let sources = super::reference::Sources::new(
        &model.curves,
        &model.edges,
        &model.vertices,
        &model.points,
        &model.surfaces,
        &model.faces,
        model.bodies.iter().all(|body| body.transform.is_none()),
    );
    for annotation in &mut model.semantic_annotations {
        if annotation.kind != cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Dimension {
            continue;
        }
        let owner = &annotation.object;
        if property(properties, owner, "SavedGeometry")?
            .is_some_and(|saved| empty_cache(saved, entries))
        {
            continue;
        }
        if annotation.runtime_type != "TechDraw::DrawViewDimension" {
            losses.push(
                FreecadLossCode::DrawingDimensionGeometryNotTransferred.note(format!(
                    "{owner}: dimension family has no admitted measurement or reference anchors"
                )),
            );
            continue;
        }
        let Some(kind) = annotation.dimension_kind.as_ref() else {
            losses.push(
                FreecadLossCode::DrawingDimensionGeometryNotTransferred
                    .note(format!("{owner}: dimension kind is absent")),
            );
            continue;
        };
        let measured = match exact_reference(properties, owner, entries, &drawings, kind, &sources)
        {
            Ok(measured) => measured,
            Err(CodecError::ResourceLimit(error)) => return Err(CodecError::ResourceLimit(error)),
            Err(error) => {
                losses.push(
                    FreecadLossCode::DrawingDimensionGeometryNotTransferred.note(format!(
                        "{owner}: saved reference geometry is invalid: {error}"
                    )),
                );
                continue;
            }
        };
        if let Some((value, anchors)) = measured {
            annotation.value = Some(value);
            if anchors.is_empty() && *kind != (DimensionKind::Angle {}) {
                losses.push(FreecadLossCode::DimensionAnchorsNotTransferred.note(format!("{owner}: exact value retained without admitted page-space reference anchors")));
            }
            annotation.reference_anchors = anchors;
        } else {
            losses.push(FreecadLossCode::DrawingDimensionGeometryNotTransferred.note(format!("{owner}: no exact admitted saved reference geometry and page placement; value and anchors remain absent")));
        }
    }
    if model
        .semantic_annotations
        .iter()
        .any(|annotation| !annotation.reference_anchors_are_valid())
    {
        return Err(CodecError::malformed("invalid dimension reference anchors"));
    }
    Ok(())
}

fn empty_cache(
    saved: &crate::native::PropertyRecord,
    entries: &BTreeMap<String, View<'_>>,
) -> bool {
    saved.type_name == "Part::PropertyTopoShapeList"
        && saved.side_entries().iter().all(|name| {
            entries
                .get(name.as_str())
                .and_then(|view| std::str::from_utf8(view.window()).ok())
                .and_then(|text| text.split_once("TShapes"))
                .is_some_and(|(_, tail)| tail.split_whitespace().eq(["0", "*"]))
        })
}

type ReferenceMeasurement = (FiniteReal, Vec<FiniteVector<2>>);

fn exact_reference(
    properties: &Properties<'_>,
    owner: &str,
    entries: &BTreeMap<String, View<'_>>,
    drawings: &BTreeMap<&str, &Drawing>,
    kind: &DimensionKind,
    sources: &super::reference::Sources<'_>,
) -> Result<Option<ReferenceMeasurement>, CodecError> {
    for flag in ["Arbitrary", "Inverted", "AngleOverride"] {
        if !is_false(properties, owner, flag)? {
            return Ok(None);
        }
    }
    if selected_integer(properties, owner, "MeasureType")? != Some("1") {
        return Ok(None);
    }
    let Some(reference) = property(properties, owner, "References2D")? else {
        return Ok(None);
    };
    let required_references = if *kind == (DimensionKind::Angle {}) {
        2
    } else {
        1
    };
    let Some(Some(link)) = reference.links().first() else {
        return Ok(None);
    };
    let mut count = 0_usize;
    let mut selectors = Vec::new();
    for target in reference.links() {
        let Some(target) = target else {
            return Ok(None);
        };
        if target.document().is_some() || target.object() != link.object() {
            return Ok(None);
        }
        if target.subelements().iter().any(|name| {
            name.strip_prefix("Edge")
                .is_none_or(|index| index.parse::<usize>().is_err())
        }) {
            return Ok(None);
        }
        let Some(total) = count.checked_add(target.subelements().len()) else {
            return Ok(None);
        };
        count = total;
        selectors.extend(target.subelements().iter().map(String::as_str));
    }
    if reference.type_name != "App::PropertyLinkSubList" || count != required_references {
        return Ok(None);
    }
    let Some(object) = link.object() else {
        return Ok(None);
    };
    let Some(view) = drawings.get(object) else {
        return Ok(None);
    };
    if view.runtime_type != "TechDraw::DrawViewPart" {
        return Ok(None);
    }
    let rotation = view.rotation_degrees.map(FiniteReal::get);
    if matches!(
        kind,
        DimensionKind::HorizontalDistance {} | DimensionKind::VerticalDistance {}
    ) && rotation != Some(0.0)
    {
        return Ok(None);
    }
    if !is_false(properties, &view.object, "Perspective")? {
        return Ok(None);
    }
    let Some(saved) = property(properties, owner, "SavedGeometry")? else {
        return Ok(None);
    };
    if saved.type_name != "Part::PropertyTopoShapeList" {
        return Ok(None);
    }
    let mut payloads = Vec::new();
    for name in saved.side_entries() {
        let Some(view) = entries.get(name) else {
            return Ok(None);
        };
        payloads.push(parse_reference_payload(name, *view)?);
    }
    let mut curves = Vec::new();
    for payload in &payloads {
        let Some(tables) = Tables::from_shape(payload) else {
            return Ok(None);
        };
        let [root_use] = tables.roots else {
            return Ok(None);
        };
        if root_use.location.resolve(tables.locations)?
            != cadmpeg_ir::transform::Transform::identity()
        {
            return Ok(None);
        }
        let root = tables.tshapes.resolve(root_use.shape)?;
        let TextTShapeGeometry::Edge {
            degenerated: false,
            representations,
            ..
        } = &root.geometry
        else {
            return Ok(None);
        };
        let [TextEdgeRepresentation::Curve3d {
            curve,
            location: 0,
            parameter_range,
        }] = representations.as_slice()
        else {
            return Ok(None);
        };
        let Some(curve) = curve
            .checked_sub(1)
            .and_then(|index| tables.curves.get(index))
        else {
            return Ok(None);
        };
        curves.push((curve, *parameter_range));
    }
    let Some(projection) = sources.project(view) else {
        return Ok(None);
    };
    if curves.len() != selectors.len()
        || curves
            .iter()
            .any(|(curve, range)| !projection.contains(curve, *range))
    {
        return Ok(None);
    }
    let page = |points: [[f64; 2]; 2]| {
        let (Some(position), Some(scale), Some(0.0)) = (view.position, view.scale, rotation) else {
            return Vec::new();
        };
        let points = points.map(|point| {
            FiniteVector::new([
                position[0] + scale.get() * point[0],
                position[1] - scale.get() * point[1],
            ])
        });
        let [Some(first), Some(second)] = points else {
            return Vec::new();
        };
        vec![first, second]
    };
    let measured = match (kind, curves.as_slice()) {
        (
            DimensionKind::Distance {}
            | DimensionKind::HorizontalDistance {}
            | DimensionKind::VerticalDistance {},
            [(TextCurve::Line { origin, direction }, range)],
        ) => {
            let origin = origin.get();
            let direction = direction.get();
            if origin.z != 0.0 || direction.z != 0.0 {
                return Ok(None);
            }
            let points = range.map(|parameter| {
                [
                    origin.x + direction.x * parameter.get(),
                    origin.y + direction.y * parameter.get(),
                ]
            });
            let delta = [points[1][0] - points[0][0], points[1][1] - points[0][1]];
            let value = match kind {
                DimensionKind::Distance {} => delta[0].hypot(delta[1]),
                DimensionKind::HorizontalDistance {} => delta[0].abs(),
                DimensionKind::VerticalDistance {} => delta[1].abs(),
                _ => return Ok(None),
            };
            FiniteReal::new(value).map(|value| (value, page(points)))
        }
        (
            DimensionKind::Radius {} | DimensionKind::Diameter {},
            [(
                TextCurve::Circle {
                    center,
                    axis,
                    ref_direction,
                    radius,
                },
                range,
            )],
        ) => {
            let center = center.get();
            let axis = axis.get();
            let direction = ref_direction.get();
            if center.z != 0.0
                || axis.x != 0.0
                || axis.y != 0.0
                || axis.z.abs() != 1.0
                || direction.z != 0.0
                || direction.x.hypot(direction.y) != 1.0
                || radius.get() <= 0.0
            {
                return Ok(None);
            }
            let center = [center.x, center.y];
            let delta = [radius.get() * direction.x, radius.get() * direction.y];
            let first = if *kind == (DimensionKind::Diameter {}) {
                [center[0] - delta[0], center[1] - delta[1]]
            } else {
                center
            };
            let second = [center[0] + delta[0], center[1] + delta[1]];
            let value = if *kind == (DimensionKind::Diameter {}) {
                2.0 * radius.get()
            } else {
                radius.get()
            };
            FiniteReal::new(value).map(|value| {
                let anchors = if range[0].get() == 0.0 && range[1].get() == std::f64::consts::TAU {
                    page([first, second])
                } else {
                    Vec::new()
                };
                (value, anchors)
            })
        }
        (
            DimensionKind::Angle {},
            [(
                TextCurve::Line {
                    direction: first, ..
                },
                _,
            ), (
                TextCurve::Line {
                    direction: second, ..
                },
                _,
            )],
        ) => {
            let first = first.get();
            let second = second.get();
            if first.z != 0.0 || second.z != 0.0 {
                return Ok(None);
            }
            let cross = first.x * second.y - first.y * second.x;
            let dot = first.x * second.x + first.y * second.y;
            if dot != 0.0 || !cross.is_finite() || cross <= 0.0 {
                return Ok(None);
            }
            FiniteReal::new(90.0).map(|value| (value, Vec::new()))
        }
        _ => None,
    };
    Ok(measured)
}

fn is_false(properties: &Properties<'_>, owner: &str, name: &str) -> Result<bool, CodecError> {
    let Some(property) = property(properties, owner, name)? else {
        return Ok(false);
    };
    let [value] = property.values() else {
        return Ok(false);
    };
    Ok(property.type_name == "App::PropertyBool"
        && value.tag == "Bool"
        && value.attributes.len() == 1
        && value
            .attributes
            .get("value")
            .is_some_and(|value| value == "false")
        && value
            .text
            .as_ref()
            .is_none_or(|text| text.trim().is_empty()))
}

fn selected_integer<'a>(
    properties: &Properties<'a>,
    owner: &str,
    name: &str,
) -> Result<Option<&'a str>, CodecError> {
    let Some(property) = property(properties, owner, name)? else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyEnumeration" {
        return Ok(None);
    }
    let [selected] = property.values() else {
        return Ok(None);
    };
    if selected.tag != "Integer"
        || selected.attributes.len() != 1
        || selected
            .text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        return Ok(None);
    }
    Ok(selected.attributes.get("value").map(String::as_str))
}
