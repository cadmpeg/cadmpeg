//! Typed marker relation definitions and geometry predicates.

use super::curves::compact_bounded_curve_tangent;
use super::endpoints::{
    collect_endpoint_markers, compact_indexed_curve_endpoint_indices,
    compact_indexed_curve_record_end, compact_legacy_code_one_line_endpoint_indices,
    copy_endpoint_markers, extended_compact_indexed_curve_endpoint_indices,
    extended_direct_object_line_endpoint_ids, extended_shifted_construction_line_endpoint_indices,
    legacy_marker104_arc_center, legacy_terminal_profile_endpoint_offset,
    marker_profile_curve_role, roster_curve_endpoint_markers, sort_endpoint_markers,
    wide_indexed_curve_endpoint_indices, wide_indexed_curve_record_is_complete,
    CompactIndexedCurveRecordEnd,
};
use super::markers::{
    compact_legacy_142_profile_curve_endpoints, finite_coordinate_pair, inline_arc_coordinates,
    legacy_extended_profile_curve_kind, marker_is_geometry_locus, marker_native_code,
    sketch_marker_prefix_at,
};
use super::relation_loci::{
    canonical_profile_loci, copy_locus, line_line_distance, linked_midpoint_operands,
    linked_single_arc_entity, linked_single_ellipse_entity, linked_single_entities,
    marker_point_locus, point_line_distance_value, profile_locus_point_charged,
    relation_operand_loci_in, role_locus, same_dimension_angle, same_dimension_length,
    ProfileEntities, RelationIndex,
};
use super::scalars::operand_kind;
use super::selections::operand_accepts_marker;
use super::transforms::{
    locus_entity, marker_entities, sort_marker_entity_ids, MarkerEntityFilter, ProfileAxis,
    SketchLocusRole,
};
use super::{
    LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER, SKETCH_POINT_TOLERANCE,
};
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::nonblank_literal;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntity, SketchEntityId,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::{BTreeMap, HashMap, HashSet};

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

/// The sketch markers relation resolution reads, indexed once per projection.
pub(crate) struct RelationMarkers<'a> {
    /// Each identity's marker; a later marker replaces an earlier one with the
    /// same identity.
    by_id: HashMap<&'a str, &'a SketchInputEntity>,
    /// For each identity, the indexed markers that link to it, each once, in
    /// source order.
    linked_from: HashMap<&'a str, Vec<&'a SketchInputEntity>>,
    /// For each history feature, its indexed markers in source order.
    by_feature: HashMap<&'a str, Vec<&'a SketchInputEntity>>,
}

impl<'a> RelationMarkers<'a> {
    /// Index the markers of every lane in lane order.
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        lanes: &'a [FeatureInputLane],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT relation markers";
        let mut index = Self::empty();
        for lane in ctx.admit_iter(lanes, OPERATION)? {
            for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                ctx.insert_hash_map(&mut index.by_id, marker.id(), marker, OPERATION)?;
            }
        }
        for lane in ctx.admit_iter(lanes, OPERATION)? {
            for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                index.index_links(ctx, marker)?;
            }
        }
        Ok(index)
    }

    /// Index the markers of one lane.
    pub(crate) fn of_lane(
        ctx: &DecodeContext<'_>,
        lane: &'a FeatureInputLane,
    ) -> Result<Self, CodecError> {
        Self::new(ctx, std::slice::from_ref(lane))
    }

    /// Index markers given by identity, in identity order.
    #[cfg(test)]
    pub(super) fn from_map(
        ctx: &DecodeContext<'_>,
        markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT relation markers";
        let mut markers = markers_by_id.values().copied().collect::<Vec<_>>();
        markers.sort_by_key(|marker| marker.id());
        let mut index = Self::empty();
        for marker in &markers {
            ctx.insert_hash_map(&mut index.by_id, marker.id(), *marker, OPERATION)?;
        }
        for marker in markers {
            index.index_links(ctx, marker)?;
        }
        Ok(index)
    }

    fn empty() -> Self {
        Self {
            by_id: HashMap::new(),
            linked_from: HashMap::new(),
            by_feature: HashMap::new(),
        }
    }

    fn index_links(
        &mut self,
        ctx: &DecodeContext<'_>,
        marker: &'a SketchInputEntity,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "index SLDPRT relation marker links";
        if !self.is_indexed(ctx, marker)? {
            return Ok(());
        }
        if let Some(feature) = marker.feature_ref.as_deref() {
            ctx.push_hash_group(&mut self.by_feature, feature, marker, OPERATION, OPERATION)?;
        }
        let mut seen_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut seen = std::collections::BTreeSet::new();
        for link in ctx.admit_iter(marker.links(), OPERATION)? {
            let target = link.entity_ref.as_str();
            if !seen_storage.with_storage(|| ctx.insert_btree_set(&mut seen, target, OPERATION))? {
                continue;
            }
            ctx.push_hash_group(&mut self.linked_from, target, marker, OPERATION, OPERATION)?;
        }
        Ok(())
    }

    /// Whether this marker is the one its identity resolves to.
    pub(super) fn is_indexed(
        &self,
        ctx: &DecodeContext<'_>,
        marker: &SketchInputEntity,
    ) -> Result<bool, CodecError> {
        Ok(ctx
            .get_hash_map(&self.by_id, marker.id(), "resolve SLDPRT relation marker")?
            .is_some_and(|indexed| std::ptr::eq(*indexed, marker)))
    }

    pub(super) fn by_id(&self) -> &HashMap<&'a str, &'a SketchInputEntity> {
        &self.by_id
    }

    pub(super) fn get(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        operation: &'static str,
    ) -> Result<Option<&'a SketchInputEntity>, CodecError> {
        Ok(ctx.get_hash_map(&self.by_id, id, operation)?.copied())
    }

    /// The indexed markers with a link to this identity, each once.
    pub(super) fn linking_to(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        operation: &'static str,
    ) -> Result<&[&'a SketchInputEntity], CodecError> {
        Ok(ctx
            .get_hash_map(&self.linked_from, id, operation)?
            .map_or(&[], Vec::as_slice))
    }

    pub(super) fn of_feature(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &str,
        operation: &'static str,
    ) -> Result<&[&'a SketchInputEntity], CodecError> {
        Ok(ctx
            .get_hash_map(&self.by_feature, feature, operation)?
            .map_or(&[], Vec::as_slice))
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

/// Whether a marker link names the relation's own handle.
pub(super) fn owner_link(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    link: &SketchInputLink,
) -> Result<bool, CodecError> {
    Ok(ctx.equal(
        link.entity_ref.as_str(),
        relation.id(),
        "match SLDPRT relation owner link",
    )? || relation.local_id() == Some(u32::from(link.local_id)))
}

/// The linked entity that every non-owner link of the marker resolves to, when
/// exactly one entity of the sketch does.
fn unique_entity_from_link_intersection(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker entity intersection";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut links = Vec::new();
    for link in ctx.admit_iter(marker.links(), OPERATION)? {
        if !owner_link(ctx, marker, link)? {
            storage.with_storage(|| ctx.push_vec(&mut links, link, OPERATION))?;
        }
    }
    let Some((first, rest)) = links.split_first() else {
        return Ok(None);
    };
    let markers_by_id = index.markers.by_id();
    let mut candidates = marker_entities(
        ctx,
        &first.entity_ref,
        markers_by_id,
        index.loci_by_marker,
        MarkerEntityFilter::All,
    )?;
    // Each remaining link's entities are resolved when a candidate first needs them.
    let mut linked = Vec::new();
    for _ in ctx.admit_iter(rest, OPERATION)? {
        storage.with_storage(|| ctx.push_vec(&mut linked, None, OPERATION))?;
    }
    ctx.retain_vec(
        &mut candidates,
        |entity| {
            if ctx.contains_text(entity.as_str(), "sketch-entity#relation-point:", OPERATION)? {
                return Ok(false);
            }
            match index.entities.entity(ctx, entity, OPERATION)? {
                Some(record) if ctx.equal(&record.sketch, sketch, OPERATION)? => {}
                _ => return Ok(false),
            }
            if !ctx.all_by(
                rest.iter().copied().zip(linked.iter_mut()),
                |(link, entities)| {
                    let entities = match entities {
                        Some(entities) => entities,
                        None => entities.insert(storage.with_storage(|| {
                            marker_entities(
                                ctx,
                                &link.entity_ref,
                                markers_by_id,
                                index.loci_by_marker,
                                MarkerEntityFilter::All,
                            )
                        })?),
                    };
                    if !ctx.contains(entities, entity, OPERATION)? {
                        return Ok(false);
                    }

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }
            Ok(true)
        },
        OPERATION,
    )?;
    sort_marker_entity_ids(ctx, &mut candidates, OPERATION)?;
    Ok(if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    })
}

#[cfg(test)]
pub(super) fn typed_marker_relation_definition_in_sketch(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    let entities = ProfileEntities::new(ctx, sketch_entities)?;
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    marker_relation_definition(
        ctx,
        marker,
        sketch,
        RelationIndex {
            entities: &entities,
            markers: &markers,
            loci_by_marker,
        },
    )
}

/// The native constraint of a relation marker: the entities of its links and
/// reverse owners, with each link and owner as a native operand.
fn native_marker_relation(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    index: RelationIndex<'_, '_>,
) -> Result<SketchConstraintDefinitionInput, CodecError> {
    const OPERATION: &str = "retain SLDPRT native marker relation";
    let markers_by_id = index.markers.by_id();
    let mut entities = Vec::new();
    for link in ctx.admit_iter(marker.links(), OPERATION)? {
        if owner_link(ctx, marker, link)? {
            continue;
        }
        let additions = marker_entities(
            ctx,
            &link.entity_ref,
            markers_by_id,
            index.loci_by_marker,
            MarkerEntityFilter::All,
        )?;
        ctx.extend_vec(&mut entities, additions, OPERATION)?;
    }
    sort_marker_entity_ids(ctx, &mut entities, OPERATION)?;
    let (owners, _owners_storage) = ctx.with_scoped_storage(OPERATION, || {
        relation_owner_markers_in(ctx, marker, index.markers)
    })?;
    for owner in ctx.admit_iter(&owners, OPERATION)? {
        let additions = marker_entities(
            ctx,
            owner.id(),
            markers_by_id,
            index.loci_by_marker,
            MarkerEntityFilter::All,
        )?;
        ctx.extend_vec(&mut entities, additions, OPERATION)?;
    }
    sort_marker_entity_ids(ctx, &mut entities, OPERATION)?;
    let mut operands = Vec::new();
    for link in ctx.admit_iter(marker.links(), OPERATION)? {
        let operand = SketchNativeOperand {
            native_kind: nonblank_literal!("sldprt:marker-local-id"),
            field: None,
            object_index: Some(u32::from(link.local_id)),
            native_ref: Some(ctx.copy_retained_text(&link.entity_ref, OPERATION)?),
        };
        ctx.push_vec(&mut operands, operand, OPERATION)?;
    }
    for owner in ctx.admit_iter(owners, OPERATION)? {
        let operand = SketchNativeOperand {
            native_kind: nonblank_literal!("sldprt:marker-constraint-owner"),
            field: None,
            object_index: owner.object_index().or(owner.local_id()),
            native_ref: Some(ctx.copy_retained_text(owner.id(), OPERATION)?),
        };
        ctx.push_vec(&mut operands, operand, OPERATION)?;
    }
    Ok(SketchConstraintDefinitionInput::Native {
        native_kind: nonblank_literal!(
            ctx,
            "sldprt:marker-relation:{}",
            marker.kind().native_code()
        )?,
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities,
        parameter: None,
        operands,
    })
}

/// The point locus a forward point link of an axis relation names in the sketch.
fn forward_axis_point_locus(
    ctx: &DecodeContext<'_>,
    link: &SketchInputLink,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<SketchLocus>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT forward axis point identity";
    let Some(linked) = index.markers.get(ctx, &link.entity_ref, OPERATION)? else {
        return Ok(None);
    };
    if !matches!(
        linked.kind(),
        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
    ) {
        return Ok(None);
    }
    let mut candidates = index
        .entities
        .with_native_ref(ctx, &link.entity_ref, OPERATION)?
        .iter()
        .copied();
    let mut next = || {
        ctx.find_by(
            &mut candidates,
            |entity| {
                Ok(ctx.equal(&entity.sketch, sketch, OPERATION)?
                    && matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ))
            },
            OPERATION,
        )
    };
    if let Some(entity) = next()? {
        if next()?.is_some() {
            return Ok(None);
        }
        return role_locus(ctx, SketchLocusRole::Entity, entity.id(), OPERATION).map(Some);
    }
    let Some(locus) = marker_point_locus(
        ctx,
        &link.entity_ref,
        index.markers.by_id(),
        index.loci_by_marker,
    )?
    else {
        return Ok(None);
    };
    Ok(
        match index
            .entities
            .entity(ctx, locus_entity(&locus), OPERATION)?
        {
            Some(entity) if ctx.equal(&entity.sketch, sketch, OPERATION)? => Some(locus),
            _ => None,
        },
    )
}

/// The typed definition of a sketch relation marker in its sketch, a native
/// definition when its operands do not resolve, or `None` for a marker that
/// owns no constraint or carries a dimension.
pub(super) fn marker_relation_definition(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    macro_rules! marker_resolved_or_none {
        ($candidate:expr) => {
            match $candidate {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    use crate::records::SketchRelationKind::HorizontalPoints;
    let entities = index.entities;
    let markers_by_id = index.markers.by_id();
    let loci_by_marker = index.loci_by_marker;
    let kind = match marker.kind() {
        SketchInputKind::Relation(kind) => Some(kind),
        SketchInputKind::Native(_) | SketchInputKind::NativeHandle(_) => None,
        _ => return Ok(None),
    };
    if !marker_owns_constraint_in(ctx, marker, index.markers)? {
        return Ok(None);
    }
    let native = || native_marker_relation(ctx, marker, index);
    let linked_kind = |link: &SketchInputLink,
                       operation: &'static str|
     -> Result<Option<SketchInputKind>, CodecError> {
        Ok(index
            .markers
            .get(ctx, &link.entity_ref, operation)?
            .map(SketchInputEntity::kind))
    };
    let Some(kind) = kind else {
        return Ok(Some(native()?));
    };
    let group = MarkerRelationGroup::of(kind);
    if let MarkerRelationGroup::SingleEntity(single) = group {
        if single == SingleEntityRelation::Fixed {
            if let Some(entity) = unique_entity_from_link_intersection(ctx, marker, sketch, index)?
            {
                return Ok(Some(SketchConstraintDefinitionInput::Fixed { entity }));
            }
        }
        if let Some((same_coordinate, _)) = single.axes() {
            // Point targets disambiguate these operands when a local/object index
            // happens to collide with the relation handle's index.
            // Forward point links are explicit operands. Reverse incidences are
            // ownership metadata and must not suppress those operands.
            const POINT_OPERATION: &str = "collect SLDPRT forward point links";
            let mut point_links = Vec::new();
            for link in ctx.admit_iter(marker.links(), POINT_OPERATION)? {
                if ctx.equal(link.entity_ref.as_str(), marker.id(), POINT_OPERATION)?
                    || matches!(
                        linked_kind(link, POINT_OPERATION)?,
                        Some(SketchInputKind::Relation(_))
                    )
                {
                    continue;
                }
                ctx.push_vec(&mut point_links, link, POINT_OPERATION)?;
            }
            if let [first_link, second_link] = point_links.as_slice() {
                if let (Some(first), Some(second)) = (
                    forward_axis_point_locus(ctx, first_link, sketch, index)?,
                    forward_axis_point_locus(ctx, second_link, sketch, index)?,
                ) {
                    if !ctx.equal(&first, &second, POINT_OPERATION)? {
                        return Ok(Some(SketchConstraintDefinitionInput::SameCoordinate {
                            relation: marker_resolved_or_none!(
                                cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                    first,
                                    second,
                                    same_coordinate,
                                )
                                .ok()
                            ),
                        }));
                    }
                }
            }
        }
    }
    Ok(Some(match group {
        MarkerRelationGroup::SingleEntity(single) => {
            const ENTITY_OPERATION: &str = "resolve SLDPRT exact marker entities";

            let axes = single.axes();
            if let Some((same_coordinate, _)) = axes {
                if let Some([first, second]) = axis_relation_point_loci(ctx, marker, sketch, index)?
                {
                    return Ok(Some(SketchConstraintDefinitionInput::SameCoordinate {
                        relation: marker_resolved_or_none!(
                            cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                first,
                                second,
                                same_coordinate,
                            )
                            .ok()
                        ),
                    }));
                }
            }
            if let Some((same_coordinate, _)) = axes {
                const POINT_OPERATION: &str = "collect SLDPRT owned point links";
                let mut point_links = Vec::new();
                for link in ctx.admit_iter(marker.links(), POINT_OPERATION)? {
                    if !owner_link(ctx, marker, link)? {
                        ctx.push_vec(&mut point_links, link, POINT_OPERATION)?;
                    }
                }
                if let [first_link, second_link] = point_links.as_slice() {
                    let is_point_link = |link: &SketchInputLink| {
                        Ok::<_, CodecError>(matches!(
                            linked_kind(link, POINT_OPERATION)?,
                            Some(SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
                        ))
                    };
                    if is_point_link(first_link)? && is_point_link(second_link)? {
                        if let Some(loci) =
                            relation_operand_loci_in(ctx, marker, index.markers, loci_by_marker)?
                        {
                            if let Ok([first, second]) = <[SketchLocus; 2]>::try_from(loci) {
                                return Ok(Some(SketchConstraintDefinitionInput::SameCoordinate {
                                    relation: marker_resolved_or_none!(
                                        cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                            first,
                                            second,
                                            same_coordinate,
                                        )
                                        .ok()
                                    ),
                                }));
                            }
                        }
                    }
                }
            }
            let inferred_entities = marker_entities(
                ctx,
                marker.id(),
                markers_by_id,
                loci_by_marker,
                MarkerEntityFilter::All,
            )?;
            let mut exact_entities = Vec::new();
            for link in ctx.admit_iter(marker.links(), ENTITY_OPERATION)? {
                if owner_link(ctx, marker, link)? {
                    continue;
                }
                let Some(linked) = index.markers.get(ctx, &link.entity_ref, ENTITY_OPERATION)?
                else {
                    continue;
                };
                if single == SingleEntityRelation::Fixed
                    && matches!(
                        linked.kind(),
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    )
                {
                    let additions = marker_entities(
                        ctx,
                        &link.entity_ref,
                        markers_by_id,
                        loci_by_marker,
                        MarkerEntityFilter::All,
                    )?;
                    ctx.extend_vec(&mut exact_entities, additions, ENTITY_OPERATION)?;
                    continue;
                }
                if !matches!(
                    linked.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                ) {
                    continue;
                }
                if let [entity] =
                    entities.with_native_ref(ctx, &link.entity_ref, ENTITY_OPERATION)?
                {
                    let identity = entity.id().try_clone_for_decode(ctx, ENTITY_OPERATION)?;
                    ctx.push_vec(&mut exact_entities, identity, ENTITY_OPERATION)?;
                }
            }
            sort_marker_entity_ids(ctx, &mut exact_entities, ENTITY_OPERATION)?;
            let direct_entities = if exact_entities.len() == 1 {
                exact_entities
            } else {
                inferred_entities
            };
            let (relation_owners, _owners_storage) = ctx
                .with_scoped_storage(ENTITY_OPERATION, || {
                    relation_owner_markers_in(ctx, marker, index.markers)
                })?;
            let point_owner_pair = matches!(relation_owners.as_slice(), [first, second]
                if matches!(first.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
                    && matches!(second.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint));
            let owner_entities = relation_owner_curve_entities(ctx, marker, index)?;
            let entities_of_marker = if point_owner_pair && axes.is_some() {
                Vec::new()
            } else {
                match owner_entities.as_slice() {
                    [owner]
                        if ctx.all_by(
                            &direct_entities,
                            |entity| {
                                Ok(ctx.equal(entity, owner, ENTITY_OPERATION)?
                                    || ctx.contains_text(
                                        entity.as_str(),
                                        "sketch-entity#relation-point:",
                                        ENTITY_OPERATION,
                                    )?)
                            },
                            ENTITY_OPERATION,
                        )? =>
                    {
                        owner_entities
                    }
                    _ => direct_entities,
                }
            };
            if let [entity] = entities_of_marker.as_slice() {
                if axes.is_some()
                    && entities.is_empty()
                    && ctx.contains_text(
                        entity.as_str(),
                        "sketch-entity#relation-point:",
                        ENTITY_OPERATION,
                    )?
                {
                    return Ok(Some(native()?));
                }
                single.definition(marker_resolved_or_none!(entities_of_marker
                    .into_iter()
                    .next()))
            } else if let Some((same_coordinate, profile_axis)) = axes {
                let loci =
                    match relation_operand_loci_in(ctx, marker, index.markers, loci_by_marker)? {
                        Some(loci) => Some(loci),
                        None => unique_axis_aligned_linked_loci_in(
                            ctx,
                            marker,
                            sketch,
                            index,
                            profile_axis,
                        )?,
                    };
                let Some(loci) = loci else {
                    return Ok(Some(native()?));
                };
                let Ok([first, second]) = <[SketchLocus; 2]>::try_from(loci) else {
                    return Ok(Some(native()?));
                };
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
            const OPERATION: &str = "resolve SLDPRT arc angle marker entity";
            let Some(entity) =
                linked_single_arc_entity(ctx, marker, markers_by_id, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            let angle = quarter.angle();
            if !entities.is_empty() {
                let Some(SketchGeometryDefinition::Arc {
                    start_angle,
                    end_angle,
                    ..
                }) = entities
                    .entity(ctx, &entity, OPERATION)?
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
            const OPERATION: &str = "resolve SLDPRT ellipse angle marker entity";
            let Some(entity) = linked_single_ellipse_entity(ctx, marker, index)? else {
                return Ok(Some(native()?));
            };
            let angle = quarter.angle();
            let Some(SketchGeometryDefinition::Ellipse {
                bounds: Some([start, end]),
                ..
            }) = entities
                .entity(ctx, &entity, OPERATION)?
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
            const BINARY_OPERATION: &str = "resolve SLDPRT binary marker entities";

            let owner_entities = relation_owner_curve_entities(ctx, marker, index)?;
            let mut storage = ctx.reserve_scoped(0, BINARY_OPERATION)?;
            let mut forward_links = Vec::new();
            let mut forward_entities = Vec::new();
            for link in ctx.admit_iter(marker.links(), BINARY_OPERATION)? {
                if owner_link(ctx, marker, link)? {
                    continue;
                }
                storage
                    .with_storage(|| ctx.push_vec(&mut forward_links, link, BINARY_OPERATION))?;
                for entity in ctx.admit_iter(
                    marker_entities(
                        ctx,
                        &link.entity_ref,
                        markers_by_id,
                        loci_by_marker,
                        MarkerEntityFilter::All,
                    )?,
                    BINARY_OPERATION,
                )? {
                    if ctx.contains_text(
                        entity.as_str(),
                        "sketch-entity#relation-point:",
                        BINARY_OPERATION,
                    )? {
                        continue;
                    }
                    ctx.push_vec(&mut forward_entities, entity, BINARY_OPERATION)?;
                }
            }
            let geometry_pair = match (owner_entities.is_empty() && !entities.is_empty())
                .then_some(forward_links.as_slice())
            {
                Some([first_link, second_link]) => {
                    let resolve =
                        |link: &SketchInputLink| -> Result<Vec<SketchEntityId>, CodecError> {
                            let mut candidates = marker_entities(
                                ctx,
                                &link.entity_ref,
                                markers_by_id,
                                loci_by_marker,
                                MarkerEntityFilter::All,
                            )?;
                            ctx.retain_vec(
                                &mut candidates,
                                |candidate| {
                                    Ok(match entities.entity(ctx, candidate, BINARY_OPERATION)? {
                                        Some(entity) => {
                                            ctx.equal(&entity.sketch, sketch, BINARY_OPERATION)?
                                        }
                                        None => false,
                                    })
                                },
                                BINARY_OPERATION,
                            )?;
                            Ok(candidates)
                        };
                    let first_candidates = resolve(first_link)?;
                    let second_candidates = resolve(second_link)?;
                    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
                    let mut ambiguous = false;
                    for first in ctx.admit_iter(&first_candidates, BINARY_OPERATION)? {
                        for second in ctx.admit_iter(&second_candidates, BINARY_OPERATION)? {
                            if ctx.equal(first, second, BINARY_OPERATION)? {
                                continue;
                            }
                            let Some(first_entity) =
                                entities.entity(ctx, first, BINARY_OPERATION)?
                            else {
                                continue;
                            };
                            let Some(second_entity) =
                                entities.entity(ctx, second, BINARY_OPERATION)?
                            else {
                                continue;
                            };
                            if !binary.matches_evaluated_geometry(first_entity, second_entity) {
                                continue;
                            }
                            if let Some((selected_first, selected_second)) = selected {
                                if !(ctx.equal(selected_first, first, BINARY_OPERATION)?
                                    && ctx.equal(selected_second, second, BINARY_OPERATION)?)
                                {
                                    ambiguous = true;
                                }
                            }
                            selected = Some((first, second));
                        }
                    }
                    match (ambiguous, selected) {
                        (false, Some((first, second))) => Some((
                            first.try_clone_for_decode(ctx, BINARY_OPERATION)?,
                            second.try_clone_for_decode(ctx, BINARY_OPERATION)?,
                        )),
                        _ => None,
                    }
                }
                _ => None,
            };
            let all_owned = owner_entities.len() != 2
                || ctx.all_by(
                    &forward_entities,
                    |entity| ctx.contains(&owner_entities, entity, BINARY_OPERATION),
                    BINARY_OPERATION,
                )?;
            let pair = if owner_entities.len() == 2 && all_owned {
                owner_entities
            } else if let Some((first, second)) = geometry_pair {
                vec![first, second]
            } else {
                let Some(pair) =
                    linked_single_entities(ctx, marker, markers_by_id, loci_by_marker)?
                else {
                    return Ok(Some(native()?));
                };
                pair
            };
            let [first, second] = pair.as_slice() else {
                return Ok(Some(native()?));
            };
            if !entities.is_empty()
                && (entities.entity(ctx, first, BINARY_OPERATION)?.is_none()
                    || entities.entity(ctx, second, BINARY_OPERATION)?.is_none())
            {
                return Ok(Some(native()?));
            }
            let [first, second] =
                marker_resolved_or_none!(<[SketchEntityId; 2]>::try_from(pair).ok());
            binary.definition(first, second)
        }
        MarkerRelationGroup::Coincidence => {
            let Some(loci) = relation_operand_loci_in(ctx, marker, index.markers, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            if loci.len() < 2 {
                return Ok(Some(native()?));
            }
            if !entities.is_empty() {
                for locus in ctx.admit_iter(&loci, "resolve SLDPRT coincidence locus")? {
                    if profile_locus_point_charged(
                        ctx,
                        locus,
                        entities,
                        "resolve SLDPRT coincidence locus",
                    )?
                    .is_none()
                    {
                        return Ok(Some(native()?));
                    }
                }
            }
            SketchConstraintDefinitionInput::CoincidentLoci { loci }
        }
        MarkerRelationGroup::AxisPoints => {
            let Some(loci) = relation_operand_loci_in(ctx, marker, index.markers, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            let Ok([first, second]) = <[SketchLocus; 2]>::try_from(loci) else {
                return Ok(Some(native()?));
            };
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
            const OPERATION: &str = "resolve SLDPRT marker intersection operands";

            if entities.is_empty() {
                return Ok(Some(native()?));
            }
            let Some(loci) = relation_operand_loci_in(ctx, marker, index.markers, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            let mut point = None;
            let mut curves = Vec::new();
            for locus in ctx.admit_iter(loci, OPERATION)? {
                let Some(entity) = entities.entity(ctx, locus_entity(&locus), OPERATION)? else {
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
                    let identity = match locus {
                        SketchLocus::Entity(identity)
                        | SketchLocus::Start(identity)
                        | SketchLocus::End(identity)
                        | SketchLocus::Center(identity) => identity,
                    };
                    ctx.push_vec(&mut curves, identity, OPERATION)?;
                } else if point.replace(locus).is_some() {
                    return Ok(Some(native()?));
                }
            }
            let (Some(point), [first, second]) = (point, curves.as_slice()) else {
                return Ok(Some(native()?));
            };
            if ctx.equal(first, second, OPERATION)? {
                return Ok(Some(native()?));
            }
            let Some(position) = profile_locus_point_charged(ctx, &point, entities, OPERATION)?
            else {
                return Ok(Some(native()?));
            };
            for identity in [first, second] {
                let Some(entity) = entities.entity(ctx, identity, OPERATION)? else {
                    return Ok(Some(native()?));
                };
                if !sketch_entity_contains_point(entity, position) {
                    return Ok(Some(native()?));
                }
            }
            let [first, second] =
                marker_resolved_or_none!(<[SketchEntityId; 2]>::try_from(curves).ok());
            SketchConstraintDefinitionInput::AtIntersection {
                point,
                first,
                second,
            }
        }
        MarkerRelationGroup::Symmetric => {
            const OPERATION: &str = "resolve SLDPRT symmetric marker operands";

            if entities.is_empty() {
                return Ok(Some(native()?));
            }
            let Some(loci) = relation_operand_loci_in(ctx, marker, index.markers, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            let mut axis = None;
            let mut points = Vec::new();
            for locus in ctx.admit_iter(loci, OPERATION)? {
                let entity = entities.entity(ctx, locus_entity(&locus), OPERATION)?;
                if matches!(locus, SketchLocus::Entity(_))
                    && entity.is_some_and(|entity| {
                        matches!(
                            *entity.geometry.definition(),
                            SketchGeometryDefinition::Line { .. }
                        )
                    })
                {
                    if axis
                        .replace(match locus {
                            SketchLocus::Entity(entity)
                            | SketchLocus::Start(entity)
                            | SketchLocus::End(entity)
                            | SketchLocus::Center(entity) => entity,
                        })
                        .is_some()
                    {
                        return Ok(Some(native()?));
                    }
                } else {
                    ctx.push_vec(&mut points, locus, OPERATION)?;
                }
            }
            let (Some(axis), [first, second]) = (axis, points.as_slice()) else {
                return Ok(Some(native()?));
            };
            if ctx.equal(first, second, OPERATION)? {
                return Ok(Some(native()?));
            }
            let Some(first_point) = profile_locus_point_charged(ctx, first, entities, OPERATION)?
            else {
                return Ok(Some(native()?));
            };
            let Some(second_point) = profile_locus_point_charged(ctx, second, entities, OPERATION)?
            else {
                return Ok(Some(native()?));
            };
            let Some(axis_entity) = entities.entity(ctx, &axis, OPERATION)? else {
                return Ok(Some(native()?));
            };
            if symmetric_loci_match_axis(first_point, second_point, axis_entity) != Some(true) {
                return Ok(Some(native()?));
            }
            let [first, second] =
                marker_resolved_or_none!(<[SketchLocus; 2]>::try_from(points).ok());
            SketchConstraintDefinitionInput::Symmetric {
                first,
                second,
                axis,
            }
        }
        MarkerRelationGroup::Midpoint => {
            const OPERATION: &str = "resolve SLDPRT profile locus";
            let Some((point, entity)) =
                linked_midpoint_operands(ctx, marker, markers_by_id, loci_by_marker)?
            else {
                return Ok(Some(native()?));
            };
            if !entities.is_empty() {
                let Some(point_position) =
                    profile_locus_point_charged(ctx, &point, entities, OPERATION)?
                else {
                    return Ok(Some(native()?));
                };
                let Some(midpoint) = entities
                    .entity(ctx, &entity, OPERATION)?
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

#[cfg(test)]
pub(super) fn unique_axis_aligned_linked_loci(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    axis: ProfileAxis,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    let entities = ProfileEntities::new(ctx, sketch_entities)?;
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    unique_axis_aligned_linked_loci_in(
        ctx,
        marker,
        sketch,
        RelationIndex {
            entities: &entities,
            markers: &markers,
            loci_by_marker,
        },
        axis,
    )
}

/// For an axis relation with two geometric operand links of which exactly one
/// resolves to a point locus, that locus and the one other profile locus of the
/// sketch aligned with it along `axis`.
fn unique_axis_aligned_linked_loci_in(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
    axis: ProfileAxis,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    const OPERATION: &str = "select SLDPRT linked axis loci";
    let markers_by_id = index.markers.by_id();
    let mut links = [None, None];
    let mut count = 0;
    if !ctx.all_by(
        marker.links(),
        |link| {
            if !relation_link_is_geometric_operand(ctx, marker, link, markers_by_id)? {
                return Ok(true);
            }
            let Some(slot) = links.get_mut(count) else {
                return Ok(false);
            };
            *slot = Some(link);
            count += 1;

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let [Some(first_link), Some(second_link)] = links else {
        return Ok(None);
    };
    let first = marker_point_locus(
        ctx,
        &first_link.entity_ref,
        markers_by_id,
        index.loci_by_marker,
    )?;
    let second = marker_point_locus(
        ctx,
        &second_link.entity_ref,
        markers_by_id,
        index.loci_by_marker,
    )?;
    let (known, known_is_first) = match (first, second) {
        (Some(known), None) => (known, true),
        (None, Some(known)) => (known, false),
        _ => return Ok(None),
    };
    let Some(known_point) = profile_locus_point_charged(ctx, &known, index.entities, OPERATION)?
    else {
        return Ok(None);
    };
    let (loci, _loci_storage) = ctx.with_scoped_storage(OPERATION, || {
        canonical_profile_loci(ctx, sketch, index.entities)
    })?;
    let mut selected: Option<&SketchLocus> = None;
    if !ctx.all_by(
        &loci,
        |(candidate_point, candidate)| {
            let aligned = if axis == ProfileAxis::U {
                same_dimension_length(candidate_point.v, known_point.v)
            } else {
                same_dimension_length(candidate_point.u, known_point.u)
            };
            if !aligned || ctx.equal(candidate, &known, OPERATION)? {
                return Ok(true);
            }
            if let Some(selected) = selected {
                if !ctx.equal(selected, candidate, OPERATION)? {
                    return Ok(false);
                }
            }
            selected = Some(candidate);

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some(candidate) = selected else {
        return Ok(None);
    };
    let candidate = copy_locus(ctx, candidate, OPERATION)?;
    let mut pair = Vec::new();
    for locus in if known_is_first {
        [known, candidate]
    } else {
        [candidate, known]
    } {
        ctx.push_vec(&mut pair, locus, OPERATION)?;
    }
    Ok(Some(pair))
}

/// The distinct point loci one axis-relation traversal collects, and the
/// relation markers it has entered.
struct AxisRelationPointCollection<'a> {
    visited: HashSet<&'a str>,
    loci: Vec<SketchLocus>,
}

impl AxisRelationPointCollection<'_> {
    fn append(&mut self, ctx: &DecodeContext<'_>, locus: SketchLocus) -> Result<(), CodecError> {
        const OPERATION: &str = "collect SLDPRT axis relation point loci";
        if ctx.contains(&self.loci, &locus, OPERATION)? {
            return Ok(());
        }
        ctx.push_vec(&mut self.loci, locus, OPERATION)
    }
}

/// The two point loci an axis relation reaches through nested relation links,
/// first through forward links alone and then with reverse point owners.
fn axis_relation_point_loci(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<[SketchLocus; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT nested axis relation points";
    if !ctx.any_by(
        relation.links(),
        |link| {
            Ok(!owner_link(ctx, relation, link)?
                && matches!(
                    index
                        .markers
                        .get(ctx, &link.entity_ref, OPERATION)?
                        .map(SketchInputEntity::kind),
                    Some(SketchInputKind::Relation(_))
                ))
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    for include_reverse_owners in [false, true] {
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut collection = AxisRelationPointCollection {
            visited: HashSet::new(),
            loci: Vec::new(),
        };
        collect_axis_relation_point_loci(
            ctx,
            relation,
            sketch,
            index,
            &mut storage,
            &mut collection,
            include_reverse_owners,
        )?;
        let mut loci = collection.loci;
        sort_axis_relation_point_loci(ctx, &mut loci)?;
        if loci.len() == 2 || include_reverse_owners {
            return Ok(<[SketchLocus; 2]>::try_from(loci).ok());
        }
        if loci.len() > 2 {
            return Ok(None);
        }
    }
    Ok(None)
}

fn sort_axis_relation_point_loci(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "sort SLDPRT axis relation point loci";
    ctx.sort_unstable_by(
        loci.as_mut_slice(),
        |value| value,
        |left, right| super::transforms::locus_key(left).cmp(&super::transforms::locus_key(right)),
        OPERATION,
    )?;
    ctx.dedup_vec(loci, "deduplicate SLDPRT axis relation point loci")
}

fn collect_axis_relation_point_loci<'a>(
    ctx: &DecodeContext<'_>,
    relation: &'a SketchInputEntity,
    sketch: &SketchId,
    index: RelationIndex<'_, 'a>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    collection: &mut AxisRelationPointCollection<'a>,
    include_reverse_owners: bool,
) -> Result<(), CodecError> {
    const OPERATION: &str = "traverse SLDPRT axis relation point loci";
    let _nesting = ctx.enter_nested(OPERATION)?;
    if !storage
        .with_storage(|| ctx.insert_hash_set(&mut collection.visited, relation.id(), OPERATION))?
    {
        return Ok(());
    }
    for link in ctx.admit_iter(relation.links(), OPERATION)? {
        if owner_link(ctx, relation, link)? {
            continue;
        }
        let Some(linked) = index.markers.get(ctx, &link.entity_ref, OPERATION)? else {
            continue;
        };
        match linked.kind() {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                append_axis_relation_point_locus(ctx, linked.id(), sketch, index, collection)?;
            }
            SketchInputKind::Relation(_) => {
                collect_axis_relation_point_loci(
                    ctx,
                    linked,
                    sketch,
                    index,
                    storage,
                    collection,
                    include_reverse_owners,
                )?;
            }
            _ => {}
        }
    }
    if include_reverse_owners {
        let (owners, _owners_storage) = ctx.with_scoped_storage(OPERATION, || {
            relation_owner_markers_in(ctx, relation, index.markers)
        })?;
        for owner in ctx.admit_iter(owners, OPERATION)? {
            if matches!(
                owner.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                append_axis_relation_point_locus(ctx, owner.id(), sketch, index, collection)?;
            }
        }
    }
    Ok(())
}

fn append_axis_relation_point_locus(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
    collection: &mut AxisRelationPointCollection<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "resolve SLDPRT axis relation point identity";
    if let Some(locus) =
        marker_point_locus(ctx, marker_id, index.markers.by_id(), index.loci_by_marker)?
    {
        return collection.append(ctx, locus);
    }
    let mut candidates = index
        .entities
        .with_native_ref(ctx, marker_id, OPERATION)?
        .iter()
        .copied();
    let mut next = || {
        ctx.find_by(
            &mut candidates,
            |entity| {
                Ok(ctx.equal(&entity.sketch, sketch, OPERATION)?
                    && matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ))
            },
            OPERATION,
        )
    };
    if let Some(entity) = next()? {
        if next()?.is_some() {
            return Ok(());
        }
        let locus = role_locus(ctx, SketchLocusRole::Entity, entity.id(), OPERATION)?;
        collection.append(ctx, locus)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn relation_owner_markers<'a>(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    markers_by_id: &'a HashMap<&str, &SketchInputEntity>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    relation_owner_markers_in(ctx, relation, &markers)
}

/// The geometry markers of the relation's feature that link to the relation,
/// in offset order.
pub(super) fn relation_owner_markers_in<'a>(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    markers: &RelationMarkers<'a>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "collect SLDPRT reverse relation owners";
    let feature = relation.feature_ref.as_deref();
    let mut owners = Vec::new();
    for marker in ctx
        .admit_iter(
            markers.linking_to(ctx, relation.id(), OPERATION)?,
            OPERATION,
        )?
        .copied()
    {
        if matches!(
            marker.kind(),
            SketchInputKind::Point
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
                | SketchInputKind::ConstrainedPoint
        ) && ctx.equal(&marker.feature_ref.as_deref(), &feature, OPERATION)?
        {
            ctx.push_vec(&mut owners, marker, OPERATION)?;
        }
    }
    ctx.stable_sort_by_key(&mut owners, |value| value.offset(), Ord::cmp, OPERATION)?;
    Ok(owners)
}

#[cfg(test)]
pub(crate) fn marker_owns_constraint(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Result<bool, CodecError> {
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    marker_owns_constraint_in(ctx, marker, &markers)
}

/// Whether a relation marker owns a sketch constraint: an owning kind with a
/// non-owner link or a reverse owner, or a horizontal or vertical relation
/// between exactly two point links.
pub(crate) fn marker_owns_constraint_in(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers: &RelationMarkers<'_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker constraint ownership";
    if !marker.kind().owns_constraint() {
        return Ok(false);
    }
    if ctx.any_by(
        marker.links(),
        |link| Ok(!owner_link(ctx, marker, link)?),
        OPERATION,
    )? {
        return Ok(true);
    }
    let (owners, _owners_storage) = ctx.with_scoped_storage(OPERATION, || {
        relation_owner_markers_in(ctx, marker, markers)
    })?;
    if !owners.is_empty() {
        return Ok(true);
    }
    if !matches!(
        marker.kind(),
        SketchInputKind::Relation(
            crate::records::SketchRelationKind::Horizontal
                | crate::records::SketchRelationKind::Vertical
        )
    ) {
        return Ok(false);
    }
    // A horizontal or vertical relation without owners owns its constraint when
    // its links other than its own handle and other relations are two points.
    let mut point_links = 0usize;
    if !ctx.all_by(
        marker.links(),
        |link| {
            if ctx.equal(link.entity_ref.as_str(), marker.id(), OPERATION)? {
                return Ok(true);
            }
            match markers
                .get(ctx, &link.entity_ref, OPERATION)?
                .map(SketchInputEntity::kind)
            {
                Some(SketchInputKind::Relation(_)) => {}
                Some(SketchInputKind::Point | SketchInputKind::ConstrainedPoint) => {
                    point_links += 1;
                }
                _ => return Ok(false),
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(false);
    }
    Ok(point_links == 2)
}

pub(super) fn relation_link_is_geometric_operand(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    link: &crate::records::SketchInputLink,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Result<bool, CodecError> {
    // Relation markers are solver handles; geometric operands come from direct
    // links or reverse incidence, never from a relation-to-relation chain.
    Ok(!owner_link(ctx, relation, link)?
        && !matches!(
            ctx.get_hash_map(
                markers_by_id,
                link.entity_ref.as_str(),
                "resolve SLDPRT relation operand link"
            )?
            .map(|marker| marker.kind()),
            Some(SketchInputKind::Relation(_))
        ))
}

fn typed_axis_relation_is_inactive(
    ctx: &DecodeContext<'_>,
    definition: &SketchConstraintDefinitionInput,
    entities: &ProfileEntities<'_>,
) -> Result<Option<bool>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT axis relation activity";
    Ok(match definition {
        SketchConstraintDefinitionInput::Horizontal { entity: id }
        | SketchConstraintDefinitionInput::Vertical { entity: id } => {
            let Some(entity) = entities.entity(ctx, id, OPERATION)? else {
                return Ok(None);
            };
            let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
                return Ok(Some(true));
            };
            Some(
                if matches!(
                    definition,
                    SketchConstraintDefinitionInput::Horizontal { .. }
                ) {
                    !same_dimension_length(start.v, end.v)
                } else {
                    !same_dimension_length(start.u, end.u)
                },
            )
        }
        SketchConstraintDefinitionInput::SameCoordinate { relation } => {
            let Some(first) =
                profile_locus_point_charged(ctx, relation.first(), entities, OPERATION)?
            else {
                return Ok(None);
            };
            let Some(second) =
                profile_locus_point_charged(ctx, relation.second(), entities, OPERATION)?
            else {
                return Ok(None);
            };
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
    entities: &ProfileEntities<'_>,
) -> Result<Option<bool>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT binary relation activity";

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
    let Some(first) = entities.entity(ctx, first, OPERATION)? else {
        return Ok(None);
    };
    let Some(second) = entities.entity(ctx, second, OPERATION)? else {
        return Ok(None);
    };
    Ok(Some(!binary_relation_matches_evaluated_geometry(
        kind, first, second,
    )))
}

#[cfg(test)]
pub(super) fn marker_relation_is_inactive(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<bool, CodecError> {
    let entities = ProfileEntities::new(ctx, sketch_entities)?;
    marker_relation_is_inactive_in(ctx, marker, definition, &entities)
}

pub(super) fn marker_relation_is_inactive_in(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    definition: &SketchConstraintDefinitionInput,
    entities: &ProfileEntities<'_>,
) -> Result<bool, CodecError> {
    use crate::records::SketchRelationKind::{
        ArcAngle180, ArcAngle270, ArcAngle90, Collinear, Concentric, Coradial, EllipseAngle180,
        EllipseAngle270, EllipseAngle90, Equal, Horizontal, MergePoints, Parallel, Perpendicular,
        Tangent, Vertical,
    };
    const OPERATION: &str = "resolve SLDPRT native relation activity";

    let SketchInputKind::Relation(kind) = marker.kind() else {
        return Ok(false);
    };
    if let Some(inactive) = typed_axis_relation_is_inactive(ctx, definition, entities)? {
        return Ok(inactive);
    }
    if let Some(inactive) = typed_binary_relation_is_inactive(ctx, kind, definition, entities)? {
        return Ok(inactive);
    }
    if let SketchConstraintDefinitionInput::CoincidentLoci { loci } = definition {
        let mut first = None;
        let mut inactive = false;
        if !ctx.all_by(
            loci,
            |locus| {
                let Some(point) = profile_locus_point_charged(
                    ctx,
                    locus,
                    entities,
                    "resolve SLDPRT coincidence activity",
                )?
                else {
                    return Ok(false);
                };
                if let Some(first) = first {
                    let first: Point2 = first;
                    inactive |= !same_dimension_length(point.u, first.u)
                        || !same_dimension_length(point.v, first.v);
                } else {
                    first = Some(point);
                }

                Ok(true)
            },
            "resolve SLDPRT coincidence activity",
        )? {
            return Ok(false);
        }
        return Ok(inactive);
    }
    let SketchConstraintDefinitionInput::Native {
        entities: native_entities,
        operands,
        ..
    } = definition
    else {
        return Ok(false);
    };
    let repeated_single_operand = match operands.as_slice() {
        [first, _, ..] if first.native_ref.is_some() => ctx.all_by(
            operands,
            |operand| {
                ctx.equal(
                    &operand.native_ref,
                    &first.native_ref,
                    "compare SLDPRT repeated relation operands",
                )
            },
            "compare SLDPRT repeated relation operands",
        )?,
        _ => false,
    };
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
    if native_entities.is_empty() || entities.is_empty() {
        return Ok(false);
    }
    let mut resolved = Vec::new();
    for id in ctx.admit_iter(native_entities, OPERATION)? {
        if let Some(entity) = entities.entity(ctx, id, OPERATION)? {
            ctx.push_vec(
                &mut resolved,
                entity.geometry.definition(),
                "collect SLDPRT native relation activity",
            )?;
        }
    }
    if resolved.len() != native_entities.len() {
        return Ok(false);
    }
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
            !matches!(resolved.as_slice(), [first, second]
                if !matches!(**first, SketchGeometryDefinition::Point { .. } | SketchGeometryDefinition::Native { .. })
                    && !matches!(**second, SketchGeometryDefinition::Point { .. } | SketchGeometryDefinition::Native { .. }))
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

/// The entities of the relation's curve owners, sorted and deduplicated.
fn relation_owner_curve_entities(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    index: RelationIndex<'_, '_>,
) -> Result<Vec<SketchEntityId>, CodecError> {
    const OPERATION: &str = "collect SLDPRT reverse curve owner entities";
    let mut entities = Vec::new();
    let (owners, _owners_storage) = ctx.with_scoped_storage(OPERATION, || {
        relation_owner_markers_in(ctx, relation, index.markers)
    })?;
    for owner in ctx.admit_iter(owners, OPERATION)? {
        if !matches!(
            owner.kind(),
            SketchInputKind::LineOrCircle | SketchInputKind::Arc
        ) {
            continue;
        }
        let additions = marker_entities(
            ctx,
            owner.id(),
            index.markers.by_id(),
            index.loci_by_marker,
            MarkerEntityFilter::All,
        )?;
        ctx.extend_vec(&mut entities, additions, OPERATION)?;
    }
    sort_marker_entity_ids(ctx, &mut entities, OPERATION)?;
    Ok(entities)
}

#[cfg(test)]
pub(super) fn line_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    line: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    line_endpoint_markers_in(ctx, line, &markers)
}

pub(super) fn line_endpoint_markers_in<'a>(
    ctx: &DecodeContext<'_>,
    line: &SketchInputEntity,
    markers: &RelationMarkers<'a>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    line_endpoint_markers_from(
        ctx,
        line,
        markers.by_id(),
        markers.linking_to(ctx, line.id(), "resolve SLDPRT linked line endpoints")?,
    )
}

/// The point markers of the line's feature with coordinates that the line
/// links to or that link to the line, in offset order. `linking` holds the
/// markers searched for a link to the line.
fn line_endpoint_markers_from<'a>(
    ctx: &DecodeContext<'_>,
    line: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    linking: &[&'a SketchInputEntity],
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked line endpoints";
    let feature = line.feature_ref.as_deref();
    let endpoint = |marker: &SketchInputEntity| -> Result<bool, CodecError> {
        Ok(marker.coordinates_m.is_some()
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
            && ctx.equal(&marker.feature_ref.as_deref(), &feature, OPERATION)?)
    };
    let mut endpoints = Vec::new();
    for link in ctx.admit_iter(line.links(), OPERATION)? {
        if let Some(marker) = ctx
            .get_hash_map(markers_by_id, link.entity_ref.as_str(), OPERATION)?
            .copied()
        {
            if endpoint(marker)? {
                ctx.push_vec(&mut endpoints, marker, OPERATION)?;
            }
        }
    }
    for marker in ctx.admit_iter(linking, OPERATION)?.copied() {
        if endpoint(marker)?
            && ctx.any_by(
                marker.links(),
                |link| ctx.equal(link.entity_ref.as_str(), line.id(), OPERATION),
                OPERATION,
            )?
        {
            ctx.push_vec(&mut endpoints, marker, OPERATION)?;
        }
    }
    sort_endpoint_markers(ctx, &mut endpoints, OPERATION)?;
    ctx.dedup_by(
        &mut endpoints,
        |left, right| ctx.equal(left.id(), right.id(), OPERATION),
        OPERATION,
    )?;
    Ok(endpoints)
}

/// A lane roster and its offset, object and reverse-link joins. Groups preserve
/// source order and include every occurrence, including repeated identities.
pub(super) struct CurveMarkers<'roster, 'a> {
    roster: &'roster [&'a SketchInputEntity],
    by_feature: HashMap<Option<&'a str>, Vec<&'a SketchInputEntity>>,
    by_offset: BTreeMap<Option<&'a str>, Vec<&'a SketchInputEntity>>,
    by_object: HashMap<(Option<&'a str>, Option<u32>), Vec<&'a SketchInputEntity>>,
    linked_from: HashMap<&'a str, Vec<&'a SketchInputEntity>>,
}

impl<'roster, 'a> CurveMarkers<'roster, 'a> {
    pub(super) fn new<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        roster: &'roster [&'a SketchInputEntity],
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "index SLDPRT curve markers";
        ctx.with_scoped_storage(OPERATION, || {
            let mut index = Self {
                roster,
                by_feature: HashMap::new(),
                by_offset: BTreeMap::new(),
                by_object: HashMap::new(),
                linked_from: HashMap::new(),
            };
            for &marker in ctx.admit_iter(roster, OPERATION)? {
                let feature = marker.feature_ref.as_deref();
                ctx.push_hash_group(&mut index.by_feature, feature, marker, OPERATION, OPERATION)?;
                ctx.push_btree_group(&mut index.by_offset, feature, marker, OPERATION, OPERATION)?;
                ctx.push_hash_group(
                    &mut index.by_object,
                    (feature, marker.object_index()),
                    marker,
                    OPERATION,
                    OPERATION,
                )?;
                let mut seen_storage = ctx.reserve_scoped(0, OPERATION)?;
                let mut seen = std::collections::BTreeSet::new();
                for link in ctx.admit_iter(marker.links(), OPERATION)? {
                    let target = link.entity_ref.as_str();
                    if seen_storage
                        .with_storage(|| ctx.insert_btree_set(&mut seen, target, OPERATION))?
                    {
                        ctx.push_hash_group(
                            &mut index.linked_from,
                            target,
                            marker,
                            OPERATION,
                            OPERATION,
                        )?;
                    }
                }
            }
            for (_, markers) in ctx.admit_iter(&mut index.by_offset, OPERATION)? {
                ctx.stable_sort_by_key(markers, |marker| marker.offset(), Ord::cmp, OPERATION)?;
            }
            Ok(index)
        })
    }

    fn next_after(
        &self,
        ctx: &DecodeContext<'_>,
        curve: &SketchInputEntity,
    ) -> Result<Option<&'a SketchInputEntity>, CodecError> {
        const OPERATION: &str = "resolve SLDPRT curve marker offset";
        let markers = ctx
            .get_btree_map(&self.by_offset, &curve.feature_ref.as_deref(), OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        let next = ctx.partition_point(
            markers,
            |marker| Ok(marker.offset() <= curve.offset()),
            OPERATION,
        )?;
        Ok(markers.get(next).copied())
    }
}

/// Resolves a curve against a lane index built once for the pass. Direct object
/// and reverse-link endpoints use keyed groups. Layout rosters retain their
/// original source order.
pub(super) fn marker_curve_endpoint_markers_in<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    index: &CurveMarkers<'_, 'a>,
    geometry: &super::endpoints::geometry_index::MarkerGeometryIndex<'a, '_, '_>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker curve endpoints";
    let feature = curve.feature_ref.as_deref();
    let markers = ctx
        .get_hash_map(&index.by_feature, &feature, OPERATION)?
        .map_or(&[][..], Vec::as_slice);
    let linking = ctx
        .get_hash_map(&index.linked_from, curve.id(), OPERATION)?
        .map_or(&[][..], Vec::as_slice);
    let direct = usize::try_from(curve.offset())
        .ok()
        .and_then(|offset| extended_direct_object_line_endpoint_ids(payload, offset));
    if curve.kind() == SketchInputKind::LineOrCircle {
        if let Some(ids) = direct {
            let resolve = |id| {
                let object = (id != 0).then_some(id);
                let candidates = ctx
                    .get_hash_map(&index.by_object, &(feature, object), OPERATION)?
                    .map_or(&[][..], Vec::as_slice);
                unique_feature_marker(ctx, candidates, curve, OPERATION, is_coordinate_point)
            };
            if let (Some(first), Some(second)) = (resolve(ids[0])?, resolve(ids[1])?) {
                if let Some(pair) = distinct_endpoints(ctx, [first, second], OPERATION)? {
                    return copy_endpoint_markers(ctx, &pair);
                }
            }
        }
    }
    marker_curve_endpoint_markers_from(
        ctx,
        payload,
        curve,
        markers_by_id,
        (markers, linking),
        Some(index),
        geometry,
    )
}

/// The two endpoint markers of a curve marker, resolved by the first layout
/// that names them. `markers_by_id` resolves the curve's links and `markers`
/// is the roster the layouts index into, which also supplies the markers that
/// link back to the curve.
pub(super) fn marker_curve_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    markers: &[&'a SketchInputEntity],
    geometry: &super::endpoints::geometry_index::MarkerGeometryIndex<'a, '_, '_>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    marker_curve_endpoint_markers_from(
        ctx,
        payload,
        curve,
        markers_by_id,
        (markers, markers),
        None,
        geometry,
    )
}

fn marker_curve_endpoint_markers_from<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    (markers, linking): (&[&'a SketchInputEntity], &[&'a SketchInputEntity]),
    index: Option<&CurveMarkers<'_, 'a>>,
    geometry: &super::endpoints::geometry_index::MarkerGeometryIndex<'a, '_, '_>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker curve endpoints";
    if index.is_none() {
        if let Some(endpoints) =
            extended_direct_object_line_endpoints(ctx, payload, curve, markers)?
        {
            return copy_endpoint_markers(ctx, &endpoints);
        }
    }
    let endpoints = line_endpoint_markers_from(ctx, curve, markers_by_id, linking)?;
    if endpoints.len() == 2 {
        return Ok(endpoints);
    }
    let shifted = usize::try_from(curve.offset()).ok().and_then(|offset| {
        extended_shifted_construction_line_endpoint_indices(payload, offset)
            .map(|indices| (offset, indices))
    });
    if let Some((offset, indices)) = shifted {
        let endpoints = if payload.get(offset + 72..offset + 76) == Some(&[0; 4]) {
            let mut owned =
                collect_endpoint_markers(ctx, markers.iter().copied(), curve, |_| true, OPERATION)?;
            sort_endpoint_markers(ctx, &mut owned, OPERATION)?;
            let selected = indices.into_iter().filter_map(|index| {
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
                ctx.reserve_vec(&mut endpoints, 1, OPERATION)?;
                endpoints.push(endpoint);
            }
            endpoints
        } else {
            super::endpoints::coordinate_roster_curve_endpoint_markers_at(
                ctx,
                payload,
                curve,
                Some(56),
                geometry,
            )?
        };
        if endpoints.len() == 2 {
            return Ok(endpoints);
        }
    }
    if let Some(endpoints) = compact_legacy_object_line_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = extended_wide_selected_axis_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = inline_arc_endpoint_markers(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) =
        compact_legacy_142_profile_curve_endpoint_markers(ctx, payload, curve, markers)?
    {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) =
        one_based_point_roster_line_endpoint_markers(ctx, payload, curve, markers)?
    {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) =
        legacy_point_roster_line_endpoint_markers(ctx, payload, curve, markers)?
    {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    let roster = index.map_or(markers, |index| index.roster);
    let endpoints = roster_curve_endpoint_markers(ctx, payload, curve, roster, geometry)?;
    if endpoints.len() == 2 {
        if let Some(direct) = legacy_marker104_arc_endpoints(ctx, payload, curve, markers)? {
            let roster = [endpoints[0], endpoints[1]];
            if legacy_marker104_arc_center(ctx, payload, curve, markers, roster, geometry)?
                .is_none()
                && legacy_marker104_arc_center(ctx, payload, curve, markers, direct, geometry)?
                    .is_some()
            {
                return copy_endpoint_markers(ctx, &direct);
            }
        }
        return Ok(endpoints);
    }
    if let Some(endpoints) = legacy_marker104_arc_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = current_coordinate_linked_line_endpoints(ctx, payload, curve, markers)?
    {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) = coordinate_centered_line_endpoints(ctx, payload, curve, markers)? {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    if let Some(endpoints) =
        legacy_terminal_profile_indexed_endpoints(ctx, payload, curve, markers)?
    {
        return copy_endpoint_markers(ctx, &endpoints);
    }
    let endpoints =
        consecutive_legacy_profile_line_endpoints_from(ctx, payload, curve, markers, index)?;
    if endpoints.len() == 2 {
        return Ok(endpoints);
    }
    if let Some(pair) = coordinate_profile_line_endpoints(ctx, payload, curve, markers_by_id)? {
        copy_endpoint_markers(ctx, &pair)
    } else {
        Ok(endpoints)
    }
}

fn coordinate_profile_line_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT coordinate profile line endpoints";
    if !coordinate_profile_line(payload, curve) {
        return Ok(None);
    }
    let feature = curve.feature_ref.as_deref();
    let mut point = None;
    if !ctx.all_by(
        curve.links(),
        |link| {
            if ctx.equal(link.entity_ref.as_str(), curve.id(), OPERATION)? {
                return Ok(true);
            }
            let Some(linked) = ctx
                .get_hash_map(markers_by_id, link.entity_ref.as_str(), OPERATION)?
                .copied()
            else {
                return Ok(false);
            };
            if !ctx.equal(&linked.feature_ref.as_deref(), &feature, OPERATION)? {
                return Ok(false);
            }
            match linked.kind() {
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                    if linked.coordinates_m.is_none() || point.replace(linked).is_some() {
                        return Ok(false);
                    }
                }
                SketchInputKind::Relation(_) => {}
                SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
                | SketchInputKind::Native(_)
                | SketchInputKind::NativeHandle(_) => return Ok(false),
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some(point) = point else {
        return Ok(None);
    };
    Ok(match (curve.coordinates_m, point.coordinates_m) {
        (Some(curve_point), Some(linked_point)) if curve_point.get() != linked_point.get() => {
            Some([curve, point])
        }
        _ => None,
    })
}

/// Whether the curve is a coordinate-bearing current profile line.
fn coordinate_profile_line(payload: &[u8], curve: &SketchInputEntity) -> bool {
    let Some(offset) = usize::try_from(curve.offset()).ok() else {
        return false;
    };
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
        return false;
    }
    true
}

/// The one marker of the owner's feature that `matches` accepts, when exactly
/// one does. The search stops at a second match.
fn unique_feature_marker<'a>(
    ctx: &DecodeContext<'_>,
    markers: &[&'a SketchInputEntity],
    owner: &SketchInputEntity,
    operation: &'static str,
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Result<Option<&'a SketchInputEntity>, CodecError> {
    let feature = owner.feature_ref.as_deref();
    let mut candidates = markers.iter().copied();
    let mut next = || {
        ctx.find_by(
            &mut candidates,
            |marker| {
                Ok(matches(marker)
                    && ctx.equal(&marker.feature_ref.as_deref(), &feature, operation)?)
            },
            operation,
        )
    };
    let Some(first) = next()? else {
        return Ok(None);
    };
    Ok(next()?.is_none().then_some(first))
}

fn is_coordinate_point(marker: &SketchInputEntity) -> bool {
    marker.coordinates_m.is_some()
        && matches!(
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
}

/// Two endpoints with distinct identities and distinct coordinates.
fn distinct_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    endpoints: [&'a SketchInputEntity; 2],
    operation: &'static str,
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    Ok((endpoints[0].coordinates_m != endpoints[1].coordinates_m
        && !ctx.equal(endpoints[0].id(), endpoints[1].id(), operation)?)
    .then_some(endpoints))
}

pub(super) fn extended_direct_object_line_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT extended direct object line endpoints";
    if curve.kind() != SketchInputKind::LineOrCircle {
        return Ok(None);
    }
    let Some(endpoint_ids) = usize::try_from(curve.offset())
        .ok()
        .and_then(|offset| extended_direct_object_line_endpoint_ids(payload, offset))
    else {
        return Ok(None);
    };
    let resolve = |id| {
        unique_feature_marker(ctx, markers, curve, OPERATION, |marker| {
            is_coordinate_point(marker)
                && if id == 0 {
                    marker.object_index().is_none()
                } else {
                    marker.object_index() == Some(id)
                }
        })
    };
    let (Some(first), Some(second)) = (resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?) else {
        return Ok(None);
    };
    distinct_endpoints(ctx, [first, second], OPERATION)
}

pub(super) fn compact_legacy_object_line_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT compact legacy object line endpoints";
    let Some(endpoint_ids) = usize::try_from(curve.offset())
        .ok()
        .and_then(|offset| compact_legacy_code_one_line_endpoint_indices(payload, offset))
    else {
        return Ok(None);
    };
    let resolve = |id| {
        unique_feature_marker(ctx, markers, curve, OPERATION, |marker| {
            marker.object_index() == Some(id) && is_coordinate_point(marker)
        })
    };
    let (Some(first), Some(second)) = (resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?) else {
        return Ok(None);
    };
    distinct_endpoints(ctx, [first, second], OPERATION)
}

fn extended_wide_selected_axis_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT extended wide selected axis endpoints";
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
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    let resolve_object = |index| {
        unique_feature_marker(ctx, markers, curve, OPERATION, |marker| {
            marker.object_index() == Some(index) && is_coordinate_point(marker)
        })
    };
    if let (Some(first), Some(second)) = (
        resolve_object(encoded[0] + 1)?,
        resolve_object(encoded[1] + 1)?,
    ) {
        if let Some(endpoints) = distinct_endpoints(ctx, [first, second], OPERATION)? {
            return Ok(Some(endpoints));
        }
    }
    let indices = encoded.map(|index| usize::try_from(index).ok()?.checked_sub(1));
    let [Some(first_index), Some(second_index)] = indices else {
        return Ok(None);
    };
    let mut points = collect_endpoint_markers(
        ctx,
        markers.iter().copied(),
        curve,
        |marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        },
        OPERATION,
    )?;
    sort_endpoint_markers(ctx, &mut points, OPERATION)?;
    let (Some(first), Some(second)) = (points.get(first_index), points.get(second_index)) else {
        return Ok(None);
    };
    distinct_endpoints(ctx, [*first, *second], OPERATION)
}

pub(super) fn legacy_marker104_arc_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT legacy marker104 arc endpoints";
    let endpoint_ids = (|| {
        let offset = usize::try_from(curve.offset()).ok()?;
        if curve.kind() != SketchInputKind::Arc
            || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
                != Some(LEGACY_SKETCH_MARKER)
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
    let Some(endpoint_ids) = endpoint_ids else {
        return Ok(None);
    };
    let resolve = |id| {
        unique_feature_marker(ctx, markers, curve, OPERATION, |marker| {
            marker.object_index() == Some(id) && is_coordinate_point(marker)
        })
    };
    let (Some(first), Some(second)) = (resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?) else {
        return Ok(None);
    };
    Ok((first.coordinates_m != second.coordinates_m).then_some([first, second]))
}

fn one_based_point_roster_line_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT one based point roster line endpoint markers";
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
        Some(indices)
    })();
    let Some(indices) = indices else {
        return Ok(None);
    };
    let feature = curve.feature_ref.as_deref();
    if ctx.any_by(
        markers,
        |marker| {
            Ok(marker.kind() == SketchInputKind::Arc
                && ctx.equal(&marker.feature_ref.as_deref(), &feature, OPERATION)?)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let mut points = collect_endpoint_markers(
        ctx,
        markers.iter().copied(),
        curve,
        |marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        },
        OPERATION,
    )?;
    sort_endpoint_markers(ctx, &mut points, OPERATION)?;
    let (Some(first), Some(second)) = (points.get(indices[0]), points.get(indices[1])) else {
        return Ok(None);
    };
    Ok((!ctx.equal(first.id(), second.id(), OPERATION)?).then_some([*first, *second]))
}

fn legacy_point_roster_line_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT legacy point roster line endpoint markers";
    let indices = (|| {
        let offset = usize::try_from(curve.offset()).ok()?;
        if curve.kind() != SketchInputKind::LineOrCircle
            || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
                != Some(LEGACY_SKETCH_MARKER)
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
    let Some(indices) = indices else {
        return Ok(None);
    };
    let mut points = collect_endpoint_markers(
        ctx,
        markers.iter().copied(),
        curve,
        |marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        },
        OPERATION,
    )?;
    sort_endpoint_markers(ctx, &mut points, OPERATION)?;
    let (Some(first), Some(second)) = (points.get(indices[0]), points.get(indices[1])) else {
        return Ok(None);
    };
    distinct_endpoints(ctx, [*first, *second], OPERATION)
}

pub(super) fn legacy_terminal_profile_indexed_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT legacy terminal profile endpoints";
    let endpoint_ids = usize::try_from(curve.offset()).ok().and_then(|offset| {
        let endpoint_offset = legacy_terminal_profile_endpoint_offset(payload, offset)?;
        let endpoint = |relative| Some(u32::from(View::u16_le_at(payload, offset + relative)?));
        let endpoint_ids = [endpoint(endpoint_offset)?, endpoint(endpoint_offset + 2)?];
        (endpoint_ids[0].checked_add(1) == Some(endpoint_ids[1])).then_some(endpoint_ids)
    });
    let Some(endpoint_ids) = endpoint_ids else {
        return Ok(None);
    };
    let resolve = |id| {
        unique_feature_marker(ctx, markers, curve, OPERATION, |marker| {
            is_coordinate_point(marker)
                && (marker.local_id() == Some(id)
                    || marker.object_index().and_then(|index| index.checked_add(1)) == Some(id))
        })
    };
    let (Some(first), Some(second)) = (resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?) else {
        return Ok(None);
    };
    Ok((!ctx.equal(first.id(), second.id(), OPERATION)?).then_some([first, second]))
}

fn inline_arc_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    arc: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT inline arc endpoints";
    let Some([_, start, end]) = usize::try_from(arc.offset())
        .ok()
        .and_then(|offset| inline_arc_coordinates(payload, offset))
    else {
        return Ok(None);
    };
    coordinate_endpoint_pair(ctx, markers, arc, [start, end], OPERATION)
}

/// The two point markers of the owner's feature at the two coordinates, each
/// the only one there, with distinct identities.
fn coordinate_endpoint_pair<'a>(
    ctx: &DecodeContext<'_>,
    markers: &[&'a SketchInputEntity],
    owner: &SketchInputEntity,
    coordinates: [cadmpeg_ir::units::FiniteVector<2>; 2],
    operation: &'static str,
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    let resolve = |coordinates: cadmpeg_ir::units::FiniteVector<2>| {
        unique_feature_marker(ctx, markers, owner, operation, |marker| {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) && marker.coordinates_m.is_some_and(|point| {
                same_dimension_length(point[0], coordinates[0])
                    && same_dimension_length(point[1], coordinates[1])
            })
        })
    };
    let (Some(first), Some(second)) = (resolve(coordinates[0])?, resolve(coordinates[1])?) else {
        return Ok(None);
    };
    Ok((!ctx.equal(first.id(), second.id(), operation)?).then_some([first, second]))
}

fn compact_legacy_142_profile_curve_endpoint_markers<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT compact legacy 142 profile endpoints";
    if curve.kind() != SketchInputKind::LineOrCircle || curve.coordinates_m.is_some() {
        return Ok(None);
    }
    let Some(endpoints) = usize::try_from(curve.offset())
        .ok()
        .and_then(|offset| compact_legacy_142_profile_curve_endpoints(payload, offset))
    else {
        return Ok(None);
    };
    coordinate_endpoint_pair(ctx, markers, curve, endpoints, OPERATION)
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
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT current coordinate linked line endpoints";
    let Some(local_id) = current_coordinate_linked_line_local_id(payload, line) else {
        return Ok(None);
    };
    // A local-link endpoint can select a coordinate-bearing curve marker before
    // the binding pass promotes it to a profile vertex. Keep that candidate in
    // the graph; the binding pass retains it as a curve only when it resolves
    // its own two endpoints.
    let feature = line.feature_ref.as_deref();
    let mut candidates = markers.iter().copied();
    let mut next = || {
        ctx.find_by(
            &mut candidates,
            |marker| {
                Ok(marker.local_id() == Some(local_id)
                    && marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    )
                    && !ctx.equal(marker.id(), line.id(), OPERATION)?
                    && ctx.equal(&marker.feature_ref.as_deref(), &feature, OPERATION)?)
            },
            OPERATION,
        )
    };
    let Some(endpoint) = next()? else {
        return Ok(None);
    };
    if next()?.is_some() {
        return Ok(None);
    }
    Ok(Some([line, endpoint]))
}

/// The local identity a current coordinate-linked line names as its endpoint.
fn current_coordinate_linked_line_local_id(
    payload: &[u8],
    line: &SketchInputEntity,
) -> Option<u32> {
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
    Some(u32::from(View::u16_le_at(cell, 2)?))
}

fn coordinate_centered_line_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    line: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT coordinate centered line endpoints";
    let center = (|| {
        let offset = usize::try_from(line.offset()).ok()?;
        let [center_u, center_v] = coordinate_centered_line_center(payload, offset)?.get();
        Some([center_u, center_v])
    })();
    let Some([center_u, center_v]) = center else {
        return Ok(None);
    };
    let mut coordinates = collect_endpoint_markers(
        ctx,
        markers.iter().copied(),
        line,
        |marker| marker.offset() > line.offset() && marker.coordinates_m.is_some(),
        OPERATION,
    )?;
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

#[cfg(test)]
fn consecutive_legacy_profile_line_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    consecutive_legacy_profile_line_endpoints_from(ctx, payload, line, markers, None)
}

fn consecutive_legacy_profile_line_endpoints_from<'a>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
    index: Option<&CurveMarkers<'_, 'a>>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT consecutive legacy profile line endpoints";
    let Some(offset) = usize::try_from(line.offset()).ok() else {
        return Ok(Vec::new());
    };
    if line.kind() != SketchInputKind::LineOrCircle
        || line.coordinates_m.is_none()
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
    {
        return Ok(Vec::new());
    }
    // The first marker of the line's feature after the line.
    let feature = line.feature_ref.as_deref();
    let next = if let Some(index) = index {
        index.next_after(ctx, line)?
    } else {
        let mut next: Option<&SketchInputEntity> = None;
        for marker in ctx.admit_iter(markers, OPERATION)?.copied() {
            if marker.offset() > line.offset()
                && next.is_none_or(|next| marker.offset() < next.offset())
                && ctx.equal(&marker.feature_ref.as_deref(), &feature, OPERATION)?
            {
                next = Some(marker);
            }
        }
        next
    };
    let Some(next) = next else {
        return Ok(Vec::new());
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
        return Ok(Vec::new());
    }
    let mut endpoints = Vec::new();
    for endpoint in [line, next] {
        ctx.push_vec(&mut endpoints, endpoint, OPERATION)?;
    }
    Ok(endpoints)
}

/// The first legacy profile-line offset of each feature in a lane.
pub(super) struct LegacyTerminalLines<'a> {
    first_by_feature: HashMap<Option<&'a str>, u64>,
}

impl<'a> LegacyTerminalLines<'a> {
    pub(super) fn new<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        payload: &[u8],
        markers: &[&'a SketchInputEntity],
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "index SLDPRT legacy terminal profile lines";
        ctx.with_scoped_storage(OPERATION, || {
            let mut first_by_feature = HashMap::new();
            for &marker in ctx.admit_iter(markers, OPERATION)? {
                let Ok(offset) = usize::try_from(marker.offset()) else {
                    continue;
                };
                if marker.kind() != SketchInputKind::LineOrCircle
                    || marker_native_code(payload, offset) != Some(0)
                    || legacy_extended_profile_curve_kind(payload, offset)
                        != Some(SketchInputKind::LineOrCircle)
                {
                    continue;
                }
                let first = ctx
                    .entry_hash_map(
                        &mut first_by_feature,
                        marker.feature_ref.as_deref(),
                        OPERATION,
                    )?
                    .or_insert(marker.offset());
                *first = (*first).min(marker.offset());
            }
            Ok(Self { first_by_feature })
        })
    }
}

fn is_legacy_terminal_indexed_profile_line(payload: &[u8], curve: &SketchInputEntity) -> bool {
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
        || offset
            .checked_add(84)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return false;
    }
    true
}

pub(super) fn legacy_terminal_indexed_profile_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Result<bool, CodecError> {
    if !is_legacy_terminal_indexed_profile_line(payload, curve) {
        return Ok(false);
    }
    let (index, _storage) = LegacyTerminalLines::new(ctx, payload, markers)?;
    legacy_terminal_indexed_profile_line_in(ctx, payload, curve, &index)
}

pub(super) fn legacy_terminal_indexed_profile_line_in(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    index: &LegacyTerminalLines<'_>,
) -> Result<bool, CodecError> {
    if !is_legacy_terminal_indexed_profile_line(payload, curve) {
        return Ok(false);
    }
    Ok(ctx
        .get_hash_map(
            &index.first_by_feature,
            &curve.feature_ref.as_deref(),
            "resolve SLDPRT legacy terminal indexed profile line",
        )?
        .is_some_and(|offset| *offset < curve.offset()))
}

#[cfg(test)]
mod typed_relations_tests;

#[cfg(test)]
mod numerical_range_tests;
