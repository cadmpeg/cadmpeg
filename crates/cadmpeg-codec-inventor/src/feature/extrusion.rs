// SPDX-License-Identifier: Apache-2.0
//! Extrusion operands and native profile selections.

use std::collections::HashSet;

use cadmpeg_ir::features::{
    BooleanOp, ExtrudeExtent, ExtrudeStart, LinearTermination, PlanarProfileRef,
};
use cadmpeg_ir::units::UnitVector3;

use super::{
    boolean, enum16, extent_target_plane, extent_target_point, extrusion_side, references,
    resolve_direction, resolve_property, ExtrusionSideSlots, PmDcFeature, PmDcFeatureEnumFamily,
    PmDcFeatureLabel, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
    EXTENT_DISTANCE, EXTENT_FROM_TO, EXTENT_SYMMETRIC_DISTANCE, EXTENT_THROUGH_ALL,
    EXTENT_THROUGH_NEXT,
};
use cadmpeg_ir::math::Vector3;

pub(super) fn project(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<
    Result<
        (
            cadmpeg_ir::features::Feature,
            cadmpeg_ir::features::FeatureResultTopology,
        ),
        cadmpeg_core::CodecError,
    >,
> {
    let projected = super::project_extrusion(ctx, source, label, index);
    match &projected {
        Some(Ok((feature, _))) if has_native_profile(feature.evaluation.definition()) => {
            losses.push(profile_loss());
        }
        None => losses.push(operand_loss()),
        _ => {}
    }
    if let Some(Ok((feature, _))) = &projected {
        for _ in 0..native_face_targets(feature.evaluation.definition()) {
            losses.push(crate::loss::InventorLossCode::FeatureExtrusionTargetFaceNative.note(
                "Extrusion target retains its stored planar face geometry; its body-face identity remains unresolved.",
            ));
        }
    }
    projected
}

pub(super) fn native_face_targets(definition: &cadmpeg_ir::features::FeatureDefinition) -> usize {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlaneTargetSource};
    let FeatureDefinition::Operation(FeatureOperation::Extrude { start, extent, .. }) = definition
    else {
        return 0;
    };
    let is_face = |side: &cadmpeg_ir::features::ExtrudeSide| {
        usize::from(matches!(
            side.termination,
            LinearTermination::ToPlane {
                source: Some(PlaneTargetSource::FaceSurface),
                ..
            }
        ))
    };
    usize::from(matches!(
        start,
        ExtrudeStart::FromPlane {
            source: Some(PlaneTargetSource::FaceSurface),
            ..
        }
    )) + match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => is_face(side),
        ExtrudeExtent::TwoSided { first, second } => is_face(first) + is_face(second),
    }
}

pub(super) struct Operands {
    pub(super) op: BooleanOp,
    pub(super) direction: UnitVector3,
    pub(super) start: ExtrudeStart,
    pub(super) extent: ExtrudeExtent,
}

/// Largest departure of the travel direction from the profile-plane normal,
/// as a cosine deficit, that still counts as perpendicular to the profile plane.
const EPS_TRAVEL_NORMAL: f64 = 1e-9;
/// Smallest distance in millimetres by which a work point lies ahead of the
/// profile plane.
const EPS_POINT_AHEAD: f64 = 1e-6;

pub(super) fn work_point_plane(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    direction: &UnitVector3,
) -> Option<LinearTermination> {
    let point = extent_target_point(source, index, ExtrusionSideSlots::FIRST)?;
    let direction_record = resolve_direction(source, 2, index)?;
    if direction_record.entity_flags != 0
        || direction_record.parameter.get() != 0.0
        || direction_record.extension.is_some_and(|value| value != 0)
    {
        return None;
    }
    let [participant] = label.participants.references() else {
        return None;
    };
    let token = source.identity.segment_token.as_str();
    let sketch = index
        .sketches
        .get(&(token, participant.record_ordinal()?))?
        .as_ref()?;
    let (profile_origin, normal, u_axis) =
        crate::sketch::project_placement(sketch, &index.transforms, &index.directions)?
            .resolved()?;
    let normal = normal.get();
    let travel = *direction.as_raw();
    let profile_origin = profile_origin.get();
    let ahead = point.origin.vector_from(profile_origin).dot(travel);
    if !(travel.dot(normal).abs() >= 1.0 - EPS_TRAVEL_NORMAL && ahead > EPS_POINT_AHEAD) {
        return None;
    }
    let oriented_normal = if travel.dot(normal) < 0.0 {
        normal.scale(-1.0)
    } else {
        normal
    };
    let plane = cadmpeg_ir::features::FeatureSupportPlaneFrame::new(
        point.origin,
        oriented_normal,
        u_axis.get(),
    )?;
    Some(LinearTermination::ToPlane {
        plane,
        native: cadmpeg_core::text::NonBlankString::new(point.native)?,
        source: Some(cadmpeg_ir::features::PlaneTargetSource::WorkPoint),
    })
}

pub(super) fn indexed_operands(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Operands> {
    let operation = enum16(source, 0, PmDcFeatureEnumFamily::PartOperation, index)?;
    let op = match operation {
        1 => BooleanOp::NewBody,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Join,
        4 => BooleanOp::Intersect,
        _ => return None,
    };
    let direction_record = resolve_direction(source, 2, index)?;
    let mut direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    if boolean(source, 3, index)? {
        direction = direction.reversed();
    }
    let extent_value = enum16(source, 6, PmDcFeatureEnumFamily::Extent, index)?;
    let symmetric = boolean(source, 7, index)?;
    let second_extent = source
        .properties
        .references()
        .get(29)
        .is_some_and(|reference| reference.index() != 0);
    let first = extrusion_side(
        source,
        index,
        ExtrusionSideSlots::FIRST,
        extent_value,
        label,
        &direction,
    )?;
    let start = if extent_value == EXTENT_FROM_TO {
        let (plane, native, source) = extent_target_plane(source, index, 8, 10, 17)?;
        ExtrudeStart::FromPlane {
            plane,
            native,
            source: Some(source),
        }
    } else {
        ExtrudeStart::ProfilePlane {}
    };
    let extent = match (extent_value, symmetric, second_extent) {
        (EXTENT_SYMMETRIC_DISTANCE | EXTENT_THROUGH_ALL, true, false) => {
            ExtrudeExtent::Symmetric { side: first }
        }
        (EXTENT_SYMMETRIC_DISTANCE, false, _) | (_, true, true) => return None,
        (_, true, false) => return None,
        (_, false, true) => {
            let second_value = enum16(source, 29, PmDcFeatureEnumFamily::Extent, index)?;
            if !matches!(
                second_value,
                EXTENT_DISTANCE | EXTENT_THROUGH_NEXT | EXTENT_THROUGH_ALL
            ) || extent_value == EXTENT_FROM_TO
            {
                return None;
            }
            ExtrudeExtent::TwoSided {
                first,
                second: extrusion_side(
                    source,
                    index,
                    ExtrusionSideSlots::SECOND,
                    second_value,
                    label,
                    &direction,
                )?,
            }
        }
        (_, false, false) => ExtrudeExtent::OneSided { side: first },
    };
    Some(Operands {
        op,
        direction,
        start,
        extent,
    })
}

pub(super) fn native_profile(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<PlanarProfileRef> {
    let slots = source.properties.references();
    if slots.get(23)? != slots.get(1)? {
        return None;
    }
    let [participant] = label.participants.references() else {
        return None;
    };
    let token = source.identity.segment_token.as_str();
    let sketch = index
        .sketches
        .get(&(token, participant.record_ordinal()?))?
        .as_ref()?;
    if index.sketch_ids.contains_key(sketch.id().as_str()) {
        return None;
    }
    let patch = super::slot_property(source, 1, index)?;
    if patch.identity.type_id.as_str() != crate::pmdc::type_id_string(super::BOUNDARY_PATCH_TYPE) {
        return None;
    }
    let boundary = references(source, 1, PmDcFeatureReferenceFamily::BoundaryPatch, index)?;
    let selection_type = crate::pmdc::type_id_string(super::PROFILE_SELECTION_TYPE);
    let mut distinct = HashSet::new();
    if boundary.references().is_empty()
        || !boundary.references().iter().all(|reference| {
            if !distinct.insert(reference.index()) {
                return false;
            }
            let Some(property) = resolve_property(token, reference.index(), index) else {
                return false;
            };
            if property.identity.type_id.as_str() != selection_type {
                return false;
            }
            let PmDcFeaturePropertyKind::ProfileSelection {
                entity_link,
                value: 0,
            } = property.kind
            else {
                return false;
            };
            entity_link
                .record_ordinal()
                .is_some_and(|ordinal| index.entity_style_links.contains(&(token, ordinal)))
        })
    {
        return None;
    }
    let direction = resolve_direction(source, 2, index)?;
    if !native_direction_controls(
        direction.entity_flags,
        direction.parameter.get(),
        direction.extension,
    ) {
        return None;
    }
    Some(PlanarProfileRef::Native(
        super::slot_property(source, 1, index)?.id(),
    ))
}

pub(super) fn native_direction_controls(
    flags: u32,
    parameter: f64,
    extension: Option<u32>,
) -> bool {
    flags == 0 && parameter == 0.0 && extension.is_none_or(|value| value == 0)
}

pub(crate) fn has_native_profile(definition: &cadmpeg_ir::features::FeatureDefinition) -> bool {
    matches!(
        definition,
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Planar(PlanarProfileRef::Native(_)),
                ..
            }
        )
    )
}

pub(super) fn profile_loss() -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::InventorLossCode::FeatureExtrusionProfileNative.note(
        "Extrusion retains its boundary-patch profile selection; the consumed planar sketch has no closed neutral graph.",
    )
}

pub(super) fn operand_loss() -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::InventorLossCode::FeatureExtrusionOperandsNative
        .note("Extrusion construction operands remain in the native feature definition.")
}

pub(crate) struct ValidationRecords<'a> {
    pub(crate) properties: &'a [super::PmDcFeatureProperty],
    pub(crate) parameters: &'a [crate::design::PmDcParameter],
    pub(crate) sketches: &'a [crate::sketch::PmDcSketch],
    pub(crate) directions: &'a [crate::sketch::PmDcDirection],
    pub(crate) transforms: &'a [crate::sketch::PmDcTransform],
    pub(crate) planes: &'a [super::PmDcWorkPlane],
    pub(crate) entity_links: &'a [super::PmDcEntityStyleLink],
    pub(crate) kernel_links: &'a [super::PmDcKernelEntityLink],
    pub(crate) model: &'a cadmpeg_ir::document::Model,
}

pub(crate) struct ValidationIndex<'a>(ProjectionIndex<'a>);

impl<'a> ValidationIndex<'a> {
    pub(crate) fn new(records: &ValidationRecords<'a>) -> Self {
        use cadmpeg_core::decode::unique_index;
        let key = |identity: &'a crate::record_identity::RecordIdentity| {
            (identity.segment_token.as_str(), identity.record_ordinal)
        };
        Self(ProjectionIndex {
            properties: unique_index(records.properties.iter().map(|r| (key(&r.identity), r))),
            parameters: unique_index(records.parameters.iter().map(|r| (key(&r.identity), r))),
            parameter_values: unique_index(
                records
                    .model
                    .parameters
                    .iter()
                    .filter_map(|r| Some((r.native_ref.as_deref()?, r))),
            ),
            directions: unique_index(records.directions.iter().map(|r| (key(&r.identity), r))),
            transforms: unique_index(records.transforms.iter().map(|r| (key(&r.identity), r))),
            work_planes: unique_index(records.planes.iter().map(|r| (key(&r.identity), r))),
            sketches: unique_index(records.sketches.iter().map(|r| (key(&r.identity), r))),
            sketch_ids: records
                .model
                .sketches
                .iter()
                .filter_map(|r| Some((r.native_ref.as_deref()?, r.id.clone())))
                .collect(),
            entity_style_links: records
                .entity_links
                .iter()
                .map(|r| key(&r.identity))
                .chain(records.kernel_links.iter().map(|r| key(&r.identity)))
                .collect(),
            ..ProjectionIndex::default()
        })
    }

    pub(crate) fn matches(
        &self,
        source: &PmDcFeature,
        label: &PmDcFeatureLabel,
        definition: &cadmpeg_ir::features::FeatureDefinition,
    ) -> bool {
        matches(&self.0, source, label, definition)
    }
}

pub(super) fn matches(
    index: &ProjectionIndex<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    definition: &cadmpeg_ir::features::FeatureDefinition,
) -> bool {
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrusionDirectionSource, FeatureDefinition, FeatureOperation, ProfileRef,
    };
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }) = definition
    else {
        return false;
    };
    let Some(expected) = indexed_operands(source, label, index) else {
        return false;
    };
    let expected_direction = ExtrudeDirection::Explicit {
        vector: expected.direction.into(),
        source: Some(ExtrusionDirectionSource::Custom {}),
    };
    if *direction != expected_direction
        || *start != expected.start
        || *extent != expected.extent
        || *op != expected.op
    {
        return false;
    }
    if matches!(profile, ProfileRef::Planar(PlanarProfileRef::Native(_))) {
        return native_profile(source, label, index)
            .is_some_and(|expected| *profile == ProfileRef::Planar(expected));
    }
    source.properties.references().get(23) == source.properties.references().get(1)
}

pub(super) fn plane_surface_controls(
    version: u8,
    references: [crate::pmdc::PmDcReference; 3],
    extension: Option<u32>,
    state: u8,
    form: u32,
    value: u32,
) -> bool {
    references == [crate::pmdc::PmDcReference::from_packed(0); 3]
        && extension == (version >= super::SURFACE_EXTENSION_FIRST_MAJOR).then_some(0)
        && state == 1
        && form == super::PLANE_SURFACE_FORM
        && value == 0
}
