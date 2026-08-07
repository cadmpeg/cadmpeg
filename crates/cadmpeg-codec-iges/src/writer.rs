// SPDX-License-Identifier: Apache-2.0
//! Bounded IGES Fixed ASCII writing.
//!
//! The writer has two deliberately separate paths. An unchanged decode with a
//! verified document baseline replays its retained source image byte for byte.
//! Otherwise the semantic writer emits the current supported neutral profile
//! and refuses a model or native record it cannot represent. A caller never
//! receives a plausible file after an unsupported value was silently dropped.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan};
use cadmpeg_ir::eval::curve_point;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{
    CensusBasis, EntityCensus, ExportReport, FidelityResolution, LossKind, LossNote, Severity,
    WritePath,
};
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::{CadIr, SourceFidelity};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

const ALLOWED_NATIVE_ARENAS: &[&str] = &[
    "cards",
    "copious_data",
    "display_attributes",
    "entities",
    "product_occurrence_expansion",
    "transformations",
];

/// Plan an IGES export, selecting replay only after checking the document
/// baseline and retained source-image integrity.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    options: crate::IgesWriteOptions,
) -> Result<ExportPlan<'_>, CodecError> {
    if let Some(bytes) = replay_bytes(input.ir, input.fidelity, options.version)? {
        return Ok(ExportPlan::buffered(
            report(
                FidelityResolution::Replayed,
                WritePath::VerbatimReplay,
                Vec::new(),
                "preserved source container replayed verbatim",
                counts_for_ir(input.ir),
            ),
            bytes,
        ));
    }

    let source_expected = input
        .ir
        .source
        .as_ref()
        .is_some_and(|source| source.format == "iges");
    let source_available = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some();
    let mut losses = Vec::new();
    if source_expected && !source_available {
        losses.push(
            LossNote::new(
                LossKind::PreservedSourceUnavailable,
                "preserved IGES source image is unavailable; semantic regeneration is required",
            )
            .with_severity(Severity::Blocking),
        );
    }
    let synthesis = synthesize(input.ir, options.version)?;
    losses.extend(synthesis.losses.clone());
    let fidelity = if source_expected && !source_available {
        FidelityResolution::Degraded {
            reason: "preserved IGES source image is unavailable".into(),
        }
    } else if input.fidelity.is_some() {
        FidelityResolution::NotConsumed
    } else {
        FidelityResolution::NotProvided
    };
    Ok(ExportPlan::buffered(
        report(
            fidelity,
            WritePath::Synthesized,
            losses,
            "IGES Fixed ASCII container regenerated from supported neutral geometry",
            synthesis.counts,
        ),
        synthesis.bytes,
    ))
}

fn replay_bytes(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    version: crate::IgesVersion,
) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(expected) = ir
        .source
        .as_ref()
        .filter(|source| source.format == "iges")
        .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
    else {
        return Ok(None);
    };
    if ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("iges_version"))
        .is_none_or(|source_version| source_version != version.name())
    {
        return Ok(None);
    }
    if crate::document_digest(ir) != *expected {
        return Ok(None);
    }
    let Some(record) = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID))
    else {
        return Ok(None);
    };
    let Some(data) = record.data.as_deref() else {
        return Err(CodecError::Malformed(
            "retained IGES source image has no bytes".into(),
        ));
    };
    if record.byte_len != data.len() as u64 || record.sha256 != sha256_hex(data) {
        return Err(CodecError::Malformed(
            "retained IGES source image failed integrity validation".into(),
        ));
    }
    Ok(Some(data.to_vec()))
}

fn report(
    fidelity: FidelityResolution,
    write_path: WritePath,
    losses: Vec<LossNote>,
    note: &str,
    counts: BTreeMap<String, usize>,
) -> ExportReport {
    ExportReport {
        format: "iges".into(),
        census: EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts,
        },
        fidelity,
        write_path,
        losses,
        notes: vec![note.into()],
    }
}

fn counts_for_ir(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if let Some(namespace) = ir.native.namespace("iges") {
        if let Some(records) = namespace.arenas.get("entities") {
            for record in records {
                if let Some(entity_type) =
                    record.field("entity_type").and_then(|value| value.as_i64())
                {
                    *counts.entry(format!("{entity_type}_entity")).or_insert(0) += 1;
                }
            }
        }
    }
    if counts.is_empty() {
        counts.insert("116_point".into(), ir.model.points.len());
    }
    counts
}

struct Synthesis {
    bytes: Vec<u8>,
    counts: BTreeMap<String, usize>,
    losses: Vec<LossNote>,
}

fn synthesize(ir: &CadIr, version: crate::IgesVersion) -> Result<Synthesis, CodecError> {
    reject_unsupported_model(ir)?;
    let losses = reject_unsupported_native(ir)?;

    let mut entities = Vec::new();
    let mut consumed_points = std::collections::BTreeSet::new();
    let mut consumed_curves = std::collections::BTreeSet::new();
    let mut edges = ir.model.edges.iter().collect::<Vec<_>>();
    edges.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for edge in edges {
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES semantic writer does not encode carrier-less edge {}",
                edge.id
            ))
        })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES edge {} references missing curve {}",
                    edge.id, curve_id
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        let span = edge_span(ir, edge, &geometry)?;
        entities.push(curve_entity(&geometry, Some(&span))?);
        consumed_curves.insert(curve_id.clone());
        consumed_points.insert(vertex_point_id(ir, &edge.start)?);
        consumed_points.insert(vertex_point_id(ir, &edge.end)?);
    }

    let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
    curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for curve in curves {
        if consumed_curves.contains(&curve.id) {
            continue;
        }
        let geometry = flatten_curve(&curve.geometry)?;
        entities.push(curve_entity(&geometry, None)?);
    }

    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for point in points {
        if consumed_points.contains(&point.id) {
            continue;
        }
        ensure_finite_point(point.position, &point.id.0)?;
        entities.push(point_entity(point.position));
    }

    let counts = entity_counts(&entities);
    Ok(Synthesis {
        bytes: encode_file(&entities, version)?,
        counts,
        losses,
    })
}

fn entity_counts(entities: &[Entity]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entity in entities {
        let name = match entity.type_code {
            100 => "100_circular_arc",
            104 => "104_conic_arc",
            110 => "110_line",
            116 => "116_point",
            126 => "126_nurbs_curve",
            124 => "124_transformation",
            106 => "106_copious_data",
            102 => "102_composite_curve",
            _ => "unknown_entity",
        };
        *counts.entry(name.into()).or_insert(0) += 1;
    }
    counts
}

fn reject_unsupported_model(ir: &CadIr) -> Result<(), CodecError> {
    let unsupported = [
        ("faces", !ir.model.faces.is_empty()),
        ("loops", !ir.model.loops.is_empty()),
        ("coedges", !ir.model.coedges.is_empty()),
        ("surfaces", !ir.model.surfaces.is_empty()),
        ("pcurves", !ir.model.pcurves.is_empty()),
        (
            "procedural_surfaces",
            !ir.model.procedural_surfaces.is_empty(),
        ),
        ("procedural_curves", !ir.model.procedural_curves.is_empty()),
        ("subds", !ir.model.subds.is_empty()),
        ("assets", !ir.model.assets.is_empty()),
        ("features", !ir.model.features.is_empty()),
        (
            "feature_input_topologies",
            !ir.model.feature_input_topologies.is_empty(),
        ),
        (
            "feature_result_topologies",
            !ir.model.feature_result_topologies.is_empty(),
        ),
        ("configurations", !ir.model.configurations.is_empty()),
        ("parameters", !ir.model.parameters.is_empty()),
        ("sketches", !ir.model.sketches.is_empty()),
        ("sketch_entities", !ir.model.sketch_entities.is_empty()),
        (
            "sketch_constraints",
            !ir.model.sketch_constraints.is_empty(),
        ),
        ("spatial_sketches", !ir.model.spatial_sketches.is_empty()),
        (
            "spatial_sketch_entities",
            !ir.model.spatial_sketch_entities.is_empty(),
        ),
        (
            "spatial_sketch_constraints",
            !ir.model.spatial_sketch_constraints.is_empty(),
        ),
        ("spreadsheets", !ir.model.spreadsheets.is_empty()),
        (
            "product_definitions",
            !ir.model.product_definitions.is_empty(),
        ),
        ("occurrences", !ir.model.occurrences.is_empty()),
        ("assembly_joints", !ir.model.assembly_joints.is_empty()),
        ("drawings", !ir.model.drawings.is_empty()),
        (
            "semantic_annotations",
            !ir.model.semantic_annotations.is_empty(),
        ),
        (
            "presentation_documents",
            !ir.model.presentation_documents.is_empty(),
        ),
        (
            "view_presentations",
            !ir.model.view_presentations.is_empty(),
        ),
        ("tessellations", !ir.model.tessellations.is_empty()),
        ("appearances", !ir.model.appearances.is_empty()),
        (
            "appearance_bindings",
            !ir.model.appearance_bindings.is_empty(),
        ),
        ("attributes", !ir.model.attributes.is_empty()),
        ("pmi", !ir.model.pmi.is_empty()),
        (
            "presentation_layers",
            !ir.model.presentation_layers.is_empty(),
        ),
    ];
    if let Some((arena, _)) = unsupported.into_iter().find(|(_, present)| *present) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode model arena {arena}"
        )));
    }
    for body in &ir.model.bodies {
        if body
            .transform
            .is_some_and(|transform| transform != cadmpeg_ir::transform::Transform::identity())
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer does not apply body transform {}",
                body.id
            )));
        }
    }
    for vertex in &ir.model.vertices {
        if !ir.model.points.iter().any(|point| point.id == vertex.point) {
            return Err(CodecError::Malformed(format!(
                "IGES vertex {} references missing point {}",
                vertex.id, vertex.point
            )));
        }
    }
    Ok(())
}

fn reject_unsupported_native(ir: &CadIr) -> Result<Vec<LossNote>, CodecError> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return Ok(Vec::new());
    };
    if let Some((arena, _)) = namespace.arenas.iter().find(|(arena, records)| {
        !records.is_empty() && !ALLOWED_NATIVE_ARENAS.contains(&arena.as_str())
    }) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer cannot preserve native arena {arena}"
        )));
    }
    if let Some(record) = namespace
        .arenas
        .get("entities")
        .into_iter()
        .flatten()
        .find(|record| {
            !matches!(
                record.field("entity_type").and_then(|value| value.as_i64()),
                Some(100 | 102 | 104 | 106 | 110 | 116 | 124 | 126)
            )
        })
    {
        let entity_type = record
            .field("entity_type")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode native entity type {entity_type}"
        )));
    }
    let mut losses = Vec::new();
    for (arena, records) in &namespace.arenas {
        if records.is_empty() {
            continue;
        }
        let message = match arena.as_str() {
            "display_attributes" => {
                "IGES display attributes are not regenerated by the bounded semantic writer"
            }
            "product_occurrence_expansion" => {
                "IGES product occurrence expansion is not regenerated by the bounded semantic writer"
            }
            _ => continue,
        };
        losses.push(
            LossNote::new(LossKind::PassthroughRecordOmitted, message)
                .with_severity(Severity::Warning),
        );
    }
    Ok(losses)
}

#[derive(Clone, Copy)]
struct Placement {
    rows: [[f64; 4]; 3],
}

struct CurveSpan {
    range: [f64; 2],
    start: Point3,
    end: Point3,
}

fn point_entity(position: Point3) -> Entity {
    Entity {
        type_code: 116,
        form: 0,
        label: "POINT",
        parameters: format!(
            "116,{},{},{};",
            number(position.x),
            number(position.y),
            number(position.z)
        )
        .into_bytes(),
        transform: None,
    }
}

fn vertex_point_id(ir: &CadIr, vertex_id: &VertexId) -> Result<PointId, CodecError> {
    let vertex = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)
        .ok_or_else(|| {
            CodecError::Malformed(format!("IGES edge references missing vertex {vertex_id}"))
        })?;
    Ok(vertex.point.clone())
}

fn point_position(ir: &CadIr, point_id: &PointId) -> Result<Point3, CodecError> {
    ir.model
        .points
        .iter()
        .find(|point| point.id == *point_id)
        .map(|point| point.position)
        .ok_or_else(|| {
            CodecError::Malformed(format!("IGES topology references missing point {point_id}"))
        })
}

fn edge_span(ir: &CadIr, edge: &Edge, geometry: &CurveGeometry) -> Result<CurveSpan, CodecError> {
    let range = edge.param_range.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for edge {}",
            edge.id
        ))
    })?;
    if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
        return Err(CodecError::Malformed(format!(
            "IGES edge {} has an invalid parameter range",
            edge.id
        )));
    }
    let start = point_position(ir, &vertex_point_id(ir, &edge.start)?)?;
    let end = point_position(ir, &vertex_point_id(ir, &edge.end)?)?;
    ensure_finite_point(start, &format!("edge {} start", edge.id))?;
    ensure_finite_point(end, &format!("edge {} end", edge.id))?;
    if matches!(
        geometry,
        CurveGeometry::Circle { .. }
            | CurveGeometry::Ellipse { .. }
            | CurveGeometry::Parabola { .. }
            | CurveGeometry::Hyperbola { .. }
            | CurveGeometry::Nurbs(_)
            | CurveGeometry::Polyline { .. }
    ) {
        let evaluated_start = curve_point(geometry, range[0]).ok_or_else(|| {
            CodecError::Malformed(format!(
                "IGES edge {} start cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        let evaluated_end = curve_point(geometry, range[1]).ok_or_else(|| {
            CodecError::Malformed(format!(
                "IGES edge {} end cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        if !close_point(start, evaluated_start) || !close_point(end, evaluated_end) {
            return Err(CodecError::Malformed(format!(
                "IGES edge {} endpoints disagree with its curve parameter range",
                edge.id
            )));
        }
    }
    Ok(CurveSpan { range, start, end })
}

fn default_range(geometry: &CurveGeometry) -> Result<[f64; 2], CodecError> {
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => Ok([0.0, TAU]),
        CurveGeometry::Nurbs(nurbs) => nurbs_domain(nurbs),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            if points.len() < 2 {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer requires at least two polyline points".into(),
                ));
            }
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            Ok([values[0], *values.last().expect("polyline has points")])
        }
        CurveGeometry::Line { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        CurveGeometry::Transformed { .. } => Err(CodecError::Malformed(
            "IGES transformed curve was not flattened before encoding".into(),
        )),
    }
}

fn curve_entity(geometry: &CurveGeometry, span: Option<&CurveSpan>) -> Result<Entity, CodecError> {
    let range = span.map_or_else(|| default_range(geometry), |span| Ok(span.range))?;
    if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
        return Err(CodecError::Malformed(
            "IGES curve parameter range is invalid".into(),
        ));
    }
    match geometry {
        CurveGeometry::Line { .. } => {
            let span = span.ok_or_else(|| {
                CodecError::NotImplemented(
                    "IGES semantic writer cannot bound an unreferenced line".into(),
                )
            })?;
            Ok(Entity {
                type_code: 110,
                form: 0,
                label: "LINE",
                parameters: format!(
                    "110,{},{},{},{},{},{};",
                    number(span.start.x),
                    number(span.start.y),
                    number(span.start.z),
                    number(span.end.x),
                    number(span.end.y),
                    number(span.end.z)
                )
                .into_bytes(),
                transform: None,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let axis = unit(*axis, "circle axis")?;
            let reference = unit(*ref_direction, "circle reference direction")?;
            if axis.dot(reference).abs() > 1.0e-10 || !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES circle basis is not an orthonormal positive-radius frame".into(),
                ));
            }
            let y_axis = axis.cross(reference);
            let end = effective_arc_end(range, true)?;
            let start_xy = [radius * range[0].cos(), radius * range[0].sin()];
            let end_xy = [radius * end.cos(), radius * end.sin()];
            Ok(Entity {
                type_code: 100,
                form: 0,
                label: "ARC",
                parameters: format!(
                    "100,0,0,0,{},{},{},{};",
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, reference, y_axis, axis)?),
            })
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let axis = unit(*axis, "ellipse axis")?;
            let major = unit(*major_direction, "ellipse major direction")?;
            if axis.dot(major).abs() > 1.0e-10
                || !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
            {
                return Err(CodecError::Malformed(
                    "IGES ellipse basis or radii are invalid".into(),
                ));
            }
            let y_axis = axis.cross(major);
            let end = effective_arc_end(range, true)?;
            let start_xy = [major_radius * range[0].cos(), minor_radius * range[0].sin()];
            let end_xy = [major_radius * end.cos(), minor_radius * end.sin()];
            Ok(Entity {
                type_code: 104,
                form: 0,
                label: "CONIC",
                parameters: format!(
                    "104,{},0,{},0,0,-1,0,{},{},{},{};",
                    number(1.0 / (major_radius * major_radius)),
                    number(1.0 / (minor_radius * minor_radius)),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, major, y_axis, axis)?),
            })
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            if range[0] == range[1] || !focal_distance.is_finite() || *focal_distance <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES parabola requires a finite non-zero parameter span".into(),
                ));
            }
            let axis = unit(*axis, "parabola axis")?;
            let major = unit(*major_direction, "parabola major direction")?;
            if axis.dot(major).abs() > 1.0e-10 {
                return Err(CodecError::Malformed(
                    "IGES parabola basis is not orthogonal".into(),
                ));
            }
            let x_axis = major.cross(axis);
            let start_xy = parabola_point(*focal_distance, range[0])?;
            let end_xy = parabola_point(*focal_distance, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 3,
                label: "CONIC",
                parameters: format!(
                    "104,1,0,0,0,{},0,0,{},{},{},{};",
                    number(-4.0 * focal_distance),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*vertex, x_axis, major, axis)?),
            })
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            if range[0] == range[1]
                || !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
            {
                return Err(CodecError::Malformed(
                    "IGES hyperbola requires positive radii and a finite span".into(),
                ));
            }
            let axis = unit(*axis, "hyperbola axis")?;
            let major = unit(*major_direction, "hyperbola major direction")?;
            if axis.dot(major).abs() > 1.0e-10 {
                return Err(CodecError::Malformed(
                    "IGES hyperbola basis is not orthogonal".into(),
                ));
            }
            let y_axis = axis.cross(major);
            let start_xy = hyperbola_point(*major_radius, *minor_radius, range[0])?;
            let end_xy = hyperbola_point(*major_radius, *minor_radius, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 2,
                label: "CONIC",
                parameters: format!(
                    "104,{},0,{},0,0,-1,0,{},{},{},{};",
                    number(1.0 / (major_radius * major_radius)),
                    number(-1.0 / (minor_radius * minor_radius)),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, major, y_axis, axis)?),
            })
        }
        CurveGeometry::Nurbs(nurbs) => encode_nurbs(nurbs, range, "NURBS"),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            let nurbs = NurbsCurve {
                degree: 1,
                knots: polyline_knots(&values),
                control_points: points.clone(),
                weights: None,
                periodic: false,
            };
            encode_nurbs(&nurbs, range, "POLYLINE")
        }
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. }
        | CurveGeometry::Transformed { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
    }
}

fn encode_nurbs(
    nurbs: &NurbsCurve,
    range: [f64; 2],
    label: &'static str,
) -> Result<Entity, CodecError> {
    let control_count = nurbs.control_points.len();
    let degree = usize::try_from(nurbs.degree)
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    if control_count == 0
        || degree == 0
        || degree >= control_count
        || nurbs.knots.len() != control_count + degree + 1
        || range[0] > range[1]
        || range.iter().any(|value| !value.is_finite())
        || nurbs.knots.iter().any(|value| !value.is_finite())
        || nurbs.knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err(CodecError::Malformed(
            "IGES NURBS degree, knot vector, or parameter range is invalid".into(),
        ));
    }
    let domain = [nurbs.knots[degree], nurbs.knots[control_count]];
    if range[0] < domain[0] || range[1] > domain[1] {
        return Err(CodecError::Malformed(
            "IGES NURBS parameter range lies outside its knot domain".into(),
        ));
    }
    if nurbs
        .control_points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return Err(CodecError::Malformed(
            "IGES NURBS control point is non-finite".into(),
        ));
    }
    let weights = match &nurbs.weights {
        Some(weights) if weights.len() == control_count => {
            if weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
            {
                return Err(CodecError::Malformed(
                    "IGES NURBS weights must be finite and positive".into(),
                ));
            }
            weights.clone()
        }
        Some(_) => {
            return Err(CodecError::Malformed(
                "IGES NURBS weight count does not match control points".into(),
            ));
        }
        None => vec![1.0; control_count],
    };
    let polynomial = nurbs.weights.is_none();
    let k = control_count - 1;
    let mut parameters = format!(
        "126,{k},{},{},{},{},{}",
        nurbs.degree,
        1,
        0,
        i32::from(polynomial),
        i32::from(nurbs.periodic)
    );
    for value in &nurbs.knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for weight in weights {
        parameters.push(',');
        parameters.push_str(&number(weight));
    }
    for point in &nurbs.control_points {
        for value in [point.x, point.y, point.z] {
            parameters.push(',');
            parameters.push_str(&number(value));
        }
    }
    parameters.push(',');
    parameters.push_str(&number(range[0]));
    parameters.push(',');
    parameters.push_str(&number(range[1]));
    parameters.push(';');
    Ok(Entity {
        type_code: 126,
        form: 0,
        label,
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

fn flatten_curve(geometry: &CurveGeometry) -> Result<CurveGeometry, CodecError> {
    match geometry {
        CurveGeometry::Transformed { basis, transform } => {
            if !transform.is_proper_rigid() {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer only applies proper-rigid curve transforms".into(),
                ));
            }
            let basis = flatten_curve(basis)?;
            apply_rigid_transform(basis, *transform)
        }
        _ => Ok(geometry.clone()),
    }
}

fn apply_rigid_transform(
    geometry: CurveGeometry,
    transform: cadmpeg_ir::transform::Transform,
) -> Result<CurveGeometry, CodecError> {
    let point = |value: Point3| transform.apply_point(value);
    let vector = |value: Vector3, label: &str| unit(transform.apply_vector(value), label);
    Ok(match geometry {
        CurveGeometry::Line { origin, direction } => CurveGeometry::Line {
            origin: point(origin),
            direction: vector(direction, "transformed line direction")?,
        },
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => CurveGeometry::Circle {
            center: point(center),
            axis: vector(axis, "transformed circle axis")?,
            ref_direction: vector(ref_direction, "transformed circle reference")?,
            radius,
        },
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Ellipse {
            center: point(center),
            axis: vector(axis, "transformed ellipse axis")?,
            major_direction: vector(major_direction, "transformed ellipse major")?,
            major_radius,
            minor_radius,
        },
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => CurveGeometry::Parabola {
            vertex: point(vertex),
            axis: vector(axis, "transformed parabola axis")?,
            major_direction: vector(major_direction, "transformed parabola major")?,
            focal_distance,
        },
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Hyperbola {
            center: point(center),
            axis: vector(axis, "transformed hyperbola axis")?,
            major_direction: vector(major_direction, "transformed hyperbola major")?,
            major_radius,
            minor_radius,
        },
        CurveGeometry::Degenerate { point: value } => CurveGeometry::Degenerate {
            point: point(value),
        },
        CurveGeometry::Nurbs(mut nurbs) => {
            nurbs.control_points = nurbs.control_points.into_iter().map(point).collect();
            CurveGeometry::Nurbs(nurbs)
        }
        CurveGeometry::Polyline {
            points,
            parameters,
            chordal_deflection,
        } => CurveGeometry::Polyline {
            points: points.into_iter().map(point).collect(),
            parameters,
            chordal_deflection,
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot flatten curve geometry {other:?}"
            )))
        }
    })
}

fn unit(vector: Vector3, label: &str) -> Result<Vector3, CodecError> {
    let norm = vector.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return Err(CodecError::Malformed(format!("IGES {label} is degenerate")));
    }
    Ok(vector.scale(1.0 / norm))
}

fn placement(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> Result<Placement, CodecError> {
    ensure_finite_point(origin, "placement origin")?;
    let x_axis = unit(x_axis, "placement x axis")?;
    let y_axis = unit(y_axis, "placement y axis")?;
    let z_axis = unit(z_axis, "placement z axis")?;
    if x_axis.dot(y_axis).abs() > 1.0e-10
        || x_axis.dot(z_axis).abs() > 1.0e-10
        || y_axis.dot(z_axis).abs() > 1.0e-10
        || x_axis.cross(y_axis).dot(z_axis) < 1.0 - 1.0e-10
    {
        return Err(CodecError::Malformed(
            "IGES placement axes are not a right-handed orthonormal frame".into(),
        ));
    }
    Ok(Placement {
        rows: [
            [x_axis.x, y_axis.x, z_axis.x, origin.x],
            [x_axis.y, y_axis.y, z_axis.y, origin.y],
            [x_axis.z, y_axis.z, z_axis.z, origin.z],
        ],
    })
}

fn effective_arc_end(range: [f64; 2], closed: bool) -> Result<f64, CodecError> {
    let sweep = range[1] - range[0];
    if !(0.0..=TAU + 1.0e-10).contains(&sweep) {
        return Err(CodecError::NotImplemented(
            "IGES conic writer requires an ordered span no larger than one revolution".into(),
        ));
    }
    if closed && sweep <= 1.0e-14 {
        Ok(range[0] + TAU)
    } else {
        Ok(range[1])
    }
}

fn parabola_point(focal_distance: f64, parameter: f64) -> Result<[f64; 2], CodecError> {
    let point = [
        -2.0 * focal_distance * parameter,
        focal_distance * parameter * parameter,
    ];
    point
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
        .ok_or_else(|| CodecError::Malformed("IGES parabola endpoint is non-finite".into()))
}

fn hyperbola_point(
    major_radius: f64,
    minor_radius: f64,
    parameter: f64,
) -> Result<[f64; 2], CodecError> {
    let point = [
        major_radius * parameter.cosh(),
        minor_radius * parameter.sinh(),
    ];
    point
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
        .ok_or_else(|| CodecError::Malformed("IGES hyperbola endpoint is non-finite".into()))
}

fn nurbs_domain(nurbs: &NurbsCurve) -> Result<[f64; 2], CodecError> {
    let degree = usize::try_from(nurbs.degree)
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    let end = nurbs.control_points.len();
    if nurbs.knots.len() <= end || degree >= nurbs.knots.len() {
        return Err(CodecError::Malformed(
            "IGES NURBS knot vector cannot provide a domain".into(),
        ));
    }
    Ok([nurbs.knots[degree], nurbs.knots[end]])
}

fn polyline_parameters(count: usize, parameters: Option<&[f64]>) -> Result<Vec<f64>, CodecError> {
    if count < 2 {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires at least two polyline points".into(),
        ));
    }
    let values = parameters.map_or_else(
        || (0..count).map(|value| value as f64).collect(),
        <[f64]>::to_vec,
    );
    if values.len() != count
        || values.iter().any(|value| !value.is_finite())
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(CodecError::Malformed(
            "IGES polyline parameters must be finite and strictly increasing".into(),
        ));
    }
    Ok(values)
}

fn polyline_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.extend([parameters[0], parameters[0]]);
    knots.extend_from_slice(&parameters[1..parameters.len() - 1]);
    knots.extend([*parameters.last().expect("polyline has points"); 2]);
    knots
}

fn close_point(left: Point3, right: Point3) -> bool {
    let scale = left
        .x
        .abs()
        .max(left.y.abs())
        .max(left.z.abs())
        .max(right.x.abs())
        .max(right.y.abs())
        .max(right.z.abs())
        .max(1.0);
    let tolerance = scale * 1.0e-8;
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn ensure_finite_point(point: Point3, label: &str) -> Result<(), CodecError> {
    if [point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
    {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "IGES point {label} has non-finite coordinates"
        )))
    }
}

#[derive(Clone)]
struct Entity {
    type_code: u32,
    form: i64,
    label: &'static str,
    parameters: Vec<u8>,
    transform: Option<Placement>,
}

fn encode_file(entities: &[Entity], version: crate::IgesVersion) -> Result<Vec<u8>, CodecError> {
    let global = format!(
        "1H,,1H;,7Hcadmpeg,13Hgenerated.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260807.000000,0.001,1000.0,6Hauthor,7Hcadmpeg,{},0,0H,0H;",
        version.global_flag()
    )
    .into_bytes();
    let global_count = global.len().div_ceil(72);
    let mut expanded = Vec::with_capacity(entities.len() * 2);
    for entity in entities {
        if let Some(placement) = entity.transform {
            let transform_parameters = placement
                .rows
                .iter()
                .flatten()
                .map(|value| number(*value))
                .collect::<Vec<_>>()
                .join(",");
            expanded.push((
                Entity {
                    type_code: 124,
                    form: 0,
                    label: "XFORM",
                    parameters: format!("124,{transform_parameters};").into_bytes(),
                    transform: None,
                },
                0_u32,
            ));
            let transform_sequence = u32::try_from(expanded.len())
                .ok()
                .and_then(|value| value.checked_mul(2).and_then(|value| value.checked_sub(1)))
                .ok_or_else(|| {
                    CodecError::Malformed("IGES transformation sequence overflows".into())
                })?;
            let mut entity = entity.clone();
            entity.transform = None;
            expanded.push((entity, transform_sequence));
        } else {
            expanded.push((entity.clone(), 0));
        }
    }
    let mut parameter_sequence = 1_u32;
    let mut directory = Vec::with_capacity(expanded.len() * 2);
    let mut parameters = Vec::new();
    for (index, (entity, transform_sequence)) in expanded.iter().enumerate() {
        let directory_sequence = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::Malformed("IGES directory sequence overflows".into()))?;
        let parameter_count = entity.parameters.len().div_ceil(64);
        let parameter_count = u32::try_from(parameter_count)
            .map_err(|_| CodecError::Malformed("IGES parameter count overflows".into()))?;
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                parameter_sequence.to_string(),
                "0".into(),
                "0".into(),
                "0".into(),
                "0".into(),
                transform_sequence.to_string(),
                "0".into(),
                "00000000".into(),
            ],
            directory_sequence,
        )?);
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                "0".into(),
                "0".into(),
                parameter_count.to_string(),
                entity.form.to_string(),
                String::new(),
                String::new(),
                entity.label.to_owned(),
                "0".into(),
            ],
            directory_sequence + 1,
        )?);
        for chunk in entity.parameters.chunks(64) {
            parameters.push(parameter_card(
                chunk,
                directory_sequence,
                parameter_sequence,
            )?);
            parameter_sequence = parameter_sequence
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("IGES parameter sequence overflows".into()))?;
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(card(b"Generated by cadmpeg", b'S', 1)?);
    for (index, chunk) in global.chunks(72).enumerate() {
        bytes.extend(card(
            chunk,
            b'G',
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        )?);
    }
    for card_bytes in directory {
        bytes.extend(card_bytes);
    }
    for card_bytes in parameters {
        bytes.extend(card_bytes);
    }
    let directory_count = expanded
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::Malformed("IGES directory count overflows".into()))?;
    let directory_count = u32::try_from(directory_count)
        .map_err(|_| CodecError::Malformed("IGES directory count overflows".into()))?;
    let parameter_count = parameter_sequence - 1;
    let terminate = format!(
        "S{start_count:07}G{global_count:07}D{directory_count:07}P{parameter_count:07}",
        start_count = 1
    );
    bytes.extend(card(terminate.as_bytes(), b'T', 1)?);
    Ok(bytes)
}

fn directory_card(fields: [String; 9], sequence: u32) -> Result<Vec<u8>, CodecError> {
    let mut payload = Vec::with_capacity(72);
    for field in fields {
        if field.len() > 8 {
            return Err(CodecError::Malformed(format!(
                "IGES Directory field is wider than eight bytes: {field}"
            )));
        }
        payload.extend_from_slice(format!("{field:>8}").as_bytes());
    }
    card(&payload, b'D', sequence)
}

fn parameter_card(
    data: &[u8],
    directory_sequence: u32,
    sequence: u32,
) -> Result<Vec<u8>, CodecError> {
    if data.len() > 64 {
        return Err(CodecError::Malformed(
            "IGES Parameter Data payload exceeds 64 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    let pointer = format!("{directory_sequence:>8}");
    payload[64..].copy_from_slice(pointer.as_bytes());
    card(&payload, b'P', sequence)
}

fn card(data: &[u8], section: u8, sequence: u32) -> Result<Vec<u8>, CodecError> {
    let width = 72;
    if data.len() > width {
        return Err(CodecError::Malformed(
            "IGES card payload exceeds 72 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 80];
    payload[..data.len()].copy_from_slice(data);
    payload[72] = section;
    let sequence = format!("{sequence:>7}");
    payload[73..].copy_from_slice(sequence.as_bytes());
    payload.push(b'\n');
    Ok(payload)
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.17}")
    }
}
