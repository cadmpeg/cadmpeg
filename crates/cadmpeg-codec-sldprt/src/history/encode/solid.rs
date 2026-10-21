// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole write encoders.

use super::super::literals::{format_angle_rad, format_length_mm};
use super::format::{format_length_like, format_point3_mm, format_vector3};
use super::support::{
    face_selection_value, profile_source, resolved_boolean_op, vertex_selection_value,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use crate::history::classify::{extrude_feature_op, is_extrude};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    holes::{HoleBottom, HoleConstruction, HoleKind, HolePlacement, HoleProfileFilter},
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeStart, FaceMaker, FaceSelection,
    InnerWireTaper, LinearTermination, PlanarProfileRef, ProfileRef,
};

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
    pub(super) profile: Option<&'a cadmpeg_ir::features::PlanarProfileRef>,
    pub(super) profile_filter: Option<&'a HoleProfileFilter>,
    pub(super) face: Option<&'a FaceSelection>,
    pub(super) placements: Option<&'a [HolePlacement]>,
    pub(super) construction: &'a HoleConstruction,
    pub(super) exit_kind: Option<&'a HoleKind>,
    pub(super) diameter:
        Option<&'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::PositiveLength>>,
    pub(super) extent: Option<&'a LinearTermination>,
    pub(super) bottom: Option<&'a HoleBottom>,
    pub(super) taper_angle:
        Option<&'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::InteriorAngle>>,
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
            let has_offset = |side: &cadmpeg_ir::features::ExtrudeSide| {
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
                    (side.draft.as_ref(), None, has_offset(side))
                }
                ExtrudeExtent::TwoSided { first, second } => (
                    first.draft.as_ref(),
                    second.draft.as_ref(),
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
            if !matches!(start, cadmpeg_ir::features::ExtrudeStart::ProfilePlane {})
                || second_side_draft.is_some()
                || direction_source.is_some()
                || solid == Some(false)
                || face_maker.is_some()
                || inner_wire_taper.is_some()
                || any_side_offset
                || length_along_profile_normal.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} uses unsupported extrusion construction controls",
                    feature.id
                )));
            }
            if *op == BooleanOp::Unresolved && existing.is_none() {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} requires retained extrusion operation data",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_extrude(record)) {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} changes unsupported extrusion semantics",
                    feature.id
                )));
            }
            if let ProfileRef::Planar(PlanarProfileRef::Unresolved(owner)) = profile {
                let retained = existing.is_some_and(|record| {
                    record.id == *owner && !record.properties.contains_key("Profile")
                });
                if !retained {
                    return Err(CodecError::UnsupportedContent(format!(
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
                            CodecError::UnsupportedContent(format!(
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
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT feature {} changes its inferred extrusion operation",
                        feature.id
                    )));
                }
            }
            let mut parameters = self.base_parameters();
            let positional_depth = (parameters.contains_key("D1")
                || existing.is_some_and(|record| {
                    resolved_parameter_names
                        .get(&record.id)
                        .is_some_and(|names| names.contains("D1"))
                }))
                && !parameters.contains_key("Depth");
            let mut properties = self.base_properties();
            if extent_is_unresolved && existing.is_none() {
                return Err(CodecError::UnsupportedContent(format!(
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
                CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} uses an unsupported extrusion termination selection",
                    feature.id
                ))
            };
            let unsupported_extent = || {
                CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} uses an unsupported extrusion extent",
                    feature.id
                ))
            };
            match extent {
                ExtrudeExtent::OneSided { side } => match &side.termination {
                    LinearTermination::Unresolved {} => {
                        if let Some(condition) =
                            existing.and_then(|record| record.properties.get("EndCondition"))
                        {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("EndCondition"),
                                condition.clone(),
                            );
                        }
                    }
                    LinearTermination::Blind { length } => {
                        if existing
                            .is_none_or(|record| record.properties.contains_key("EndCondition"))
                        {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("EndCondition"),
                                "Blind".into(),
                            );
                        }
                        let key = if positional_depth {
                            cadmpeg_core::nonblank_literal!("D1")
                        } else {
                            cadmpeg_core::nonblank_literal!("Depth")
                        };
                        let value = format_length_like(
                            self.dimension(length)?.into(),
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
                    | LinearTermination::ToShape { .. } => {
                        return Err(CodecError::UnsupportedContent(format!(
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
                                format_length_mm(self.dimension(offset)?),
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
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("Depth"),
                            format_length_mm(self.dimension(offset)?.into()),
                        );
                    }
                },
                ExtrudeExtent::Symmetric { side } => match &side.termination {
                    LinearTermination::Blind { length } => {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("EndCondition"),
                            "Symmetric".into(),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("Depth"),
                            format_length_mm(self.dimension(length)?.into()),
                        );
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
                            parameters.insert(
                                cadmpeg_core::nonblank_literal!("Depth"),
                                format_length_mm(self.dimension(first)?.into()),
                            );
                            parameters.insert(
                                cadmpeg_core::nonblank_literal!("Depth2"),
                                format_length_mm(self.dimension(second)?.into()),
                            );
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
            match direction {
                cadmpeg_ir::features::ExtrudeDirection::Unresolved {} => {
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT feature {} has an unresolved extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {} => {
                    properties.remove("Direction");
                }
                cadmpeg_ir::features::ExtrudeDirection::ReversedProfileNormal {} => {
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT feature {} uses a reversed profile-normal extrusion direction",
                        feature.id
                    )));
                }
                cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } => {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("Direction"),
                        format_vector3((*vector).into()),
                    );
                }
            }
            if let Some(draft) = first_draft {
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("Draft"),
                    format_angle_rad(self.dimension(draft)?.into()),
                );
            }
            if *op != BooleanOp::Unresolved
                && (existing.is_some_and(|record| record.properties.contains_key("Operation"))
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
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} changes unsupported hole construction semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| classify(record) != Some(FeatureClass::Hole)) {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} changes unsupported hole semantics",
                    feature.id
                )));
            }
            if existing.is_none() && diameter.is_none() {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} has an unresolved hole diameter",
                    feature.id
                )));
            }
            let mut parameters = self.base_parameters();
            if let Some(diameter) = diameter {
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("Diameter"),
                    format_length_mm(self.dimension(diameter)?.into()),
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
                        return Err(CodecError::UnsupportedContent(format!(
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
                            format_angle_rad(self.dimension(drill_point_angle)?.into()),
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
                            format_length_mm(self.dimension(diameter)?.into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CounterboreDepth"),
                            format_length_mm(self.dimension(depth)?.into()),
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
                            format_length_mm(self.dimension(diameter)?.into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CounterboreDepth"),
                            format_length_mm(self.dimension(depth)?.into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("DrillPointAngle"),
                            format_angle_rad(self.dimension(drill_point_angle)?.into()),
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
                            format_length_mm(self.dimension(diameter)?.into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("CountersinkAngle"),
                            format_angle_rad(self.dimension(angle)?.into()),
                        );
                    }
                    HoleKind::Counterdrill { .. } => {
                        return Err(CodecError::UnsupportedContent(format!(
                            "SLDPRT feature {} has unsupported counterdrill construction",
                            feature.id
                        )));
                    }
                    HoleKind::Chamfer { .. } => {
                        return Err(CodecError::UnsupportedContent(format!(
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
                    format_length_mm(self.dimension(major_diameter)?.into()),
                );
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("ThreadDepth"),
                    format_length_mm(self.dimension(thread_depth)?.into()),
                );
                if let Some(pitch) = pitch {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("ThreadPitch"),
                        format_length_mm(self.dimension(pitch)?.into()),
                    );
                } else {
                    parameters.remove("ThreadPitch");
                }
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("DrillPointAngle"),
                    format_angle_rad(self.dimension(drill_point_angle)?.into()),
                );
            }
            let mut properties = self.base_properties();
            match face.as_ref().map(|face| (face, face_selection_value(face))) {
                Some((_, Some(selection))) => {
                    properties.insert(cadmpeg_core::nonblank_literal!("Face"), selection);
                }
                Some((FaceSelection::Unresolved, _)) if existing.is_some() => {}
                Some((FaceSelection::Unresolved, _)) => {
                    return Err(CodecError::UnsupportedContent(format!(
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
                    return Err(CodecError::UnsupportedContent(format!(
                                       "SLDPRT feature {} has placements that require native generated-surface identities",
                                       feature.id
                                   )));
                }
            }
            match extent {
                Some(LinearTermination::Blind { length: depth }) => {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("Depth"),
                        format_length_mm(self.dimension(depth)?.into()),
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
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT feature {} changes unsupported hole termination",
                        feature.id
                    )))
                }
                None if existing.is_some() => {}
                None => {
                    return Err(CodecError::UnsupportedContent(format!(
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
