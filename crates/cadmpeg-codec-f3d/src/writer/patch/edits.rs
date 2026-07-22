// SPDX-License-Identifier: Apache-2.0
//! Edit validators that diff the target against the baseline and build edit sets.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::history_records::{AsmBulletinBoard, AsmDeltaState, AsmEntityChange};
use crate::records::{
    ActEntity, ActGuid, ActRootComponent, DesignMaterialAssignment, LostEdgeReference,
    SketchCurveGeometry,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    BlendRadiusLaw, Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry,
    ProceduralCurve, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face, Sense};
use cadmpeg_ir::transform::Transform;

use super::geometry::{
    orthonormal_pair, valid_edited_curve_structure, valid_edited_nurbs_direction,
};
use super::records::{canonical_guid, native_stream};
use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::{
    f3d_native, finite_point, finite_vector, history_change_kind, normalized_face_sense_to_native,
};

pub(crate) type SketchPointEdit = (u64, u32, cadmpeg_ir::math::Point2);
pub(crate) type PersistentReferenceEdit = (u64, u32, u64);
pub(crate) type BodyMemberEdit = (u64, u64, u16);
pub(crate) type ActGuidEdit = (u64, Vec<u8>);
pub(crate) type SketchCurveEdit = (u64, u32, SketchCurveGeometry);
pub(crate) type SketchRelationEdit = Vec<(u64, Vec<u8>)>;

pub(crate) fn validate_creation_timestamp_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, f64>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map_or(&[][..], |native| native.creation_timestamps.as_slice());
    let target = target_native
        .as_ref()
        .map_or(&[][..], |native| native.creation_timestamps.as_slice());
    let by_id = baseline
        .iter()
        .map(|timestamp| (timestamp.id.as_str(), timestamp))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|timestamp| timestamp.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D timestamp regeneration requires the unchanged timestamp-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for timestamp in target {
        let before = by_id[timestamp.id.as_str()];
        let mut normalized = timestamp.clone();
        normalized.unix_microseconds = before.unix_microseconds;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D timestamp edit changes structural fields: {}",
                timestamp.id
            )));
        }
        if timestamp.unix_microseconds == before.unix_microseconds {
            continue;
        }
        if !timestamp.unix_microseconds.is_finite() {
            return Err(CodecError::Malformed(format!(
                "F3D creation timestamp {} is non-finite",
                timestamp.id
            )));
        }
        let record_index = usize::try_from(timestamp.record_index).map_err(|_| {
            CodecError::Malformed(format!(
                "F3D timestamp record index exceeds usize: {}",
                timestamp.id
            ))
        })?;
        edits.insert(record_index, timestamp.unix_microseconds);
    }
    Ok(edits)
}

pub(crate) fn validate_edge_continuity_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (Sense, String)>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.edge_continuities)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.edge_continuities)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-continuity regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.continuity.clone_from(&before.continuity);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge-continuity edit changes structural fields: {id}"
            )));
        }
        if after.continuity == before.continuity && after.sense == before.sense {
            continue;
        }
        if !matches!(after.continuity.as_str(), "tangent" | "unknown") {
            return Err(CodecError::Malformed(format!(
                "F3D edge continuity {id} has unsupported token {}",
                after.continuity
            )));
        }
        edits.insert(
            after.record_index as usize,
            (after.sense, after.continuity.clone()),
        );
    }
    Ok(edits)
}

pub(crate) fn validate_edge_ownership_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, i64>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.edge_ownerships)
        .unwrap_or_default();
    let target_ownerships = f3d_native(target)?
        .map(|native| native.edge_ownerships)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|ownership| (ownership.id.as_str(), ownership))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_ownerships
        .iter()
        .map(|ownership| (ownership.id.as_str(), ownership))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-ownership regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.owner_coedge.clone_from(&before.owner_coedge);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge-ownership edit changes structural fields: {id}"
            )));
        }
        if after.owner_coedge == before.owner_coedge {
            continue;
        }
        let owner = if let Some(owner) = &after.owner_coedge {
            let coedge = target
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *owner)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D edge ownership {id} references missing coedge {owner}"
                    ))
                })?;
            if coedge.edge != after.edge {
                return Err(CodecError::Malformed(format!(
                    "F3D edge ownership {id} selects a coedge of another edge"
                )));
            }
            owner
                .as_str()
                .rsplit_once('#')
                .and_then(|(_, index)| index.parse::<i64>().ok())
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D owning coedge {owner} has no native record index"
                    ))
                })?
        } else {
            -1
        };
        edits.insert(after.record_index as usize, owner);
    }
    Ok(edits)
}

pub(crate) fn validate_vertex_ownership_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (i64, u8)>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.vertex_ownerships)
        .unwrap_or_default();
    let target_native = f3d_native(target)?
        .map(|native| native.vertex_ownerships)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_native
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D vertex-ownership regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.owning_edge.clone_from(&before.owning_edge);
        normalized.endpoint_index = before.endpoint_index;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex-ownership edit changes structural fields: {id}"
            )));
        }
        if after.owning_edge == before.owning_edge && after.endpoint_index == before.endpoint_index
        {
            continue;
        }
        let edge = target
            .model
            .edges
            .iter()
            .find(|edge| edge.id == after.owning_edge)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D vertex ownership {id} references missing edge {}",
                    after.owning_edge
                ))
            })?;
        let valid = match after.endpoint_index {
            0 => edge.start == after.vertex,
            1 => edge.end == after.vertex,
            _ => false,
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "F3D vertex ownership {id} has an inconsistent endpoint slot"
            )));
        }
        let edge_record = after
            .owning_edge
            .as_str()
            .rsplit_once('#')
            .and_then(|(_, index)| index.parse::<i64>().ok())
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D owning edge {} has no native record index",
                    after.owning_edge
                ))
            })?;
        edits.insert(
            after.record_index as usize,
            (edge_record, after.endpoint_index),
        );
    }
    Ok(edits)
}

pub(crate) fn validate_face_sidedness_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, crate::records::FaceContainment>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.face_sidedness)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.face_sidedness)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sidedness regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.containment = before.containment;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face-sidedness edit changes structural fields: {id}"
            )));
        }
        if after.containment == before.containment {
            continue;
        }
        match (before.containment, after.containment) {
            (Some(_), Some(containment)) => {
                edits.insert(after.record_index as usize, containment);
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D face sidedness {id} cannot change record width"
                )));
            }
        }
    }
    Ok(edits)
}

pub(crate) fn validate_tolerant_vertex_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (f64, [f64; 2])>, CodecError> {
    let baseline_vertices = baseline
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<BTreeMap<_, _>>();
    let target_vertices = target
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<BTreeMap<_, _>>();
    if baseline_vertices.keys().ne(target_vertices.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-vertex regeneration requires the unchanged vertex-id set".into(),
        ));
    }
    for (id, before) in &baseline_vertices {
        let after = target_vertices[id];
        let mut normalized = after.clone();
        normalized.tolerance = before.tolerance;
        if &normalized != *before {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex edit changes fields other than tolerance: {id}"
            )));
        }
        if after.tolerance != before.tolerance
            && (before.tolerance.is_none() || after.tolerance.is_none())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex tolerance {id} cannot change record width"
            )));
        }
    }
    let baseline_tails = f3d_native(baseline)?
        .map(|native| native.tolerant_vertex_tails)
        .unwrap_or_default();
    let target_tails = f3d_native(target)?
        .map(|native| native.tolerant_vertex_tails)
        .unwrap_or_default();
    let baseline_by_id = baseline_tails
        .iter()
        .map(|tail| (tail.id.as_str(), tail))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_tails
        .iter()
        .map(|tail| (tail.id.as_str(), tail))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-vertex regeneration requires the unchanged tail-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.leading_tolerances = before.leading_tolerances;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-vertex tail edit changes structural fields: {id}"
            )));
        }
        let tolerance = target_vertices[after.vertex.as_str()]
            .tolerance
            .ok_or_else(|| {
                CodecError::Malformed(format!("tolerant vertex {id} has no tolerance"))
            })?;
        if !tolerance.is_finite()
            || after
                .leading_tolerances
                .iter()
                .any(|value| !value.is_finite())
        {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant vertex {id} has non-finite fields"
            )));
        }
        if tolerance
            != baseline_vertices[after.vertex.as_str()]
                .tolerance
                .unwrap_or(tolerance)
            || after.leading_tolerances != before.leading_tolerances
        {
            // A negative tolerance is the unevaluated sentinel, stored
            // verbatim; a non-negative tolerance converts back from
            // millimetres to centimetres.
            let stored = if tolerance < 0.0 {
                tolerance
            } else {
                tolerance / LEN_TO_MM
            };
            edits.insert(
                after.record_index as usize,
                (stored, after.leading_tolerances),
            );
        }
    }
    Ok(edits)
}

pub(crate) fn validate_tolerant_edge_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, f64>, CodecError> {
    let baseline_edges = baseline
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let target_edges = target
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    if baseline_edges.keys().ne(target_edges.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-edge regeneration requires the unchanged edge-id set".into(),
        ));
    }
    for (id, before) in &baseline_edges {
        let after = target_edges[id];
        if after.tolerance != before.tolerance
            && (before.tolerance.is_none() || after.tolerance.is_none())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge tolerance {id} cannot change record width"
            )));
        }
    }
    let baseline_tails = f3d_native(baseline)?
        .map(|native| native.tolerant_edge_tails)
        .unwrap_or_default();
    let target_tails = f3d_native(target)?
        .map(|native| native.tolerant_edge_tails)
        .unwrap_or_default();
    let baseline_by_id = baseline_tails
        .iter()
        .map(|tail| (tail.id.as_str(), tail))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_tails
        .iter()
        .map(|tail| (tail.id.as_str(), tail))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-edge regeneration requires the unchanged tail-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        if after != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-edge tail edit changes retained fields: {id}"
            )));
        }
        let tolerance = target_edges[after.edge.as_str()]
            .tolerance
            .ok_or_else(|| CodecError::Malformed(format!("tolerant edge {id} has no tolerance")))?;
        if !tolerance.is_finite() || tolerance < 0.0 || after.trailing_integers[1] != 0 {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant edge {id} has invalid fields"
            )));
        }
        if baseline_edges[after.edge.as_str()].tolerance != Some(tolerance) {
            edits.insert(after.record_index as usize, tolerance / LEN_TO_MM);
        }
    }
    Ok(edits)
}

pub(crate) fn validate_tolerant_coedge_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, [f64; 2]>, CodecError> {
    let baseline_coedges = baseline
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let target_coedges = target
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    if baseline_coedges.keys().ne(target_coedges.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-coedge regeneration requires the unchanged coedge-id set".into(),
        ));
    }
    for (id, before) in &baseline_coedges {
        let after = target_coedges[id];
        if after.use_curve != before.use_curve
            || after.use_curve_parameter_range != before.use_curve_parameter_range
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D coedge use-curve edit changes embedded record structure: {id}"
            )));
        }
    }
    let baseline_parameters = f3d_native(baseline)?
        .map(|native| native.tolerant_coedge_parameters)
        .unwrap_or_default();
    let target_parameters = f3d_native(target)?
        .map(|native| native.tolerant_coedge_parameters)
        .unwrap_or_default();
    let baseline_by_id = baseline_parameters
        .iter()
        .map(|parameters| (parameters.id.as_str(), parameters))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_parameters
        .iter()
        .map(|parameters| (parameters.id.as_str(), parameters))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-coedge regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.parameter_range = before.parameter_range;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-coedge edit changes structural fields: {id}"
            )));
        }
        if after.parameter_range.iter().any(|value| !value.is_finite()) {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant coedge {id} has non-finite parameters"
            )));
        }
        if after.parameter_range != before.parameter_range {
            edits.insert(after.record_index as usize, after.parameter_range);
        }
    }
    Ok(edits)
}

pub(crate) fn validate_wire_topology_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, crate::records::WireSide>, CodecError> {
    let baseline_wires = f3d_native(baseline)?
        .map(|native| native.wire_topologies)
        .unwrap_or_default();
    let target_wires = f3d_native(target)?
        .map(|native| native.wire_topologies)
        .unwrap_or_default();
    let baseline_by_id = baseline_wires
        .iter()
        .map(|wire| (wire.id.as_str(), wire))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_wires
        .iter()
        .map(|wire| (wire.id.as_str(), wire))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D wire regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.side = before.side;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D wire edit changes structural fields: {id}"
            )));
        }
        if after.side != before.side {
            edits.insert(after.record_index as usize, after.side);
        }
    }
    Ok(edits)
}

pub(crate) enum ProceduralSurfaceEdit {
    Extrusion {
        parameter_interval: [f64; 2],
        direction: Vector3,
        native_position: Point3,
    },
    BlendRadii([f64; 2]),
}

pub(crate) struct NurbsSurfaceEdit {
    pub(crate) surface: NurbsSurface,
    pub(crate) periodic: Option<[bool; 2]>,
}

pub(crate) struct NurbsCurveEdit {
    pub(crate) curve: NurbsCurve,
    pub(crate) periodic: Option<bool>,
}

#[derive(Clone)]
pub(crate) struct NurbsPcurveEdit {
    pub(crate) geometry: PcurveGeometry,
    pub(crate) periodic: Option<bool>,
    pub(crate) wrapper_reversed: Option<bool>,
    pub(crate) native_tail_flags: Option<[bool; 4]>,
    pub(crate) parameter_range: Option<[f64; 2]>,
    pub(crate) fit_tolerance: Option<f64>,
}

#[derive(Clone)]
pub(crate) struct ProceduralCurveEdit {
    pub(crate) definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    pub(crate) fit_tolerance: Option<f64>,
}

pub(crate) fn validate_material_assignment_appearances(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, crate::materials::ProteinAppearanceEdit>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline_appearances = baseline
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    let target_appearances = target
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    if baseline_appearances.keys().ne(target_appearances.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D material regeneration requires the unchanged appearance-id set".into(),
        ));
    }
    let target_assignments = target_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let baseline_assignments = baseline_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let mut appearance_edits = BTreeMap::new();
    for (id, before) in baseline_appearances {
        let after = target_appearances[id];
        let mut normalized = after.clone();
        normalized.physical_token.clone_from(&before.physical_token);
        normalized.base_color = before.base_color;
        normalized.properties.clone_from(&before.properties);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance edit changes fields outside physical token or Protein values: {id}"
            )));
        }
        if before.properties.keys().ne(after.properties.keys()) {
            return Err(CodecError::NotImplemented(format!(
                "F3D Protein property regeneration requires the unchanged property set: {id}"
            )));
        }
        let mut edit = crate::materials::ProteinAppearanceEdit::default();
        if after.base_color != before.base_color {
            let color = after.base_color.ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D appearance color: {id}"))
            })?;
            if before.base_color.is_none()
                || ![color.r, color.g, color.b, color.a]
                    .into_iter()
                    .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
                || color.a != 1.0
            {
                return Err(CodecError::Malformed(format!(
                    "F3D Protein color {id} must replace an existing opaque finite RGBA color"
                )));
            }
            edit.color = Some(color);
        }
        for (name, before_value) in &before.properties {
            let after_value = after.properties[name];
            if after_value == *before_value {
                continue;
            }
            let valid = after_value.is_finite()
                && match name.as_str() {
                    "reflectivity_at_0deg" | "surface_roughness" => {
                        (0.0..=1.0).contains(&after_value)
                    }
                    "refraction_index" => (1.0..=4.0).contains(&after_value),
                    _ => false,
                };
            if !valid {
                return Err(CodecError::Malformed(format!(
                    "F3D Protein property {id}.{name} is outside its writable range"
                )));
            }
            edit.properties.insert(name.clone(), after_value);
        }
        if edit.color.is_some() || !edit.properties.is_empty() {
            let guid = after.visual_guid.clone().ok_or_else(|| {
                CodecError::NotImplemented(format!("F3D appearance {id} has no visual GUID"))
            })?;
            appearance_edits.insert(guid, edit);
        }
        if after.physical_token == before.physical_token {
            continue;
        }
        let synchronized = target_assignments.iter().any(|assignment| {
            after.visual_guid.as_deref().is_some_and(|guid| {
                crate::materials::visual_guid_matches(guid, &assignment.visual_guid)
            }) && after.physical_token == assignment.physical_token
        });
        if !synchronized {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance {id} changed without a synchronized material assignment"
            )));
        }
    }
    for before in baseline_assignments {
        let Some(after) = target_assignments
            .iter()
            .find(|assignment| assignment.id == before.id)
        else {
            continue;
        };
        if after.physical_token != before.physical_token
            && !target.model.appearances.iter().any(|appearance| {
                appearance.visual_guid.as_deref().is_some_and(|guid| {
                    crate::materials::visual_guid_matches(guid, &after.visual_guid)
                }) && appearance.physical_token == after.physical_token
            })
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D material assignment {} changed without its appearance physical token",
                before.id
            )));
        }
    }
    Ok(appearance_edits)
}

pub(crate) fn validate_material_assignment_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<DesignMaterialAssignment>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D material-assignment regeneration requires the unchanged assignment-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignMaterialAssignment>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.entity_suffix = before.entity_suffix;
        normalized.visual_guid.clone_from(&before.visual_guid);
        normalized.physical_token.clone_from(&before.physical_token);
        normalized.visual_preset.clone_from(&before.visual_preset);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D material-assignment edit changes fields outside writable strings: {id}"
            )));
        }
        if after == before {
            continue;
        }
        let suffix = after
            .entity_id
            .rsplit_once('_')
            .and_then(|(_, suffix)| suffix.parse::<u64>().ok())
            .ok_or_else(|| CodecError::Malformed(format!("invalid assignment entity id: {id}")))?;
        if suffix != after.entity_suffix {
            return Err(CodecError::Malformed(format!(
                "F3D assignment entity id and suffix disagree: {id}"
            )));
        }
        validate_utf16_replacement(id, &before.entity_id, &after.entity_id)?;
        validate_utf16_replacement(id, &before.visual_guid, &after.visual_guid)?;
        validate_optional_utf16_replacement(
            id,
            before.physical_token.as_deref(),
            after.physical_token.as_deref(),
        )?;
        validate_optional_utf16_replacement(
            id,
            before.visual_preset.as_deref(),
            after.visual_preset.as_deref(),
        )?;
        edits
            .entry(native_stream(id, ":material-assignment#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn validate_utf16_replacement(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.encode_utf16().count() != after.encode_utf16().count() {
        return Err(CodecError::NotImplemented(format!(
            "F3D native string {id} must retain its UTF-16 length"
        )));
    }
    Ok(())
}

fn validate_optional_utf16_replacement(
    id: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), CodecError> {
    match (before, after) {
        (Some(before), Some(after)) => validate_utf16_replacement(id, before, after),
        (None, None) => Ok(()),
        _ => Err(CodecError::NotImplemented(format!(
            "cannot add or remove F3D native string carrier: {id}"
        ))),
    }
}

pub(crate) fn validate_lost_edge_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<LostEdgeReference>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.lost_edge_references[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.lost_edge_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D lost-edge regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<LostEdgeReference>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.class_tag.clone_from(&before.class_tag);
        normalized.record_index = before.record_index;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D lost-edge edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.class_tag.len() != 3 || !after.class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(CodecError::Malformed(format!(
                "F3D lost-edge class tag must contain three digits: {id}"
            )));
        }
        edits
            .entry(native_stream(id, ":lost-edge-reference#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

pub(crate) fn validate_act_appearance_bindings(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<(), CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline_entities = baseline_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let target_entities = target_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    if baseline.model.appearance_bindings.len() != target.model.appearance_bindings.len() {
        return Err(CodecError::NotImplemented(
            "F3D ACT regeneration requires the unchanged appearance-binding count".into(),
        ));
    }
    let target_bindings = target
        .model
        .appearance_bindings
        .iter()
        .map(|binding| {
            (
                (binding.target.clone(), binding.appearance.clone()),
                binding,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut baseline_entities_by_source = HashMap::<_, Vec<_>>::new();
    for entity in baseline_entities {
        baseline_entities_by_source
            .entry(entity.entity_id.as_str())
            .or_default()
            .push(entity);
    }
    let target_entities_by_id = target_entities
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    for before in &baseline.model.appearance_bindings {
        let after = target_bindings
            .get(&(before.target.clone(), before.appearance.clone()))
            .copied()
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "F3D appearance binding target or appearance changed: {}",
                    before.id
                ))
            })?;
        let mut normalized = after.clone();
        normalized.id.clone_from(&before.id);
        normalized
            .source_entity_id
            .clone_from(&before.source_entity_id);
        normalized.channels.clone_from(&before.channels);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance binding edit changes fields outside its derived identity, source, or channels: {}",
                before.id
            )));
        }
        if after == before {
            continue;
        }
        let before_entity = before
            .source_entity_id
            .as_deref()
            .and_then(|source| baseline_entities_by_source.get(source))
            .and_then(|entities| {
                entities
                    .iter()
                    .copied()
                    .find(|entity| before.channels == entity.channels)
            });
        let after_entity = before_entity.and_then(|before_entity| {
            target_entities_by_id
                .get(before_entity.id.as_str())
                .copied()
                .filter(|entity| after.channels == entity.channels)
        });
        if before_entity.is_none() || after_entity.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance binding {} must remain synchronized with one ACT entity",
                before.id
            )));
        }
        if after.source_entity_id != before.source_entity_id
            && after
                .source_entity_id
                .as_deref()
                .is_none_or(|source| !after.id.contains(source))
        {
            return Err(CodecError::Malformed(format!(
                "F3D appearance binding id does not contain its changed source entity: {}",
                after.id
            )));
        }
    }
    let derived_bindings = baseline
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            Some((
                binding.source_entity_id.clone()?,
                binding.channels.clone(),
                binding,
            ))
        })
        .fold(HashMap::<_, Vec<_>>::new(), |mut grouped, entry| {
            grouped.entry(entry.0).or_default().push((entry.1, entry.2));
            grouped
        });
    let assignment_entities = target_native
        .as_ref()
        .into_iter()
        .flat_map(|native| &native.design_material_assignments)
        .map(|assignment| assignment.entity_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (before, after) in baseline_entities.iter().zip(target_entities) {
        let matching_bindings = derived_bindings.get(&before.entity_id);
        let derived_binding = matching_bindings.is_some_and(|bindings| {
            bindings
                .iter()
                .any(|(channels, _)| channels == &before.channels)
        });
        let assignment_synchronized = assignment_entities.contains(after.entity_id.as_str());
        if before.entity_id != after.entity_id && derived_binding && !assignment_synchronized {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {} changed without its material-assignment carrier",
                before.id
            )));
        }
        let synchronized = matching_bindings.is_some_and(|bindings| {
            bindings.iter().any(|(_, before_binding)| {
                target_bindings
                    .get(&(
                        before_binding.target.clone(),
                        before_binding.appearance.clone(),
                    ))
                    .is_some_and(|binding| {
                        binding.source_entity_id.as_deref() == Some(after.entity_id.as_str())
                            && binding.channels == after.channels
                    })
            })
        });
        if (before.entity_id != after.entity_id || before.channels != after.channels)
            && derived_binding
            && !synchronized
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {} changed without a synchronized appearance binding",
                before.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_act_entity_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActEntity>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT entity regeneration requires the unchanged entity-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActEntity>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.channels.clone_from(&before.channels);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity edit changes fields other than entity_id or channel GUIDs: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.entity_id.encode_utf16().count() != before.entity_id.encode_utf16().count() {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity id {id} must retain its UTF-16 length"
            )));
        }
        if after.channels.keys().ne(before.channels.keys())
            || after.channels.keys().ne(after.channel_guid_offsets.keys())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {id} must retain its channel set and offsets"
            )));
        }
        for (name, guid) in &after.channels {
            let before_guid = &before.channels[name];
            if guid.encode_utf16().count() != before_guid.encode_utf16().count()
                || !canonical_guid(guid)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D ACT channel {name} on {id} must be a same-length canonical GUID"
                )));
            }
        }
        edits
            .entry(native_stream(id, ":act-entity#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

pub(crate) fn validate_act_guid_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActGuidEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|guid| (guid.id.as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|guid| (guid.id.as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT GUID regeneration requires the unchanged GUID-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActGuidEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized: ActGuid = after.clone();
        normalized.guid.clone_from(&before.guid);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID edit changes fields other than guid: {id}"
            )));
        }
        if after.guid == before.guid {
            continue;
        }
        if after.guid.encode_utf16().count() != before.guid.encode_utf16().count() {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID {id} must retain its UTF-16 length"
            )));
        }
        if !canonical_guid(&after.guid) {
            return Err(CodecError::Malformed(format!(
                "F3D ACT GUID {id} is not canonical"
            )));
        }
        let encoded = after
            .guid
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let stream = native_stream(id, ":act-guid#")?;
        edits
            .entry(stream)
            .or_default()
            .push((after.guid_offset, encoded));
    }
    Ok(edits)
}

pub(crate) fn validate_act_root_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActRootComponent>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_root_components[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_root_components[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT root regeneration requires the unchanged root-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActRootComponent>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index = before.record_index;
        normalized.instance_root_record = before.instance_root_record;
        normalized.components_root_record = before.components_root_record;
        normalized.registry_flag = before.registry_flag;
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.display_name.clone_from(&before.display_name);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT root edit changes fields outside fixed numeric graph links: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.registry_flag > 1 {
            return Err(CodecError::Malformed(format!(
                "F3D ACT root registry flag must be zero or one: {id}"
            )));
        }
        if after.entity_id.encode_utf16().count() != before.entity_id.encode_utf16().count()
            || after.display_name.encode_utf16().count()
                != before.display_name.encode_utf16().count()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT root strings must retain their UTF-16 lengths: {id}"
            )));
        }
        edits
            .entry(native_stream(id, ":act-root-component#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

pub(crate) struct DesignObjectEdit {
    pub(crate) integers: Vec<(u64, Vec<u8>)>,
    pub(crate) strings: Vec<(u64, Vec<u8>)>,
}

pub(crate) fn validate_design_object_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<DesignObjectEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_objects[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_objects[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D design-object regeneration requires the unchanged object-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignObjectEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_ids.clone_from(&before.entity_ids);
        normalized.self_guid.clone_from(&before.self_guid);
        normalized.parent_guid.clone_from(&before.parent_guid);
        normalized.revision = before.revision;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D design-object edit changes fields outside its fixed object payload: {id}"
            )));
        }
        if after.entity_ids.len() != before.entity_ids.len()
            || after.entity_ids.len() != after.entity_id_offsets.len()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D design object {id} must retain its entity-id cardinality"
            )));
        }
        let mut integers = after
            .entity_ids
            .iter()
            .zip(&before.entity_ids)
            .zip(&after.entity_id_offsets)
            .filter(|((value, before), _)| value != before)
            .map(|((&value, _), &offset)| (offset, value.to_le_bytes().to_vec()))
            .collect::<Vec<_>>();
        if after.revision != before.revision {
            integers.push((after.revision_offset, after.revision.to_le_bytes().to_vec()));
        }
        let mut strings = Vec::new();
        if after.self_guid != before.self_guid {
            validate_fixed_design_string(id, &before.self_guid, &after.self_guid)?;
            strings.push((after.self_guid_offset, after.self_guid.as_bytes().to_vec()));
        }
        if after.parent_guid != before.parent_guid {
            let before_parent = before.parent_guid.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D object parent GUID: {id}"))
            })?;
            let after_parent = after.parent_guid.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D object parent GUID: {id}"))
            })?;
            validate_fixed_design_string(id, before_parent, after_parent)?;
            strings.push((
                after.parent_guid_offset.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D object {id} has no parent-GUID offset"))
                })?,
                after_parent.as_bytes().to_vec(),
            ));
        }
        if integers.is_empty() && strings.is_empty() {
            continue;
        }
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":design-object#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid design-object id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(DesignObjectEdit { integers, strings });
    }
    Ok(edits)
}

fn validate_fixed_design_string(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.len() != after.len()
        || !after
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(CodecError::NotImplemented(format!(
            "F3D object string {id} must retain its encoded length and alphabet"
        )));
    }
    Ok(())
}

pub(crate) fn validate_configuration_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<u8>>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.design_configurations)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.design_configurations)
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|configuration| (configuration.entry_name.as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|configuration| (configuration.entry_name.as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "retained F3D configuration editing requires the unchanged entry-name set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (name, before) in baseline {
        let after = target[name];
        if before.id != after.id || before.kind != after.kind {
            return Err(CodecError::NotImplemented(format!(
                "retained F3D configuration edit changes entry identity: {name}"
            )));
        }
        if before.payload != after.payload {
            edits.insert(
                name.to_owned(),
                serde_json::to_vec(&after.payload).map_err(|error| {
                    CodecError::Malformed(format!(
                        "cannot encode retained F3D configuration {name}: {error}"
                    ))
                })?,
            );
        }
    }
    Ok(edits)
}

pub(crate) struct EntityHeaderEdit {
    pub(crate) record_reference: Option<(u64, u32)>,
    pub(crate) references: Vec<(u64, u32)>,
}

pub(crate) fn validate_entity_header_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<EntityHeaderEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D entity-header regeneration requires the unchanged header-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<EntityHeaderEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_reference = before.record_reference;
        normalized
            .reference_indices
            .clone_from(&before.reference_indices);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity-header edit changes fields outside fixed record references: {id}"
            )));
        }
        if after.record_reference == before.record_reference
            && after.reference_indices == before.reference_indices
        {
            continue;
        }
        if after.reference_indices.len() != after.reference_offsets.len() {
            return Err(CodecError::Malformed(format!(
                "F3D entity header {id} has mismatched reference values and offsets"
            )));
        }
        let record_reference = if after.record_reference == before.record_reference {
            None
        } else {
            Some((
                after.record_reference_offset.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "F3D entity header {id} has no writable owning-record reference"
                    ))
                })?,
                after.record_reference.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove F3D entity-header record reference: {id}"
                    ))
                })?,
            ))
        };
        let references = after
            .reference_offsets
            .iter()
            .copied()
            .zip(after.reference_indices.iter().copied())
            .zip(&before.reference_indices)
            .filter_map(|((offset, value), before)| (value != *before).then_some((offset, value)))
            .collect();
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":design-entity-header#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid design-entity-header id {id}"))
            })?;
        edits.entry(stream).or_default().push(EntityHeaderEdit {
            record_reference,
            references,
        });
    }
    Ok(edits)
}

pub(crate) fn validate_body_member_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<BodyMemberEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-membership regeneration requires the unchanged member-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<BodyMemberEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_suffix = before.entity_suffix;
        normalized.flags = before.flags;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-member edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after.entity_suffix == before.entity_suffix && after.flags == before.flags {
            continue;
        }
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":design-body-member#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid design-body-member id {id}")))?;
        edits.entry(stream).or_default().push((
            after.byte_offset,
            after.entity_suffix,
            after.flags,
        ));
    }
    Ok(edits)
}

pub(crate) fn validate_body_visibility_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<(u64, bool)>>, CodecError> {
    let baseline_bodies = baseline
        .model
        .bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<BTreeMap<_, _>>();
    let target_bodies = target
        .model
        .bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<BTreeMap<_, _>>();
    if baseline_bodies.keys().ne(target_bodies.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-visibility regeneration requires the unchanged body-id set".into(),
        ));
    }
    let metadata = f3d_native(baseline)?
        .map(|native| {
            native
                .body_visibilities
                .into_iter()
                .map(|item| (item.body.clone(), item))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut edits = BTreeMap::<String, Vec<(u64, bool)>>::new();
    for (id, before) in baseline_bodies {
        let after = target_bodies[id];
        if after.visible == before.visible {
            continue;
        }
        let visible = after.visible.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body visibility: {id}"))
        })?;
        let record = metadata.get(id).ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "F3D body visibility {id} has no joined Design browser node"
            ))
        })?;
        edits
            .entry(record.stream.clone())
            .or_default()
            .push((record.byte_offset, visible));
    }
    Ok(edits)
}

pub(crate) fn validate_transform_hint_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, [bool; 3]>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.transform_hints)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.transform_hints)
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|hints| (hints.id.as_str(), hints))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|hints| (hints.id.as_str(), hints))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D transform-hint regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.rotation = before.rotation;
        normalized.reflection = before.reflection;
        normalized.shear = before.shear;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D transform-hint edit changes structural fields: {id}"
            )));
        }
        let flags = [after.rotation, after.reflection, after.shear];
        if flags != [before.rotation, before.reflection, before.shear] {
            edits.insert(after.record_index as usize, flags);
        }
    }
    Ok(edits)
}

pub(crate) fn validate_body_native_key_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BodyNativeKeyEdits, CodecError> {
    let baseline_native = f3d_native(baseline)?.unwrap_or_default();
    let target_native = f3d_native(target)?.unwrap_or_default();
    let baseline = baseline_native
        .body_native_keys
        .iter()
        .map(|key| (key.id.as_str(), key))
        .collect::<BTreeMap<_, _>>();
    let target = target_native
        .body_native_keys
        .iter()
        .map(|key| (key.id.as_str(), key))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-key regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BodyNativeKeyEdits::default();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.asm_body_key = before.asm_body_key;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-key edit changes structural fields: {id}"
            )));
        }
        if after.asm_body_key != before.asm_body_key {
            let key = after.asm_body_key.map_or(Ok(-1), |key| {
                i64::try_from(key).map_err(|_| {
                    CodecError::Malformed(format!("F3D ASM body key exceeds i64::MAX: {key}"))
                })
            })?;
            edits.asm.insert(after.record_index as usize, key);
            let mut joined = Vec::new();
            joined.extend(
                baseline_native
                    .body_visibilities
                    .iter()
                    .filter(|visibility| visibility.body == before.body)
                    .map(|visibility| (visibility.stream.clone(), visibility.asm_body_key_offset)),
            );
            if let Some(old_key) = before.asm_body_key {
                joined.extend(
                    baseline_native
                        .design_material_assignments
                        .iter()
                        .filter(|assignment| assignment.asm_body_key == old_key)
                        .map(|assignment| {
                            Ok((
                                native_stream(&assignment.id, ":material-assignment#")?,
                                assignment.asm_body_key_offset,
                            ))
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                );
            }
            if !joined.is_empty() {
                let design_key = after.asm_body_key.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove joined F3D ASM body key: {id}"
                    ))
                })?;
                for (stream, offset) in joined {
                    edits
                        .design
                        .entry(stream)
                        .or_default()
                        .insert((offset, design_key));
                }
            }
        }
    }
    Ok(edits)
}

#[derive(Default)]
pub(crate) struct BodyNativeKeyEdits {
    pub(crate) asm: BTreeMap<usize, i64>,
    pub(crate) design: BTreeMap<String, BTreeSet<(u64, u64)>>,
}

pub(crate) struct ConstructionRecipeEdit {
    pub(crate) record_index: Option<(u64, i32)>,
    pub(crate) design_id: Option<(u64, Vec<u8>)>,
}

pub(crate) fn validate_construction_recipe_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ConstructionRecipeEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D construction-recipe regeneration requires the unchanged recipe-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ConstructionRecipeEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index = before.record_index;
        normalized.design_id.clone_from(&before.design_id);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D construction-recipe edit changes fields other than record_index or design_id: {id}"
            )));
        }
        if after.record_index == before.record_index && after.design_id == before.design_id {
            continue;
        }
        let record_index = (after.record_index != before.record_index)
            .then(|| {
                after
                    .record_index_offset
                    .map(|offset| (offset, after.record_index))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(format!(
                            "F3D construction recipe {id} has no writable record-index carrier"
                        ))
                    })
            })
            .transpose()?;
        let design_id = if after.design_id == before.design_id {
            None
        } else {
            let before_value = before.design_id.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D recipe design id: {id}"))
            })?;
            let after_value = after.design_id.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D recipe design id: {id}"))
            })?;
            let offset = after.design_id_offset.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "F3D construction recipe {id} has no writable design-id carrier"
                ))
            })?;
            if after_value.len() != before_value.len()
                || !after_value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                return Err(CodecError::NotImplemented(format!(
                    "ASCII F3D recipe design id {id} must retain its encoded length"
                )));
            }
            let encoded = after_value.as_bytes().to_vec();
            Some((offset, encoded))
        };
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":construction-recipe#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid construction-recipe id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(ConstructionRecipeEdit {
                record_index,
                design_id,
            });
    }
    Ok(edits)
}

pub(crate) fn validate_persistent_reference_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<PersistentReferenceEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D persistent-reference regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<PersistentReferenceEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.value = before.value;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D persistent-reference edit changes fields other than value: {id}"
            )));
        }
        if after.value == before.value {
            continue;
        }
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":persistent-reference#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid persistent-reference id {id}"))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((after.byte_offset, after.value_offset, after.value));
    }
    Ok(edits)
}

/// The three history-preamble fields a patch writes back, carried as
/// non-optional values. [`validate_history_state_edits`] only builds one after
/// it has confirmed both `stream_size` and `history_entry_count` are present, so
/// a [`HistoryEdits`] that holds a preamble cannot reach the patcher with either
/// field absent — the invariant `patch_history_states` previously asserted with
/// `expect`.
pub(crate) struct PreambleEdit {
    pub(crate) byte_offset: u64,
    pub(crate) stream_size: i64,
    pub(crate) history_entry_count: i64,
}

#[derive(Default)]
pub(crate) struct HistoryEdits {
    pub(crate) preamble: Option<PreambleEdit>,
    pub(crate) states: Vec<AsmDeltaState>,
    pub(crate) boards: Vec<AsmBulletinBoard>,
    pub(crate) changes: Vec<AsmEntityChange>,
}

pub(crate) fn validate_history_state_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, HistoryEdits>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|history| (history.id.as_str(), history))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id
        .keys()
        .copied()
        .ne(target.iter().map(|history| history.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D history regeneration requires the unchanged history-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, HistoryEdits> = BTreeMap::new();
    for history in target {
        let before = baseline_by_id[history.id.as_str()];
        if before.states.len() != history.states.len() {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes the state count: {}",
                history.id
            )));
        }
        let mut normalized = history.clone();
        normalized.stream_size = before.stream_size;
        normalized.history_entry_count = before.history_entry_count;
        for (state, before_state) in normalized.states.iter_mut().zip(&before.states) {
            state.state_id = before_state.state_id;
            state.version_flag = before_state.version_flag;
            state.state_flag = before_state.state_flag;
            state.previous_ref = before_state.previous_ref;
            state.next_ref = before_state.next_ref;
            state.node_index = before_state.node_index;
            state.partner_ref = before_state.partner_ref;
            state.owner_ref = before_state.owner_ref;
            if state.bulletin_boards.len() != before_state.bulletin_boards.len() {
                return Err(CodecError::NotImplemented(format!(
                    "F3D history edit changes the bulletin-board count: {}",
                    history.id
                )));
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter_mut()
                .zip(&before_state.bulletin_boards)
            {
                board.owner_ref = before_board.owner_ref;
                board.number = before_board.number;
                if board.changes.len() != before_board.changes.len() {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D history edit changes the entity-change count: {}",
                        board.id
                    )));
                }
                for (change, before_change) in board.changes.iter_mut().zip(&before_board.changes) {
                    change.kind = before_change.kind;
                    change.old_ref = before_change.old_ref;
                    change.new_ref = before_change.new_ref;
                }
            }
        }
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes fields outside the fixed delta-state header: {}",
                history.id
            )));
        }
        let stream = history
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":asm-history#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid ASM history id {}", history.id))
            })?;
        if let Some(size) = history.stream_size {
            if history
                .states
                .first()
                .is_none_or(|state| state.state_id != size)
                || history
                    .history_entry_count
                    .is_none_or(|entry_count| entry_count < 0)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
        }
        if history.stream_size != before.stream_size
            || history.history_entry_count != before.history_entry_count
        {
            let (Some(stream_size), Some(history_entry_count)) =
                (history.stream_size, history.history_entry_count)
            else {
                return Err(CodecError::NotImplemented(format!(
                    "cannot add or remove the F3D history preamble: {}",
                    history.id
                )));
            };
            if history.byte_offset == 0 || history_entry_count < 0 {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            edits.entry(stream.clone()).or_default().preamble = Some(PreambleEdit {
                byte_offset: history.byte_offset,
                stream_size,
                history_entry_count,
            });
        }
        for (state, before_state) in history.states.iter().zip(&before.states) {
            if state != before_state {
                edits
                    .entry(stream.clone())
                    .or_default()
                    .states
                    .push(state.clone());
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter()
                .zip(&before_state.bulletin_boards)
            {
                if board.owner_ref != before_board.owner_ref || board.number != before_board.number
                {
                    edits
                        .entry(stream.clone())
                        .or_default()
                        .boards
                        .push(board.clone());
                }
                for (change, before_change) in board.changes.iter().zip(&before_board.changes) {
                    if change != before_change {
                        if change.kind != history_change_kind(change.old_ref, change.new_ref)? {
                            return Err(CodecError::Malformed(format!(
                                "F3D entity change {} has a kind inconsistent with its references",
                                change.id
                            )));
                        }
                        edits
                            .entry(stream.clone())
                            .or_default()
                            .changes
                            .push(change.clone());
                    }
                }
            }
        }
    }
    Ok(edits)
}

pub(crate) fn validate_sketch_point_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchPointEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|point| point.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-point regeneration requires the unchanged point-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchPointEdit>> = BTreeMap::new();
    for point in target {
        let before = by_id[point.id.as_str()];
        let mut normalized = point.clone();
        normalized.coordinates = before.coordinates;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-point edit changes fields other than coordinates: {}",
                point.id
            )));
        }
        if point.coordinates == before.coordinates {
            continue;
        }
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            return Err(CodecError::Malformed(format!(
                "F3D sketch point {} has non-finite coordinates",
                point.id
            )));
        }
        let stream = point
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":sketch-point#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-point id {}", point.id))
            })?;
        edits.entry(stream).or_default().push((
            point.byte_offset,
            point.coordinate_offset,
            point.coordinates,
        ));
    }
    Ok(edits)
}

pub(crate) fn validate_sketch_curve_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchCurveEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|curve| curve.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchCurveEdit>> = BTreeMap::new();
    for curve in target {
        let before = by_id[curve.id.as_str()];
        let mut normalized = curve.clone();
        normalized.geometry.clone_from(&before.geometry);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit changes fields other than geometry: {}",
                curve.id
            )));
        }
        if curve.geometry == before.geometry {
            continue;
        }
        let geometry = curve.geometry.clone().ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove sketch-curve geometry: {}", curve.id))
        })?;
        if !valid_sketch_geometry(&geometry)
            || !same_sketch_layout(before.geometry.as_ref(), &geometry)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit requires valid geometry with the original native layout: {}",
                curve.id
            )));
        }
        let stream = curve
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":sketch-curve-identity#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-curve id {}", curve.id))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((curve.byte_offset, curve.geometry_offset, geometry));
    }
    Ok(edits)
}

fn same_sketch_layout(before: Option<&SketchCurveGeometry>, after: &SketchCurveGeometry) -> bool {
    match (before, after) {
        (Some(SketchCurveGeometry::Line { .. }), SketchCurveGeometry::Line { .. })
        | (Some(SketchCurveGeometry::Arc { .. }), SketchCurveGeometry::Arc { .. }) => true,
        (
            Some(SketchCurveGeometry::Nurbs {
                carrier_reference: old_carrier,
                subtype_class_tag: old_tag,
                subtype_record_index: old_index,
                degree: old_degree,
                scalar_width: old_width,
                knots: old_knots,
                weights: old_weights,
                control_points: old_points,
                ..
            }),
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                scalar_width,
                knots,
                weights,
                control_points,
                ..
            },
        ) => {
            old_carrier == carrier_reference
                && old_tag == subtype_class_tag
                && old_index == subtype_record_index
                && old_degree == degree
                && old_width == scalar_width
                && old_knots.len() == knots.len()
                && old_weights.len() == weights.len()
                && old_points.len() == control_points.len()
        }
        _ => false,
    }
}

fn valid_sketch_geometry(geometry: &SketchCurveGeometry) -> bool {
    match geometry {
        SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        } => finite_point(*start) && finite_point(*end) && orthonormal_pair(*direction, *normal),
        SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            finite_point(*center)
                && orthonormal_pair(*normal, *reference_direction)
                && radius.is_finite()
                && *radius > 0.0
                && start_angle.is_finite()
                && end_angle.is_finite()
        }
        SketchCurveGeometry::Nurbs {
            degree,
            fit_tolerance,
            scalar_width,
            knots,
            weights,
            control_points,
            ..
        } => {
            *scalar_width == 8
                && fit_tolerance.is_finite()
                && *fit_tolerance >= 0.0
                && knots.len() == control_points.len() + *degree as usize + 1
                && knots.iter().all(|knot| knot.is_finite())
                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                && (weights.is_empty() || weights.len() == control_points.len())
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
                && control_points.iter().all(|point| finite_point(*point))
        }
    }
}

pub(crate) fn validate_sketch_relation_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchRelationEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|relation| (relation.id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|relation| relation.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-relation regeneration requires the unchanged relation-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchRelationEdit>> = BTreeMap::new();
    for relation in target {
        let before = by_id[relation.id.as_str()];
        let mut normalized = relation.clone();
        normalized.owner_reference = before.owner_reference;
        normalized
            .auxiliary_references
            .clone_from(&before.auxiliary_references);
        normalized.members.clone_from(&before.members);
        normalized.state = before.state;
        normalized
            .constraint_kinds
            .clone_from(&before.constraint_kinds);
        normalized.unknown_constraint_bits = before.unknown_constraint_bits;
        normalized.return_members.clone_from(&before.return_members);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-relation edit changes fields outside its writable references and constraint mask: {}",
                relation.id
            )));
        }
        if relation.state == before.state
            && relation.owner_reference == before.owner_reference
            && relation.auxiliary_references == before.auxiliary_references
            && relation.members == before.members
            && relation.return_members == before.return_members
        {
            continue;
        }
        let (kinds, unknown) =
            crate::design::decode::sketch::decode_constraint_kinds(relation.state);
        if kinds != relation.constraint_kinds || unknown != relation.unknown_constraint_bits {
            return Err(CodecError::Malformed(format!(
                "F3D sketch relation {} has a mask inconsistent with its typed constraint kinds",
                relation.id
            )));
        }
        let stream = relation
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":sketch-relation#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-relation id {}", relation.id))
            })?;
        let mut values = Vec::new();
        collect_sketch_reference_edits(
            relation,
            &before.members,
            &relation.members,
            &relation.member_offsets,
            &mut values,
        )?;
        collect_sketch_reference_edits(
            relation,
            &before.auxiliary_references,
            &relation.auxiliary_references,
            &relation.auxiliary_reference_offsets,
            &mut values,
        )?;
        if relation.owner_reference != before.owner_reference {
            values.push((
                relation.byte_offset + u64::from(relation.owner_reference_offset),
                relation.owner_reference.to_le_bytes().to_vec(),
            ));
        }
        collect_sketch_reference_edits(
            relation,
            &before.return_members,
            &relation.return_members,
            &relation.return_member_offsets,
            &mut values,
        )?;
        if relation.state != before.state {
            // The stored mask width follows the source form: a `0x01`-marked
            // u32 or an unmarked u64.
            let marked = usize::try_from(relation.state_offset)
                .ok()
                .and_then(|offset| offset.checked_sub(1))
                .and_then(|offset| relation.raw_bytes.get(offset))
                == Some(&1);
            let encoded = if marked {
                u32::try_from(relation.state)
                    .map_err(|_| {
                        CodecError::NotImplemented(format!(
                            "F3D sketch relation {} stores a u32 mask that cannot carry the requested 64-bit state",
                            relation.id
                        ))
                    })?
                    .to_le_bytes()
                    .to_vec()
            } else {
                relation.state.to_le_bytes().to_vec()
            };
            values.push((
                relation.byte_offset + u64::from(relation.state_offset),
                encoded,
            ));
        }
        edits.entry(stream).or_default().push(values);
    }
    Ok(edits)
}

fn collect_sketch_reference_edits(
    relation: &crate::records::SketchRelation,
    before: &[u32],
    after: &[u32],
    offsets: &[u32],
    edits: &mut Vec<(u64, Vec<u8>)>,
) -> Result<(), CodecError> {
    if before.len() != after.len() || after.len() != offsets.len() {
        return Err(CodecError::NotImplemented(format!(
            "F3D sketch relation {} must retain reference cardinality and offsets",
            relation.id
        )));
    }
    edits.extend(
        before
            .iter()
            .zip(after)
            .zip(offsets)
            .filter(|((before, after), _)| before != after)
            .map(|((_, after), offset)| {
                (
                    relation.byte_offset + u64::from(*offset),
                    after.to_le_bytes().to_vec(),
                )
            }),
    );
    Ok(())
}

pub(crate) fn validate_body_transform_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Transform>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-transform regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.transform = before.transform;
        normalized.color = before.color;
        normalized.visible = before.visible;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform, color, or visibility: {id}"
            )));
        }
        if after.transform == before.transform {
            continue;
        }
        let transform = after.transform.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body transform: {id}"))
        })?;
        if !valid_transform(transform) || before.transform.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D body transform {id} must replace an existing finite affine transform"
            )));
        }
        edits.insert(id.to_owned(), transform);
    }
    Ok(edits)
}

pub(crate) fn validate_body_color_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-color regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.transform = before.transform;
        normalized.visible = before.visible;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform, color, or visibility: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body color: {id}"))
        })?;
        if before.color.is_none()
            || ![color.r, color.g, color.b, color.a]
                .into_iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
            || color.a != 1.0
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D body color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

pub(crate) fn validate_edge_range_edits(
    baseline: &[Edge],
    target: &[Edge],
) -> Result<BTreeMap<String, [f64; 2]>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-range regeneration requires the unchanged edge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.param_range = before.param_range;
        normalized.tolerance = before.tolerance;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge edit changes fields other than parameter range: {id}"
            )));
        }
        if after.param_range == before.param_range {
            continue;
        }
        let range = after.param_range.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D edge range: {id}"))
        })?;
        if before.param_range.is_none()
            || !range[0].is_finite()
            || !range[1].is_finite()
            || range[0] == range[1]
        {
            return Err(CodecError::Malformed(format!(
                "edited F3D edge range {id} must replace an existing finite non-degenerate range"
            )));
        }
        edits.insert(id.to_owned(), range);
    }
    Ok(edits)
}

pub(crate) fn validate_face_sense_edits(
    baseline_ir: &CadIr,
    target_ir: &CadIr,
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = &baseline_ir.model.faces;
    let target = &target_ir.model.faces;
    let native_senses = f3d_native(baseline_ir)?
        .map(|native| {
            native
                .face_sidedness
                .into_iter()
                .map(|metadata| {
                    (
                        metadata.face,
                        (metadata.native_sense, metadata.normalized_sense),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sense regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.color = before.color;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            let (native_before, normalized_before) = native_senses
                .get(&before.id)
                .copied()
                .unwrap_or((before.sense, before.sense));
            let native_after =
                normalized_face_sense_to_native(after.sense, native_before, normalized_before);
            edits.insert(id.to_owned(), native_after);
        }
    }
    Ok(edits)
}

pub(crate) fn validate_face_color_edits(
    baseline: &[Face],
    target: &[Face],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-color regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense or color: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D face color: {id}"))
        })?;
        if before.color.is_none()
            || ![color.r, color.g, color.b, color.a]
                .into_iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
            || color.a != 1.0
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D face color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

pub(crate) fn validate_coedge_sense_edits(
    baseline: &[Coedge],
    target: &[Coedge],
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D coedge-sense regeneration requires the unchanged coedge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D coedge edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            edits.insert(id.to_owned(), after.sense);
        }
    }
    Ok(edits)
}

fn valid_transform(transform: Transform) -> bool {
    transform
        .rows
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        && transform.rows[3][0] == 0.0
        && transform.rows[3][1] == 0.0
        && transform.rows[3][2] == 0.0
        && transform.rows[3][3] != 0.0
}

pub(crate) fn validate_curve_edits(
    baseline: &[Curve],
    target: &[Curve],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        edited.insert(id.to_owned());
        let valid = match after {
            CurveGeometry::Line { origin, direction }
                if matches!(before, CurveGeometry::Line { .. }) =>
            {
                finite_point(*origin)
                    && finite_vector(*direction)
                    && (direction.norm() - 1.0).abs() <= 1e-9
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } if matches!(before, CurveGeometry::Circle { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius > 0.0
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } if matches!(before, CurveGeometry::Ellipse { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *major_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius > 0.0
                    && *minor_radius > 0.0
                    && *minor_radius <= *major_radius
            }
            CurveGeometry::Degenerate { point }
                if matches!(before, CurveGeometry::Degenerate { .. }) =>
            {
                finite_point(*point)
            }
            CurveGeometry::Nurbs(after) => {
                let CurveGeometry::Nurbs(before) = before else {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D regeneration cannot change curve {id} into a NURBS carrier"
                    )));
                };
                (id.starts_with("f3d:brep:entity#")
                    || id.starts_with("f3d:brep:tolerant-coedge-curve#")
                    || (id.starts_with("f3d:brep:procedural_surface#")
                        && (id.ends_with(":directrix") || id.ends_with(":spine"))))
                    && valid_edited_curve_structure(before, after)
                    && before.weights.is_some() == after.weights.is_some()
                    && before.control_points.len() == after.control_points.len()
                    && after.control_points.iter().copied().all(finite_point)
                    && after.weights.as_ref().is_none_or(|weights| {
                        weights.len() == after.control_points.len()
                            && weights
                                .iter()
                                .all(|weight| weight.is_finite() && *weight > 0.0)
                    })
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to curve {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D curve {id} has an invalid frame or radius"
            )));
        }
    }
    Ok(edited)
}

pub(crate) fn validate_pcurve_edits(
    baseline: &[Pcurve],
    target: &[Pcurve],
) -> Result<BTreeMap<String, NurbsPcurveEdit>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D pcurve regeneration requires the unchanged pcurve-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        let (
            PcurveGeometry::Nurbs {
                degree: _,
                knots: before_knots,
                control_points: before_points,
                weights: before_weights,
                periodic: before_periodic,
            },
            PcurveGeometry::Nurbs {
                degree: after_degree,
                knots: after_knots,
                control_points: after_points,
                weights: after_weights,
                periodic: after_periodic,
            },
        ) = (&before.geometry, &after.geometry)
        else {
            return Err(CodecError::NotImplemented(format!(
                "F3D regeneration does not support this pcurve edit: {id}"
            )));
        };
        let valid = id.starts_with("f3d:brep:entity#")
            && valid_edited_nurbs_direction(
                before_knots,
                *after_degree,
                after_knots,
                after_points.len(),
            )
            && before_points.len() == after_points.len()
            && before_weights.is_some() == after_weights.is_some()
            && before_weights.as_ref().map(Vec::len) == after_weights.as_ref().map(Vec::len)
            && after_weights.as_ref().is_none_or(|weights| {
                weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
            })
            && after_points
                .iter()
                .all(|point| point.u.is_finite() && point.v.is_finite());
        let contract_valid = before.wrapper_reversed.is_some() == after.wrapper_reversed.is_some()
            && before.native_tail_flags.is_some() == after.native_tail_flags.is_some()
            && before.parameter_range.is_some() == after.parameter_range.is_some()
            && before.fit_tolerance.is_some() == after.fit_tolerance.is_some()
            && after
                .parameter_range
                .is_none_or(|range| range.into_iter().all(f64::is_finite) && range[0] <= range[1])
            && after
                .fit_tolerance
                .is_none_or(|tolerance| tolerance.is_finite() && tolerance >= 0.0);
        if !valid || !contract_valid {
            return Err(CodecError::NotImplemented(format!(
                "F3D pcurve edit changes fixed cache structure: {id}"
            )));
        }
        edits.insert(
            id.to_owned(),
            NurbsPcurveEdit {
                geometry: after.geometry.clone(),
                periodic: (before_periodic != after_periodic).then_some(*after_periodic),
                wrapper_reversed: (before.wrapper_reversed != after.wrapper_reversed)
                    .then_some(after.wrapper_reversed)
                    .flatten(),
                native_tail_flags: (before.native_tail_flags != after.native_tail_flags)
                    .then_some(after.native_tail_flags)
                    .flatten(),
                parameter_range: (before.parameter_range != after.parameter_range)
                    .then_some(after.parameter_range)
                    .flatten(),
                fit_tolerance: (before.fit_tolerance != after.fit_tolerance)
                    .then_some(after.fit_tolerance)
                    .flatten(),
            },
        );
    }
    Ok(edits)
}

pub(crate) fn validate_surface_edits(
    baseline: &[Surface],
    target: &[Surface],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D surface regeneration requires the unchanged surface-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        let valid = match after {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } if matches!(before, SurfaceGeometry::Plane { .. }) => {
                finite_point(*origin) && orthonormal_pair(*normal, *u_axis)
            }
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } if matches!(before, SurfaceGeometry::Sphere { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } if matches!(before, SurfaceGeometry::Torus { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius != 0.0
                    && *minor_radius != 0.0
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if matches!(before, SurfaceGeometry::Cylinder { .. }) => {
                finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } if matches!(before, SurfaceGeometry::Cone { .. }) => {
                finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
                    && ratio.is_finite()
                    && *ratio > 0.0
                    && half_angle.is_finite()
                    && *half_angle >= 0.0
                    && *half_angle < std::f64::consts::FRAC_PI_2
            }
            SurfaceGeometry::Nurbs(after) => {
                let SurfaceGeometry::Nurbs(before) = before else {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D regeneration cannot change surface {id} into a NURBS carrier"
                    )));
                };
                (id.starts_with("f3d:brep:entity#")
                    || (id.starts_with("f3d:brep:procedural_surface#")
                        && (id.ends_with(":support0") || id.ends_with(":support1"))))
                    && valid_edited_nurbs_direction(
                        &before.u_knots,
                        after.u_degree,
                        &after.u_knots,
                        usize::try_from(after.u_count).unwrap_or(usize::MAX),
                    )
                    && valid_edited_nurbs_direction(
                        &before.v_knots,
                        after.v_degree,
                        &after.v_knots,
                        usize::try_from(after.v_count).unwrap_or(usize::MAX),
                    )
                    && before.u_count == after.u_count
                    && before.v_count == after.v_count
                    && before.weights.is_some() == after.weights.is_some()
                    && after.control_points.len()
                        == usize::try_from(after.u_count)
                            .ok()
                            .and_then(|u| {
                                usize::try_from(after.v_count)
                                    .ok()
                                    .and_then(|v| u.checked_mul(v))
                            })
                            .unwrap_or(usize::MAX)
                    && after.control_points.iter().copied().all(finite_point)
                    && after.weights.as_ref().is_none_or(|weights| {
                        weights.len() == after.control_points.len()
                            && weights
                                .iter()
                                .all(|weight| weight.is_finite() && *weight > 0.0)
                    })
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to surface {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D surface {id} requires a finite orthonormal frame and valid radius"
            )));
        }
        edited.insert(id.to_owned());
    }
    Ok(edited)
}

pub(crate) fn validate_procedural_surface_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, ProceduralSurfaceEdit>, CodecError> {
    let baseline = baseline
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D procedural-surface regeneration requires the unchanged construction-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after == before {
            continue;
        }
        if before.id != after.id || before.surface != after.surface {
            return Err(CodecError::NotImplemented(format!(
                "F3D procedural-surface edit changes immutable carrier fields: {id}"
            )));
        }
        let edit = match (&before.definition, &after.definition) {
            (
                ProceduralSurfaceDefinition::Extrusion {
                    directrix: before_directrix,
                    parameter_interval: before_parameter_interval,
                    direction: before_direction,
                    native_position: before_native_position,
                },
                ProceduralSurfaceDefinition::Extrusion {
                    directrix: after_directrix,
                    parameter_interval: after_parameter_interval,
                    direction: after_direction,
                    native_position: after_native_position,
                },
            ) if before_directrix == after_directrix => {
                let interval = after_parameter_interval.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D extrusion interval is missing: {id}"))
                })?;
                let position = after_native_position.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D extrusion native position is missing: {id}"))
                })?;
                if !interval.into_iter().all(f64::is_finite) || interval[0] >= interval[1] {
                    return Err(CodecError::Malformed(format!(
                        "F3D extrusion interval must be finite and ordered: {id}"
                    )));
                }
                if !finite_vector(*after_direction) || after_direction.norm() == 0.0 {
                    return Err(CodecError::Malformed(format!(
                        "F3D extrusion direction must be finite and nonzero: {id}"
                    )));
                }
                if ![position.x, position.y, position.z]
                    .into_iter()
                    .all(f64::is_finite)
                {
                    return Err(CodecError::Malformed(format!(
                        "F3D extrusion native position must be finite: {id}"
                    )));
                }
                (before_parameter_interval != after_parameter_interval
                    || before_direction != after_direction
                    || before_native_position != after_native_position)
                    .then_some(ProceduralSurfaceEdit::Extrusion {
                        parameter_interval: interval,
                        direction: *after_direction,
                        native_position: position,
                    })
            }
            (
                ProceduralSurfaceDefinition::Blend {
                    supports: before_supports,
                    spine: before_spine,
                    radius: before_radius,
                    cross_section: before_cross_section,
                    native: before_native,
                },
                ProceduralSurfaceDefinition::Blend {
                    supports: after_supports,
                    spine: after_spine,
                    radius: after_radius,
                    cross_section: after_cross_section,
                    native: after_native,
                },
            ) if before_supports == after_supports
                && before_spine == after_spine
                && before_cross_section == after_cross_section
                && before_native == after_native =>
            {
                let values = match after_radius {
                    BlendRadiusLaw::Constant { signed_radius } => [*signed_radius; 2],
                    BlendRadiusLaw::Linear { start, end } => [*start, *end],
                    BlendRadiusLaw::Law { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "F3D explicit blend-law regeneration is unsupported: {id}"
                        )));
                    }
                };
                if !values.into_iter().all(f64::is_finite) || values.contains(&0.0) {
                    return Err(CodecError::Malformed(format!(
                        "F3D rolling-ball radii must be finite and nonzero: {id}"
                    )));
                }
                (before_radius != after_radius).then_some(ProceduralSurfaceEdit::BlendRadii(values))
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D procedural-surface edit changes non-writable construction fields: {id}"
                )));
            }
        };
        if let Some(edit) = edit {
            edits.insert(id.to_owned(), edit);
        }
    }
    Ok(edits)
}

pub(crate) fn validate_procedural_surface_fit_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, f64>, CodecError> {
    let baseline = baseline
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after.cache_fit_tolerance == before.cache_fit_tolerance {
            continue;
        }
        let tolerance = after.cache_fit_tolerance.ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "cannot remove F3D procedural-surface fit tolerance: {id}"
            ))
        })?;
        if before.cache_fit_tolerance.is_none() || !tolerance.is_finite() || tolerance < 0.0 {
            return Err(CodecError::Malformed(format!(
                "F3D procedural-surface fit tolerance must replace a finite nonnegative value: {id}"
            )));
        }
        edits.insert(id.to_owned(), tolerance);
    }
    Ok(edits)
}

pub(crate) fn validate_procedural_curve_edits(
    baseline: &[ProceduralCurve],
    target: &[ProceduralCurve],
) -> Result<BTreeMap<String, ProceduralCurveEdit>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D procedural-curve regeneration requires the unchanged construction-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after.curve != before.curve {
            return Err(CodecError::NotImplemented(format!(
                "F3D procedural-curve edit changes its solved curve: {id}"
            )));
        }
        let definition = match (&before.definition, &after.definition) {
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix { .. },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix { .. },
            ) if before.definition != after.definition => Some(after.definition.clone()),
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
                    source: before_source,
                    labels: before_labels,
                    codes: before_codes,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
                    source: after_source,
                    labels: after_labels,
                    codes: after_codes,
                    ..
                },
            ) if before_source == after_source
                && before_labels == after_labels
                && before_codes == after_codes
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                    source: before_source,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                    source: after_source,
                    ..
                },
            ) if before_source == after_source && before.definition != after.definition => {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
                    context: before_context,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
                    context: after_context,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
                    context: before_context,
                    base: before_base,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
                    context: after_context,
                    base: after_base,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_base == after_base
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
                    context: before_context,
                    surface_parameter_ranges: before_surface_ranges,
                    first_pcurve_parameter_range: before_pcurve_range,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
                    context: after_context,
                    surface_parameter_ranges: after_surface_ranges,
                    first_pcurve_parameter_range: after_pcurve_range,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_surface_ranges == after_surface_ranges
                && before_pcurve_range == after_pcurve_range
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
                    context: before_context,
                    source: before_source,
                    tail: before_tail,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
                    context: after_context,
                    source: after_source,
                    tail: after_tail,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_source == after_source
                && match (before_tail, after_tail) {
                    (
                        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { .. },
                        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { .. },
                    ) => true,
                    (
                        cadmpeg_ir::geometry::ProjectionTail::Ranged {
                            role: before_role, ..
                        },
                        cadmpeg_ir::geometry::ProjectionTail::Ranged {
                            role: after_role, ..
                        },
                    ) => {
                        before_role.len() == after_role.len()
                            && matches!(after_role.as_str(), "surf1" | "surf2")
                    }
                    _ => false,
                }
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context: before_context,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context: after_context,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                    context: before_context,
                    third: before_third,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                    context: after_context,
                    third: after_third,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_third == after_third
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                    family: before_family,
                    context: before_context,
                    tail: before_tail,
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                    family: after_family,
                    context: after_context,
                    tail: after_tail,
                },
            ) if before_family == after_family
                && before_context.sides == after_context.sides
                && before_tail == after_tail
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
                    context: before_context,
                    silhouette: before_silhouette,
                    cast_surface: before_cast,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
                    context: after_context,
                    silhouette: after_silhouette,
                    cast_surface: after_cast,
                    ..
                },
            ) if before_context == after_context
                && std::mem::discriminant(before_silhouette)
                    == std::mem::discriminant(after_silhouette)
                && before_cast == after_cast
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
                    components: before_components,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
                    components: after_components,
                    ..
                },
            ) if before_components == after_components && before.definition != after.definition => {
                Some(after.definition.clone())
            }
            (before, after) if before == after => None,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D procedural-curve edit changes a non-writable definition: {id}"
                )))
            }
        };
        let fit_tolerance = if after.cache_fit_tolerance == before.cache_fit_tolerance {
            None
        } else {
            let tolerance = after.cache_fit_tolerance.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "cannot remove F3D procedural-curve fit tolerance: {id}"
                ))
            })?;
            if before.cache_fit_tolerance.is_none() || !tolerance.is_finite() || tolerance < 0.0 {
                return Err(CodecError::Malformed(format!(
                    "F3D procedural-curve fit tolerance must replace a finite nonnegative value: {id}"
                )));
            }
            Some(tolerance)
        };
        if definition.is_some() || fit_tolerance.is_some() {
            edits.insert(
                id.to_owned(),
                ProceduralCurveEdit {
                    definition,
                    fit_tolerance,
                },
            );
        }
    }
    Ok(edits)
}
