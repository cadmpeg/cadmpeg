// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole write encoders.

use super::super::extrude_direction::{cleared_reversal, reversal_flag, REVERSE_DECODED};
use super::super::literals::{format_angle_rad, format_length_mm};
use super::format::{format_length_like, format_point3_mm, format_vector3};
use super::support::{
    face_selection_value, profile_source, resolved_boolean_op, vertex_selection_value,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use crate::history::classify::{extrude_feature_op, is_extrude};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::holes::{
    HoleBottom, HoleConstruction, HoleKind, HolePlacement, HoleProfileFilter,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::sweeps::{FaceMaker, InnerWireTaper};

/// The decoded fields of one `Extrude` operation, borrowed from the feature definition.
#[derive(Clone, Copy)]
pub(super) struct ExtrudeDefinition<'a> {
    pub(super) profile: &'a ProfileRef,
    pub(super) direction: &'a ExtrudeDirection,
    pub(super) start: &'a ExtrudeStart,
    pub(super) extent: &'a ExtrudeExtent,
    pub(super) op: &'a BooleanOp,
    pub(super) solid: Option<bool>,
    pub(super) face_maker: Option<&'a FaceMaker>,
    pub(super) inner_wire_taper: Option<&'a InnerWireTaper>,
    pub(super) length_along_profile_normal: Option<bool>,
    pub(super) allow_multi_profile_faces: Option<bool>,
}

/// The decoded fields of one `Hole` operation, borrowed from the feature definition.
#[derive(Clone, Copy)]
pub(super) struct HoleDefinition<'a> {
    pub(super) profile: Option<&'a cadmpeg_ir::features::profiles::PlanarProfileRef>,
    pub(super) profile_filter: Option<&'a HoleProfileFilter>,
    pub(super) face: Option<&'a FaceSelection>,
    pub(super) placements: Option<&'a [HolePlacement]>,
    pub(super) construction: &'a HoleConstruction,
    pub(super) exit_kind: Option<&'a HoleKind>,
    pub(super) diameter: Option<&'a cadmpeg_ir::scalar::PositiveLength>,
    pub(super) extent: Option<&'a LinearTermination>,
    pub(super) bottom: Option<&'a HoleBottom>,
    pub(super) taper_angle: Option<&'a cadmpeg_ir::scalar::InteriorAngle>,
    pub(super) allow_multi_profile_faces: Option<bool>,
}

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_extrude(
        &self,
        definition: ExtrudeDefinition<'_>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let ExtrudeDefinition {
            profile,
            direction,
            start,
            extent,
            op,
            solid,
            face_maker,
            inner_wire_taper,
            length_along_profile_normal,
            allow_multi_profile_faces,
        } = definition;
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        let resolved_parameter_names = self.resolved_parameter_names;
        Ok({
            // Writer accepts only a first-side draft; second-side draft or
            // any side offset is rejected.
            let has_offset = |side: &cadmpeg_ir::features::extents::ExtrudeSide| {
                matches!(
                    side.termination,
                    LinearTermination::ToFace {
                        offset: Some(_),
                        ..
                    } | LinearTermination::OffsetFromFace { .. }
                )
            };
            let (first_draft, second_side_draft, any_side_offset) = match extent {
                ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                    (side.draft, None, has_offset(side))
                }
                ExtrudeExtent::TwoSided { first, second } => (
                    first.draft,
                    second.draft,
                    has_offset(first) || has_offset(second),
                ),
            };
            let extent_is_unresolved = matches!(
                extent,
                ExtrudeExtent::OneSided { side }
                if matches!(side.termination, LinearTermination::Unresolved {})
            );
            let direction_source = match direction {
                ExtrudeDirection::Explicit { source, .. } => source.as_ref(),
                _ => None,
            };
            if !matches!(
                start,
                cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {}
            ) || second_side_draft.is_some()
                || direction_source.is_some()
                || solid == Some(false)
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
            if let ProfileRef::Planar(PlanarProfileRef::Unresolved(owner)) = profile {
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
            let profile_source = if existing.is_some_and(|record| {
                !record.properties.contains_key("Profile")
                    && (matches!(profile, ProfileRef::Planar(PlanarProfileRef::Unresolved(owner)) if owner == &record.id)
                        || matches!(profile, ProfileRef::Planar(PlanarProfileRef::Native(native)) if native == &record.id))
            }) {
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
            let positional_depth2 = (parameters.contains_key("D2")
                || existing.is_some_and(|record| {
                    resolved_parameter_names
                        .get(&record.id)
                        .is_some_and(|names| names.contains("D2"))
                }))
                && !parameters.contains_key("Depth2");
            // The record's name for the first depth: its dimension `D1`, or
            // the one length parameter the projection reads as the depth.
            let first_depth_name = if positional_depth {
                Some(cadmpeg_core::nonblank_literal!("D1"))
            } else {
                existing
                    .and_then(|record| {
                        crate::history::literals::sole_length_parameter(&record.parameters)
                    })
                    .map(|(name, _)| name.clone())
            };
            // A depth goes under the record's name for it when the record has
            // one, so the record never states a depth twice.
            let depth_entry = |stored: Option<cadmpeg_core::text::NonBlankString>,
                               named: cadmpeg_core::text::NonBlankString,
                               length: cadmpeg_ir::scalar::Length| {
                match stored {
                    Some(stored) => {
                        let value = format_length_like(
                            length,
                            existing
                                .and_then(|record| record.parameters.get(stored.as_str()))
                                .map(String::as_str),
                        );
                        (stored, value)
                    }
                    None => (named, format_length_mm(length)),
                }
            };
            let first_depth = |length: cadmpeg_ir::scalar::Length| {
                depth_entry(
                    first_depth_name.clone(),
                    cadmpeg_core::nonblank_literal!("Depth"),
                    length,
                )
            };
            // A two-sided extent is read from `Depth` and `Depth2`, or from
            // `D1` and `D2`.
            let two_sided_first_depth = |length: cadmpeg_ir::scalar::Length| {
                depth_entry(
                    positional_depth.then(|| cadmpeg_core::nonblank_literal!("D1")),
                    cadmpeg_core::nonblank_literal!("Depth"),
                    length,
                )
            };
            let second_depth = |length: cadmpeg_ir::scalar::Length| {
                depth_entry(
                    positional_depth2.then(|| cadmpeg_core::nonblank_literal!("D2")),
                    cadmpeg_core::nonblank_literal!("Depth2"),
                    length,
                )
            };
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
            let unsupported_termination_selection = || {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses an unsupported extrusion termination selection",
                    feature.id
                ))
            };
            let unsupported_extent = || {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses an unsupported extrusion extent",
                    feature.id
                ))
            };
            match extent {
                ExtrudeExtent::OneSided { side } => match &side.termination {
                    LinearTermination::Unresolved {} => {}
                    LinearTermination::Blind { length } => {
                        if properties.contains_key("EndCondition") || existing.is_none() {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("EndCondition"),
                                "Blind".into(),
                            );
                        }
                        let key = first_depth_name
                            .clone()
                            .unwrap_or(cadmpeg_core::nonblank_literal!("Depth"));
                        let value = format_length_like(
                            (*length).into(),
                            existing
                                .and_then(|record| record.parameters.get(key.as_str()))
                                .map(String::as_str),
                        );
                        parameters.insert(key, value);
                    }
                    LinearTermination::ThroughAll {} => {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "ThroughAll".into(),
                        );
                    }
                    LinearTermination::ThroughNext {} => {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "ThroughNext".into(),
                        );
                    }
                    LinearTermination::ToFirst {}
                    | LinearTermination::ToLast {}
                    | LinearTermination::ToPlane { .. }
                    | LinearTermination::ToShape { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination",
                            feature.id
                        )));
                    }
                    LinearTermination::ToFace { face, offset } => {
                        let Some(selection) = face_selection_value(face) else {
                            return Err(unsupported_termination_selection());
                        };
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "ToFace".into(),
                        );
                        properties.insert(cadmpeg_core::nonblank_literal!("Face"), selection);
                        if let Some(offset) = offset {
                            parameters.insert(
                                cadmpeg_core::nonblank_literal!("Depth"),
                                format_length_mm(*offset),
                            );
                        }
                    }
                    LinearTermination::ToVertex { vertex } => {
                        let Some(selection) = vertex_selection_value(vertex) else {
                            return Err(unsupported_termination_selection());
                        };
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "ToVertex".into(),
                        );
                        properties.insert(cadmpeg_core::nonblank_literal!("Vertex"), selection);
                    }
                    LinearTermination::OffsetFromFace { face, offset } => {
                        let Some(selection) = face_selection_value(face) else {
                            return Err(unsupported_termination_selection());
                        };
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "OffsetFromFace".into(),
                        );
                        properties.insert(cadmpeg_core::nonblank_literal!("Face"), selection);
                        let (key, value) = first_depth((*offset).into());
                        parameters.insert(key, value);
                    }
                },
                ExtrudeExtent::Symmetric { side } => match &side.termination {
                    LinearTermination::Blind { length } => {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "Symmetric".into(),
                        );
                        let (key, value) = first_depth((*length).into());
                        parameters.insert(key, value);
                    }
                    _ => return Err(unsupported_extent()),
                },
                ExtrudeExtent::TwoSided { first, second } => {
                    match (&first.termination, &second.termination) {
                        (
                            LinearTermination::Blind { length: first },
                            LinearTermination::Blind { length: second },
                        ) => {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("EndCondition"),
                                "TwoSided".into(),
                            );
                            for (key, value) in [
                                two_sided_first_depth((*first).into()),
                                second_depth((*second).into()),
                            ] {
                                parameters.insert(key, value);
                            }
                        }
                        (LinearTermination::ThroughAll {}, LinearTermination::ThroughAll {}) => {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("EndCondition"),
                                "ThroughAllBoth".into(),
                            );
                        }
                        _ => return Err(unsupported_extent()),
                    }
                }
            }
            // The flag comes from the decoded record, which holds the end
            // spec's flag where the native record does not.
            let flagged = reversal_flag(&feature.source_properties);
            let cleared = cleared_reversal(feature);
            properties.remove(REVERSE_DECODED);
            match direction {
                cadmpeg_ir::features::extents::ExtrudeDirection::Unresolved {}
                    if self.contradicted_travel
                        && existing
                            .is_some_and(|record| !record.properties.contains_key("Direction")) => {
                }
                cadmpeg_ir::features::extents::ExtrudeDirection::Unresolved {} => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {}
                    if self.contradicted_travel && !cleared =>
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires an explicit model-space extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {} => {
                    properties.remove("Direction");
                    // An edit cleared a reversal the decoder read: the writer
                    // clears the end-spec flag (`sync_neutral_features`).
                    if cleared {
                        properties.remove("Reverse");
                    }
                }
                cadmpeg_ir::features::extents::ExtrudeDirection::ReversedProfileNormal {}
                    if flagged && matches!(op, BooleanOp::Join | BooleanOp::NewBody) =>
                {
                    properties.remove("Direction");
                }
                cadmpeg_ir::features::extents::ExtrudeDirection::ReversedProfileNormal {} => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses a reversed profile-normal extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::extents::ExtrudeDirection::Explicit { vector, .. } => {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("Direction"),
                        format_vector3((*vector).into()),
                    );
                }
            }
            if let Some(draft) = first_draft {
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("Draft"),
                    format_angle_rad(draft.into()),
                );
            }
            if *op != BooleanOp::Unresolved
                && (properties.contains_key("Operation")
                    || existing.and_then(extrude_feature_op).is_none())
            {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Operation"),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            if let Some(profile_source) = profile_source {
                properties.insert(cadmpeg_core::nonblank_literal!("Profile"), profile_source);
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
        definition: HoleDefinition<'_>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let HoleDefinition {
            profile,
            profile_filter,
            face,
            placements,
            construction,
            exit_kind,
            diameter,
            extent,
            bottom,
            taper_angle,
            allow_multi_profile_faces,
        } = definition;
        let feature = self.feature;
        let existing = self.existing;
        let (kind, specification) = match construction {
            HoleConstruction::Form {
                kind,
                specification,
            } => (Some(kind), specification.as_ref()),
            HoleConstruction::NativeThread { .. } => (None, None),
        };
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
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("Diameter"),
                    format_length_mm((*diameter).into()),
                );
            }
            if let Some(kind) = kind {
                match kind {
                    HoleKind::Unresolved(_)
                    | HoleKind::PartialCounterbore(..)
                    | HoleKind::PartialCountersink(..)
                        if existing.is_some() => {}
                    HoleKind::Unresolved(_)
                    | HoleKind::PartialCounterbore(..)
                    | HoleKind::PartialCountersink(..) => {
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
                            cadmpeg_core::nonblank_literal!("DrillPointAngle"),
                            format_angle_rad((*drill_point_angle).into()),
                        );
                    }
                    HoleKind::Counterbore { diameter, depth } => {
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CounterboreDiameter"),
                            format_length_mm((*diameter).into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CounterboreDepth"),
                            format_length_mm((*depth).into()),
                        );
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
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CounterboreDiameter"),
                            format_length_mm((*diameter).into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CounterboreDepth"),
                            format_length_mm((*depth).into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("DrillPointAngle"),
                            format_angle_rad((*drill_point_angle).into()),
                        );
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        parameters.remove("CounterboreDiameter");
                        parameters.remove("CounterboreDepth");
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters.remove("DrillPointAngle");
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CountersinkDiameter"),
                            format_length_mm((*diameter).into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CountersinkAngle"),
                            format_angle_rad((*angle).into()),
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
            } else if let HoleConstruction::NativeThread {
                major_diameter,
                thread_depth,
                pitch,
                drill_point_angle,
            } = construction
            {
                parameters.remove("CounterboreDiameter");
                parameters.remove("CounterboreDepth");
                parameters.remove("CountersinkDiameter");
                parameters.remove("CountersinkAngle");
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("ThreadMajorDiameter"),
                    format_length_mm((*major_diameter).into()),
                );
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("ThreadDepth"),
                    format_length_mm((*thread_depth).into()),
                );
                if let Some(pitch) = pitch {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("ThreadPitch"),
                        format_length_mm((*pitch).into()),
                    );
                } else {
                    parameters.remove("ThreadPitch");
                }
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("DrillPointAngle"),
                    format_angle_rad((*drill_point_angle).into()),
                );
            }
            let mut properties = feature.source_properties.clone();
            match face.as_ref().map(|face| (face, face_selection_value(face))) {
                Some((_, Some(selection))) => {
                    properties.insert(cadmpeg_core::nonblank_literal!("Face"), selection);
                }
                Some((FaceSelection::Unresolved, _)) if existing.is_some() => {}
                Some((FaceSelection::Unresolved, _)) => {
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
            match placements.unwrap_or_default() {
                [cadmpeg_ir::features::holes::HolePlacement::Directed {
                    position,
                    direction,
                }] => {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("Position"),
                        format_point3_mm(*position),
                    );
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("Direction"),
                        format_vector3((*direction).into()),
                    );
                }
                [] if existing.is_none() => {
                    properties.remove("Position");
                    properties.remove("Direction");
                }
                [] => {}
                placements
                    if existing.is_some()
                        && placements.iter().all(|placement| {
                            matches!(
                                placement,
                                cadmpeg_ir::features::holes::HolePlacement::Axis { .. }
                            )
                        }) => {}
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                                       "SLDPRT feature {} has placements that require native generated-surface identities",
                                       feature.id
                                   )));
                }
            }
            match extent {
                Some(LinearTermination::Blind { length: depth }) => {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("Depth"),
                        format_length_mm((*depth).into()),
                    );
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("EndCondition"),
                        "Blind".into(),
                    );
                }
                Some(LinearTermination::ThroughAll {}) => {
                    parameters.remove("Depth");
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("EndCondition"),
                        "ThroughAll".into(),
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use std::collections::HashMap;

    #[test]
    fn retained_travel_writes_require_a_contradiction() {
        for (contradiction, unresolved, supported) in [
            (false, false, true),
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            let record: crate::records::Feature = json::from_value(serde_json::json!({
                "id":"native", "parent":"history", "ordinal":0,
                "name":"Extrude", "kind":"BossExtrude", "xml_tag":"Extrusion",
                "input_class":"moExtrusion_c", "parameters":{"Depth":"10mm"},
                "properties":{"EndCondition":"Blind", "Operation":"Join"}
            }))
            .expect("synthetic feature record");
            let history: crate::records::FeatureHistory = json::from_value(serde_json::json!({
                "id":"history", "features":[record.clone()]
            }))
            .expect("synthetic history");
            let mut feature = crate::history::project::project_features(&[history])
                .expect("feature projection")
                .into_iter()
                .next()
                .expect("extrusion");
            if unresolved {
                feature.evaluation.edit(|definition, _| {
                    if let FeatureDefinition::Operation(FeatureOperation::Extrude {
                        direction,
                        ..
                    }) = definition
                    {
                        *direction = ExtrudeDirection::Unresolved {};
                    }
                });
            }
            let encoded = NeutralFeatureEncoder {
                contradicted_travel: contradiction,
                feature: &feature,
                existing: Some(&record),
                principal_planes_by_record: &HashMap::new(),
                record_sources: &HashMap::new(),
                retained_tree_node_roles: &HashMap::new(),
                feature_sources: &HashMap::new(),
                sketch_sources: &HashMap::new(),
                parent_sources: &HashMap::new(),
                resolved_parameter_names: &HashMap::new(),
            }
            .encode();
            assert_eq!(encoded.is_ok(), supported);
            if let Ok(encoded) = encoded {
                assert!(!encoded.properties.contains_key("Direction"));
            }
        }
    }

    /// The first `length` leaf of `value`.
    fn first_length(value: &mut serde_json::Value) -> Option<&mut serde_json::Value> {
        match value {
            serde_json::Value::Object(fields) => {
                if fields
                    .get("length")
                    .is_some_and(serde_json::Value::is_number)
                {
                    return fields.get_mut("length");
                }
                fields.values_mut().find_map(first_length)
            }
            serde_json::Value::Array(items) => items.iter_mut().find_map(first_length),
            _ => None,
        }
    }

    /// A record that holds one user-named length and no `Depth` or `D1` is
    /// read with that length as its depth. A depth edit is written to it,
    /// and the record gains no `Depth`.
    #[test]
    fn a_depth_edit_is_written_to_the_records_one_length_parameter() {
        let record: crate::records::Feature = json::from_value(serde_json::json!({
            "id":"native", "parent":"history", "ordinal":0,
            "name":"Extrude", "kind":"BossExtrude", "xml_tag":"Extrusion",
            "input_class":"moExtrusion_c", "parameters":{"Plate thickness":"10mm"},
            "properties":{"EndCondition":"Blind", "Operation":"Join"}
        }))
        .expect("synthetic feature record");
        let history: crate::records::FeatureHistory = json::from_value(serde_json::json!({
            "id":"history", "features":[record.clone()]
        }))
        .expect("synthetic history");
        let mut feature = crate::history::project::project_features(&[history])
            .expect("feature projection")
            .into_iter()
            .next()
            .expect("extrusion");
        let mut wire =
            json::to_value(feature.evaluation.definition()).expect("definition wire form");
        let length = first_length(&mut wire).expect("the record's length is the depth");
        assert_eq!(length.as_f64(), Some(10.0));
        *length = serde_json::json!(11.0);
        feature
            .evaluation
            .set_definition(json::from_value(wire).expect("edited definition"));
        let encoded = NeutralFeatureEncoder {
            contradicted_travel: false,
            feature: &feature,
            existing: Some(&record),
            principal_planes_by_record: &HashMap::new(),
            record_sources: &HashMap::new(),
            retained_tree_node_roles: &HashMap::new(),
            feature_sources: &HashMap::new(),
            sketch_sources: &HashMap::new(),
            parent_sources: &HashMap::new(),
            resolved_parameter_names: &HashMap::new(),
        }
        .encode()
        .expect("a blind depth edit encodes");
        let (name, depth) = crate::history::literals::sole_length_parameter(&encoded.parameters)
            .expect("the record holds one length");
        assert_eq!(name.as_str(), "Plate thickness");
        assert_eq!(depth.get().to_string(), "11", "{:?}", encoded.parameters);
    }

    /// The end-spec reversal flag of a one-sided extrusion that adds
    /// material projects against its profile normal and writes from the
    /// decoded record, which holds the flag where the native record does
    /// not. An edit that clears a decoded reversal drops the flag from the
    /// record; a cut, whose flag the decoder leaves unread, and the
    /// decoder's fallback keep it.
    #[test]
    fn a_reversal_writes_from_the_decoded_record() {
        let record = |operation: &str, reverse: bool| -> crate::records::Feature {
            let mut properties = serde_json::json!({"EndCondition":"Blind", "Operation":operation});
            if reverse {
                properties["Reverse"] = "true".into();
            }
            json::from_value(serde_json::json!({
                "id":"native", "parent":"history", "ordinal":0,
                "name":"Extrude", "kind":"BossExtrude", "xml_tag":"Extrusion",
                "input_class":"moExtrusion_c", "parameters":{"Depth":"10mm"},
                "properties":properties
            }))
            .expect("synthetic feature record")
        };
        let project = |record: &crate::records::Feature| {
            let history: crate::records::FeatureHistory = json::from_value(serde_json::json!({
                "id":"history", "features":[record.clone()]
            }))
            .expect("synthetic history");
            crate::history::project::project_features(&[history])
                .expect("feature projection")
                .into_iter()
                .next()
                .expect("extrusion")
        };
        let direction =
            |feature: &cadmpeg_ir::features::Feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) => {
                    direction.clone()
                }
                other => panic!("an extrusion: {other:?}"),
            };
        let set_direction = |feature: &mut cadmpeg_ir::features::Feature,
                             travel: ExtrudeDirection| {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Extrude {
                    direction, ..
                }) = definition
                {
                    *direction = travel;
                }
            });
        };
        let encode_over = |contradicted_travel: bool,
                           feature: &cadmpeg_ir::features::Feature,
                           existing: &crate::records::Feature| {
            NeutralFeatureEncoder {
                contradicted_travel,
                feature,
                existing: Some(existing),
                principal_planes_by_record: &HashMap::new(),
                record_sources: &HashMap::new(),
                retained_tree_node_roles: &HashMap::new(),
                feature_sources: &HashMap::new(),
                sketch_sources: &HashMap::new(),
                parent_sources: &HashMap::new(),
                resolved_parameter_names: &HashMap::new(),
            }
            .encode()
        };
        let encode = |feature: &cadmpeg_ir::features::Feature,
                      existing: &crate::records::Feature| {
            encode_over(false, feature, existing)
        };
        let written = |encoded: &NeutralFeatureEncoding| {
            assert!(!encoded
                .properties
                .contains_key(crate::history::extrude_direction::REVERSE_DECODED));
            encoded.properties.get("Reverse").cloned()
        };
        let flag = Some("true".to_owned());
        // A join reads the flag; a native record without it is the end
        // spec's flag, which only the decoded record holds.
        let join = project(&record("Join", true));
        assert_eq!(direction(&join), ExtrudeDirection::ReversedProfileNormal {});
        for native in [record("Join", true), record("Join", false)] {
            let encoded = encode(&join, &native).expect("the decoded record keeps the flag");
            assert_eq!(written(&encoded), flag);
        }
        let mut unflagged = join.clone();
        unflagged.source_properties.remove("Reverse");
        assert!(encode(&unflagged, &record("Join", false)).is_err());
        // An edit that clears the reversal writes along the normal, also
        // where the stored solid still shows the old travel; the writer
        // clears the end-spec flag and names the stale solid.
        let mut cleared = join.clone();
        set_direction(&mut cleared, ExtrudeDirection::ProfileNormal {});
        assert!(crate::history::extrude_direction::cleared_reversal(
            &cleared
        ));
        for contradicted in [false, true] {
            let encoded = encode_over(contradicted, &cleared, &record("Join", true))
                .expect("an edit along the normal");
            assert_eq!(written(&encoded), None);
        }
        // The fallback keeps the flag.
        let mut fallback = cleared.clone();
        fallback.source_properties.insert(
            cadmpeg_core::nonblank_literal!("reverse_decoded"),
            "false".to_owned(),
        );
        let encoded = encode(&fallback, &record("Join", true)).expect("the fallback");
        assert_eq!(written(&encoded), flag);
        // Travel along the normal that the stored solid contradicts, and that
        // no edit cleared, has no implicit encoding.
        assert!(encode_over(true, &fallback, &record("Join", true)).is_err());
        // A cut leaves its flag unread and keeps it.
        let cut = project(&record("Cut", true));
        assert_eq!(direction(&cut), ExtrudeDirection::ProfileNormal {});
        assert!(crate::history::extrude_direction::reversal_unread(
            &cut.source_properties
        ));
        let encoded = encode(&cut, &record("Cut", true)).expect("a cut keeps its flag");
        assert_eq!(written(&encoded), flag);
        let mut reversed_cut = cut;
        set_direction(
            &mut reversed_cut,
            ExtrudeDirection::ReversedProfileNormal {},
        );
        assert!(encode(&reversed_cut, &record("Cut", true)).is_err());
        assert_eq!(
            direction(&project(&record("Join", false))),
            ExtrudeDirection::ProfileNormal {}
        );
    }
}
