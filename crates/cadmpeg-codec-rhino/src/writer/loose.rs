// SPDX-License-Identifier: Apache-2.0
//! Native point and curve objects for lower-dimensional shell members.

use cadmpeg_ir::codec::write::EncodeBuffer;
use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::CadIr;

use crate::loss::RhinoLossCode;

use super::{
    attributed_object_record, bounded_line_payload, check_frame, check_nurbs_curve, circle_payload,
    close_point, curve_object_record, nurbs_curve_payload, point_cloud_payload, WritableEdgeCurve,
    WriteError, ARC_CLASS, LINE_CLASS, NURBS_CURVE_CLASS, POINT_CLASS, POINT_CLOUD_CLASS,
};

pub(super) fn write(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    document: &CadIr,
    output: &mut EncodeBuffer<'_>,
    losses: &mut Vec<LossNote>,
    materials: &super::tables::materials::MaterialPlan,
    written_curves: &mut BTreeSet<String>,
) -> Result<usize, WriteError> {
    let body = &ir.model.bodies[0];
    let mut seen_vertices = BTreeSet::new();
    let free = ir
        .model
        .shells
        .iter()
        .flat_map(cadmpeg_ir::topology::Shell::free_vertices)
        .filter(|id| seen_vertices.insert((*id).clone()))
        .collect::<Vec<_>>();
    let mut points = Vec::new();
    let mut point_targets = Vec::new();
    let mut seen_points = BTreeSet::new();
    for id in &free {
        let Some(vertex) = ir.model.vertices.iter().find(|vertex| vertex.id == **id) else {
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "free vertex {id} omitted: dangling vertex reference"
            )));
            continue;
        };
        let Some(point) = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
        else {
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "free vertex {} omitted: dangling point reference",
                vertex.id
            )));
            continue;
        };
        if vertex.tolerance.is_some() {
            losses.push(RhinoLossCode::WriterFreeVertexMetadataReduced.note(format!(
                "free vertex {} is written as point {} without its tolerance",
                vertex.id, point.id,
            )));
        }
        if super::is_support(point.source_object.as_ref()) {
            losses.push(RhinoLossCode::WriterFreeVertexMetadataReduced.note(format!(
                "free vertex {} and point {} are written without the point's dependency role",
                vertex.id, point.id,
            )));
        }
        if !seen_points.insert(&point.id) {
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "body {} shared free-point carrier is expanded into independent point-cloud vertices", body.id,
            )));
        }
        points.push(point.position().get());
        let target = if super::tables::materials::bound_appearance(document, point.id.as_str())
            .is_some()
        {
            Some((
                point.id.to_string(),
                cadmpeg_ir::appearance::AppearanceTarget::Point(point.id.clone()),
            ))
        } else if super::tables::materials::bound_appearance(document, vertex.id.as_str()).is_some()
        {
            Some((
                vertex.id.to_string(),
                cadmpeg_ir::appearance::AppearanceTarget::Vertex(vertex.id.clone()),
            ))
        } else {
            None
        };
        point_targets.push(target);
    }
    let mut count = 0;
    if !points.is_empty() {
        let mut members = std::collections::BTreeSet::new();
        members.insert(body.id.as_str().to_owned());
        for region in &ir.model.regions {
            members.insert(region.id.as_str().to_owned());
        }
        for shell in &ir.model.shells {
            members.insert(shell.id.as_str().to_owned());
        }
        let members = members.into_iter().collect::<Vec<_>>();
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "body {} free-vertex region/shell grouping is reduced to independent point objects; {} member record(s): {}",
            body.id,
            members.len(),
            members.join(", ")
        )));
    }
    if !points.is_empty() {
        let (kind, class, payload) = if points.len() == 1 {
            let point = points[0];
            let mut payload = vec![0x10];
            payload.extend(point.x.to_le_bytes());
            payload.extend(point.y.to_le_bytes());
            payload.extend(point.z.to_le_bytes());
            (1, POINT_CLASS, payload)
        } else {
            let colors = if point_targets.iter().any(Option::is_some) {
                point_targets
                    .iter()
                    .map(|target| {
                        target
                            .as_ref()
                            .and_then(|(_, target)| document.effective_display_color(target))
                            .or(body.color)
                    })
                    .collect::<Option<Vec<_>>>()
            } else {
                None
            };
            for binding in &document.model.appearance_bindings {
                let target = binding.target.typed_record_target(&document.model);
                if point_targets.iter().flatten().any(|(_, selected)| {
                    matches!(selected, cadmpeg_ir::appearance::AppearanceTarget::Point(_))
                        && target.as_ref().unwrap_or(&binding.target) == selected
                }) {
                    let detail = if colors.is_some() && binding.visible.is_some() {
                        "is reduced to a point-cloud color; its material link and visibility are omitted"
                    } else if colors.is_some() {
                        "is reduced to a point-cloud color; its material link is omitted"
                    } else {
                        "is omitted because the point-cloud colors are incomplete"
                    };
                    losses.push(
                        RhinoLossCode::WriterPointCloudAppearanceReduced
                            .note(format!("appearance binding {} {detail}", binding.id)),
                    );
                }
            }
            (
                2,
                POINT_CLOUD_CLASS,
                point_cloud_payload(&points, colors.as_deref())?,
            )
        };
        let individual = if points.len() == 1 {
            point_targets[0].as_ref()
        } else {
            None
        };
        let identity = if let Some((identity, _)) = individual {
            identity.clone()
        } else if ir.model.faces.is_empty() && ir.model.edges.is_empty() {
            body.id.to_string()
        } else {
            format!("{}.free-vertices", body.id)
        };
        let display = individual
            .map(|(_, target)| crate::writer::presentation::display(document, target, None));
        if ir.model.faces.is_empty()
            && body.color.is_some()
            && display
                .and_then(|value| value.color)
                .is_some_and(|color| Some(color) != body.color)
        {
            losses.push(RhinoLossCode::WriterBodyColorReduced.note(format!(
                "body {} color is replaced by the free point's display color",
                body.id
            )));
        }
        ctx.charge_entities(1, "write Rhino object record")?;
        output.append(
            &attributed_object_record(
                document,
                kind,
                class,
                &payload,
                &identity,
                crate::writer::presentation::Display {
                    name: display
                        .and_then(|value| value.name)
                        .or(body.name.as_deref()),
                    color: display.and_then(|value| value.color).or(body.color),
                    visible: display.and_then(|value| value.visible).or(body.visible),
                },
                materials,
            )?,
            "retain Rhino Brep records",
        )?;
        count += 1;
    }
    let wires = ir
        .model
        .shells
        .iter()
        .flat_map(cadmpeg_ir::topology::Shell::wire_edges)
        .collect::<BTreeSet<_>>();
    let mut emitted_segments = BTreeSet::new();
    for id in &wires {
        let Some(edge) = ir.model.edges.iter().find(|edge| edge.id == **id) else {
            losses.push(
                RhinoLossCode::WriterEntityOmitted
                    .note(format!("wire edge {id} omitted: dangling edge reference")),
            );
            continue;
        };
        match curve_payload(ir, edge) {
            Ok(((class, payload), conversion)) => {
                let parent = edge
                    .curve()
                    .and_then(|id| ir.model.curves.iter().find(|row| row.id == *id));
                let segments = parent.and_then(|parent| {
                    let domain = edge.param_range()?.get();
                    let construction = parent.geometry.procedural_construction()?;
                    super::compounds::exact_linear_segments(ir, parent, domain)
                        .map(|segments| (parent, construction, segments))
                });
                if let Some((parent, construction, segments)) = segments {
                    let mut staged = Vec::new();
                    let mut unsupported = false;
                    for segment in segments {
                        match super::compounds::payload(ir, segment, &mut BTreeSet::new()) {
                            Ok((payload, notes)) => staged.push((segment, payload, notes)),
                            Err(WriteError::Omit(_)) => {
                                unsupported = true;
                                break;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    if !unsupported {
                        let ids = staged
                            .iter()
                            .map(|(segment, _, _)| segment.id.as_str())
                            .collect::<Vec<_>>();
                        losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                            "curve {} compound construction {} is written as independent exact line segments {}; the compound record and its component parameters are omitted",
                            parent.id, construction, ids.join(", ")
                        )));
                        for (segment, payload, notes) in staged {
                            if !emitted_segments.insert(segment.id.clone()) {
                                continue;
                            }
                            let display = crate::writer::presentation::display(
                                document,
                                &cadmpeg_ir::appearance::AppearanceTarget::Edge(edge.id.clone()),
                                None,
                            );
                            ctx.charge_entities(payload.record_count, "write Rhino curve records")?;
                            output.append(
                                &curve_object_record(
                                    document,
                                    &payload,
                                    segment.id.as_str(),
                                    crate::writer::presentation::Display {
                                        name: display.name.or(body.name.as_deref()),
                                        color: display.color.or(body.color),
                                        visible: display.visible.or(body.visible),
                                    },
                                    materials,
                                )?,
                                "retain Rhino Brep records",
                            )?;
                            written_curves
                                .extend(payload.represented.iter().map(ToString::to_string));
                            losses.extend(notes);
                            count += 1;
                        }
                        continue;
                    }
                }
                losses.extend(conversion);
                let display = crate::writer::presentation::display(
                    document,
                    &cadmpeg_ir::appearance::AppearanceTarget::Edge(edge.id.clone()),
                    None,
                );
                ctx.charge_entities(1, "write Rhino object record")?;
                output.append(
                    &attributed_object_record(
                        document,
                        4,
                        class,
                        &payload,
                        edge.id.as_str(),
                        crate::writer::presentation::Display {
                            name: display.name.or(body.name.as_deref()),
                            color: display.color.or(body.color),
                            visible: display.visible.or(body.visible),
                        },
                        materials,
                    )?,
                    "retain Rhino Brep records",
                )?;
                count += 1;
            }
            Err(WriteError::Omit(reason)) => {
                let Some(parent) = edge
                    .curve()
                    .and_then(|id| ir.model.curves.iter().find(|row| row.id == *id))
                else {
                    losses.push(
                        RhinoLossCode::WriterEntityOmitted
                            .note(format!("wire edge {} omitted: {reason}", edge.id)),
                    );
                    continue;
                };
                let construction = parent
                    .geometry
                    .procedural_construction()
                    .map_or_else(String::new, |id| {
                        format!("; procedural construction {id} omitted")
                    });
                losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                    "wire edge {} omitted: {reason}{construction}",
                    edge.id
                )));
                let mut pending =
                    std::collections::VecDeque::from(super::compounds::components(ir, parent));
                while let Some(curve) = pending.pop_front() {
                    if !emitted_segments.insert(curve.id.clone()) {
                        continue;
                    }
                    match super::compounds::payload(ir, curve, &mut BTreeSet::new()) {
                        Ok((payload, notes)) => {
                            let display = super::presentation::display(
                                document,
                                &cadmpeg_ir::appearance::AppearanceTarget::Edge(edge.id.clone()),
                                None,
                            );
                            ctx.charge_entities(payload.record_count, "write Rhino curve records")?;
                            output.append(
                                &curve_object_record(
                                    document,
                                    &payload,
                                    curve.id.as_str(),
                                    super::presentation::Display {
                                        name: display.name.or(body.name.as_deref()),
                                        color: display.color.or(body.color),
                                        visible: display.visible.or(body.visible),
                                    },
                                    materials,
                                )?,
                                "retain Rhino archive bytes",
                            )?;
                            if super::is_support(curve.source_object.as_ref()) {
                                losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                                    "support curve {} is written as an independent curve object",
                                    curve.id
                                )));
                            }
                            for id in &payload.represented {
                                emitted_segments.insert(id.clone());
                                written_curves.insert(id.to_string());
                            }
                            pending.extend(super::compounds::unrepresented_children(
                                ir,
                                curve,
                                &payload.represented,
                            ));
                            losses.extend(notes);
                            count += 1;
                        }
                        Err(WriteError::Omit(reason)) => {
                            losses.push(
                                RhinoLossCode::WriterEntityOmitted
                                    .note(format!("curve {} omitted: {reason}", curve.id)),
                            );
                            pending.extend(super::compounds::components(ir, curve));
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
    if !wires.is_empty() {
        let mut regions = std::collections::BTreeSet::new();
        let mut shells = std::collections::BTreeSet::new();
        for shell in &ir.model.shells {
            if shell.wire_edges().is_empty() {
                continue;
            }
            shells.insert(shell.id.as_str().to_owned());
            for region in &ir.model.regions {
                if region.shells.iter().any(|id| id == &shell.id) {
                    regions.insert(region.id.as_str().to_owned());
                }
            }
        }
        let mut members: Vec<String> = regions.into_iter().chain(shells).collect();
        for edge in ir
            .model
            .edges
            .iter()
            .filter(|edge| wires.contains(&edge.id))
        {
            members.push(edge.id.to_string());
            for id in [&edge.start, &edge.end] {
                members.push(id.to_string());
                if let Some(vertex) = ir.model.vertices.iter().find(|vertex| vertex.id == *id) {
                    members.push(vertex.point.to_string());
                }
            }
        }
        members.sort();
        members.dedup();
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "body {} wire ownership is reduced to independent curve objects; {} member record(s): {}",
            body.id,
            members.len(),
            members.join(", ")
        )));
    }
    Ok(count)
}

/// The wire edge's parameter range in increasing order. A reversed edge
/// still converts over its increasing span; evaluation maps either endpoint.
fn normalized_range(lo: f64, hi: f64) -> Option<[cadmpeg_ir::scalar::FiniteReal; 2]> {
    use cadmpeg_ir::scalar::FiniteReal;
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    if !(lo < hi) {
        return None;
    }
    Some([FiniteReal::new(lo)?, FiniteReal::new(hi)?])
}

pub(super) fn curve_payload(
    ir: &CadIr,
    edge: &Edge,
) -> Result<(super::ClassPayload, Option<LossNote>), WriteError> {
    let id = edge.curve().ok_or_else(|| {
        WriteError::Omit("an edge without a held curve has no exact standalone encoding".into())
    })?;
    let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *id) else {
        return Err(WriteError::Omit(format!(
            "dangling curve reference for wire edge {}",
            edge.id
        )));
    };
    let domain = edge
        .param_range()
        .ok_or_else(|| {
            WriteError::Omit("an unbounded wire edge has no finite standalone encoding".into())
        })?
        .finite_components();
    let [lo, hi] = cadmpeg_ir::scalar::FiniteReal::raw_array(domain);
    if lo == hi {
        return Err(WriteError::Omit("wire edge has an empty interval".into()));
    }
    let mut conversion = None;
    let geometry = match curve.geometry.solved() {
        Some(SolvedCurveGeometry::Line(line)) => {
            if (line.direction().as_raw().norm() - 1.0).abs() > super::EPS_WRITE_DEGENERATE {
                return Err(WriteError::Omit(
                    "wire line parameterization exceeds Rhino's unit-direction bound".into(),
                ));
            }
            WritableEdgeCurve::Line(*line)
        }
        Some(SolvedCurveGeometry::Circle(circle)) => {
            check_frame(curve.id.as_str(), *circle.frame(), "circle")?;
            if !(hi > lo) || hi - lo > std::f64::consts::TAU + super::MAX_ARC_SPAN_EXCESS {
                return Err(WriteError::Omit(
                    "a wire circle spanning more than one turn requires an independent trimmed-curve encoding".into(),
                ));
            }
            WritableEdgeCurve::Circle(*circle)
        }
        Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let degree = usize::try_from(nurbs.degree()).map_err(|_| {
                WriteError::Omit(format!(
                    "wire edge {} NURBS degree exceeds address space",
                    edge.id
                ))
            })?;
            let range = [lo.min(hi), lo.max(hi)];
            if nurbs.periodic() {
                let Some(range) = normalized_range(lo, hi) else {
                    return Err(WriteError::Omit(format!(
                        "wire edge {} periodic carrier has no finite increasing range",
                        edge.id
                    )));
                };
                let converted = super::conics::clamp_curve(curve.id.as_str(), nurbs, range)?;
                conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                    "wire edge {} periodic NURBS carrier {} was clamped to an exact nonperiodic span",
                    edge.id, curve.id
                )));
                WritableEdgeCurve::NurbsOwned(converted)
            } else {
                if [
                    nurbs.knots()[degree],
                    nurbs.knots()[nurbs.control_points().len()],
                ] != range
                {
                    let range = normalized_range(lo, hi).ok_or_else(|| {
                        WriteError::Omit(format!("wire edge {} has no increasing range", edge.id))
                    })?;
                    let converted = super::conics::clamp_curve(curve.id.as_str(), nurbs, range)?;
                    conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                        "wire edge {} carrier {} was restricted to the exact edge span; its unused curve domain is omitted",
                        edge.id, curve.id
                    )));
                    WritableEdgeCurve::NurbsOwned(converted)
                } else {
                    match check_nurbs_curve(curve.id.as_str(), nurbs) {
                        Ok(()) => WritableEdgeCurve::Nurbs(nurbs),
                        Err(WriteError::Omit(_)) => {
                            let range = normalized_range(lo, hi).ok_or_else(|| {
                                WriteError::Omit(format!(
                                    "wire edge {} has no increasing range",
                                    edge.id
                                ))
                            })?;
                            let converted =
                                super::conics::clamp_curve(curve.id.as_str(), nurbs, range)?;
                            conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                                "wire edge {} carrier {} was clamped to its exact span",
                                edge.id, curve.id
                            )));
                            WritableEdgeCurve::NurbsOwned(converted)
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Some(SolvedCurveGeometry::Polyline(polyline)) => {
            let range = normalized_range(lo, hi).ok_or_else(|| {
                WriteError::Omit(format!("wire edge {} has no increasing range", edge.id))
            })?;
            let converted = super::polyline::convert(
                curve.id.as_str(),
                polyline,
                Some(cadmpeg_ir::scalar::FiniteReal::raw_array(range)),
            )?;
            conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                "wire edge {} polyline carrier {} was converted to an exact degree-one NURBS; its chordal deflection is omitted",
                edge.id, curve.id
            )));
            WritableEdgeCurve::NurbsOwned(converted)
        }
        Some(SolvedCurveGeometry::Ellipse(ellipse)) => {
            let Some(range) = normalized_range(lo, hi) else {
                return Err(WriteError::Omit(format!(
                    "wire edge {} ellipse carrier has no finite increasing range",
                    edge.id
                )));
            };
            let converted = super::conics::ellipse_over_range(curve.id.as_str(), ellipse, range)?;
            conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                "wire edge {} ellipse carrier {} was converted to its exact NURBS form",
                edge.id, curve.id
            )));
            WritableEdgeCurve::NurbsOwned(converted)
        }
        Some(SolvedCurveGeometry::Parabola(parabola)) => {
            let Some(range) = normalized_range(lo, hi) else {
                return Err(WriteError::Omit(format!(
                    "wire edge {} parabola carrier has no finite increasing range",
                    edge.id
                )));
            };
            let converted = super::conics::parabola_over_range(curve.id.as_str(), parabola, range)?;
            conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                "wire edge {} parabola carrier {} was converted to its exact NURBS form",
                edge.id, curve.id
            )));
            WritableEdgeCurve::NurbsOwned(converted)
        }
        Some(SolvedCurveGeometry::Hyperbola(hyperbola)) => {
            let Some(range) = normalized_range(lo, hi) else {
                return Err(WriteError::Omit(format!(
                    "wire edge {} hyperbola carrier has no finite increasing range",
                    edge.id
                )));
            };
            let converted =
                super::conics::hyperbola_over_range(curve.id.as_str(), hyperbola, range)?;
            conversion = Some(RhinoLossCode::WriterCarrierResolved.note(format!(
                "wire edge {} hyperbola carrier {} was converted to its exact NURBS form",
                edge.id, curve.id
            )));
            WritableEdgeCurve::NurbsOwned(converted)
        }
        _ => {
            return Err(WriteError::Omit(format!(
                "wire carrier curve {} has no supported exact standalone encoding",
                curve.id.as_str()
            )));
        }
    };
    let mut endpoints = Vec::new();
    for (id, parameter) in [&edge.start, &edge.end].into_iter().zip(domain) {
        let Some(vertex) = ir.model.vertices.iter().find(|vertex| vertex.id == *id) else {
            return Err(WriteError::Omit(format!(
                "dangling vertex reference for wire edge {}",
                edge.id
            )));
        };
        let Some(point) = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
        else {
            return Err(WriteError::Omit(format!(
                "dangling point reference for wire edge {}",
                edge.id
            )));
        };
        let expected = geometry
            .point(parameter)
            .map_err(|failure| match failure {
                cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit) => {
                    WriteError::Encode(limit.into())
                }
                _ => WriteError::Omit(format!(
                    "wire edge {} endpoint evaluation has no supported finite result",
                    edge.id,
                )),
            })?
            .get();
        let tolerance = edge.tolerance.or(vertex.tolerance).map_or(
            ir.tolerances.linear_or_default().get(),
            cadmpeg_ir::scalar::PositiveReal::get,
        );
        if !close_point(expected, point.position().get(), tolerance) {
            return Err(WriteError::Omit(format!(
                "wire edge {} carrier disagrees with its endpoint",
                edge.id,
            )));
        }
        endpoints.push(expected);
    }
    let payload = match geometry {
        WritableEdgeCurve::Singular => {
            return Err(WriteError::Omit(format!(
                "wire edge {} has no curve",
                edge.id
            )))
        }
        WritableEdgeCurve::Line(_) => {
            if lo > hi {
                endpoints.reverse();
            }
            let from = endpoints[0];
            let to = endpoints[1];
            (
                LINE_CLASS,
                bounded_line_payload(
                    [from.x, from.y, from.z],
                    [to.x, to.y, to.z],
                    [lo.min(hi), lo.max(hi)],
                    3,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)?),
        WritableEdgeCurve::NurbsOwned(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(&nurbs)?),
        WritableEdgeCurve::Circle(circle) => {
            let center = circle.center().get();
            let axis = circle.frame().axis().as_raw();
            let ref_direction = circle.frame().reference().as_raw();
            let radius = circle.radius().get();
            (
                ARC_CLASS,
                circle_payload(center, *axis, *ref_direction, radius, [lo, hi], [lo, hi], 3)?,
            )
        }
    };
    Ok((payload, conversion))
}
