// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole write encoders.

use super::super::{format_angle_rad, format_length_mm};
use super::format::{format_length_like, format_point3_mm, format_vector3};
use super::support::{
    face_selection_value, profile_source, require_direction, resolved_boolean_op,
    vertex_selection_value,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use crate::history::classify::{extrude_feature_op, is_extrude};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Angle, BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeStart, ExtrusionDirectionSource,
    ExtrusionFaceMaker, FaceSelection, HoleBottom, HoleKind, HolePlacement, HoleProfileFilter,
    HoleSpecification, InnerWireTaper, Length, LinearTermination, ProfileRef,
};

#[allow(
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    clippy::ptr_arg,
    reason = "Encoder arguments are borrowed from one FeatureDefinition match."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_extrude(
        &self,
        profile: &ProfileRef,
        direction: &ExtrudeDirection,
        start: &ExtrudeStart,
        extent: &ExtrudeExtent,
        op: &BooleanOp,
        direction_source: &Option<ExtrusionDirectionSource>,
        solid: &Option<bool>,
        face_maker: &Option<ExtrusionFaceMaker>,
        inner_wire_taper: &Option<InnerWireTaper>,
        length_along_profile_normal: &Option<bool>,
        allow_multi_profile_faces: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        let resolved_parameter_names = self.resolved_parameter_names;
        Ok({
            // Writer accepts only a first-side draft; second-side draft or
            // any side offset is rejected.
            let (first_draft, second_side_draft, any_side_offset) = match extent {
                ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                    (side.draft, None, side.offset.is_some())
                }
                ExtrudeExtent::TwoSided { first, second } => (
                    first.draft,
                    second.draft,
                    first.offset.is_some() || second.offset.is_some(),
                ),
            };
            let extent_is_unresolved = matches!(
                extent,
                ExtrudeExtent::OneSided { side }
                if matches!(side.termination, LinearTermination::Unresolved)
            );
            if !matches!(start, cadmpeg_ir::features::ExtrudeStart::ProfilePlane)
                || second_side_draft.is_some()
                || direction_source.is_some()
                || *solid == Some(false)
                || face_maker.is_some()
                || inner_wire_taper.is_some()
                || any_side_offset
                || length_along_profile_normal.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported extrusion construction controls",
                    feature.id
                )));
            }
            if *op == BooleanOp::Unresolved && existing.is_none() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires retained extrusion operation data",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_extrude(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported extrusion semantics",
                    feature.id
                )));
            }
            if let ProfileRef::Unresolved(owner) = profile {
                let retained = existing.is_some_and(|record| {
                    record.id == *owner && !record.properties.contains_key("Profile")
                });
                if !retained {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires retained extrusion profile data",
                        feature.id
                    )));
                }
            }
            let implicit_profile = existing.is_some_and(|record| {
                !record.properties.contains_key("Profile")
                    && (matches!(profile, ProfileRef::Unresolved(owner) if owner == &record.id)
                        || matches!(profile, ProfileRef::Native(native) if native == &record.id))
            });
            let profile_source = if implicit_profile {
                None
            } else {
                Some(
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "SLDPRT feature {} references a missing extrusion profile",
                                feature.id
                            ))
                        })?,
                )
            };
            if let Some(record) = existing {
                if !record.properties.contains_key("Operation")
                    && *op != BooleanOp::Unresolved
                    && extrude_feature_op(record).is_some_and(|native_op| native_op != *op)
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes its inferred extrusion operation",
                        feature.id
                    )));
                }
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let positional_depth = (parameters.contains_key("D1")
                || existing.is_some_and(|record| {
                    resolved_parameter_names
                        .get(&record.id)
                        .is_some_and(|names| names.contains("D1"))
                }))
                && !parameters.contains_key("Depth");
            let mut properties = feature.source_properties.clone();
            if extent_is_unresolved && existing.is_none() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires retained extrusion extent data",
                    feature.id
                )));
            }
            if !extent_is_unresolved {
                parameters.remove("Depth");
                parameters.remove("Depth2");
                parameters.remove("Draft");
                properties.remove("Direction");
                properties.remove("Face");
                properties.remove("Vertex");
            }
            let unsupported_extent = || {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses an unsupported extrusion extent",
                    feature.id
                ))
            };
            match extent {
                ExtrudeExtent::OneSided { side } => match &side.termination {
                    LinearTermination::Unresolved => {}
                    LinearTermination::Blind { length } => {
                        if properties.contains_key("EndCondition") || existing.is_none() {
                            properties.insert("EndCondition".into(), "Blind".into());
                        }
                        let key = if positional_depth { "D1" } else { "Depth" };
                        parameters.insert(
                            key.into(),
                            format_length_like(
                                length.0,
                                existing
                                    .and_then(|record| record.parameters.get(key))
                                    .map(String::as_str),
                            ),
                        );
                    }
                    LinearTermination::ThroughAll => {
                        properties.insert("EndCondition".into(), "ThroughAll".into());
                    }
                    LinearTermination::ThroughNext => {
                        properties.insert("EndCondition".into(), "ThroughNext".into());
                    }
                    LinearTermination::ToFirst
                    | LinearTermination::ToLast
                    | LinearTermination::ToShape { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination",
                            feature.id
                        )));
                    }
                    LinearTermination::ToFace { face, offset }
                        if face_selection_value(face).is_some() =>
                    {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToFace".into());
                        properties.insert("Face".into(), selection);
                        if let Some(offset) = offset {
                            parameters.insert("Depth".into(), format_length_mm(offset.0));
                        }
                    }
                    LinearTermination::ToVertex { vertex }
                        if vertex_selection_value(vertex).is_some() =>
                    {
                        let selection = vertex_selection_value(vertex).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToVertex".into());
                        properties.insert("Vertex".into(), selection);
                    }
                    LinearTermination::OffsetFromFace { face, offset }
                        if face_selection_value(face).is_some() =>
                    {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "OffsetFromFace".into());
                        properties.insert("Face".into(), selection);
                        parameters.insert("Depth".into(), format_length_mm(offset.0));
                    }
                    LinearTermination::ToFace { .. }
                    | LinearTermination::ToVertex { .. }
                    | LinearTermination::OffsetFromFace { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination selection",
                            feature.id
                        )));
                    }
                },
                ExtrudeExtent::Symmetric { side } => match &side.termination {
                    LinearTermination::Blind { length } => {
                        properties.insert("EndCondition".into(), "Symmetric".into());
                        parameters.insert("Depth".into(), format_length_mm(length.0));
                    }
                    _ => return Err(unsupported_extent()),
                },
                ExtrudeExtent::TwoSided { first, second } => {
                    match (&first.termination, &second.termination) {
                        (
                            LinearTermination::Blind { length: first },
                            LinearTermination::Blind { length: second },
                        ) => {
                            properties.insert("EndCondition".into(), "TwoSided".into());
                            parameters.insert("Depth".into(), format_length_mm(first.0));
                            parameters.insert("Depth2".into(), format_length_mm(second.0));
                        }
                        (LinearTermination::ThroughAll, LinearTermination::ThroughAll) => {
                            properties.insert("EndCondition".into(), "ThroughAllBoth".into());
                        }
                        _ => return Err(unsupported_extent()),
                    }
                }
            }
            match direction {
                cadmpeg_ir::features::ExtrudeDirection::Unresolved => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal => {
                    properties.remove("Direction");
                }
                cadmpeg_ir::features::ExtrudeDirection::ReversedProfileNormal => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses a reversed profile-normal extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::ExtrudeDirection::Explicit(direction) => {
                    require_direction(*direction, &feature.id, "extrusion direction")?;
                    properties.insert("Direction".into(), format_vector3(*direction));
                }
            }
            if let Some(draft) = first_draft {
                if !draft.0.is_finite() {
                    return Err(CodecError::malformed(format_args!(
                        "SLDPRT feature {} has a non-finite extrusion draft",
                        feature.id
                    )));
                }
                parameters.insert("Draft".into(), format_angle_rad(draft.0));
            }
            if *op != BooleanOp::Unresolved
                && (properties.contains_key("Operation")
                    || existing.and_then(extrude_feature_op).is_none())
            {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            if !implicit_profile {
                properties.insert(
                    "Profile".into(),
                    profile_source.expect("non-implicit profile was resolved"),
                );
            }
            let kind = existing.map_or_else(
                || match op {
                    BooleanOp::Unresolved => "Extrusion".into(),
                    BooleanOp::Join => "BossExtrude".into(),
                    BooleanOp::Cut => "CutExtrude".into(),
                    BooleanOp::NewBody | BooleanOp::Intersect => "Extrusion".into(),
                },
                |record| record.kind.clone(),
            );
            NeutralFeatureEncoding {
                kind,
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_hole(
        &self,
        profile: &Option<ProfileRef>,
        profile_filter: &Option<HoleProfileFilter>,
        face: &Option<FaceSelection>,
        placements: &Option<Vec<HolePlacement>>,
        kind: &HoleKind,
        exit_kind: &Option<HoleKind>,
        diameter: &Option<Length>,
        extent: &Option<LinearTermination>,
        bottom: &Option<HoleBottom>,
        taper_angle: &Option<Angle>,
        specification: &Option<Box<HoleSpecification>>,
        allow_multi_profile_faces: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if profile.is_some()
                || profile_filter.is_some()
                || exit_kind.is_some()
                || bottom.is_some()
                || taper_angle.is_some()
                || specification.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported hole construction semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| classify(record) != Some(FeatureClass::Hole)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported hole semantics",
                    feature.id
                )));
            }
            if existing.is_none() && diameter.is_none() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has an unresolved hole diameter",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            if let Some(diameter) = diameter {
                parameters.insert("Diameter".into(), format_length_mm(diameter.0));
            }
            match kind {
                HoleKind::Unresolved(_)
                | HoleKind::PartialCounterbore { .. }
                | HoleKind::PartialCountersink { .. }
                    if existing.is_some() => {}
                HoleKind::Unresolved(_)
                | HoleKind::PartialCounterbore { .. }
                | HoleKind::PartialCountersink { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved hole entry construction",
                        feature.id
                    )));
                }
                HoleKind::Simple => {
                    parameters.remove("CounterboreDiameter");
                    parameters.remove("CounterboreDepth");
                    parameters.remove("CountersinkDiameter");
                    parameters.remove("CountersinkAngle");
                    parameters.remove("ThreadMajorDiameter");
                    parameters.remove("ThreadDepth");
                    parameters.remove("ThreadPitch");
                    parameters.remove("DrillPointAngle");
                }
                HoleKind::SimpleDrilled { drill_point_angle } => {
                    parameters.remove("CounterboreDiameter");
                    parameters.remove("CounterboreDepth");
                    parameters.remove("CountersinkDiameter");
                    parameters.remove("CountersinkAngle");
                    parameters.remove("ThreadMajorDiameter");
                    parameters.remove("ThreadDepth");
                    parameters.remove("ThreadPitch");
                    parameters.insert(
                        "DrillPointAngle".into(),
                        format_angle_rad(drill_point_angle.0),
                    );
                }
                HoleKind::Counterbore { diameter, depth } => {
                    parameters.remove("CountersinkDiameter");
                    parameters.remove("CountersinkAngle");
                    parameters.remove("ThreadMajorDiameter");
                    parameters.remove("ThreadDepth");
                    parameters.remove("ThreadPitch");
                    parameters.insert("CounterboreDiameter".into(), format_length_mm(diameter.0));
                    parameters.insert("CounterboreDepth".into(), format_length_mm(depth.0));
                    parameters.remove("DrillPointAngle");
                }
                HoleKind::CounterboreDrilled {
                    diameter,
                    depth,
                    drill_point_angle,
                } => {
                    parameters.remove("CountersinkDiameter");
                    parameters.remove("CountersinkAngle");
                    parameters.remove("ThreadMajorDiameter");
                    parameters.remove("ThreadDepth");
                    parameters.remove("ThreadPitch");
                    parameters.insert("CounterboreDiameter".into(), format_length_mm(diameter.0));
                    parameters.insert("CounterboreDepth".into(), format_length_mm(depth.0));
                    parameters.insert(
                        "DrillPointAngle".into(),
                        format_angle_rad(drill_point_angle.0),
                    );
                }
                HoleKind::Countersink { diameter, angle } => {
                    parameters.remove("CounterboreDiameter");
                    parameters.remove("CounterboreDepth");
                    parameters.remove("ThreadMajorDiameter");
                    parameters.remove("ThreadDepth");
                    parameters.remove("ThreadPitch");
                    parameters.remove("DrillPointAngle");
                    parameters.insert("CountersinkDiameter".into(), format_length_mm(diameter.0));
                    parameters.insert("CountersinkAngle".into(), format_angle_rad(angle.0));
                }
                HoleKind::Threaded {
                    major_diameter,
                    thread_depth,
                    pitch,
                    drill_point_angle,
                } => {
                    parameters.remove("CounterboreDiameter");
                    parameters.remove("CounterboreDepth");
                    parameters.remove("CountersinkDiameter");
                    parameters.remove("CountersinkAngle");
                    parameters.insert(
                        "ThreadMajorDiameter".into(),
                        format_length_mm(major_diameter.0),
                    );
                    parameters.insert("ThreadDepth".into(), format_length_mm(thread_depth.0));
                    if let Some(pitch) = pitch {
                        parameters.insert("ThreadPitch".into(), format_length_mm(pitch.0));
                    } else {
                        parameters.remove("ThreadPitch");
                    }
                    parameters.insert(
                        "DrillPointAngle".into(),
                        format_angle_rad(drill_point_angle.0),
                    );
                }
                HoleKind::Counterdrill { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unsupported counterdrill construction",
                        feature.id
                    )));
                }
                HoleKind::Chamfer { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unsupported chamfered-hole construction",
                        feature.id
                    )));
                }
            }
            let mut properties = feature.source_properties.clone();
            match face {
                Some(face) if face_selection_value(face).is_some() => {
                    properties.insert(
                        "Face".into(),
                        face_selection_value(face).expect("guarded above"),
                    );
                }
                Some(FaceSelection::Unresolved) if existing.is_some() => {}
                Some(FaceSelection::Unresolved) => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved hole face selection",
                        feature.id
                    )));
                }
                Some(_) => {
                    return Err(CodecError::malformed(format_args!(
                        "SLDPRT feature {} has an empty hole face selection",
                        feature.id
                    )));
                }
                None => {
                    properties.remove("Face");
                }
            }
            match placements.as_deref().unwrap_or_default() {
                [cadmpeg_ir::features::HolePlacement::Directed {
                    position,
                    direction,
                }] => {
                    if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite()
                    {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has a non-finite hole position",
                            feature.id
                        )));
                    }
                    require_direction(*direction, &feature.id, "hole direction")?;
                    properties.insert("Position".into(), format_point3_mm(*position));
                    properties.insert("Direction".into(), format_vector3(*direction));
                }
                [] if existing.is_none() => {
                    properties.remove("Position");
                    properties.remove("Direction");
                }
                [] => {}
                placements
                    if existing.is_some()
                        && placements.iter().all(|placement| {
                            matches!(placement, cadmpeg_ir::features::HolePlacement::Axis { .. })
                        }) => {}
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                                       "SLDPRT feature {} has placements that require native generated-surface identities",
                                       feature.id
                                   )));
                }
            }
            match extent {
                Some(LinearTermination::Blind {
                    length: Length(depth),
                }) => {
                    parameters.insert("Depth".into(), format_length_mm(*depth));
                    properties.insert("EndCondition".into(), "Blind".into());
                }
                Some(LinearTermination::ThroughAll) => {
                    parameters.remove("Depth");
                    properties.insert("EndCondition".into(), "ThroughAll".into());
                }
                Some(_) => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported hole termination",
                        feature.id
                    )))
                }
                None if existing.is_some() => {}
                None => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved hole termination",
                        feature.id
                    )))
                }
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Hole".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }
}
