//! Typed marker relation definitions and geometry predicates.

use super::curves::compact_bounded_curve_tangent;
use super::endpoints::{
    collect_endpoint_markers, copy_endpoint_markers, sort_endpoint_markers,
    compact_indexed_curve_endpoint_indices, compact_indexed_curve_record_end,
    compact_legacy_code_one_line_endpoint_indices, extended_compact_indexed_curve_endpoint_indices,
    extended_direct_object_line_endpoint_ids, extended_shifted_construction_line_endpoint_indices,
    legacy_marker104_arc_center, legacy_terminal_profile_endpoint_offset,
    marker_profile_curve_role, roster_curve_endpoint_markers, wide_indexed_curve_endpoint_indices,
    wide_indexed_curve_record_is_complete, CompactIndexedCurveRecordEnd,
};
use super::markers::{
    compact_legacy_142_profile_curve_endpoints, finite_coordinate_pair, inline_arc_coordinates,
    legacy_extended_profile_curve_kind, marker_is_geometry_locus, marker_native_code,
    sketch_marker_prefix_at,
};
use super::relation_loci::{
    canonical_profile_loci, find_profile_entity, line_line_distance, linked_midpoint_operands, linked_single_arc_entity,
    linked_single_ellipse_entity, linked_single_entities, marker_point_locus,
    point_line_distance_value, profile_locus_point_charged, relation_operand_loci, same_dimension_angle,
    same_dimension_length,
};
use super::scalars::operand_kind;
use super::selections::operand_accepts_marker;
use super::transforms::{
    sort_marker_entity_ids, MarkerEntityFilter, locus_entity, locus_key, marker_entities, ProfileAxis,
};
use super::{
    LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER, SKETCH_POINT_TOLERANCE,
};
use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_core::nonblank_literal;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntity, SketchEntityId,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::{HashMap, HashSet};

const EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12: f64 = 1.0e-12;
const EPS_TYPED_RELATIONS_SKETCH_ENTITY_MIDPOINT_E12: f64 = 1.0e-12;
const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12: f64 = 1.0e-12;
const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9: f64 = 1.0e-9;

/// A relation kind narrowed to the single-entity forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SingleEntityRelation {
    Horizontal,
    Vertical,
    Fixed,
}

impl SingleEntityRelation {
    /// The constrained coordinate and the profile axis it is measured along, when this
    /// relation is axis-aligned.
    fn axes(self) -> Option<(SketchCoordinateAxis, ProfileAxis)> {
        match self {
            Self::Horizontal => Some((SketchCoordinateAxis::V, ProfileAxis::U)),
            Self::Vertical => Some((SketchCoordinateAxis::U, ProfileAxis::V)),
            Self::Fixed => None,
        }
    }

    fn definition(self, entity: SketchEntityId) -> SketchConstraintDefinitionInput {
        match self {
            Self::Horizontal => SketchConstraintDefinitionInput::Horizontal { entity },
            Self::Vertical => SketchConstraintDefinitionInput::Vertical { entity },
            Self::Fixed => SketchConstraintDefinitionInput::Fixed { entity },
        }
    }
}

/// A quarter-turn sweep named by an arc- or ellipse-angle relation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuarterTurn {
    Quarter,
    Half,
    ThreeQuarters,
}

impl QuarterTurn {
    fn angle(self) -> cadmpeg_ir::scalar::PositiveAngle {
        match self {
            Self::Quarter => cadmpeg_ir::scalar::PositiveAngle::QUARTER_TURN,
            Self::Half => cadmpeg_ir::scalar::PositiveAngle::HALF_TURN,
            Self::ThreeQuarters => cadmpeg_ir::scalar::PositiveAngle::THREE_QUARTER_TURN,
        }
    }
}

/// A relation kind narrowed to the two-entity forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryRelation {
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Collinear,
    Concentric,
    Coradial,
}

impl BinaryRelation {
    fn definition(
        self,
        first: SketchEntityId,
        second: SketchEntityId,
    ) -> SketchConstraintDefinitionInput {
        match self {
            Self::Parallel => SketchConstraintDefinitionInput::Parallel { first, second },
            Self::Perpendicular => SketchConstraintDefinitionInput::Perpendicular { first, second },
            Self::Tangent => SketchConstraintDefinitionInput::Tangent { first, second },
            Self::Equal => SketchConstraintDefinitionInput::Equal { first, second },
            Self::Collinear => SketchConstraintDefinitionInput::Collinear { first, second },
            Self::Concentric => SketchConstraintDefinitionInput::Concentric { first, second },
            Self::Coradial => SketchConstraintDefinitionInput::Coradial { first, second },
        }
    }
}

/// The relation-kind groups the typed marker projection handles as one shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerRelationGroup {
    SingleEntity(SingleEntityRelation),
    Dimensional,
    ArcQuarter(QuarterTurn),
    EllipseQuarter(QuarterTurn),
    Binary(BinaryRelation),
    Coincidence,
    AxisPoints,
    AtIntersection,
    Symmetric,
    Midpoint,
    Other,
}

impl MarkerRelationGroup {
    fn of(kind: crate::records::SketchRelationKind) -> Self {
        use crate::records::SketchRelationKind as Kind;
        match kind {
            Kind::Horizontal => Self::SingleEntity(SingleEntityRelation::Horizontal),
            Kind::Vertical => Self::SingleEntity(SingleEntityRelation::Vertical),
            Kind::Fixed => Self::SingleEntity(SingleEntityRelation::Fixed),
            Kind::ArcAngle90 => Self::ArcQuarter(QuarterTurn::Quarter),
            Kind::ArcAngle180 => Self::ArcQuarter(QuarterTurn::Half),
            Kind::ArcAngle270 => Self::ArcQuarter(QuarterTurn::ThreeQuarters),
            Kind::EllipseAngle90 => Self::EllipseQuarter(QuarterTurn::Quarter),
            Kind::EllipseAngle180 => Self::EllipseQuarter(QuarterTurn::Half),
            Kind::EllipseAngle270 => Self::EllipseQuarter(QuarterTurn::ThreeQuarters),
            Kind::Parallel => Self::Binary(BinaryRelation::Parallel),
            Kind::Perpendicular => Self::Binary(BinaryRelation::Perpendicular),
            Kind::Tangent => Self::Binary(BinaryRelation::Tangent),
            Kind::Equal => Self::Binary(BinaryRelation::Equal),
            Kind::Collinear => Self::Binary(BinaryRelation::Collinear),
            Kind::Concentric => Self::Binary(BinaryRelation::Concentric),
            Kind::Coradial => Self::Binary(BinaryRelation::Coradial),
            Kind::Coincident | Kind::MergePoints => Self::Coincidence,
            Kind::HorizontalPoints | Kind::VerticalPoints => Self::AxisPoints,
            Kind::AtIntersection => Self::AtIntersection,
            Kind::Symmetric => Self::Symmetric,
            Kind::Midpoint => Self::Midpoint,
            Kind::Distance | Kind::Angle | Kind::Radius | Kind::Diameter => Self::Dimensional,
            Kind::OffsetEdge
            | Kind::ArcAngleTop
            | Kind::ArcAngleBottom
            | Kind::ArcAngleLeft
            | Kind::ArcAngleRight
            | Kind::SnapGrid
            | Kind::SnapLength
            | Kind::SnapAngle
            | Kind::UseEdge
            | Kind::EllipseAngleTop
            | Kind::EllipseAngleBottom
            | Kind::EllipseAngleLeft
            | Kind::EllipseAngleRight
            | Kind::AtPierce
            | Kind::DoubleDistance
            | Kind::AngleThreePoint
            | Kind::ArcLength
            | Kind::Normal
            | Kind::NormalPoints
            | Kind::SketchOffset
            | Kind::AlongX
            | Kind::AlongY
            | Kind::AlongZ
            | Kind::AlongXPoints
            | Kind::AlongYPoints
            | Kind::AlongZPoints
            | Kind::ParallelYz
            | Kind::ParallelZx
            | Kind::Intersection
            | Kind::Patterned
            | Kind::IsoByPoint
            | Kind::SameIsoparametric
            | Kind::FitSpline
            | Kind::EqualCurvature
            | Kind::EqualTangent
            | Kind::TangentFace
            | Kind::AlongX3d
            | Kind::AlongY3d
            | Kind::AlongXPoints3d
            | Kind::AlongYPoints3d
            | Kind::Traction
            | Kind::BeltTraction
            | Kind::BlockFixedLock
            | Kind::BlockNormalLock
            | Kind::BlockRotateLock
            | Kind::FakeSlotConstraint
            | Kind::FixedSlot
            | Kind::SameSlots
            | Kind::LinearPatternCount
            | Kind::CircularPatternCount
            | Kind::RadialOffset
            | Kind::PlanarOffset
            | Kind::EqualCurvature3dAligned
            | Kind::FlangeFaceDistance
            | Kind::ConicRho
            | Kind::C3Touch
            | Kind::DoubleAngle
            | Kind::SameCurveLength => Self::Other,
        }
    }
}

#[cfg(test)]
pub(super) fn typed_marker_relation_definition(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    typed_marker_relation_definition_in_sketch(
        ctx,
        marker,
        &SketchId::mint("synthetic:test:sketch#unbound").unwrap(),
        &[],
        markers_by_id,
        loci_by_marker,
    )
}


fn unique_entity_from_link_intersection(
    ctx: &DecodeContext<'_>, marker: &SketchInputEntity, sketch: &SketchId, sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>, loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker entity intersection";
    charge_relation_marker_links(ctx, marker, markers_by_id, OPERATION)?;
    let mut links = marker.links().iter().filter(|link| !relation_link_identifies_owner(marker, link));
    let Some(first) = links.next() else { return Ok(None); };
    let mut candidates = marker_entities(ctx, &first.entity_ref, markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
    let mut count = 0;
    for index in 0..candidates.len() {
        let entity = &candidates[index];
        charge_typed_endpoint_work(ctx, entity.as_str().len(), 8, OPERATION)?;
        if entity.as_str().contains("sketch-entity#relation-point:") || !entity_is_in_sketch(ctx, entity, sketch, sketch_entities, OPERATION)? { continue; }
        let mut matches = true;
        for link in links.clone() {
            let linked = marker_entities(ctx, &link.entity_ref, markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
            if !marker_entity_ids_contain(ctx, &linked, entity, OPERATION)? { matches = false; break; }
        }
        if matches { ctx.charge_work(u64_from_index(std::mem::size_of::<SketchEntityId>()), OPERATION)?; candidates.swap(count, index); count += 1; }
    }
    candidates.truncate(count);
    sort_marker_entity_ids(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 1 { candidates.into_iter().next() } else { None })
}

fn marker_entity_ids_contain(
    ctx: &DecodeContext<'_>, identities: &[SketchEntityId], identity: &SketchEntityId, operation: &'static str,
) -> Result<bool, CodecError> {
    for candidate in identities {
        charge_typed_endpoint_work(ctx, candidate.as_str().len(), 8, operation)?;
        charge_typed_endpoint_work(ctx, identity.as_str().len(), 8, operation)?;
        if candidate == identity { return Ok(true); }
    }
    Ok(false)
}

fn entity_is_in_sketch(
    ctx: &DecodeContext<'_>, identity: &SketchEntityId, sketch: &SketchId, entities: &[SketchEntity], operation: &'static str,
) -> Result<bool, CodecError> {
    for entity in entities {
        charge_typed_endpoint_work(ctx, entity.id().as_str().len(), 8, operation)?;
        charge_typed_endpoint_work(ctx, identity.as_str().len(), 8, operation)?;
        charge_typed_endpoint_work(ctx, entity.sketch.as_str().len(), 8, operation)?;
        charge_typed_endpoint_work(ctx, sketch.as_str().len(), 8, operation)?;
        if entity.id() == identity && entity.sketch == *sketch { return Ok(true); }
    }
    Ok(false)
}


pub(super) fn typed_marker_relation_definition_in_sketch(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    macro_rules! marker_resolved_or_none {
        ($candidate:expr) => {
            match $candidate { Some(value) => value, None => return Ok(None) }
        };
    }
    use crate::records::SketchRelationKind::HorizontalPoints;
    let kind = match marker.kind() {
        SketchInputKind::Relation(kind) => Some(kind),
        SketchInputKind::Native(_) | SketchInputKind::NativeHandle(_) => None,
        _ => return Ok(None),
    };
    if !marker_owns_constraint(ctx, marker, markers_by_id)? {
        return Ok(None);
    }
    let native = || -> Result<SketchConstraintDefinitionInput, CodecError> {
        const OPERATION: &str = "retain SLDPRT native marker relation";
        let mut entities = Vec::new();
        for link in marker.links() {
            ctx.charge_work(u64_from_index(link.entity_ref.len()).checked_add(u64_from_index(marker.id().len()))
                .and_then(|bytes| bytes.checked_mul(4)).and_then(|work| work.checked_add(16))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            if relation_link_identifies_owner(marker, link) { continue; }
            let additions = marker_entities(ctx, &link.entity_ref, markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
            ctx.charge_work(u64_from_index(additions.len()).checked_mul(u64_from_index(std::mem::size_of::<SketchEntityId>()))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            ctx.reserve_collection_vec(&mut entities, additions.len(), OPERATION)?;
            entities.extend(additions);
        }
        sort_marker_entity_ids(ctx, &mut entities, OPERATION)?;
        let owners = relation_owner_markers(ctx, marker, markers_by_id)?;
        for owner in &owners {
            let additions = marker_entities(ctx, owner.id(), markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
            ctx.charge_work(u64_from_index(additions.len()).checked_mul(u64_from_index(std::mem::size_of::<SketchEntityId>()))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            ctx.reserve_collection_vec(&mut entities, additions.len(), OPERATION)?;
            entities.extend(additions);
        }
        sort_marker_entity_ids(ctx, &mut entities, OPERATION)?;
        let mut operands = Vec::new();
        for link in marker.links() {
            ctx.charge_work(u64_from_index(link.entity_ref.len()).checked_mul(4).and_then(|work| work.checked_add(64))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            ctx.reserve_collection_vec(&mut operands, 1, OPERATION)?;
            operands.push(SketchNativeOperand {
                native_kind: nonblank_literal!("sldprt:marker-local-id"), field: None,
                object_index: Some(u32::from(link.local_id)),
                native_ref: Some(crate::text_admission::format_retained(ctx, format_args!("{}", link.entity_ref), OPERATION)?),
            });
        }
        for owner in owners {
            ctx.charge_work(u64_from_index(owner.id().len()).checked_mul(4).and_then(|work| work.checked_add(64))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
            ctx.reserve_collection_vec(&mut operands, 1, OPERATION)?;
            operands.push(SketchNativeOperand {
                native_kind: nonblank_literal!("sldprt:marker-constraint-owner"), field: None,
                object_index: owner.object_index().or(owner.local_id()),
                native_ref: Some(crate::text_admission::format_retained(ctx, format_args!("{}", owner.id()), OPERATION)?),
            });
        }
        Ok(SketchConstraintDefinitionInput::Native {
            native_kind: nonblank_literal!("sldprt:marker-relation:{}", marker.kind().native_code()),
            native_state: None, native_flags: None, native_properties: std::collections::BTreeMap::new(),
            entities, parameter: None, operands,
        })
    };
    let Some(kind) = kind else {
        return Ok(Some(native()?));
    };
    let group = MarkerRelationGroup::of(kind);
    if let MarkerRelationGroup::SingleEntity(single) = group {
        if single == SingleEntityRelation::Fixed {
            if let Some(entity) = unique_entity_from_link_intersection(
                ctx, marker,
                sketch,
                sketch_entities,
                markers_by_id,
                loci_by_marker,
            )? {
                return Ok(Some(SketchConstraintDefinitionInput::Fixed { entity }));
            }
        }
        if let Some((same_coordinate, _)) = single.axes() {
            // Point targets disambiguate these operands when a local/object index
            // happens to collide with the relation handle's index.
            // Forward point links are explicit operands. Reverse incidences are
            // ownership metadata and must not suppress those operands.
            const POINT_OPERATION: &str = "collect SLDPRT forward point links";
            charge_relation_marker_links(ctx, marker, markers_by_id, POINT_OPERATION)?;
            let mut point_links = Vec::new();
            for link in marker.links() {
                if link.entity_ref == marker.id() || matches!(markers_by_id.get(link.entity_ref.as_str()).map(|linked| linked.kind()), Some(SketchInputKind::Relation(_))) { continue; }
                ctx.reserve_collection_vec(&mut point_links, 1, POINT_OPERATION)?;
                point_links.push(link);
            }
            if let [first_link, second_link] = point_links.as_slice() {
                let point_locus = |link: &SketchInputLink| -> Result<Option<SketchLocus>, CodecError> {
                    const OPERATION: &str = "resolve SLDPRT forward axis point identity";
                    charge_relation_marker_links(ctx, marker, markers_by_id, OPERATION)?;
                    let Some(linked) = markers_by_id.get(link.entity_ref.as_str()) else { return Ok(None); };
                    if !matches!(linked.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint) { return Ok(None); }
                    let mut selected = None;
                    for entity in sketch_entities {
                        charge_typed_endpoint_work(ctx, entity.sketch.as_str().len(), 4, OPERATION)?;
                        charge_typed_endpoint_work(ctx, sketch.as_str().len(), 4, OPERATION)?;
                        charge_typed_endpoint_work(ctx, entity.native_ref.as_deref().map_or(0, str::len), 4, OPERATION)?;
                        charge_typed_endpoint_work(ctx, link.entity_ref.len(), 4, OPERATION)?;
                        if entity.sketch != *sketch || entity.native_ref.as_deref() != Some(link.entity_ref.as_str())
                            || !matches!(entity.geometry.definition(), SketchGeometryDefinition::Point { .. }) { continue; }
                        if selected.is_some() { return Ok(None); }
                        selected = Some(entity.id());
                    }
                    if let Some(identity) = selected {
                        return super::transforms::SketchLocusRole::Entity.copy_locus(ctx, identity, OPERATION).map(Some);
                    }
                    let Some(locus) = marker_point_locus(ctx, &link.entity_ref, markers_by_id, loci_by_marker)? else { return Ok(None); };
                    for entity in sketch_entities {
                        charge_typed_endpoint_work(ctx, entity.sketch.as_str().len(), 4, OPERATION)?;
                        charge_typed_endpoint_work(ctx, sketch.as_str().len(), 4, OPERATION)?;
                        charge_typed_endpoint_work(ctx, entity.id().as_str().len(), 4, OPERATION)?;
                        charge_typed_endpoint_work(ctx, locus_entity(&locus).as_str().len(), 4, OPERATION)?;
                        if entity.sketch == *sketch && entity.id() == locus_entity(&locus) { return Ok(Some(locus)); }
                    }
                    Ok(None)
                };
                if let (Some(first), Some(second)) =
                    (point_locus(first_link)?, point_locus(second_link)?)
                {
                    if first != second {
                        return Ok(Some(SketchConstraintDefinitionInput::SameCoordinate {
                            relation: marker_resolved_or_none!(cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                first,
                                second,
                                same_coordinate,
                            )
                            .ok()),
                        }));
                    }
                }
            }
        }
    }
    Ok(Some(match group {
        MarkerRelationGroup::SingleEntity(single) => {
            let axes = single.axes();
            if let Some((same_coordinate, _)) = axes {
                if let Some([first, second]) = axis_relation_point_loci(ctx, 
                    marker,
                    sketch,
                    sketch_entities,
                    markers_by_id,
                    loci_by_marker,
                )? {
                    return Ok(Some(SketchConstraintDefinitionInput::SameCoordinate {
                        relation: marker_resolved_or_none!(cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                            first,
                            second,
                            same_coordinate,
                        )
                        .ok()),
                    }));
                }
            }
            if let Some((same_coordinate, _)) = axes {
                const POINT_OPERATION: &str = "collect SLDPRT owned point links";
                charge_relation_marker_links(ctx, marker, markers_by_id, POINT_OPERATION)?;
                let mut point_links = Vec::new();
                for link in marker.links() {
                    if relation_link_identifies_owner(marker, link) { continue; }
                    ctx.reserve_collection_vec(&mut point_links, 1, POINT_OPERATION)?;
                    point_links.push(link);
                }
                if let [first_link, second_link] = point_links.as_slice() {
                    let point_links = [first_link, second_link];
                    if point_links.into_iter().all(|link| {
                        matches!(
                            markers_by_id
                                .get(link.entity_ref.as_str())
                                .map(|linked| linked.kind()),
                            Some(SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
                        )
                    }) {
                        if let Some(loci) =
                            relation_operand_loci(ctx, marker, markers_by_id, loci_by_marker)?
                        {
                            if let Ok([first, second]) = <[SketchLocus; 2]>::try_from(loci) {
                                return Ok(Some(SketchConstraintDefinitionInput::SameCoordinate {
                                    relation: marker_resolved_or_none!(cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                        first,
                                        second,
                                        same_coordinate,
                                    )
                                    .ok()),
                                }));
                            }
                        }
                    }
                }
            }
            let inferred_entities = marker_entities(ctx, marker.id(), markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
            const ENTITY_OPERATION: &str = "resolve SLDPRT exact marker entities";
            charge_relation_marker_links(ctx, marker, markers_by_id, ENTITY_OPERATION)?;
            let mut exact_entities = Vec::new();
            for link in marker.links() {
                if relation_link_identifies_owner(marker, link) { continue; }
                let Some(linked) = markers_by_id.get(link.entity_ref.as_str()) else { continue; };
                if single == SingleEntityRelation::Fixed && matches!(linked.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint | SketchInputKind::LineOrCircle | SketchInputKind::Arc) {
                    let additions = marker_entities(ctx, &link.entity_ref, markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
                    ctx.reserve_collection_vec(&mut exact_entities, additions.len(), ENTITY_OPERATION)?;
                    ctx.charge_work(u64_from_index(additions.len()).checked_mul(u64_from_index(std::mem::size_of::<SketchEntityId>()))
                        .ok_or_else(|| ctx.refuse_codec_limit(ENTITY_OPERATION, u64::MAX - 1, u64::MAX))?, ENTITY_OPERATION)?;
                    exact_entities.extend(additions);
                    continue;
                }
                if !matches!(linked.kind(), SketchInputKind::LineOrCircle | SketchInputKind::Arc) { continue; }
                let mut selected = None;
                let mut ambiguous = false;
                for entity in sketch_entities {
                    charge_typed_endpoint_work(ctx, entity.native_ref.as_deref().map_or(0, str::len), 8, ENTITY_OPERATION)?;
                    charge_typed_endpoint_work(ctx, link.entity_ref.len(), 8, ENTITY_OPERATION)?;
                    if entity.native_ref.as_deref() != Some(link.entity_ref.as_str()) { continue; }
                    if selected.is_some() { ambiguous = true; break; }
                    selected = Some(entity.id());
                }
                if ambiguous { continue; }
                if let Some(identity) = selected {
                    ctx.reserve_collection_vec(&mut exact_entities, 1, ENTITY_OPERATION)?;
                    exact_entities.push(super::transforms::copy_sketch_entity_identity(ctx, identity, ENTITY_OPERATION)?);
                }
            }
            sort_marker_entity_ids(ctx, &mut exact_entities, ENTITY_OPERATION)?;
            let direct_entities = if exact_entities.len() == 1 {
                exact_entities
            } else {
                inferred_entities
            };
            let relation_owners = relation_owner_markers(ctx, marker, markers_by_id)?;
            let point_owner_pair = matches!(relation_owners.as_slice(), [first, second]
                if matches!(first.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
                    && matches!(second.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint));
            let owner_entities =
                relation_owner_curve_entities(ctx, marker, markers_by_id, loci_by_marker)?;
            for identity in &direct_entities {
                charge_typed_endpoint_work(ctx, identity.as_str().len(), 8, ENTITY_OPERATION)?;
                for owner in &owner_entities { charge_typed_endpoint_work(ctx, owner.as_str().len(), 8, ENTITY_OPERATION)?; }
            }
            let entities = if point_owner_pair && axes.is_some() {
                Vec::new()
            } else {
                match owner_entities.as_slice() {
                    [owner]
                        if direct_entities.iter().all(|entity| {
                            entity == owner
                                || entity.as_str().contains("sketch-entity#relation-point:")
                        }) =>
                    {
                        owner_entities
                    }
                    _ => direct_entities,
                }
            };
            if let [entity] = entities.as_slice() {
                if axes.is_some()
                    && sketch_entities.is_empty()
                    && entity.as_str().contains("sketch-entity#relation-point:")
                {
                    return Ok(Some(native()?));
                }
                single.definition(marker_resolved_or_none!(entities.into_iter().next()))
            } else if let Some((same_coordinate, profile_axis)) = axes {
                let loci = match relation_operand_loci(ctx, marker, markers_by_id, loci_by_marker)? {
                    Some(loci) => Some(loci),
                    None => unique_axis_aligned_linked_loci(ctx, marker, sketch, sketch_entities, markers_by_id, loci_by_marker, profile_axis)?,
                };
                let Some(loci) = loci else {
                    return Ok(Some(native()?));
                };
                let Ok([first, second]) = <[SketchLocus; 2]>::try_from(loci) else { return Ok(Some(native()?)); };
                match cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                    first,
                    second,
                    same_coordinate,
                ) {
                    Ok(relation) => SketchConstraintDefinitionInput::SameCoordinate { relation },
                    Err(_) => native()?,
                }
            } else {
                return Ok(Some(native()?));
            }
        }
        MarkerRelationGroup::ArcQuarter(quarter) => {
            let Some(entity) = linked_single_arc_entity(ctx, marker, markers_by_id, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            let angle = quarter.angle();
            if !sketch_entities.is_empty() {
                let Some(SketchGeometryDefinition::Arc {
                    start_angle,
                    end_angle,
                    ..
                }) = (sketch_entities
                    .iter()
                    .find(|candidate| candidate.id() == &entity))
                .map(|entity| entity.geometry.definition())
                else {
                    return Ok(Some(native()?));
                };
                let raw = end_angle.get() - start_angle.get();
                let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                if sweep <= EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
                    && raw.abs()
                        > EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
                {
                    sweep = std::f64::consts::TAU;
                }
                if !same_dimension_angle(sweep, angle.get()) {
                    return Ok(Some(native()?));
                }
            }
            SketchConstraintDefinitionInput::ArcAngle { entity, angle }
        }
        MarkerRelationGroup::EllipseQuarter(quarter) => {
            let Some(entity) = linked_single_ellipse_entity(
                ctx, marker,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            )? else {
                return Ok(Some(native()?));
            };
            let angle = quarter.angle();
            let Some(SketchGeometryDefinition::Ellipse {
                bounds: Some([start, end]),
                ..
            }) = (sketch_entities
                .iter()
                .find(|candidate| candidate.id() == &entity))
            .map(|entity| entity.geometry.definition())
            else {
                return Ok(Some(native()?));
            };
            let raw = end.get() - start.get();
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
                && raw.abs() > EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
            {
                sweep = std::f64::consts::TAU;
            }
            if !same_dimension_angle(sweep, angle.get()) {
                return Ok(Some(native()?));
            }
            SketchConstraintDefinitionInput::EllipseAngle { entity, angle }
        }
        MarkerRelationGroup::Binary(binary) => {
            let owner_entities =
                relation_owner_curve_entities(ctx, marker, markers_by_id, loci_by_marker)?;
            const BINARY_OPERATION: &str = "resolve SLDPRT binary marker entities";
            charge_relation_marker_links(ctx, marker, markers_by_id, BINARY_OPERATION)?;
            let mut forward_entities = Vec::new();
            for link in marker.links() {
                if relation_link_identifies_owner(marker, link) { continue; }
                for entity in marker_entities(ctx, &link.entity_ref, markers_by_id, loci_by_marker, MarkerEntityFilter::All)? {
                    charge_typed_endpoint_work(ctx, entity.as_str().len(), 8, BINARY_OPERATION)?;
                    if entity.as_str().contains("sketch-entity#relation-point:") { continue; }
                    ctx.reserve_collection_vec(&mut forward_entities, 1, BINARY_OPERATION)?;
                    ctx.charge_work(u64_from_index(std::mem::size_of::<SketchEntityId>()), BINARY_OPERATION)?;
                    forward_entities.push(entity);
                }
            }
            let geometry_pair = if owner_entities.is_empty() && !sketch_entities.is_empty() {
                let mut links = marker.links().iter().filter(|link| !relation_link_identifies_owner(marker, link));
                if let (Some(first_link), Some(second_link), None) = (links.next(), links.next(), links.next()) {
                    let resolve = |link: &SketchInputLink| -> Result<Vec<SketchEntityId>, CodecError> {
                        let mut candidates = marker_entities(ctx, &link.entity_ref, markers_by_id, loci_by_marker, MarkerEntityFilter::All)?;
                        let mut count = 0;
                        for index in 0..candidates.len() {
                            if !entity_is_in_sketch(ctx, &candidates[index], sketch, sketch_entities, BINARY_OPERATION)? { continue; }
                            ctx.charge_work(u64_from_index(std::mem::size_of::<SketchEntityId>()), BINARY_OPERATION)?;
                            candidates.swap(count, index); count += 1;
                        }
                        candidates.truncate(count);
                        Ok(candidates)
                    };
                    let candidates = [resolve(first_link)?, resolve(second_link)?];
                    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
                    let mut ambiguous = false;
                    for first in &candidates[0] {
                        for second in &candidates[1] {
                            charge_typed_endpoint_work(ctx, first.as_str().len(), 8, BINARY_OPERATION)?;
                            charge_typed_endpoint_work(ctx, second.as_str().len(), 8, BINARY_OPERATION)?;
                            if first == second { continue; }
                            let Some(first_entity) = find_profile_entity(ctx, sketch_entities, first, BINARY_OPERATION)? else { continue; };
                            let Some(second_entity) = find_profile_entity(ctx, sketch_entities, second, BINARY_OPERATION)? else { continue; };
                            ctx.charge_work(512, BINARY_OPERATION)?;
                            if !binary.matches_evaluated_geometry(first_entity, second_entity) { continue; }
                            if let Some((selected_first, selected_second)) = selected {
                                charge_typed_endpoint_work(ctx, selected_first.as_str().len(), 8, BINARY_OPERATION)?;
                                charge_typed_endpoint_work(ctx, selected_second.as_str().len(), 8, BINARY_OPERATION)?;
                                if selected_first != first || selected_second != second { ambiguous = true; }
                            }
                            selected = Some((first, second));
                        }
                    }
                    if ambiguous { None } else if let Some((first, second)) = selected {
                        Some((super::transforms::copy_sketch_entity_identity(ctx, first, BINARY_OPERATION)?, super::transforms::copy_sketch_entity_identity(ctx, second, BINARY_OPERATION)?))
                    } else { None }
                } else { None }
            } else { None };
            let mut all_owned = true;
            if owner_entities.len() == 2 {
                for entity in &forward_entities {
                    if !marker_entity_ids_contain(ctx, &owner_entities, entity, BINARY_OPERATION)? { all_owned = false; break; }
                }
            }
            let entities = if owner_entities.len() == 2 && all_owned
            {
                owner_entities
            } else if let Some((first, second)) = geometry_pair {
                vec![first, second]
            } else {
                let Some(entities) = linked_single_entities(ctx, marker, markers_by_id, loci_by_marker)?
                else {
                    return Ok(Some(native()?));
                };
                entities
            };
            let [first, second] = entities.as_slice() else {
                return Ok(Some(native()?));
            };
            if !sketch_entities.is_empty() {
                let Some(_first_entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id() == first)
                else {
                    return Ok(Some(native()?));
                };
                let Some(_second_entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id() == second)
                else {
                    return Ok(Some(native()?));
                };
            }
            let [first, second] = marker_resolved_or_none!(<[SketchEntityId; 2]>::try_from(entities).ok());
            binary.definition(first, second)
        }
        MarkerRelationGroup::Coincidence => {
            let Some(loci) = relation_operand_loci(ctx, marker, markers_by_id, loci_by_marker)? else {
                return Ok(Some(native()?));
            };
            if loci.len() < 2 {
                return Ok(Some(native()?));
            }
            if !sketch_entities.is_empty() {
                for locus in &loci {
                    if profile_locus_point_charged(ctx, locus, sketch_entities, "resolve SLDPRT coincidence locus")?.is_none() {
                        return Ok(Some(native()?));
                    }
                }
            }
            SketchConstraintDefinitionInput::CoincidentLoci { loci }
        }
        MarkerRelationGroup::AxisPoints => {
            let Some(loci) = relation_operand_loci(ctx, marker, markers_by_id, loci_by_marker)? else {
                return Ok(Some(native()?));
            };
            let Ok([first, second]) = <[SketchLocus; 2]>::try_from(loci) else { return Ok(Some(native()?)); };
            match cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                first,
                second,
                if kind == HorizontalPoints {
                    SketchCoordinateAxis::V
                } else {
                    SketchCoordinateAxis::U
                },
            ) {
                    Ok(relation) => SketchConstraintDefinitionInput::SameCoordinate { relation },
                    Err(_) => native()?,
                }
        }
        MarkerRelationGroup::AtIntersection => {
            if sketch_entities.is_empty() {
                return Ok(Some(native()?));
            }
            let Some(loci) = relation_operand_loci(ctx, marker, markers_by_id, loci_by_marker)? else {
                return Ok(Some(native()?));
            };
            const OPERATION: &str = "resolve SLDPRT marker intersection operands";
            let mut point = None;
            let mut entities = Vec::new();
            for locus in loci {
                let Some(entity) = find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?
                else {
                    return Ok(Some(native()?));
                };
                let entity_locus = matches!(locus, SketchLocus::Entity(_));
                if entity_locus
                    && !matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                            | SketchGeometryDefinition::Native { .. }
                    )
                {
                    ctx.charge_work(u64_from_index(std::mem::size_of::<SketchEntityId>()), OPERATION)?;
                    ctx.reserve_collection_vec(&mut entities, 1, OPERATION)?;
                    let identity = match locus {
                        SketchLocus::Entity(identity) | SketchLocus::Start(identity)
                        | SketchLocus::End(identity) | SketchLocus::Center(identity) => identity,
                    };
                    entities.push(identity);
                } else if point.replace(locus).is_some() {
                    return Ok(Some(native()?));
                }
            }
            let (Some(point), [first, second]) = (point, entities.as_slice()) else {
                return Ok(Some(native()?));
            };
            charge_typed_endpoint_work(ctx, first.as_str().len(), 4, OPERATION)?;
            charge_typed_endpoint_work(ctx, second.as_str().len(), 4, OPERATION)?;
            if first == second {
                return Ok(Some(native()?));
            }
            let Some(position) = profile_locus_point_charged(ctx, &point, sketch_entities, OPERATION)? else {
                return Ok(Some(native()?));
            };
            for identity in [first, second] {
                let Some(entity) = find_profile_entity(ctx, sketch_entities, identity, OPERATION)? else {
                    return Ok(Some(native()?));
                };
                ctx.charge_work(256, OPERATION)?;
                if !sketch_entity_contains_point(entity, position) {
                    return Ok(Some(native()?));
                }
            }
            let [first, second] = marker_resolved_or_none!(<[SketchEntityId; 2]>::try_from(entities).ok());
            SketchConstraintDefinitionInput::AtIntersection { point, first, second }
        }
        MarkerRelationGroup::Symmetric => {
            if sketch_entities.is_empty() {
                return Ok(Some(native()?));
            }
            let Some(loci) = relation_operand_loci(ctx, marker, markers_by_id, loci_by_marker)? else {
                return Ok(Some(native()?));
            };
            const OPERATION: &str = "resolve SLDPRT symmetric marker operands";
            let mut axis = None;
            let mut points = Vec::new();
            for locus in loci {
                let entity = find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?;
                if matches!(locus, SketchLocus::Entity(_))
                    && entity.is_some_and(|entity| {
                        matches!(
                            *entity.geometry.definition(),
                            SketchGeometryDefinition::Line { .. }
                        )
                    })
                {
                    if axis.replace(match locus {
                        SketchLocus::Entity(entity) | SketchLocus::Start(entity) | SketchLocus::End(entity) | SketchLocus::Center(entity) => entity,
                    }).is_some() {
                        return Ok(Some(native()?));
                    }
                } else {
                    ctx.charge_work(u64_from_index(std::mem::size_of::<SketchLocus>()), OPERATION)?;
                    ctx.reserve_collection_vec(&mut points, 1, OPERATION)?;
                    points.push(locus);
                }
            }
            let (Some(axis), [first, second]) = (axis, points.as_slice()) else {
                return Ok(Some(native()?));
            };
            charge_typed_endpoint_work(ctx, locus_entity(first).as_str().len(), 4, OPERATION)?;
            charge_typed_endpoint_work(ctx, locus_entity(second).as_str().len(), 4, OPERATION)?;
            if first == second {
                return Ok(Some(native()?));
            }
            let Some(first_point) = profile_locus_point_charged(ctx, first, sketch_entities, OPERATION)? else {
                return Ok(Some(native()?));
            };
            let Some(second_point) = profile_locus_point_charged(ctx, second, sketch_entities, OPERATION)? else {
                return Ok(Some(native()?));
            };
            let Some(axis_entity) = find_profile_entity(ctx, sketch_entities, &axis, OPERATION)? else {
                return Ok(Some(native()?));
            };
            ctx.charge_work(256, OPERATION)?;
            if symmetric_loci_match_axis(first_point, second_point, axis_entity) != Some(true) {
                return Ok(Some(native()?));
            }
            let [first, second] = marker_resolved_or_none!(<[SketchLocus; 2]>::try_from(points).ok());
            SketchConstraintDefinitionInput::Symmetric { first, second, axis }
        }
        MarkerRelationGroup::Midpoint => {
            let Some((point, entity)) =
                linked_midpoint_operands(ctx, marker, markers_by_id, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            if !sketch_entities.is_empty() {
                let Some(point_position) = profile_locus_point_charged(ctx, &point, sketch_entities, "resolve SLDPRT profile locus")? else {
                    return Ok(Some(native()?));
                };
                let Some(midpoint) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id() == &entity)
                    .and_then(sketch_entity_midpoint)
                else {
                    return Ok(Some(native()?));
                };
                if !same_dimension_length(point_position.u, midpoint.u)
                    || !same_dimension_length(point_position.v, midpoint.v)
                {
                    return Ok(Some(native()?));
                }
            }
            SketchConstraintDefinitionInput::Midpoint { point, entity }
        }
        MarkerRelationGroup::Dimensional => return Ok(None),
        MarkerRelationGroup::Other => native()?,
    }))
}

fn sketch_entity_midpoint(entity: &SketchEntity) -> Option<Point2> {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some(Point2::new(
            start.u.midpoint(end.u),
            start.v.midpoint(end.v),
        )),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let raw = end_angle.get() - start_angle.get();
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_MIDPOINT_E12
                && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_MIDPOINT_E12
            {
                sweep = std::f64::consts::TAU;
            }
            let angle = start_angle.get() + sweep * 0.5;
            Some(Point2::new(
                center.u + radius.get() * angle.cos(),
                center.v + radius.get() * angle.sin(),
            ))
        }
        _ => None,
    }
}

pub(super) fn sketch_entity_contains_point(entity: &SketchEntity, point: Point2) -> bool {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let scale = du.abs().max(dv.abs());
            let length = du.hypot(dv);
            if !length.is_finite() || length <= SKETCH_POINT_TOLERANCE {
                return false;
            }
            let u = du / scale;
            let v = dv / scale;
            let parameter = (((point.u - start.u) / scale) * u + ((point.v - start.v) / scale) * v)
                / (u * u + v * v);
            let distance =
                ((point.u - start.u) * (dv / length) - (point.v - start.v) * (du / length)).abs();
            distance <= SKETCH_POINT_TOLERANCE
                && (-SKETCH_POINT_TOLERANCE..=1.0 + SKETCH_POINT_TOLERANCE).contains(&parameter)
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            length > SKETCH_POINT_TOLERANCE
                && ((point.u - origin.u) * direction.v - (point.v - origin.v) * direction.u).abs()
                    <= SKETCH_POINT_TOLERANCE * length
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.get())
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            if !same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.get())
            {
                return false;
            }
            let raw = end_angle.get() - start_angle.get();
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
            {
                sweep = std::f64::consts::TAU;
            }
            let parameter = ((point.v - center.v).atan2(point.u - center.u) - start_angle.get())
                .rem_euclid(std::f64::consts::TAU);
            parameter <= sweep + EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let major_radius = radii.major();
            let minor_radius = radii.minor();
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            if !du.is_finite() || !dv.is_finite() {
                return false;
            }
            let x = du * cosine + dv * sine;
            let y = -du * sine + dv * cosine;
            let equation = (x / major_radius.get()).powi(2) + (y / minor_radius.get()).powi(2);
            if !equation.is_finite()
                || (equation - 1.0).abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
            {
                return false;
            }
            match bounds {
                Some([start, end]) => {
                    let parameter = ((y / minor_radius.get()).atan2(x / major_radius.get())
                        - start.get())
                    .rem_euclid(std::f64::consts::TAU);
                    let raw = end.get() - start.get();
                    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                    if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                        && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                    {
                        sweep = std::f64::consts::TAU;
                    }
                    parameter <= sweep + EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
                }
                None => true,
            }
        }
        SketchGeometryDefinition::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let x = du.mul_add(cosine, dv * sine);
            let y = (-du).mul_add(sine, dv * cosine);
            if !x.is_finite() || !y.is_finite() {
                return false;
            }
            let ratio = y / minor_radius.get();
            let parameter = if ratio.is_finite() {
                ratio.asinh()
            } else {
                (y.abs().ln() - minor_radius.get().ln() + std::f64::consts::LN_2).copysign(y)
            };
            let Some(Ok((_, expected_x))) =
                cadmpeg_ir::scalar::FiniteReal::new(parameter).map(|parameter| {
                    cadmpeg_ir::math::scaled_sinh_cosh(major_radius.magnitude(), parameter)
                })
            else {
                return false;
            };
            let on_curve = (x - expected_x.get()).abs() <= SKETCH_POINT_TOLERANCE * (1.0 + x.abs());
            on_curve
                && bounds.as_ref().is_none_or(|[start, end]| {
                    (start.get().min(end.get()) - SKETCH_POINT_TOLERANCE
                        ..=start.get().max(end.get()) + SKETCH_POINT_TOLERANCE)
                        .contains(&parameter)
                })
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let cosine = axis_angle.get().cos();
            let sine = axis_angle.get().sin();
            let du = point.u - vertex.u;
            let dv = point.v - vertex.v;
            let x = du * cosine + dv * sine;
            let parameter = -du * sine + dv * cosine;
            let Some(axial) = cadmpeg_ir::math::product_quotient(
                [parameter, parameter],
                [4.0, focal_length.get()],
            ) else {
                return false;
            };
            let on_curve = (x - axial.get()).abs() <= SKETCH_POINT_TOLERANCE * (1.0 + x.abs());
            on_curve
                && bounds.as_ref().is_none_or(|[start, end]| {
                    (start.get().min(end.get()) - SKETCH_POINT_TOLERANCE
                        ..=start.get().max(end.get()) + SKETCH_POINT_TOLERANCE)
                        .contains(&parameter)
                })
        }
        SketchGeometryDefinition::Point { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::Nurbs { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => false,
    }
}

pub(super) fn symmetric_loci_match_axis(
    first: Point2,
    second: Point2,
    axis: &SketchEntity,
) -> Option<bool> {
    let SketchGeometryDefinition::Line { start, end } = *axis.geometry.definition() else {
        return None;
    };
    let du = end.u - start.u;
    let dv = end.v - start.v;
    let length = du.hypot(dv);
    if length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let coordinates = |point: Point2| {
        (
            ((point.u - start.u) * du + (point.v - start.v) * dv) / length,
            ((point.u - start.u) * dv - (point.v - start.v) * du) / length,
        )
    };
    let (first_along, first_across) = coordinates(first);
    let (second_along, second_across) = coordinates(second);
    Some(
        same_dimension_length(first_along, second_along)
            && same_dimension_length(first_across, -second_across),
    )
}

pub(super) fn binary_relation_matches_evaluated_geometry(
    kind: crate::records::SketchRelationKind,
    first: &SketchEntity,
    second: &SketchEntity,
) -> bool {
    match MarkerRelationGroup::of(kind) {
        MarkerRelationGroup::Binary(binary) => binary.matches_evaluated_geometry(first, second),
        _ => false,
    }
}

impl BinaryRelation {
    /// Whether the evaluated geometry of the two entities satisfies this relation.
    fn matches_evaluated_geometry(self, first: &SketchEntity, second: &SketchEntity) -> bool {
        use BinaryRelation::{
            Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
        };
        match self {
            Parallel => line_relation_value(first, second, |cross, _dot, lengths| {
                cross.abs() <= SKETCH_POINT_TOLERANCE * lengths
            }),
            Perpendicular => line_relation_value(first, second, |_cross, dot, lengths| {
                dot.abs() <= SKETCH_POINT_TOLERANCE * lengths
            }),
            Collinear => line_line_distance(first, second)
                .is_some_and(|distance| same_dimension_length(distance, 0.0)),
            Concentric => centered_geometry(first)
                .zip(centered_geometry(second))
                .is_some_and(|(first, second)| {
                    same_dimension_length(first.u, second.u)
                        && same_dimension_length(first.v, second.v)
                }),
            Coradial => centered_geometry(first)
                .zip(circular_radius(first))
                .zip(centered_geometry(second).zip(circular_radius(second)))
                .is_some_and(
                    |((first_center, first_radius), (second_center, second_radius))| {
                        same_dimension_length(first_center.u, second_center.u)
                            && same_dimension_length(first_center.v, second_center.v)
                            && same_dimension_length(first_radius, second_radius)
                    },
                ),
            Equal => equal_geometry_size(first, second),
            Tangent => tangent_geometry(first, second),
        }
    }
}

fn line_relation_value(
    first: &SketchEntity,
    second: &SketchEntity,
    predicate: impl FnOnce(f64, f64, f64) -> bool,
) -> bool {
    let Some((first_u, first_v, first_length)) = line_direction(first) else {
        return false;
    };
    let Some((second_u, second_v, second_length)) = line_direction(second) else {
        return false;
    };
    predicate(
        first_u * second_v - first_v * second_u,
        first_u * second_u + first_v * second_v,
        first_length * second_length,
    )
}

fn line_direction(entity: &SketchEntity) -> Option<(f64, f64, f64)> {
    let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
        return None;
    };
    let u = end.u - start.u;
    let v = end.v - start.v;
    let length = u.hypot(v);
    (length > SKETCH_POINT_TOLERANCE).then_some((u, v, length))
}

fn centered_geometry(entity: &SketchEntity) -> Option<Point2> {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Circle { center, .. }
        | SketchGeometryDefinition::Arc { center, .. }
        | SketchGeometryDefinition::Ellipse { center, .. } => Some(center.get()),
        _ => None,
    }
}

fn circular_radius(entity: &SketchEntity) -> Option<f64> {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Circle { radius, .. }
        | SketchGeometryDefinition::Arc { radius, .. } => Some(radius.get()),
        _ => None,
    }
}

fn equal_geometry_size(first: &SketchEntity, second: &SketchEntity) -> bool {
    match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometryDefinition::Line {
                start: second_start,
                end: second_end,
            },
        ) => same_dimension_length(
            (first_end.u - first_start.u).hypot(first_end.v - first_start.v),
            (second_end.u - second_start.u).hypot(second_end.v - second_start.v),
        ),
        (
            SketchGeometryDefinition::Circle {
                radius: first_radius,
                ..
            }
            | SketchGeometryDefinition::Arc {
                radius: first_radius,
                ..
            },
            SketchGeometryDefinition::Circle {
                radius: second_radius,
                ..
            }
            | SketchGeometryDefinition::Arc {
                radius: second_radius,
                ..
            },
        ) => same_dimension_length(first_radius.get(), second_radius.get()),
        (
            SketchGeometryDefinition::Ellipse {
                radii: first_radii, ..
            },
            SketchGeometryDefinition::Ellipse {
                radii: second_radii,
                ..
            },
        ) => {
            same_dimension_length(first_radii.major().get(), second_radii.major().get())
                && same_dimension_length(first_radii.minor().get(), second_radii.minor().get())
        }
        _ => false,
    }
}

fn tangent_geometry(first: &SketchEntity, second: &SketchEntity) -> bool {
    let line_circle = |line: &SketchEntity, circle: &SketchEntity| {
        if let SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            ..
        } = circle.geometry.definition()
        {
            let Some((du, dv, length)) = line_direction(line) else {
                return false;
            };
            let normal = [-dv / length, du / length];
            let major = [major_angle.get().cos(), major_angle.get().sin()];
            let minor = [-major[1], major[0]];
            let support = ((radii.major().get() * (normal[0] * major[0] + normal[1] * major[1]))
                .powi(2)
                + (radii.minor().get() * (normal[0] * minor[0] + normal[1] * minor[1])).powi(2))
            .sqrt();
            return point_line_distance_value(center.get(), line)
                .is_some_and(|distance| same_dimension_length(distance, support));
        }
        centered_geometry(circle)
            .zip(circular_radius(circle))
            .and_then(|(center, radius)| {
                point_line_distance_value(center, line).map(|distance| (distance, radius))
            })
            .is_some_and(|(distance, radius)| same_dimension_length(distance, radius))
    };
    if matches!(
        *first.geometry.definition(),
        SketchGeometryDefinition::Line { .. }
    ) {
        return line_circle(first, second);
    }
    if matches!(
        *second.geometry.definition(),
        SketchGeometryDefinition::Line { .. }
    ) {
        return line_circle(second, first);
    }
    centered_geometry(first)
        .zip(circular_radius(first))
        .zip(centered_geometry(second).zip(circular_radius(second)))
        .is_some_and(
            |((first_center, first_radius), (second_center, second_radius))| {
                let center_distance =
                    (second_center.u - first_center.u).hypot(second_center.v - first_center.v);
                same_dimension_length(center_distance, first_radius + second_radius)
                    || same_dimension_length(center_distance, (first_radius - second_radius).abs())
            },
        )
}

pub(super) fn unique_axis_aligned_linked_loci(
    ctx: &DecodeContext<'_>, marker: &SketchInputEntity, sketch: &SketchId,
    sketch_entities: &[SketchEntity], markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>, axis: ProfileAxis,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    const OPERATION: &str = "select SLDPRT linked axis loci";
    ctx.charge_work(u64_from_index(markers_by_id.len()), OPERATION)?;
    let key_bytes = markers_by_id.keys().try_fold(0u64, |bytes, key| {
        bytes.checked_add(u64_from_index(key.len())).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
    })?;
    let mut links = [None, None];
    let mut count = 0;
    for link in marker.links() {
        ctx.charge_work(key_bytes.checked_add(u64_from_index(link.entity_ref.len())).and_then(|bytes| bytes.checked_add(u64_from_index(marker.id().len())))
            .and_then(|bytes| bytes.checked_mul(4)).and_then(|work| work.checked_add(16))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if !relation_link_is_geometric_operand(marker, link, markers_by_id) { continue; }
        if count == links.len() { return Ok(None); }
        links[count] = Some(link);
        count += 1;
    }
    let [Some(first_link), Some(second_link)] = links else { return Ok(None); };
    let first = marker_point_locus(ctx, &first_link.entity_ref, markers_by_id, loci_by_marker)?;
    let second = marker_point_locus(ctx, &second_link.entity_ref, markers_by_id, loci_by_marker)?;
    let (known, known_is_first) = match (first, second) {
        (Some(known), None) => (known, true),
        (None, Some(known)) => (known, false),
        _ => return Ok(None),
    };
    let Some(known_point) = profile_locus_point_charged(ctx, &known, sketch_entities, OPERATION)? else { return Ok(None); };
    let mut selected: Option<SketchLocus> = None;
    for (candidate_point, candidate) in canonical_profile_loci(ctx, sketch, sketch_entities)? {
        let selected_id = selected.as_ref().map_or("", |locus| locus_entity(locus).as_str());
        let bytes = u64_from_index(locus_entity(&candidate).as_str().len()).checked_add(u64_from_index(locus_entity(&known).as_str().len()))
            .and_then(|bytes| bytes.checked_add(u64_from_index(selected_id.len()))).and_then(|bytes| bytes.checked_mul(4))
            .and_then(|work| work.checked_add(128)).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(bytes, OPERATION)?;
        let aligned = if axis == ProfileAxis::U { same_dimension_length(candidate_point.v, known_point.v) } else { same_dimension_length(candidate_point.u, known_point.u) };
        if candidate == known || !aligned { continue; }
        if selected.as_ref().is_some_and(|selected| selected != &candidate) { return Ok(None); }
        selected = Some(candidate);
    }
    let Some(candidate) = selected else { return Ok(None); };
    Ok(Some(if known_is_first { vec![known, candidate] } else { vec![candidate, known] }))
}

struct AxisRelationPointCollection<'a> {
    visited: HashSet<&'a str>,
    visited_bytes: u64,
    loci: Vec<SketchLocus>,
    locus_bytes: u64,
}

impl<'a> AxisRelationPointCollection<'a> {
    fn append(&mut self, ctx: &DecodeContext<'_>, locus: SketchLocus) -> Result<(), CodecError> {
        const OPERATION: &str = "collect SLDPRT axis relation point loci";
        let bytes = u64_from_index(locus_entity(&locus).as_str().len());
        ctx.charge_work(self.locus_bytes.checked_add(bytes).and_then(|bytes| bytes.checked_mul(4))
            .and_then(|work| work.checked_add(u64_from_index(self.loci.len()).checked_add(1)?
                .checked_mul(u64_from_index(std::mem::size_of::<SketchLocus>()))?))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
        if self.loci.contains(&locus) { return Ok(()); }
        let next_bytes = self.locus_bytes.checked_add(bytes)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.reserve_collection_vec(&mut self.loci, 1, OPERATION)?;
        self.loci.push(locus);
        self.locus_bytes = next_bytes;
        Ok(())
    }
}

fn axis_relation_point_loci(
    ctx: &DecodeContext<'_>, relation: &SketchInputEntity, sketch: &SketchId,
    sketch_entities: &[SketchEntity], markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<[SketchLocus; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT nested axis relation points";
    charge_relation_marker_links(ctx, relation, markers_by_id, OPERATION)?;
    if !relation.links().iter().any(|link| {
        !relation_link_identifies_owner(relation, link) && matches!(markers_by_id.get(link.entity_ref.as_str()).map(|marker| marker.kind()), Some(SketchInputKind::Relation(_)))
    }) { return Ok(None); }
    let mut collection = AxisRelationPointCollection { visited: HashSet::new(), visited_bytes: 0, loci: Vec::new(), locus_bytes: 0 };
    collect_axis_relation_point_loci(ctx, relation, sketch, sketch_entities, markers_by_id, loci_by_marker, &mut collection, false)?;
    sort_axis_relation_point_loci(ctx, &mut collection.loci)?;
    if collection.loci.len() == 2 { return Ok(collection.loci.try_into().ok()); }
    if collection.loci.len() > 2 { return Ok(None); }
    collection.loci.clear();
    collection.locus_bytes = 0;
    collection.visited.clear();
    collection.visited_bytes = 0;
    collect_axis_relation_point_loci(ctx, relation, sketch, sketch_entities, markers_by_id, loci_by_marker, &mut collection, true)?;
    sort_axis_relation_point_loci(ctx, &mut collection.loci)?;
    Ok(collection.loci.try_into().ok())
}

fn sort_axis_relation_point_loci(ctx: &DecodeContext<'_>, loci: &mut Vec<SketchLocus>) -> Result<(), CodecError> {
    const OPERATION: &str = "sort SLDPRT axis relation point loci";
    let count = u64_from_index(loci.len());
    ctx.charge_work(count, OPERATION)?;
    let max_bytes = loci.iter().map(|locus| locus_entity(locus).as_str().len()).max().unwrap_or(0);
    let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
    ctx.charge_work(count.checked_mul(levels).and_then(|work| work.checked_mul(64))
        .and_then(|work| u64_from_index(max_bytes).checked_mul(2).and_then(|bytes| bytes.checked_add(1)).and_then(|bytes| work.checked_mul(bytes)))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    loci.sort_unstable_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    loci.dedup();
    Ok(())
}

// The collector keeps the sketch, marker indexes, and locus indexes separate
// because each lookup has a distinct ownership boundary.
#[allow(clippy::too_many_arguments)]
fn collect_axis_relation_point_loci<'a>(
    ctx: &DecodeContext<'_>, relation: &'a SketchInputEntity, sketch: &SketchId,
    sketch_entities: &[SketchEntity], markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>, collection: &mut AxisRelationPointCollection<'a>,
    include_reverse_owners: bool,
) -> Result<(), CodecError> {
    const OPERATION: &str = "traverse SLDPRT axis relation point loci";
    let _nesting = ctx.enter_nested(OPERATION)?;
    let bytes = u64_from_index(relation.id().len());
    ctx.charge_work(collection.visited_bytes.checked_add(bytes).and_then(|bytes| bytes.checked_mul(4))
        .and_then(|work| work.checked_add(u64_from_index(collection.visited.len()).checked_add(1)?
            .checked_mul(u64_from_index(std::mem::size_of::<&str>()))?))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    if collection.visited.contains(relation.id()) { return Ok(()); }
    let next_bytes = collection.visited_bytes.checked_add(bytes)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_collection_items(1, OPERATION)?;
    collection.visited.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    collection.visited.insert(relation.id());
    collection.visited_bytes = next_bytes;
    charge_relation_marker_links(ctx, relation, markers_by_id, OPERATION)?;
    for link in relation.links().iter().filter(|link| !relation_link_identifies_owner(relation, link)) {
        let Some(linked) = markers_by_id.get(link.entity_ref.as_str()) else { continue; };
        match linked.kind() {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint => append_axis_relation_point_locus(ctx, linked.id(), sketch, sketch_entities, markers_by_id, loci_by_marker, collection)?,
            SketchInputKind::Relation(_) => collect_axis_relation_point_loci(ctx, linked, sketch, sketch_entities, markers_by_id, loci_by_marker, collection, include_reverse_owners)?,
            _ => {}
        }
    }
    if include_reverse_owners {
        for owner in relation_owner_markers(ctx, relation, markers_by_id)? {
            if matches!(owner.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint) {
                append_axis_relation_point_locus(ctx, owner.id(), sketch, sketch_entities, markers_by_id, loci_by_marker, collection)?;
            }
        }
    }
    Ok(())
}

fn append_axis_relation_point_locus(
    ctx: &DecodeContext<'_>, marker_id: &str, sketch: &SketchId,
    sketch_entities: &[SketchEntity], markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>, collection: &mut AxisRelationPointCollection<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "resolve SLDPRT axis relation point identity";
    if let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? { return collection.append(ctx, locus); }
    let mut selected = None;
    for entity in sketch_entities {
        charge_typed_endpoint_work(ctx, entity.sketch.as_str().len(), 4, OPERATION)?;
        charge_typed_endpoint_work(ctx, sketch.as_str().len(), 4, OPERATION)?;
        charge_typed_endpoint_work(ctx, entity.native_ref.as_deref().map_or(0, str::len), 4, OPERATION)?;
        charge_typed_endpoint_work(ctx, marker_id.len(), 4, OPERATION)?;
        if entity.sketch != *sketch || entity.native_ref.as_deref() != Some(marker_id)
            || !matches!(entity.geometry.definition(), SketchGeometryDefinition::Point { .. }) { continue; }
        if selected.is_some() { return Ok(()); }
        selected = Some(entity.id());
    }
    if let Some(identity) = selected {
        let locus = super::transforms::SketchLocusRole::Entity.copy_locus(ctx, identity, OPERATION)?;
        collection.append(ctx, locus)?;
    }
    Ok(())
}

fn charge_relation_marker_links(
    ctx: &DecodeContext<'_>, relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>, operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(markers_by_id.len()), operation)?;
    let key_bytes = markers_by_id.keys().try_fold(0u64, |bytes, key| bytes.checked_add(u64_from_index(key.len())))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    for link in relation.links() {
        ctx.charge_work(key_bytes.checked_add(u64_from_index(link.entity_ref.len()))
            .and_then(|bytes| bytes.checked_add(u64_from_index(relation.id().len())))
            .and_then(|bytes| bytes.checked_mul(4)).and_then(|work| work.checked_add(64))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?, operation)?;
    }
    Ok(())
}

pub(super) fn relation_owner_markers<'a>(
    ctx: &DecodeContext<'_>, relation: &SketchInputEntity,
    markers_by_id: &'a HashMap<&str, &SketchInputEntity>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "collect SLDPRT reverse relation owners";
    let mut owners = Vec::new();
    for marker in markers_by_id.values().copied() {
        charge_typed_endpoint_work(ctx, marker.feature_ref.as_deref().map_or(0, str::len), 4, OPERATION)?;
        charge_typed_endpoint_work(ctx, relation.feature_ref.as_deref().map_or(0, str::len), 4, OPERATION)?;
        if marker.feature_ref != relation.feature_ref || !matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::LineOrCircle | SketchInputKind::Arc | SketchInputKind::ConstrainedPoint) { continue; }
        for link in marker.links() {
            charge_typed_endpoint_work(ctx, link.entity_ref.len(), 4, OPERATION)?;
            charge_typed_endpoint_work(ctx, relation.id().len(), 4, OPERATION)?;
            if link.entity_ref != relation.id() { continue; }
            ctx.reserve_collection_vec(&mut owners, 1, OPERATION)?;
            owners.push(marker);
            break;
        }
    }
    let count = u64_from_index(owners.len());
    let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
    ctx.charge_work(count.checked_mul(levels).and_then(|work| work.checked_mul(64))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)?;
    owners.sort_unstable_by_key(|marker| marker.offset());
    Ok(owners)
}


pub(crate) fn marker_owns_constraint(
    ctx: &DecodeContext<'_>, marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Result<bool, CodecError> {
    charge_relation_marker_links(ctx, marker, markers_by_id, "resolve SLDPRT marker constraint ownership")?;
    let mut axis_point_links = marker
        .links()
        .iter()
        .filter(|link| link.entity_ref != marker.id())
        .filter(|link| {
            !matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|linked| linked.kind()),
                Some(SketchInputKind::Relation(_))
            )
        });
    let axis_point_pair = matches!(
        marker.kind(),
        SketchInputKind::Relation(
            crate::records::SketchRelationKind::Horizontal
                | crate::records::SketchRelationKind::Vertical
        )
    ) && relation_owner_markers(ctx, marker, markers_by_id)?.is_empty()
        && axis_point_links.clone().count() == 2
        && axis_point_links.all(|link| {
            matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|linked| linked.kind()),
                Some(SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
            )
        });
    Ok(marker.kind().owns_constraint()
        && (axis_point_pair
            || marker
                .links()
                .iter()
                .any(|link| !relation_link_identifies_owner(marker, link))
            || !relation_owner_markers(ctx, marker, markers_by_id)?.is_empty()))
}

pub(super) fn relation_link_identifies_owner(
    relation: &SketchInputEntity,
    link: &crate::records::SketchInputLink,
) -> bool {
    link.entity_ref == relation.id() || relation.local_id() == Some(u32::from(link.local_id))
}

pub(super) fn relation_link_is_geometric_operand(
    relation: &SketchInputEntity,
    link: &crate::records::SketchInputLink,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> bool {
    // Relation markers are solver handles; geometric operands come from direct
    // links or reverse incidence, never from a relation-to-relation chain.
    !relation_link_identifies_owner(relation, link)
        && !matches!(
            markers_by_id
                .get(link.entity_ref.as_str())
                .map(|marker| marker.kind()),
            Some(SketchInputKind::Relation(_))
        )
}

fn typed_axis_relation_is_inactive(
    ctx: &DecodeContext<'_>, definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<Option<bool>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT axis relation activity";
    ctx.charge_work(64, OPERATION)?;
    Ok(match definition {
        SketchConstraintDefinitionInput::Horizontal { entity: id }
        | SketchConstraintDefinitionInput::Vertical { entity: id } => {
            let Some(entity) = find_profile_entity(ctx, sketch_entities, id, OPERATION)? else { return Ok(None); };
            let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else { return Ok(Some(true)); };
            Some(if matches!(definition, SketchConstraintDefinitionInput::Horizontal { .. }) {
                !same_dimension_length(start.v, end.v)
            } else { !same_dimension_length(start.u, end.u) })
        }
        SketchConstraintDefinitionInput::SameCoordinate { relation } => {
            let Some(first) = profile_locus_point_charged(ctx, relation.first(), sketch_entities, OPERATION)? else { return Ok(None); };
            let Some(second) = profile_locus_point_charged(ctx, relation.second(), sketch_entities, OPERATION)? else { return Ok(None); };
            Some(match relation.axis() {
                SketchCoordinateAxis::V => !same_dimension_length(first.v, second.v),
                SketchCoordinateAxis::U => !same_dimension_length(first.u, second.u),
            })
        }
        _ => None,
    })
}

fn typed_binary_relation_is_inactive(
    ctx: &DecodeContext<'_>,
    kind: crate::records::SketchRelationKind,
    definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<Option<bool>, CodecError> {
    use crate::records::SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let ((Parallel, SketchConstraintDefinitionInput::Parallel { first, second })
    | (Perpendicular, SketchConstraintDefinitionInput::Perpendicular { first, second })
    | (Tangent, SketchConstraintDefinitionInput::Tangent { first, second })
    | (Equal, SketchConstraintDefinitionInput::Equal { first, second })
    | (Collinear, SketchConstraintDefinitionInput::Collinear { first, second })
    | (Concentric, SketchConstraintDefinitionInput::Concentric { first, second })
    | (Coradial, SketchConstraintDefinitionInput::Coradial { first, second })) = (kind, definition)
    else {
        return Ok(None);
    };
    const OPERATION: &str = "resolve SLDPRT binary relation activity";
    let Some(first) = find_profile_entity(ctx, sketch_entities, first, OPERATION)? else { return Ok(None); };
    let Some(second) = find_profile_entity(ctx, sketch_entities, second, OPERATION)? else { return Ok(None); };
    ctx.charge_work(256, OPERATION)?;
    Ok(Some(!binary_relation_matches_evaluated_geometry(
        kind, first, second,
    )))
}

pub(super) fn marker_relation_is_inactive(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<bool, CodecError> {
    use crate::records::SketchRelationKind::{
        ArcAngle180, ArcAngle270, ArcAngle90, Collinear, Concentric, Coradial, EllipseAngle180,
        EllipseAngle270, EllipseAngle90, Equal, Horizontal, MergePoints, Parallel, Perpendicular,
        Tangent, Vertical,
    };

    let SketchInputKind::Relation(kind) = marker.kind() else {
        return Ok(false);
    };
    if let Some(inactive) = typed_axis_relation_is_inactive(ctx, definition, sketch_entities)? {
        return Ok(inactive);
    }
    if let Some(inactive) = typed_binary_relation_is_inactive(ctx, kind, definition, sketch_entities)? {
        return Ok(inactive);
    }
    if let SketchConstraintDefinitionInput::CoincidentLoci { loci } = definition {
        let mut first = None;
        let mut inactive = false;
        for locus in loci {
            let Some(point) = profile_locus_point_charged(ctx, locus, sketch_entities, "resolve SLDPRT coincidence activity")? else { return Ok(false); };
            if let Some(first) = first {
                let first: Point2 = first;
                inactive |= !same_dimension_length(point.u, first.u) || !same_dimension_length(point.v, first.v);
            } else { first = Some(point); }
        }
        return Ok(inactive);
    }
    let SketchConstraintDefinitionInput::Native {
        entities, operands, ..
    } = definition
    else {
        return Ok(false);
    };
    let mut repeated_single_operand = operands.len() >= 2 && operands[0].native_ref.is_some();
    if repeated_single_operand {
        for operand in operands {
            let first_bytes = operands[0].native_ref.as_deref().map_or(0, str::len);
            let bytes = operand.native_ref.as_deref().map_or(0, str::len);
            let work = u64_from_index(first_bytes).checked_add(u64_from_index(bytes))
                .and_then(|bytes| bytes.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit("compare SLDPRT repeated relation operands", u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, "compare SLDPRT repeated relation operands")?;
            if operand.native_ref != operands[0].native_ref { repeated_single_operand = false; break; }
        }
    }
    if repeated_single_operand
        && matches!(
            kind,
            Horizontal
                | Vertical
                | Parallel
                | Perpendicular
                | Tangent
                | Equal
                | Collinear
                | Concentric
                | Coradial
        )
    {
        return Ok(true);
    }
    if entities.is_empty() || sketch_entities.is_empty() {
        return Ok(false);
    }
    let mut resolved = Vec::new();
    for id in entities {
        if let Some(entity) = find_profile_entity(ctx, sketch_entities, id, "resolve SLDPRT native relation activity")? {
            ctx.reserve_collection_vec(&mut resolved, 1, "collect SLDPRT native relation activity")?;
            resolved.push(entity.geometry.definition());
        }
    }
    if resolved.len() != entities.len() { return Ok(false); }
    ctx.charge_work(u64_from_index(resolved.len()).checked_add(64)
        .ok_or_else(|| ctx.refuse_codec_limit("compare SLDPRT native relation activity", u64::MAX - 1, u64::MAX))?, "compare SLDPRT native relation activity")?;
    Ok(match kind {
        ArcAngle90 | ArcAngle180 | ArcAngle270 => {
            !matches!(resolved.as_slice(), [SketchGeometryDefinition::Arc { .. }])
        }
        EllipseAngle90 | EllipseAngle180 | EllipseAngle270 => !matches!(
            resolved.as_slice(),
            [SketchGeometryDefinition::Ellipse { .. }]
        ),
        Horizontal | Vertical => !matches!(
            resolved.as_slice(),
            [SketchGeometryDefinition::Line { .. }]
                | [
                    SketchGeometryDefinition::Point { .. },
                    SketchGeometryDefinition::Point { .. }
                ]
        ),
        Parallel | Perpendicular | Tangent | Equal | Collinear | Concentric | Coradial => {
            resolved.len() != 2
                || resolved.iter().any(|entity| {
                    matches!(
                        **entity,
                        SketchGeometryDefinition::Point { .. }
                            | SketchGeometryDefinition::Native { .. }
                    )
                })
        }
        crate::records::SketchRelationKind::Coincident | MergePoints => {
            let [SketchGeometryDefinition::Point { position: first }, SketchGeometryDefinition::Point { position: second }] =
                resolved.as_slice()
            else {
                return Ok(false);
            };
            !same_dimension_length(first.u, second.u) || !same_dimension_length(first.v, second.v)
        }
        _ => false,
    })
}

fn relation_owner_curve_entities(
    ctx: &DecodeContext<'_>, relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>, loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Vec<SketchEntityId>, CodecError> {
    const OPERATION: &str = "collect SLDPRT reverse curve owner entities";
    let mut entities = Vec::new();
    for owner in relation_owner_markers(ctx, relation, markers_by_id)? {
        ctx.charge_work(8, OPERATION)?;
        if !matches!(owner.kind(), SketchInputKind::LineOrCircle | SketchInputKind::Arc) { continue; }
        for entity in marker_entities(ctx, owner.id(), markers_by_id, loci_by_marker, MarkerEntityFilter::All)? {
            ctx.charge_work(u64_from_index(std::mem::size_of::<SketchEntityId>()), OPERATION)?;
            ctx.reserve_collection_vec(&mut entities, 1, OPERATION)?;
            entities.push(entity);
        }
    }
    sort_marker_entity_ids(ctx, &mut entities, OPERATION)?;
    Ok(entities)
}


fn charge_typed_endpoint_work(ctx: &DecodeContext<'_>, len: usize, factor: u64, operation: &'static str) -> Result<(), CodecError> {
    let work = u64_from_index(len).checked_add(1).and_then(|work| work.checked_mul(factor))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)
}

fn charge_typed_endpoint_roster(
    ctx: &DecodeContext<'_>, owner: &SketchInputEntity, markers: &[&SketchInputEntity], operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(512, operation)?;
    for marker in markers {
        charge_typed_endpoint_work(ctx, marker.feature_ref.as_deref().map_or(0, str::len), 8, operation)?;
        charge_typed_endpoint_work(ctx, owner.feature_ref.as_deref().map_or(0, str::len), 8, operation)?;
        charge_typed_endpoint_work(ctx, marker.id().len(), 8, operation)?;
        charge_typed_endpoint_work(ctx, owner.id().len(), 8, operation)?;
    }
    Ok(())
}

pub(super) fn line_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>, line: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked line endpoints";
    for link in line.links() {
        charge_typed_endpoint_work(ctx, link.entity_ref.len(), 8, OPERATION)?;
        if let Some(marker) = markers_by_id.get(link.entity_ref.as_str()) {
            charge_typed_endpoint_work(ctx, marker.feature_ref.as_deref().map_or(0, str::len), 8, OPERATION)?;
            charge_typed_endpoint_work(ctx, line.feature_ref.as_deref().map_or(0, str::len), 8, OPERATION)?;
        }
    }
    for marker in markers_by_id.values() {
        charge_typed_endpoint_work(ctx, marker.feature_ref.as_deref().map_or(0, str::len), 8, OPERATION)?;
        charge_typed_endpoint_work(ctx, line.feature_ref.as_deref().map_or(0, str::len), 8, OPERATION)?;
        charge_typed_endpoint_work(ctx, marker.id().len(), 8, OPERATION)?;
        for link in marker.links() {
            charge_typed_endpoint_work(ctx, link.entity_ref.len(), 4, OPERATION)?;
            charge_typed_endpoint_work(ctx, line.id().len(), 4, OPERATION)?;
        }
    }
    let candidates = line
        .links()
        .iter()
        .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
        .chain(markers_by_id.values().copied().filter(|candidate| {
            candidate
                .links()
                .iter()
                .any(|link| link.entity_ref == line.id())
        }))
        .filter(|endpoint| {
            endpoint.feature_ref == line.feature_ref
                && endpoint.coordinates_m.is_some()
                && matches!(
                    endpoint.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
    let mut endpoints = Vec::new();
    for endpoint in candidates {
        if endpoints.len() == endpoints.capacity() { charge_typed_endpoint_work(ctx, endpoints.len(), 4, OPERATION)?; }
        ctx.reserve_collection_vec(&mut endpoints, 1, OPERATION)?;
        endpoints.push(endpoint);
    }
    sort_endpoint_markers(ctx, &mut endpoints, OPERATION)?;
    for endpoint in &endpoints { charge_typed_endpoint_work(ctx, endpoint.id().len(), 4, OPERATION)?; }
    endpoints.dedup_by_key(|endpoint| endpoint.id());
    Ok(endpoints)
}

pub(super) fn marker_curve_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>, payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    markers: &[&'a SketchInputEntity],
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker curve endpoints";
    ctx.charge_work(1024, OPERATION)?;
    if let Some(endpoints) = extended_direct_object_line_endpoints(payload, curve, markers) {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    let endpoints = line_endpoint_markers(ctx, curve, markers_by_id)?;
    if endpoints.len() == 2 {
        return Ok(endpoints);
    }
    let shifted = usize::try_from(curve.offset()).ok().and_then(|offset| {
        extended_shifted_construction_line_endpoint_indices(payload, offset)
            .map(|indices| (offset, indices))
    });
    if let Some((offset, indices)) = shifted {
        let endpoints = if payload.get(offset + 72..offset + 76) == Some(&[0; 4]) {
            let mut owned = collect_endpoint_markers(ctx, markers.iter().copied(), curve, |_| true, OPERATION)?;
            sort_endpoint_markers(ctx, &mut owned, OPERATION)?;
            let selected = indices
                .into_iter()
                .filter_map(|index| {
                    let index = usize::try_from(index).ok()?.checked_sub(1)?;
                    owned.get(index).copied().filter(|marker| {
                        marker.coordinates_m.is_some()
                            && matches!(
                                marker.kind(),
                                SketchInputKind::Point
                                    | SketchInputKind::ConstrainedPoint
                                    | SketchInputKind::LineOrCircle
                                    | SketchInputKind::Arc
                            )
                    })
                });
            let mut endpoints = Vec::new();
            for endpoint in selected {
                ctx.reserve_collection_vec(&mut endpoints, 1, OPERATION)?;
                endpoints.push(endpoint);
            }
            endpoints
        } else {
            super::endpoints::coordinate_roster_curve_endpoint_markers_at(ctx,
                payload,
                curve,
                markers,
                Some(56),
            )?
        };
        if endpoints.len() == 2 {
            return Ok(endpoints);
        }
    }
    if let Some(endpoints) = compact_legacy_object_line_endpoints(payload, curve, markers) {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = extended_wide_selected_axis_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = inline_arc_endpoint_markers(payload, curve, markers) {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) =
        compact_legacy_142_profile_curve_endpoint_markers(payload, curve, markers)
    {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = one_based_point_roster_line_endpoint_markers(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = legacy_point_roster_line_endpoint_markers(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    let endpoints = roster_curve_endpoint_markers(ctx, payload, curve, markers)?;
    if endpoints.len() == 2 {
        if let Some(direct) = legacy_marker104_arc_endpoints(ctx, payload, curve, markers)? {
            let roster = [endpoints[0], endpoints[1]];
            if legacy_marker104_arc_center(ctx, payload, curve, markers, roster)?.is_none()
                && legacy_marker104_arc_center(ctx, payload, curve, markers, direct)?.is_some()
            {
                return copy_endpoint_markers(ctx, &direct);
            }
        }
        return Ok(endpoints);
    }
    if let Some(endpoints) = legacy_marker104_arc_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = current_coordinate_linked_line_endpoints(payload, curve, markers) {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = coordinate_centered_line_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = legacy_terminal_profile_indexed_endpoints(payload, curve, markers) {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    let endpoints = consecutive_legacy_profile_line_endpoints(payload, curve, markers);
    if endpoints.len() == 2 {
        return Ok(endpoints);
    }
    if let Some(pair) = coordinate_profile_line_endpoints(payload, curve, markers_by_id) {
        copy_endpoint_markers(ctx, &pair)
    } else { Ok(endpoints) }
}

fn coordinate_profile_line_endpoints<'a>(
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset()).ok()?;
    if curve.kind() != SketchInputKind::LineOrCircle
        || curve.coordinates_m.is_none()
        || !matches!(
            payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()),
            Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER
        )
        || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 64..offset + 66) != Some(&[0x1e, 0x00])
    {
        return None;
    }
    let mut point = None;
    for link in curve
        .links()
        .iter()
        .filter(|link| link.entity_ref != curve.id())
    {
        let linked = markers_by_id.get(link.entity_ref.as_str()).copied()?;
        if linked.feature_ref != curve.feature_ref {
            return None;
        }
        match linked.kind() {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                if linked.coordinates_m.is_none() || point.replace(linked).is_some() {
                    return None;
                }
            }
            SketchInputKind::Relation(_) => {}
            SketchInputKind::LineOrCircle
            | SketchInputKind::Arc
            | SketchInputKind::Native(_)
            | SketchInputKind::NativeHandle(_) => return None,
        }
    }
    let point = point?;
    (curve.coordinates_m?.get() != point.coordinates_m?.get()).then_some([curve, point])
}

pub(super) fn extended_direct_object_line_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    if curve.kind() != SketchInputKind::LineOrCircle {
        return None;
    }
    let offset = usize::try_from(curve.offset()).ok()?;
    let endpoint_ids = extended_direct_object_line_endpoint_ids(payload, offset)?;
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && if id == 0 {
                    marker.object_index().is_none()
                } else {
                    marker.object_index() == Some(id)
                }
        });
        let marker = candidates.next()?;
        candidates.next().is_none().then_some(marker)
    };
    let endpoints = [resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?];
    (endpoints[0].id() != endpoints[1].id()
        && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
}

pub(super) fn compact_legacy_object_line_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset()).ok()?;
    let endpoint_ids = compact_legacy_code_one_line_endpoint_indices(payload, offset)?;
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index() == Some(id)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let marker = candidates.next()?;
        candidates.next().is_none().then_some(marker)
    };
    let endpoints = [resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?];
    (endpoints[0].id() != endpoints[1].id()
        && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
}

fn extended_wide_selected_axis_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT extended wide selected axis endpoints";
    charge_typed_endpoint_roster(ctx, curve, markers, OPERATION)?;
    let encoded = (|| {
    let offset = usize::try_from(curve.offset()).ok()?;
    if curve.kind() != SketchInputKind::LineOrCircle
        || payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
            != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 31)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 39..offset + 48) != Some(&[0; 9])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&[0; 4])
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&[0x00, 0x00, 0x02, 0x00])
        || payload.get(offset + 84..offset + 88) != Some(&[0; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    let endpoint = |relative| Some(u32::from(View::u16_le_at(payload, offset + relative)?));
    let encoded = [endpoint(64)?, endpoint(66)?];
    if encoded[0] == encoded[1] || encoded.contains(&u32::from(u16::MAX)) {
        return None;
    }
        Some(encoded)
    })();
    let Some(encoded) = encoded else { return Ok(None); };
    let resolve_object = |index| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index() == Some(index)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let object_endpoints = encoded.map(|index| resolve_object(index + 1));
    if let [Some(first), Some(second)] = object_endpoints {
        if first.id() != second.id() && first.coordinates_m != second.coordinates_m {
            return Ok(Some([first, second]));
        }
    }
    let indices = encoded.map(|index| usize::try_from(index).ok()?.checked_sub(1));
    let [Some(first_index), Some(second_index)] = indices else {
        return Ok(None);
    };
    let mut points = collect_endpoint_markers(ctx, markers.iter().copied(), curve,
        |marker| marker.coordinates_m.is_some() && matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint), OPERATION)?;
    sort_endpoint_markers(ctx, &mut points, OPERATION)?;
    Ok((|| {
    let endpoints = [*points.get(first_index)?, *points.get(second_index)?];
    (endpoints[0].id() != endpoints[1].id()
        && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
    })())
}

pub(super) fn legacy_marker104_arc_endpoints<'a>(
    ctx: &DecodeContext<'_>, payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT legacy marker104 arc endpoints";
    ctx.charge_work(512, OPERATION)?;
    let endpoint_ids = (|| {
    let offset = usize::try_from(curve.offset()).ok()?;
    if curve.kind() != SketchInputKind::Arc
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x05, 0x00, 0x01, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || compact_indexed_curve_record_end(payload, offset)
            != Some(CompactIndexedCurveRecordEnd::Marker104)
    {
        return None;
    }
    let endpoint_id = |relative| {
        let id = View::u16_le_at(payload, offset + relative)?;
        (!matches!(id, 0 | u16::MAX)).then_some(u32::from(id))
    };
    let endpoint_ids = [endpoint_id(56)?, endpoint_id(58)?];
    if endpoint_ids[0] == endpoint_ids[1] {
        return None;
    }
    Some(endpoint_ids)
    })();
    let Some(endpoint_ids) = endpoint_ids else { return Ok(None); };
    for marker in markers {
        charge_typed_endpoint_work(ctx, marker.feature_ref.as_deref().map_or(0, str::len), 4, OPERATION)?;
        charge_typed_endpoint_work(ctx, curve.feature_ref.as_deref().map_or(0, str::len), 4, OPERATION)?;
        ctx.charge_work(128, OPERATION)?;
    }
    Ok((|| {
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index() == Some(id)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = [resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?];
    (endpoints[0].coordinates_m != endpoints[1].coordinates_m).then_some(endpoints)
    })())
}

fn one_based_point_roster_line_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT one based point roster line endpoint markers";
    charge_typed_endpoint_roster(ctx, curve, markers, OPERATION)?;
    let indices = (|| {
    let offset = usize::try_from(curve.offset()).ok()?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 40..offset + 48) != Some(&[0; 8])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let endpoint_index =
        |relative| usize::from(View::u16_le_at(payload, offset + relative)?).checked_sub(1);
    let indices = [endpoint_index(56)?, endpoint_index(58)?];
    if indices[0] == indices[1] {
        return None;
    }
    if markers.iter().any(|marker| {
        marker.feature_ref == curve.feature_ref && marker.kind() == SketchInputKind::Arc
    }) {
        return None;
    }
        Some(indices)
    })();
    let Some(indices) = indices else { return Ok(None); };
    let mut points = collect_endpoint_markers(ctx, markers.iter().copied(), curve,
        |marker| marker.coordinates_m.is_some() && matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint), OPERATION)?;
    sort_endpoint_markers(ctx, &mut points, OPERATION)?;
    Ok((|| {
    let endpoints = [*points.get(indices[0])?, *points.get(indices[1])?];
    (endpoints[0].id() != endpoints[1].id()).then_some(endpoints)
    })())
}

fn legacy_point_roster_line_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT legacy point roster line endpoint markers";
    charge_typed_endpoint_roster(ctx, curve, markers, OPERATION)?;
    let indices = (|| {
    let offset = usize::try_from(curve.offset()).ok()?;
    if curve.kind() != SketchInputKind::LineOrCircle
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 39..offset + 48) != Some(&[0; 9])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&[0; 4])
        || payload
            .get(offset + 76..offset + 80)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || payload
            .get(offset + 80..offset + 84)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let index = |relative| Some(usize::from(View::u16_le_at(payload, offset + relative)?));
    let indices = [index(56)?, index(58)?];
    if indices[0] == indices[1] || indices.contains(&usize::from(u16::MAX)) {
        return None;
    }
        Some(indices)
    })();
    let Some(indices) = indices else { return Ok(None); };
    let mut points = collect_endpoint_markers(ctx, markers.iter().copied(), curve,
        |marker| marker.coordinates_m.is_some() && matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint), OPERATION)?;
    sort_endpoint_markers(ctx, &mut points, OPERATION)?;
    Ok((|| {
    let endpoints = [*points.get(indices[0])?, *points.get(indices[1])?];
    (endpoints[0].id() != endpoints[1].id()
        && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
    })())
}

pub(super) fn legacy_terminal_profile_indexed_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset()).ok()?;
    let endpoint_offset = legacy_terminal_profile_endpoint_offset(payload, offset)?;
    let endpoint = |relative| Some(u32::from(View::u16_le_at(payload, offset + relative)?));
    let endpoint_ids = [endpoint(endpoint_offset)?, endpoint(endpoint_offset + 2)?];
    if endpoint_ids[0].checked_add(1) != Some(endpoint_ids[1]) {
        return None;
    }
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some()
                && (marker.local_id() == Some(id)
                    || marker.object_index().and_then(|index| index.checked_add(1)) == Some(id))
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let resolved = endpoint_ids.map(resolve);
    let [Some(first), Some(second)] = resolved else {
        return None;
    };
    (first.id() != second.id()).then_some([first, second])
}

fn inline_arc_endpoint_markers<'a>(
    payload: &[u8],
    arc: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(arc.offset()).ok()?;
    let [_, start, end] = inline_arc_coordinates(payload, offset)?;
    let endpoint = |coordinates: cadmpeg_ir::units::FiniteVector<2>| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == arc.feature_ref
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some_and(|point| {
                    same_dimension_length(point[0], coordinates[0])
                        && same_dimension_length(point[1], coordinates[1])
                })
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = [endpoint(start)?, endpoint(end)?];
    (endpoints[0].id() != endpoints[1].id()).then_some(endpoints)
}

fn compact_legacy_142_profile_curve_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    if curve.kind() != SketchInputKind::LineOrCircle || curve.coordinates_m.is_some() {
        return None;
    }
    let offset = usize::try_from(curve.offset()).ok()?;
    let [start, end] = compact_legacy_142_profile_curve_endpoints(payload, offset)?;
    let resolve = |coordinates: cadmpeg_ir::units::FiniteVector<2>| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some_and(|point| {
                    same_dimension_length(point[0], coordinates[0])
                        && same_dimension_length(point[1], coordinates[1])
                })
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = [resolve(start)?, resolve(end)?];
    (endpoints[0].id() != endpoints[1].id()).then_some(endpoints)
}

pub(super) fn current_undetailed_bounded_curve_is_line(payload: &[u8], offset: usize) -> bool {
    let supported_prefix = matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(marker) if marker == SKETCH_MARKER || marker == LEGACY_EXTENDED_SKETCH_MARKER
    );
    let profile_locus = payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]);
    let extended_geometry_locus = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_is_geometry_locus(payload, offset);
    let distinct = |endpoints: [u32; 2]| endpoints[0] != endpoints[1];
    let compact_indexed_record = compact_indexed_curve_endpoint_indices(payload, offset)
        .is_some_and(distinct)
        || extended_compact_indexed_curve_endpoint_indices(payload, offset).is_some_and(distinct);
    let complete_indexed_record = wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && wide_indexed_curve_record_is_complete(payload, offset)
        || compact_indexed_record
            && matches!(
                compact_indexed_curve_record_end(payload, offset),
                Some(
                    CompactIndexedCurveRecordEnd::Marker84
                        | CompactIndexedCurveRecordEnd::Marker96
                        | CompactIndexedCurveRecordEnd::Marker104
                )
            );
    supported_prefix
        && (profile_locus || extended_geometry_locus)
        && complete_indexed_record
        && compact_bounded_curve_tangent(payload, offset).is_none()
}

fn current_coordinate_linked_line_endpoints<'a>(
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(line.offset()).ok()?;
    let cell = payload.get(offset + 86..offset + 98)?;
    let kind = operand_kind(cell[..2].try_into().ok()?)?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 64..offset + 66) != Some(&[0x1e, 0x00])
        || payload.get(offset + 82..offset + 86) != Some(&[0x00, 0x00, 0x01, 0x00])
        || !operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
        || !operand_accepts_marker(kind, SketchInputKind::Arc)
        || cell[4..8] != [0xff; 4]
        || cell[8..12] != [0; 4]
        || payload.get(offset + 98..offset + 102) != Some(&[0; 4])
        || payload.get(offset + 102..offset + 106) != Some(&(-2i32).to_le_bytes())
        || payload.get(offset + 106..offset + 148) != Some(&[0; 42])
        || !sketch_marker_prefix_at(payload, offset.checked_add(152)?)
    {
        return None;
    }
    let local_id = u32::from(View::u16_le_at(cell, 2)?);
    // A local-link endpoint can select a coordinate-bearing curve marker before
    // the binding pass promotes it to a profile vertex. Keep that candidate in
    // the graph; the binding pass retains it as a curve only when it resolves
    // its own two endpoints.
    let mut endpoints = markers.iter().copied().filter(|marker| {
        marker.feature_ref == line.feature_ref
            && marker.id() != line.id()
            && marker.local_id() == Some(local_id)
            && marker.coordinates_m.is_some()
            && matches!(
                marker.kind(),
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
            )
    });
    let endpoint = endpoints.next()?;
    endpoints.next().is_none().then_some([line, endpoint])
}

fn coordinate_centered_line_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    line: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT coordinate centered line endpoints";
    charge_typed_endpoint_roster(ctx, line, markers, OPERATION)?;
    let center = (|| {
    let offset = usize::try_from(line.offset()).ok()?;
    let [center_u, center_v] = coordinate_centered_line_center(payload, offset)?.get();
        Some([center_u, center_v])
    })();
    let Some([center_u, center_v]) = center else { return Ok(None); };
    let mut coordinates = collect_endpoint_markers(ctx, markers.iter().copied(), line,
        |marker| marker.offset() > line.offset() && marker.coordinates_m.is_some(), OPERATION)?;
    sort_endpoint_markers(ctx, &mut coordinates, OPERATION)?;
    Ok((|| {
    let [first, second, ..] = coordinates.as_slice() else {
        return None;
    };
    let [first_u, first_v] = first.coordinates_m?.get();
    let [second_u, second_v] = second.coordinates_m?.get();
    let centered = same_dimension_length((first_u + second_u) * 0.5, center_u)
        && same_dimension_length((first_v + second_v) * 0.5, center_v);
    (centered && (first_u != second_u || first_v != second_v)).then_some([*first, *second])
    })())
}

fn coordinate_centered_line_center(
    payload: &[u8],
    offset: usize,
) -> Option<cadmpeg_ir::units::FiniteVector<2>> {
    if payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return None;
    }
    if payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_is_geometry_locus(payload, offset)
        && payload.get(offset + 64..offset + 66) == Some(&[0x1e, 0x00])
        && payload.get(offset + 82..offset + 86) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 86..offset + 92) == Some(&[0; 6])
        && payload.get(offset + 92..offset + 96) == Some(&(-2i32).to_le_bytes())
        && payload.get(offset + 96..offset + 138) == Some(&[0; 42])
        && sketch_marker_prefix_at(payload, offset.checked_add(142)?)
    {
        return finite_coordinate_pair(payload, offset + 66);
    }
    let direct = View::u16_le_at(payload, offset + 74)?;
    let count = View::u16_le_at(payload, offset + 76)?;
    let tagged = View::u16_le_at(payload, offset + 82)?;
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 56..offset + 58) == Some(&[0x1e, 0x00])
        && ((direct == 1 && count == 0 && tagged == 0)
            || (direct == 0 && (1..=3).contains(&count) && tagged <= 1))
        && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && payload.get(offset + 84..offset + 88) == Some(&(-2i32).to_le_bytes())
        && payload.get(offset + 88..offset + 130) == Some(&[0; 42])
        && sketch_marker_prefix_at(payload, offset.checked_add(134)?)
    {
        return finite_coordinate_pair(payload, offset + 58);
    }
    None
}

fn consecutive_legacy_profile_line_endpoints<'a>(
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    let Some(offset) = usize::try_from(line.offset()).ok() else {
        return Vec::new();
    };
    if line.kind() != SketchInputKind::LineOrCircle
        || line.coordinates_m.is_none()
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
    {
        return Vec::new();
    }
    let Some(next) = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref == line.feature_ref && marker.offset() > line.offset())
        .min_by_key(|marker| marker.offset())
    else {
        return Vec::new();
    };
    if next.coordinates_m.is_none()
        || !matches!(
            next.kind(),
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
        )
        || usize::try_from(next.offset())
            .ok()
            .is_none_or(|next_offset| !sketch_marker_prefix_at(payload, next_offset))
    {
        return Vec::new();
    }
    vec![line, next]
}

pub(super) fn legacy_terminal_indexed_profile_line(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> bool {
    let Some(offset) = usize::try_from(curve.offset()).ok() else {
        return false;
    };
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(0)
        || !(marker_is_geometry_locus(payload, offset)
            || payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]))
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || sketch_marker_prefix_at(payload, offset.saturating_add(84))
    {
        return false;
    }
    markers.iter().copied().any(|sibling| {
        let Some(sibling_offset) = usize::try_from(sibling.offset()).ok() else {
            return false;
        };
        sibling.feature_ref == curve.feature_ref
            && sibling.offset() < curve.offset()
            && sibling.kind() == SketchInputKind::LineOrCircle
            && marker_native_code(payload, sibling_offset) == Some(0)
            && legacy_extended_profile_curve_kind(payload, sibling_offset)
                == Some(SketchInputKind::LineOrCircle)
    })
}

#[cfg(test)]
mod typed_relations_tests;

#[cfg(test)]
mod numerical_range_tests;
