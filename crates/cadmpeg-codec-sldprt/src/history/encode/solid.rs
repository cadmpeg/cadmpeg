// SPDX-License-Identifier: Apache-2.0
//! Extrude, revolve, sweep, loft, rib, hole, pattern, and helical-sweep write encoders.

use super::super::{
    extrude_feature_op, face_selection_value, format_angle_rad, format_length_like,
    format_length_mm, format_point3_mm, format_vector3, is_extrude, is_loft, is_revolve, is_sweep,
    path_source, pattern_form, profile_source, require_count, require_direction,
    require_same_family, resolved_boolean_op, valid_direction, vertex_selection_value,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeExtent, FaceSelection, FeatureDefinition, HoleKind, Length, PatternForm,
    PatternKind, PatternSeed, ProfileRef, RevolveExtent, RibDraft, RibSide, SweepMode, Termination,
};

#[allow(
    clippy::unnecessary_wraps,
    reason = "Per-feature encoders use one fallible dispatch interface."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_extrude(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Extrude {
            profile,
            direction,
            start,
            extent,
            op,
            direction_source,
            solid,
            face_maker,
            inner_wire_taper,
            length_along_profile_normal,
            allow_multi_profile_faces,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
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
                    if matches!(side.termination, Termination::Unresolved)
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
                            CodecError::Malformed(format!(
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
                    Termination::Unresolved => {}
                    Termination::Blind { length } => {
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
                    Termination::ThroughAll => {
                        properties.insert("EndCondition".into(), "ThroughAll".into());
                    }
                    Termination::ThroughNext => {
                        properties.insert("EndCondition".into(), "ThroughNext".into());
                    }
                    Termination::ToFirst | Termination::ToLast | Termination::ToShape { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination",
                            feature.id
                        )));
                    }
                    Termination::ToFace { face, offset }
                        if face_selection_value(face).is_some() =>
                    {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToFace".into());
                        properties.insert("Face".into(), selection);
                        if let Some(offset) = offset {
                            parameters.insert("Depth".into(), format_length_mm(offset.0));
                        }
                    }
                    Termination::ToVertex { vertex }
                        if vertex_selection_value(vertex).is_some() =>
                    {
                        let selection = vertex_selection_value(vertex).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToVertex".into());
                        properties.insert("Vertex".into(), selection);
                    }
                    Termination::OffsetFromFace { face, offset }
                        if face_selection_value(face).is_some() =>
                    {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "OffsetFromFace".into());
                        properties.insert("Face".into(), selection);
                        parameters.insert("Depth".into(), format_length_mm(offset.0));
                    }
                    Termination::ToFace { .. }
                    | Termination::ToVertex { .. }
                    | Termination::OffsetFromFace { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination selection",
                            feature.id
                        )));
                    }
                    Termination::Angle { .. } => return Err(unsupported_extent()),
                },
                ExtrudeExtent::Symmetric { side } => match &side.termination {
                    Termination::Blind { length } => {
                        properties.insert("EndCondition".into(), "Symmetric".into());
                        parameters.insert("Depth".into(), format_length_mm(length.0));
                    }
                    _ => return Err(unsupported_extent()),
                },
                ExtrudeExtent::TwoSided { first, second } => {
                    match (&first.termination, &second.termination) {
                        (
                            Termination::Blind { length: first },
                            Termination::Blind { length: second },
                        ) => {
                            properties.insert("EndCondition".into(), "TwoSided".into());
                            parameters.insert("Depth".into(), format_length_mm(first.0));
                            parameters.insert("Depth2".into(), format_length_mm(second.0));
                        }
                        (Termination::ThroughAll, Termination::ThroughAll) => {
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
                    return Err(CodecError::Malformed(format!(
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
            (kind, parameters, properties)
        })
    }

    pub(super) fn encode_hole(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Hole {
            profile,
            profile_filter,
            face,
            position,
            direction,
            placements,
            kind,
            exit_kind,
            diameter,
            extent,
            bottom,
            taper_angle,
            specification,
            allow_multi_profile_faces,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if profile.is_some()
                || profile_filter.is_some()
                || position.is_some()
                || direction.is_some()
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
                HoleKind::Unresolved { .. } if existing.is_some() => {}
                HoleKind::Unresolved { .. } => {
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
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an empty hole face selection",
                        feature.id
                    )));
                }
                None => {
                    properties.remove("Face");
                }
            }
            match placements.as_slice() {
                [cadmpeg_ir::features::HolePlacement::Directed {
                    position,
                    direction,
                }] => {
                    if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite()
                    {
                        return Err(CodecError::Malformed(format!(
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
                Some(Termination::Blind {
                    length: Length(depth),
                }) => {
                    parameters.insert("Depth".into(), format_length_mm(*depth));
                    properties.insert("EndCondition".into(), "Blind".into());
                }
                Some(Termination::ThroughAll) => {
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
            (
                existing.map_or_else(|| "Hole".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_revolve(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Revolve { construction, op } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if construction.axis_reference.is_some()
                || construction.solid == Some(false)
                || construction.face_maker_class.is_some()
                || construction.fuse_order.is_some()
                || construction.allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported revolution construction controls",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_revolve(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported revolution semantics",
                    feature.id
                )));
            }
            if existing.is_none()
                && (construction.profile.is_none()
                    || construction.axis.is_none()
                    || construction.extent.is_none()
                    || *op == BooleanOp::Unresolved)
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved revolution construction",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            if let Some(extent) = &construction.extent {
                parameters.remove("Angle");
                parameters.remove("Angle2");
                match extent {
                    RevolveExtent::OneSided {
                        termination: Termination::Angle { angle },
                    } => {
                        properties.insert("EndCondition".into(), "OneSided".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    RevolveExtent::Symmetric {
                        termination: Termination::Angle { angle },
                    } => {
                        properties.insert("EndCondition".into(), "Symmetric".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    RevolveExtent::TwoSided {
                        first: Termination::Angle { angle: first },
                        second: Termination::Angle { angle: second },
                    } => {
                        properties.insert("EndCondition".into(), "TwoSided".into());
                        parameters.insert("Angle".into(), format_angle_rad(first.0));
                        parameters.insert("Angle2".into(), format_angle_rad(second.0));
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses a linear revolution extent",
                            feature.id
                        )));
                    }
                }
            }
            if let Some(axis) = construction.axis {
                if !valid_direction(axis.direction) {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a degenerate revolution axis",
                        feature.id
                    )));
                }
                properties.insert("AxisOrigin".into(), format_point3_mm(axis.origin));
                properties.insert("AxisDirection".into(), format_vector3(axis.direction));
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            if let Some(profile) = &construction.profile {
                let profile_source =
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing revolution profile",
                                feature.id
                            ))
                        })?;
                properties.insert("Profile".into(), profile_source);
            }
            (
                existing.map_or_else(|| "Revolve".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_sweep(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Sweep {
            section,
            sections,
            path,
            mode,
            orientation,
            transition,
            transformation,
            path_tangent,
            linearize,
            twist,
            path_extent,
            guide_rail,
            taper,
            scale,
            allow_multi_profile_faces,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if !sections.is_empty()
                || orientation.is_some()
                || transition.is_some()
                || transformation.is_some()
                || *path_tangent
                || *linearize
                || path_extent.is_some()
                || guide_rail.is_some()
                || taper.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported sweep construction semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_sweep(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes operation family",
                    feature.id
                )));
            }
            let profile_source =
                match section {
                    cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Generated {
                        ..
                    }) if existing.is_some() => None,
                    cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(_))
                        if existing
                            .is_some_and(|record| !record.properties.contains_key("Profile")) =>
                    {
                        None
                    }
                    cadmpeg_ir::features::SweepSection::Profile(profile) => Some(
                        profile_source(profile, record_sources, feature_sources, sketch_sources)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "SLDPRT feature {} references a missing sweep profile",
                                    feature.id
                                ))
                            })?,
                    ),
                    cadmpeg_ir::features::SweepSection::Unresolved(_) => None,
                    cadmpeg_ir::features::SweepSection::Generated(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported generated sweep section",
                            feature.id
                        )));
                    }
                };
            let path_source = match path {
                Some(path) => Some(
                    path_source(path, record_sources, sketch_sources).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing sweep path",
                            feature.id
                        ))
                    })?,
                ),
                None => None,
            };
            if existing.is_none() && (profile_source.is_none() || path_source.is_none()) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved sweep operands",
                    feature.id
                )));
            }
            if existing.is_none() && *mode == SweepMode::Unresolved {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved sweep result semantics",
                    feature.id
                )));
            }
            if existing.is_none()
                && matches!(
                    mode,
                    SweepMode::Solid {
                        op: BooleanOp::Unresolved
                    }
                )
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has an unresolved boolean operation",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            match twist {
                Some(twist) => {
                    parameters.insert("Twist".into(), format_angle_rad(twist.0));
                }
                None => {
                    parameters.remove("Twist");
                }
            }
            match scale {
                Some(scale) if scale.is_finite() && *scale > 0.0 => {
                    parameters.insert("Scale".into(), scale.to_string());
                }
                Some(_) => {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid sweep scale",
                        feature.id
                    )))
                }
                None => {
                    parameters.remove("Scale");
                }
            }
            let mut properties = feature.source_properties.clone();
            if let Some(profile) = profile_source {
                properties.insert("Profile".into(), profile);
            }
            if let Some(path) = path_source {
                properties.insert("Path".into(), path);
            }
            match mode {
                SweepMode::Solid { op } if *op != BooleanOp::Unresolved => {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op(*op, &feature.id)?.into(),
                    );
                }
                SweepMode::Solid { .. } => {}
                SweepMode::Surface => {
                    properties.remove("Operation");
                }
                SweepMode::Unresolved => {}
            }
            (
                existing.map_or_else(
                    || {
                        match mode {
                            SweepMode::Surface => "Surface-Sweep",
                            SweepMode::Solid { .. } | SweepMode::Unresolved => "Sweep",
                        }
                        .into()
                    },
                    |record| record.kind.clone(),
                ),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_loft(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Loft {
            sections,
            guides,
            centerline,
            op,
            closed,
            solid,
            ruled,
            max_degree,
            check_compatibility,
            allow_multi_profile_faces,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if centerline.is_some()
                || !solid
                || *ruled
                || max_degree.is_some()
                || check_compatibility.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported loft result semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_loft(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported loft semantics",
                    feature.id
                )));
            }
            if existing.is_none() && (sections.len() < 2 || *op == BooleanOp::Unresolved) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved loft construction semantics",
                    feature.id
                )));
            }
            if sections
                .iter()
                .any(|section| matches!(section, cadmpeg_ir::features::LoftSection::Point(_)))
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses a point loft section",
                    feature.id
                )));
            }
            let profile_sources = sections
                .iter()
                .filter_map(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => Some(profile),
                    cadmpeg_ir::features::LoftSection::Point(_) => None,
                })
                .map(|profile| {
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} references a missing loft profile",
                        feature.id
                    ))
                })?;
            let guide_sources = guides
                .iter()
                .map(|path| path_source(path, record_sources, sketch_sources))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} references a missing loft guide",
                        feature.id
                    ))
                })?;
            let mut properties = feature.source_properties.clone();
            if !profile_sources.is_empty() || existing.is_none() {
                properties.insert("Profiles".into(), profile_sources.join(","));
            }
            if guide_sources.is_empty() && existing.is_none() {
                properties.remove("Guides");
            } else if !guide_sources.is_empty() {
                properties.insert("Guides".into(), guide_sources.join(","));
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            if *closed || existing.is_none() || properties.contains_key("Closed") {
                properties.insert("Closed".into(), closed.to_string());
            }
            (
                existing.map_or_else(|| "Loft".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_rib(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Rib { construction, op } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            require_same_family(existing, &feature.id, &["Rib"])?;
            if existing.is_none()
                && (construction.profile.is_none()
                    || construction.direction.is_none()
                    || construction.thickness.is_none()
                    || construction.side.is_none()
                    || construction.draft == RibDraft::Unresolved
                    || *op == BooleanOp::Unresolved)
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved rib construction",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            if let Some(thickness) = construction.thickness {
                parameters.insert("Thickness".into(), format_length_mm(thickness.0));
            }
            match construction.draft {
                RibDraft::Angle(draft) => {
                    parameters.insert("Draft".into(), format_angle_rad(draft.0));
                }
                RibDraft::None => {
                    parameters.remove("Draft");
                }
                RibDraft::Unresolved => {}
            }
            let mut properties = feature.source_properties.clone();
            if let Some(profile) = &construction.profile {
                let profile_source =
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing rib profile",
                                feature.id
                            ))
                        })?;
                properties.insert("Profile".into(), profile_source);
            }
            if let Some(direction) = construction.direction {
                require_direction(direction, &feature.id, "rib direction")?;
                properties.insert("Direction".into(), format_vector3(direction));
            }
            if let Some(side) = construction.side {
                properties.insert("BothSides".into(), (side == RibSide::Centered).to_string());
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            (
                existing.map_or_else(|| "Rib".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_pattern(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Pattern { seeds, pattern } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        let parent_sources = self.parent_sources;
        Ok({
            let expected_form = match pattern {
                PatternKind::Unresolved { form } => *form,
                PatternKind::Linear { .. } | PatternKind::LinearOffsets { .. } => {
                    Some(PatternForm::Linear)
                }
                PatternKind::Circular { .. } | PatternKind::CircularAngles { .. } => {
                    Some(PatternForm::Circular)
                }
                PatternKind::CurveDriven { .. } => Some(PatternForm::CurveDriven),
                PatternKind::Mirror { .. } | PatternKind::MirrorReference { .. } => {
                    Some(PatternForm::Mirror)
                }
                PatternKind::Scale { .. } => Some(PatternForm::Scale),
                PatternKind::Composite { .. } => Some(PatternForm::Composite),
            };
            if existing.is_some_and(|record| {
                expected_form.is_some_and(|form| pattern_form(record) != Some(form))
            }) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes pattern form",
                    feature.id
                )));
            }
            let mut seed_sources = Vec::new();
            for seed in seeds {
                match seed {
                    PatternSeed::Feature(seed) => {
                        seed_sources.push(parent_sources.get(seed).cloned().ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing pattern seed",
                                feature.id
                            ))
                        })?);
                    }
                    PatternSeed::Faces(_) | PatternSeed::Bodies(_) if existing.is_some() => {}
                    PatternSeed::Faces(_) | PatternSeed::Bodies(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has a source-less topology pattern seed",
                            feature.id
                        )));
                    }
                    PatternSeed::Occurrences(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has component-occurrence pattern seeds",
                            feature.id
                        )));
                    }
                }
            }
            if seed_sources.is_empty()
                && (!matches!(
                    expected_form,
                    Some(PatternForm::Linear | PatternForm::CurveDriven)
                ) || existing.is_none())
                && !matches!(pattern, PatternKind::Unresolved { .. })
            {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has no pattern seeds",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            if !seed_sources.is_empty() {
                properties.insert("Seeds".into(), seed_sources.join(","));
            }
            match pattern {
                PatternKind::Unresolved { .. } => {
                    if existing.is_none() {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved pattern construction",
                            feature.id
                        )));
                    }
                }
                PatternKind::Linear {
                    direction,
                    spacing,
                    count,
                    second,
                } => {
                    match direction {
                        Some(direction) => {
                            require_direction(*direction, &feature.id, "pattern")?;
                            properties.insert("Direction".into(), format_vector3(*direction));
                        }
                        None if existing.is_some() => {}
                        None => {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved pattern direction",
                                feature.id
                            )));
                        }
                    }
                    require_count(*count, &feature.id)?;
                    if !spacing.0.is_finite() || spacing.0 <= 0.0 {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has invalid linear-pattern spacing",
                            feature.id
                        )));
                    }
                    let spacing_key =
                        if parameters.contains_key("D3") && !parameters.contains_key("Spacing") {
                            "D3"
                        } else {
                            "Spacing"
                        };
                    let count_key =
                        if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                            "D1"
                        } else {
                            "Count"
                        };
                    parameters.insert(
                        spacing_key.into(),
                        format_length_like(
                            spacing.0,
                            existing
                                .and_then(|record| record.parameters.get(spacing_key))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(count_key.into(), count.to_string());
                    if let Some(second) = second {
                        require_direction(second.direction, &feature.id, "second pattern")?;
                        require_count(second.count, &feature.id)?;
                        if !second.spacing.0.is_finite() || second.spacing.0 <= 0.0 {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has invalid second linear-pattern spacing",
                                feature.id
                            )));
                        }
                        properties.insert("Direction2".into(), format_vector3(second.direction));
                        parameters.insert("D4".into(), format_length_like(second.spacing.0, None));
                        parameters.insert("D2".into(), second.count.to_string());
                    }
                }
                PatternKind::Circular {
                    axis_origin,
                    axis_dir,
                    angle,
                    count,
                } => {
                    require_direction(*axis_dir, &feature.id, "pattern axis")?;
                    require_count(*count, &feature.id)?;
                    properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                    properties.insert("AxisDirection".into(), format_vector3(*axis_dir));
                    parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    parameters.insert("Count".into(), count.to_string());
                }
                PatternKind::CurveDriven {
                    path,
                    spacing,
                    count,
                } => {
                    require_count(*count, &feature.id)?;
                    if !spacing.0.is_finite() || spacing.0 <= 0.0 {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has invalid curve-pattern spacing",
                            feature.id
                        )));
                    }
                    match path {
                        Some(path) => {
                            let path = path_source(path, record_sources, sketch_sources)
                                .ok_or_else(|| {
                                    CodecError::Malformed(format!(
                                        "SLDPRT feature {} references a missing pattern path",
                                        feature.id
                                    ))
                                })?;
                            properties.insert("Path".into(), path);
                        }
                        None if existing.is_some() => {}
                        None => {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved curve-pattern path",
                                feature.id
                            )));
                        }
                    }
                    let spacing_key =
                        if parameters.contains_key("D3") && !parameters.contains_key("Spacing") {
                            "D3"
                        } else {
                            "Spacing"
                        };
                    let count_key =
                        if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                            "D1"
                        } else {
                            "Count"
                        };
                    parameters.insert(
                        spacing_key.into(),
                        format_length_like(
                            spacing.0,
                            existing
                                .and_then(|record| record.parameters.get(spacing_key))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(count_key.into(), count.to_string());
                }
                PatternKind::Mirror {
                    plane_origin,
                    plane_normal,
                } => {
                    require_direction(*plane_normal, &feature.id, "mirror plane normal")?;
                    properties.insert("PlaneOrigin".into(), format_point3_mm(*plane_origin));
                    properties.insert("PlaneNormal".into(), format_vector3(*plane_normal));
                }
                PatternKind::MirrorReference { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved mirror plane",
                        feature.id
                    )));
                }
                PatternKind::LinearOffsets { .. }
                | PatternKind::CircularAngles { .. }
                | PatternKind::Scale { .. }
                | PatternKind::Composite { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses a pattern form that cannot be written",
                        feature.id
                    )));
                }
            }
            let kind = existing.map_or_else(
                || match expected_form {
                    Some(PatternForm::Linear) => "LinearPattern".into(),
                    Some(PatternForm::Circular) => "CircularPattern".into(),
                    Some(PatternForm::CurveDriven) => "CrvPattern".into(),
                    Some(PatternForm::Mirror) => "Mirror".into(),
                    Some(PatternForm::Scale | PatternForm::Composite) => "Pattern".into(),
                    None => "Pattern".into(),
                },
                |record| record.kind.clone(),
            );
            (kind, parameters, properties)
        })
    }

    pub(super) fn encode_helical_sweep(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::HelicalSweep { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses a helical sweep that cannot be written",
            feature.id
        )))
    }
}
