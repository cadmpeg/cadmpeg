//! Relation definition and profile locus resolution.

use super::grid::{quantize, GridPoint};
use super::markers::marker_is_geometry_locus;
use super::relation_geometry::relation_uses_solver_line_operand;
use super::relation_records::{relation_uses_dynamic_operands, relation_uses_solver_points};
use super::transforms::{
    compatible_marker_transform_candidates, locus_entity, locus_key, marker_entities,
    sketch_entity_locus_points, sketch_frame_marker_transform, sort_marker_entity_ids,
    MarkerEntityFilter, MarkerTransform, ProfileAxis, SketchLocusRole,
};
use super::typed_relations::{
    line_endpoint_markers_in, owner_link, relation_link_is_geometric_operand,
    relation_owner_markers_in, sketch_entity_contains_point, RelationMarkers,
};
use super::SKETCH_POINT_TOLERANCE;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::{index_from_u64, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

// Relation geometry is projected after feature coordinates are rounded to the
// model-space quantum. Keep the ordinary identity comparison strict, but allow
// the bounded error that two independently rounded operand points can add to a
// stored dimensional value.
const RELATION_DIMENSION_RELATIVE_TOLERANCE: f64 = 1.0e-9;
const RELATION_GEOMETRY_QUANTUM_MM: f64 = 1.0e-8;
const RELATION_GEOMETRY_ABSOLUTE_TOLERANCE_MM: f64 = 2.0 * RELATION_GEOMETRY_QUANTUM_MM;
const EPS_RELATION_LOCI_SAME_DIMENSION_ANGLE_E9: f64 = 1e-9;
const EPS_RELATION_LOCI_MARKER_CENTER_DIMENSIONED_ENTITY_E8: f64 = 1e-8;
const EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9: f64 = 1e-9;
const QUALIFIED_POINT_SUFFIX: &str = ":qualified-point";

fn same_relation_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= RELATION_GEOMETRY_ABSOLUTE_TOLERANCE_MM
            .max(RELATION_DIMENSION_RELATIVE_TOLERANCE * left.abs().max(right.abs()).max(1.0))
}

/// The sketch entities relation resolution reads, indexed once per projection.
///
/// Each group keeps the entities in slice order, so a scan of one group visits
/// the entities that a filtered scan of the whole slice visits, in the same order.
pub(super) struct ProfileEntities<'a> {
    all: &'a [SketchEntity],
    /// The first entity with each identity.
    by_id: HashMap<&'a SketchEntityId, &'a SketchEntity>,
    by_sketch: HashMap<&'a SketchId, Vec<&'a SketchEntity>>,
    by_native_ref: HashMap<&'a str, Vec<&'a SketchEntity>>,
    by_geometry_ref: HashMap<&'a str, Vec<&'a SketchEntity>>,
    /// Entities by every native, geometry and endpoint reference they carry.
    by_reference: HashMap<&'a str, Vec<&'a SketchEntity>>,
}

impl<'a> ProfileEntities<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        all: &'a [SketchEntity],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT relation sketch entities";
        let mut index = Self {
            all,
            by_id: HashMap::new(),
            by_sketch: HashMap::new(),
            by_native_ref: HashMap::new(),
            by_geometry_ref: HashMap::new(),
            by_reference: HashMap::new(),
        };
        for entity in ctx.admit_iter(all, OPERATION)? {
            ctx.entry_hash_map(&mut index.by_id, entity.id(), OPERATION)?
                .or_insert(entity);
            ctx.push_hash_group(
                &mut index.by_sketch,
                &entity.sketch,
                entity,
                OPERATION,
                OPERATION,
            )?;
            if let Some(reference) = entity.native_ref.as_deref() {
                ctx.push_hash_group(
                    &mut index.by_native_ref,
                    reference,
                    entity,
                    OPERATION,
                    OPERATION,
                )?;
                ctx.push_hash_group(
                    &mut index.by_reference,
                    reference,
                    entity,
                    OPERATION,
                    OPERATION,
                )?;
            }
            if let Some(reference) = entity.geometry_ref.as_deref() {
                ctx.push_hash_group(
                    &mut index.by_geometry_ref,
                    reference,
                    entity,
                    OPERATION,
                    OPERATION,
                )?;
                ctx.push_hash_group(
                    &mut index.by_reference,
                    reference,
                    entity,
                    OPERATION,
                    OPERATION,
                )?;
            }
            for reference in ctx.admit_iter(&entity.endpoint_refs, OPERATION)? {
                ctx.push_hash_group(
                    &mut index.by_reference,
                    reference.as_str(),
                    entity,
                    OPERATION,
                    OPERATION,
                )?;
            }
        }
        Ok(index)
    }

    pub(super) fn all(&self) -> &'a [SketchEntity] {
        self.all
    }

    pub(super) fn by_id(&self) -> &HashMap<&'a SketchEntityId, &'a SketchEntity> {
        &self.by_id
    }

    pub(super) fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// The first entity with this identity.
    pub(super) fn entity(
        &self,
        ctx: &DecodeContext<'_>,
        id: &SketchEntityId,
        operation: &'static str,
    ) -> Result<Option<&'a SketchEntity>, CodecError> {
        Ok(ctx.get_hash_map(&self.by_id, id, operation)?.copied())
    }

    pub(super) fn in_sketch(
        &self,
        ctx: &DecodeContext<'_>,
        sketch: &SketchId,
        operation: &'static str,
    ) -> Result<&[&'a SketchEntity], CodecError> {
        Ok(ctx
            .get_hash_map(&self.by_sketch, sketch, operation)?
            .map_or(&[], Vec::as_slice))
    }

    pub(super) fn with_native_ref(
        &self,
        ctx: &DecodeContext<'_>,
        reference: &str,
        operation: &'static str,
    ) -> Result<&[&'a SketchEntity], CodecError> {
        Ok(ctx
            .get_hash_map(&self.by_native_ref, reference, operation)?
            .map_or(&[], Vec::as_slice))
    }

    pub(super) fn with_geometry_ref(
        &self,
        ctx: &DecodeContext<'_>,
        reference: &str,
        operation: &'static str,
    ) -> Result<&[&'a SketchEntity], CodecError> {
        Ok(ctx
            .get_hash_map(&self.by_geometry_ref, reference, operation)?
            .map_or(&[], Vec::as_slice))
    }

    /// Entities that carry the reference as their native, geometry or endpoint
    /// reference; an entity carrying it twice appears twice.
    fn with_reference(
        &self,
        ctx: &DecodeContext<'_>,
        reference: &str,
        operation: &'static str,
    ) -> Result<&[&'a SketchEntity], CodecError> {
        Ok(ctx
            .get_hash_map(&self.by_reference, reference, operation)?
            .map_or(&[], Vec::as_slice))
    }
}

/// The indexes one relation projection resolves every relation against.
#[derive(Clone, Copy)]
pub(super) struct RelationIndex<'s, 'a> {
    pub(super) entities: &'s ProfileEntities<'a>,
    pub(super) markers: &'s RelationMarkers<'a>,
    pub(super) loci_by_marker: &'s HashMap<String, Vec<SketchLocus>>,
}

impl<'a> RelationIndex<'_, 'a> {
    fn markers_by_id(&self) -> &HashMap<&'a str, &'a SketchInputEntity> {
        self.markers.by_id()
    }
}

pub(super) fn role_locus(
    ctx: &DecodeContext<'_>,
    role: SketchLocusRole,
    entity: &SketchEntityId,
    operation: &'static str,
) -> Result<SketchLocus, CodecError> {
    let entity = entity.try_clone_for_decode(ctx, operation)?;
    Ok(match role {
        SketchLocusRole::Entity => SketchLocus::Entity(entity),
        SketchLocusRole::Start => SketchLocus::Start(entity),
        SketchLocusRole::End => SketchLocus::End(entity),
        SketchLocusRole::Center => SketchLocus::Center(entity),
    })
}

pub(super) fn copy_locus(
    ctx: &DecodeContext<'_>,
    locus: &SketchLocus,
    operation: &'static str,
) -> Result<SketchLocus, CodecError> {
    role_locus(
        ctx,
        SketchLocusRole::of_locus(locus),
        locus_entity(locus),
        operation,
    )
}

fn is_point(entity: &SketchEntity) -> bool {
    matches!(
        *entity.geometry.definition(),
        SketchGeometryDefinition::Point { .. }
    )
}

fn is_line(entity: &SketchEntity) -> bool {
    matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Line { .. }
    )
}

fn is_circular(entity: &SketchEntity) -> bool {
    matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Arc { .. }
    )
}

/// The one entity among `candidates` that `matches` accepts, if exactly one does.
fn unique_entity<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &[&'a SketchEntity],
    operation: &'static str,
    mut matches: impl FnMut(&SketchEntity) -> Result<bool, CodecError>,
) -> Result<Option<&'a SketchEntity>, CodecError> {
    let mut remaining = candidates.iter().copied();
    let Some(first) = ctx.find_by(&mut remaining, |entity| matches(entity), operation)? else {
        return Ok(None);
    };
    if ctx
        .find_by(&mut remaining, |entity| matches(entity), operation)?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(first))
}

fn in_sketch(
    ctx: &DecodeContext<'_>,
    entity: &SketchEntity,
    sketch: &SketchId,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.equal(&entity.sketch, sketch, operation)
}

pub(super) fn linked_single_arc_entity(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked arc entity";
    let mut has_link = false;
    if !ctx.all_by(
        marker.links(),
        |link| {
            if owner_link(ctx, marker, link)? {
                return Ok(true);
            }
            has_link = true;
            if !matches!(
                ctx.get_hash_map(markers_by_id, link.entity_ref.as_str(), OPERATION)?
                    .map(|marker| marker.kind()),
                Some(SketchInputKind::Arc)
            ) {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    if !has_link {
        return Ok(None);
    }
    let Some(entities) = linked_single_entities(ctx, marker, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    Ok(if entities.len() == 1 {
        entities.into_iter().next()
    } else {
        None
    })
}

pub(super) fn linked_single_ellipse_entity(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    index: RelationIndex<'_, '_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    let Some(entities) =
        linked_single_entities(ctx, marker, index.markers_by_id(), index.loci_by_marker)?
    else {
        return Ok(None);
    };
    let [identity] = entities.as_slice() else {
        return Ok(None);
    };
    let Some(entity) =
        index
            .entities
            .entity(ctx, identity, "resolve SLDPRT linked ellipse entity")?
    else {
        return Ok(None);
    };
    if !matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Ellipse { .. }
    ) {
        return Ok(None);
    }
    Ok(entities.into_iter().next())
}

pub(super) fn linked_midpoint_operands(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked midpoint operands";
    let mut operands = [None, None];
    let mut count = 0;
    if !ctx.all_by(
        marker.links(),
        |link| {
            if owner_link(ctx, marker, link)? {
                return Ok(true);
            }
            let Some(slot) = operands.get_mut(count) else {
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
    let [Some(first), Some(second)] = operands else {
        return Ok(None);
    };
    let mut point = None;
    let mut entity = None;
    for link in [first, second] {
        let Some(linked_marker) =
            ctx.get_hash_map(markers_by_id, link.entity_ref.as_str(), OPERATION)?
        else {
            return Ok(None);
        };
        let Some(loci) = ctx.get_hash_map(loci_by_marker, link.entity_ref.as_str(), OPERATION)?
        else {
            return Ok(None);
        };
        let Some(locus) = unique_locus(ctx, loci)? else {
            return Ok(None);
        };
        match linked_marker.kind() {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint if point.is_none() => {
                point = Some(locus);
            }
            SketchInputKind::LineOrCircle | SketchInputKind::Arc if entity.is_none() => {
                entity = Some(match locus {
                    SketchLocus::Entity(entity)
                    | SketchLocus::Start(entity)
                    | SketchLocus::End(entity)
                    | SketchLocus::Center(entity) => entity,
                });
            }
            _ => return Ok(None),
        }
    }
    Ok(point.zip(entity))
}

/// The distinct point loci of a relation's geometric operand links, then of its
/// reverse owners, or `None` when one of them has no point locus.
pub(super) fn relation_operand_loci_in(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    markers: &RelationMarkers<'_>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    const OPERATION: &str = "collect SLDPRT marker relation operand loci";
    let (owners, _owners_storage) = ctx.with_scoped_storage(OPERATION, || {
        relation_owner_markers_in(ctx, relation, markers)
    })?;
    let mut loci = Vec::new();
    let add = |loci: &mut Vec<SketchLocus>, marker: &str| -> Result<bool, CodecError> {
        let Some(locus) = marker_point_locus(ctx, marker, markers.by_id(), loci_by_marker)? else {
            return Ok(false);
        };
        if !ctx.contains(loci, &locus, OPERATION)? {
            ctx.push_vec(loci, locus, OPERATION)?;
        }
        Ok(true)
    };
    if !ctx.all_by(
        relation.links(),
        |link| {
            if relation_link_is_geometric_operand(ctx, relation, link, markers.by_id())?
                && !add(&mut loci, &link.entity_ref)?
            {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    if !ctx.all_by(
        &owners,
        |owner| {
            if !add(&mut loci, owner.id())? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    Ok((!loci.is_empty()).then_some(loci))
}

#[cfg(test)]
pub(super) fn relation_operand_loci(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    relation_operand_loci_in(ctx, relation, &markers, loci_by_marker)
}

pub(super) fn linked_single_entities(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchEntityId>>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT single linked entities";
    let mut result: Vec<SketchEntityId> = Vec::new();
    if !ctx.all_by(
        marker.links(),
        |link| {
            if owner_link(ctx, marker, link)? {
                return Ok(true);
            }
            let entities = marker_entities(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                MarkerEntityFilter::All,
            )?;
            let [entity] = entities.as_slice() else {
                return Ok(false);
            };
            if ctx.contains(&result, entity, OPERATION)? {
                return Ok(true);
            }
            if let Some(entity) = entities.into_iter().next() {
                ctx.push_vec(&mut result, entity, OPERATION)?;
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    Ok(Some(result))
}

#[cfg(test)]
pub(super) fn relation_constraint_is_inactive(
    ctx: &DecodeContext<'_>,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<bool, CodecError> {
    let (profile, _profile_storage) = ctx
        .with_scoped_storage("index SLDPRT relation sketch entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    relation_constraint_is_inactive_in(ctx, parameter, definition, &profile)
}

pub(super) fn relation_constraint_is_inactive_in(
    ctx: &DecodeContext<'_>,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    definition: &SketchConstraintDefinitionInput,
    profile: &ProfileEntities<'_>,
) -> Result<bool, CodecError> {
    const ENTITY: &str = "resolve SLDPRT relation activity entity";
    const LOCUS: &str = "resolve SLDPRT profile locus";
    let Some(parameter) = parameter else {
        return Ok(false);
    };
    let entity = |id: &SketchEntityId| profile.entity(ctx, id, ENTITY);
    let locus_point = |locus: &SketchLocus| profile_locus_point_charged(ctx, locus, profile, LOCUS);
    Ok(match definition {
        SketchConstraintDefinitionInput::DistanceLoci { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let measured = if let (Some(first), Some(second)) =
                (locus_point(first)?, locus_point(second)?)
            {
                Some((second.u - first.u).hypot(second.v - first.v))
            } else {
                let point_line =
                    |point: &SketchLocus, line: &SketchLocus| -> Result<Option<f64>, CodecError> {
                        let Some(point) = locus_point(point)? else {
                            return Ok(None);
                        };
                        let SketchLocus::Entity(line) = line else {
                            return Ok(None);
                        };
                        let Some(line) = entity(line)? else {
                            return Ok(None);
                        };
                        Ok(point_line_distance_value(point, line))
                    };
                match point_line(first, second)? {
                    Some(value) => Some(value),
                    None => point_line(second, first)?,
                }
            };
            measured
                .is_some_and(|measured| !same_relation_dimension_length(measured, expected.get()))
        }
        SketchConstraintDefinitionInput::HorizontalDistance { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let (Some(first), Some(second)) = (locus_point(first)?, locus_point(second)?) else {
                return Ok(false);
            };
            !same_relation_dimension_length((second.u - first.u).abs(), expected.get())
        }
        SketchConstraintDefinitionInput::VerticalDistance { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let (Some(first), Some(second)) = (locus_point(first)?, locus_point(second)?) else {
                return Ok(false);
            };
            !same_relation_dimension_length((second.v - first.v).abs(), expected.get())
        }
        SketchConstraintDefinitionInput::Distance { entities, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let [first, second] = entities.as_slice() else {
                return Ok(true);
            };
            let (Some(first), Some(second)) = (entity(first)?, entity(second)?) else {
                return Ok(false);
            };
            line_line_distance(first, second)
                .is_some_and(|measured| !same_relation_dimension_length(measured, expected.get()))
        }
        SketchConstraintDefinitionInput::Angle { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let (Some(first), Some(second)) = (entity(first)?, entity(second)?) else {
                return Ok(false);
            };
            line_line_angle(first, second)
                .is_some_and(|measured| !same_dimension_angle(measured, expected.get()))
        }
        SketchConstraintDefinitionInput::Radius { entity: id, .. }
        | SketchConstraintDefinitionInput::Diameter { entity: id, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let Some(entity) = entity(id)? else {
                return Ok(false);
            };
            let radius = match entity.geometry.definition() {
                SketchGeometryDefinition::Circle { radius, .. }
                | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                _ => return Ok(true),
            };
            let measured = if matches!(definition, SketchConstraintDefinitionInput::Diameter { .. })
            {
                radius * 2.0
            } else {
                radius
            };
            !same_dimension_length(measured, expected.get())
        }
        SketchConstraintDefinitionInput::RepeatedRadius { entities, .. }
        | SketchConstraintDefinitionInput::RepeatedDiameter { entities, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let diameter = matches!(
                definition,
                SketchConstraintDefinitionInput::RepeatedDiameter { .. }
            );
            let mut inactive = false;
            if !ctx.all_by(
                entities,
                |id| {
                    let Some(entity) = entity(id)? else {
                        return Ok(false);
                    };
                    let radius = match *entity.geometry.definition() {
                        SketchGeometryDefinition::Circle { radius, .. }
                        | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                        _ => return Ok(false),
                    };
                    let measured = if diameter { radius * 2.0 } else { radius };
                    inactive |= !same_dimension_length(measured, expected.get());

                    Ok(true)
                },
                "compare SLDPRT repeated-radius activity",
            )? {
                return Ok(false);
            }
            inactive
        }
        _ => false,
    })
}

#[cfg(test)]
pub(super) fn typed_relation_definition(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    typed_relation_definition_with_profile_axis(
        ctx,
        relation,
        parameter,
        SketchRelationEntities {
            sketch,
            sketch_entities,
        },
        markers_by_id,
        loci_by_marker,
        None,
    )
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) struct SketchRelationEntities<'a> {
    pub sketch: &'a SketchId,
    pub sketch_entities: &'a [SketchEntity],
}

#[cfg(test)]
pub(super) fn typed_relation_definition_with_profile_axis(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    profile: SketchRelationEntities<'_>,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    let (entities, _entities_storage) = ctx
        .with_scoped_storage("index SLDPRT relation sketch entities", || {
            ProfileEntities::new(ctx, profile.sketch_entities)
        })?;
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    relation_definition(
        ctx,
        relation,
        parameter,
        profile.sketch,
        RelationIndex {
            entities: &entities,
            markers: &markers,
            loci_by_marker,
        },
        profile_axis,
    )
}

/// The value a cell holds, computed and stored on first use; `copy` makes the
/// value returned to the caller.
fn memoized<T, R>(
    cell: &OnceCell<T>,
    compute: impl FnOnce() -> Result<T, CodecError>,
    copy: impl FnOnce(&T) -> Result<R, CodecError>,
) -> Result<R, CodecError> {
    if let Some(value) = cell.get() {
        return copy(value);
    }
    let value = compute()?;
    copy(cell.get_or_init(|| value))
}

/// The typed definition of a dimensional relation, resolved against the
/// projection's indexes. Horizontal and vertical point distances measure along
/// `profile_axis`, or along the default profile axis when none is given.
pub(super) fn relation_definition(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    const OPERATION: &str = "retain SLDPRT relation parameter identity";
    const POINT: &str = "retain SLDPRT relation point identity";
    const CURVE: &str = "retain SLDPRT relation curve identity";
    const ENTITY: &str = "resolve SLDPRT relation entity";

    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    let Some(parameter) = parameter else {
        return Ok(None);
    };
    let parameter_id = parameter.id.try_clone_for_decode(ctx, OPERATION)?;
    macro_rules! resolved_or_none {
        ($candidate:expr) => {
            match $candidate {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let entities = index.entities;
    let markers_by_id = index.markers_by_id();
    let loci_by_marker = index.loci_by_marker;
    let profile_axis = match relation.family {
        PointPointHorizontalDistance => Some(profile_axis.unwrap_or(ProfileAxis::U)),
        PointPointVerticalDistance => Some(profile_axis.unwrap_or(ProfileAxis::V)),
        _ => None,
    };
    let dynamic = relation_uses_dynamic_operands(relation);
    // Each operand's marker, point and curve are resolved once and reused by
    // every resolver below.
    let marker_cache: [OnceCell<Option<&str>>; 2] = Default::default();
    let marker = |operand: usize| -> Result<Option<&str>, CodecError> {
        let resolve = || relation_operand_marker_in(ctx, relation, operand, sketch, index.markers);
        match marker_cache.get(operand) {
            Some(cell) => memoized(cell, resolve, |marker| Ok(*marker)),
            None => resolve(),
        }
    };
    let resolve_point = |operand: usize| -> Result<Option<SketchLocus>, CodecError> {
        const SCAN: &str = "scan SLDPRT relation point identities";
        let (geometry_ref, _geometry_ref_storage) =
            ctx.format_scoped(format_args!("{}:operand:{operand}", relation.id), SCAN)?;
        if let Some(entity) = entities
            .with_geometry_ref(ctx, &geometry_ref, SCAN)?
            .first()
            .copied()
            .filter(|entity| is_point(entity))
        {
            return role_locus(ctx, SketchLocusRole::Entity, entity.id(), POINT).map(Some);
        }
        let Some(marker) = marker(operand)? else {
            return Ok(None);
        };
        if matches!(
            relation.operands.get(operand).map(|operand| operand.kind),
            Some(FeatureInputOperandKind::Native(
                NativeOperandTag::TAG_837B | NativeOperandTag::TAG_BC7C
            ))
        ) {
            if let Some(locus) = qualified_or_linked_point_locus(
                ctx,
                marker,
                markers_by_id,
                loci_by_marker,
                entities,
            )? {
                return Ok(Some(locus));
            }
        }
        if let Some(entity) = ctx.find_by(
            entities.with_native_ref(ctx, marker, SCAN)?.iter().copied(),
            |entity| Ok(is_point(entity)),
            SCAN,
        )? {
            return role_locus(ctx, SketchLocusRole::Entity, entity.id(), POINT).map(Some);
        }
        if dynamic && dynamic_point_operand(relation, operand) {
            dynamic_marker_point_locus(ctx, marker, sketch, index)
        } else {
            marker_point_locus(ctx, marker, markers_by_id, loci_by_marker)
        }
    };
    let point_cache: [OnceCell<Option<SketchLocus>>; 2] = Default::default();
    let point = |operand: usize| -> Result<Option<SketchLocus>, CodecError> {
        match point_cache.get(operand) {
            Some(cell) => memoized(
                cell,
                || resolve_point(operand),
                |locus| {
                    locus
                        .as_ref()
                        .map(|locus| copy_locus(ctx, locus, POINT))
                        .transpose()
                },
            ),
            None => resolve_point(operand),
        }
    };
    let resolve_curve = |operand: usize| -> Result<Option<SketchEntityId>, CodecError> {
        match solver_line_entity(ctx, relation, operand, sketch, entities)? {
            Some(entity) => Ok(Some(entity)),
            None => match marker(operand)? {
                Some(marker) => single_marker_line_entity_in(
                    ctx,
                    marker,
                    markers_by_id,
                    loci_by_marker,
                    entities,
                ),
                None => Ok(None),
            },
        }
    };
    let curve_cache: [OnceCell<Option<SketchEntityId>>; 2] = Default::default();
    let curve = |operand: usize| -> Result<Option<SketchEntityId>, CodecError> {
        match curve_cache.get(operand) {
            Some(cell) => memoized(
                cell,
                || resolve_curve(operand),
                |entity| {
                    entity
                        .as_ref()
                        .map(|entity| entity.try_clone_for_decode(ctx, CURVE))
                        .transpose()
                },
            ),
            None => resolve_curve(operand),
        }
    };
    if relation_uses_solver_points(relation) && (point(0)?.is_none() || point(1)?.is_none()) {
        return Ok(None);
    }
    let dynamic_point_pair = if dynamic {
        match relation.family {
            PointPointDistance | PointPointHorizontalDistance | PointPointVerticalDistance => {
                unique_dynamic_marker_point_pair(
                    ctx,
                    relation,
                    sketch,
                    parameter,
                    (point(0)?, point(1)?),
                    index,
                    profile_axis,
                )?
            }
            _ => None,
        }
    } else {
        None
    };
    let dynamic_direct_point_pair = if dynamic
        && matches!(
            relation.family,
            PointPointDistance | PointPointHorizontalDistance | PointPointVerticalDistance
        )
        && dynamic_point_pair.is_none()
    {
        unique_dynamic_direct_point_roster_pair(
            ctx,
            relation,
            sketch,
            parameter,
            index,
            profile_axis,
        )?
    } else {
        None
    };
    // A family-scoped native tag is not a locus identity. If marker lookup
    // finds no resolved center carrier, the complete profile roster is the
    // defined witness; an ambiguous arc-center carrier must remain unresolved
    // instead of being bypassed by a coincidental whole-sketch distance.
    let mut roster_has_no_center = dynamic
        && dynamic_point_pair.is_none()
        && dynamic_direct_point_pair.is_none()
        && ctx.all_by(
            &relation.operands,
            |operand| Ok(operand.entity_ref.is_none()),
            "scan SLDPRT roster relation operands",
        )?;
    if roster_has_no_center {
        for operand in 0..2 {
            if let Some(marker) = marker(operand)? {
                if dynamic_marker_center_candidates(ctx, marker, sketch, index)?.is_some() {
                    roster_has_no_center = false;
                    break;
                }
            }
        }
    }
    let dynamic_roster_point_pair = if roster_has_no_center {
        match relation.family {
            PointPointDistance => {
                unique_profile_distance_loci_pair(ctx, sketch, parameter, entities)?
            }
            PointPointHorizontalDistance | PointPointVerticalDistance => {
                unique_profile_axis_distance_pair(
                    ctx,
                    sketch,
                    parameter,
                    entities,
                    resolved_or_none!(profile_axis),
                )?
            }
            _ => None,
        }
    } else {
        None
    };
    let dynamic_point_line_pair = if dynamic && relation.family == PointLineDistance {
        let known_point = point(0)?;
        let selected = if let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
            parameter.value.as_ref()
        {
            let point_candidates = match known_point {
                Some(point) => {
                    let mut candidates = Vec::new();
                    ctx.push_vec(
                        &mut candidates,
                        point,
                        "collect SLDPRT dynamic point candidates",
                    )?;
                    candidates
                }
                None => match marker(0)? {
                    Some(marker) => dynamic_marker_point_candidates(ctx, marker, sketch, index)?,
                    None => Vec::new(),
                },
            };
            let line_candidates = dynamic_line_operand_candidates(ctx, relation, 1, sketch, index)?;
            unique_point_line_candidate_pair(
                ctx,
                *expected,
                &point_candidates,
                &line_candidates,
                entities,
            )?
        } else {
            None
        };
        match selected {
            Some(pair) => Some(pair),
            None => unique_dynamic_roster_point_line_pair(
                ctx,
                relation,
                sketch,
                parameter,
                point(0)?,
                curve(1)?,
                entities,
            )?,
        }
    } else {
        None
    };
    let dynamic_line_distance_pair = if dynamic && relation.family == LineLineDistance {
        match unique_dynamic_marker_line_distance_pair(ctx, relation, sketch, parameter, index)? {
            Some(pair) => Some(pair),
            None => {
                if let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                    parameter.value.as_ref()
                {
                    let unique_partner = |known: &SketchEntityId| {
                        unique_profile_matched_entity(
                            ctx,
                            sketch,
                            known,
                            entities,
                            |known, candidate| {
                                line_line_distance(known, candidate).is_some_and(|measured| {
                                    same_relation_dimension_length(measured, expected.get())
                                })
                            },
                        )
                    };
                    match (curve(0)?, curve(1)?) {
                        (Some(first), None) => {
                            unique_partner(&first)?.map(|partner| (first, partner))
                        }
                        (None, Some(second)) => {
                            unique_partner(&second)?.map(|partner| (partner, second))
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
        }
    } else {
        None
    };
    // Dynamic line operands carry a family tag, not a stable line identity.
    // When marker and solver-line joins do not produce a pair, the complete
    // owner sketch is the remaining semantic scope. Accept it only when the
    // stored operands contain no explicit identity and exactly one pair meets
    // the native distance.
    let dynamic_roster_line_distance_pair =
        if dynamic && relation.family == LineLineDistance && dynamic_line_distance_pair.is_none() {
            unique_dynamic_roster_line_distance_pair(ctx, relation, sketch, parameter, entities)?
        } else {
            None
        };
    let dynamic_angle_pair = if dynamic && relation.family == Angle {
        match unique_dynamic_marker_line_angle_pair(ctx, relation, sketch, parameter, index)? {
            Some(pair) => Some(pair),
            None => match (curve(0)?, curve(1)?) {
                (Some(first), None) => unique_dynamic_profile_line_angle_entity(
                    ctx, sketch, &first, parameter, entities,
                )?
                .map(|partner| (first, partner)),
                (None, Some(second)) => unique_dynamic_profile_line_angle_entity(
                    ctx, sketch, &second, parameter, entities,
                )?
                .map(|partner| (partner, second)),
                _ => None,
            },
        }
    } else {
        None
    };
    // A family-scoped angular relation may carry only an unresolved relation
    // handle or an address that does not materialize a line entity. In that
    // case the complete owning sketch is the semantic scope; accept the pair
    // only when exactly one unordered line pair has the stored unoriented
    // angle. A resolved line remains authoritative and does not enter this
    // unrelated-pair fallback.
    let dynamic_roster_angle_pair = if dynamic
        && relation.family == Angle
        && dynamic_angle_pair.is_none()
        && curve(0)?.is_none()
        && curve(1)?.is_none()
    {
        unique_dynamic_roster_line_angle_pair(ctx, sketch, parameter, entities)?
    } else {
        None
    };
    if dynamic {
        let witnessed = match relation.family {
            PointPointDistance | PointPointHorizontalDistance | PointPointVerticalDistance => {
                dynamic_point_pair.is_some()
                    || dynamic_direct_point_pair.is_some()
                    || dynamic_roster_point_pair.is_some()
                    || (point(0)?.is_some() && point(1)?.is_some())
            }
            PointLineDistance => {
                dynamic_point_line_pair.is_some() || (point(0)?.is_some() && curve(1)?.is_some())
            }
            LineLineDistance => {
                dynamic_line_distance_pair.is_some()
                    || dynamic_roster_line_distance_pair.is_some()
                    || (curve(0)?.is_some() && curve(1)?.is_some())
            }
            Angle => {
                dynamic_angle_pair.is_some()
                    || dynamic_roster_angle_pair.is_some()
                    || (curve(0)?.is_some() && curve(1)?.is_some())
            }
            CircleDiameter => true,
        };
        if !witnessed {
            return Ok(None);
        }
    }
    let locus_point = |locus: &SketchLocus| {
        profile_locus_point_charged(ctx, locus, entities, "resolve SLDPRT profile locus")
    };
    Ok(match relation.family {
        PointPointDistance => {
            let first = point(0)?;
            let second = point(1)?;
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match dynamic_point_pair
                .or(dynamic_direct_point_pair)
                .or(dynamic_roster_point_pair)
            {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => {
                        match doubled_profile_distance_loci_in(
                            ctx,
                            relation,
                            (0, 1),
                            sketch,
                            parameter,
                            index,
                        )? {
                            Some(pair) => pair,
                            None => {
                                let partner = resolved_or_none!(unique_profile_distance_locus_in(
                                    ctx, sketch, &known, parameter, entities
                                )?);
                                (known, partner)
                            }
                        }
                    }
                    (None, Some(known)) => {
                        match doubled_profile_distance_loci_in(
                            ctx,
                            relation,
                            (1, 0),
                            sketch,
                            parameter,
                            index,
                        )? {
                            Some(pair) => pair,
                            None => {
                                let partner = resolved_or_none!(unique_profile_distance_locus_in(
                                    ctx, sketch, &known, parameter, entities
                                )?);
                                (partner, known)
                            }
                        }
                    }
                    (None, None) => {
                        resolved_or_none!(unique_profile_distance_loci_pair(
                            ctx, sketch, parameter, entities
                        )?)
                    }
                },
            };
            if ctx.equal(&first, &second, ENTITY)? {
                return Ok(None);
            }
            if !entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    resolved_or_none!(parameter.value.as_ref())
                else {
                    return Ok(None);
                };
                let first_point = resolved_or_none!(locus_point(&first)?);
                let second_point = resolved_or_none!(locus_point(&second)?);
                if !same_relation_dimension_length(
                    (second_point.u - first_point.u).hypot(second_point.v - first_point.v),
                    expected.get(),
                ) {
                    if dynamic {
                        return Ok(None);
                    }
                    let horizontal = same_dimension_length(
                        (second_point.u - first_point.u).abs(),
                        expected.get(),
                    );
                    let vertical = same_dimension_length(
                        (second_point.v - first_point.v).abs(),
                        expected.get(),
                    );
                    let projected_distance_operands = ctx.all_by(
                        &relation.operands,
                        |operand| {
                            Ok(operand.kind
                                == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C))
                        },
                        "scan SLDPRT projected distance operands",
                    )?;
                    if projected_distance_operands && horizontal != vertical {
                        return Ok(Some(if horizontal {
                            SketchConstraintDefinitionInput::HorizontalDistance {
                                first,
                                second,
                                parameter: parameter_id,
                            }
                        } else {
                            SketchConstraintDefinitionInput::VerticalDistance {
                                first,
                                second,
                                parameter: parameter_id,
                            }
                        }));
                    }
                    if authoritative {
                        return Ok(Some(SketchConstraintDefinitionInput::DistanceLoci {
                            first,
                            second,
                            parameter: parameter_id,
                        }));
                    }
                    (first, second) =
                        resolved_or_none!(unique_repaired_profile_distance_loci_pair(
                            ctx, sketch, &first, &second, parameter, entities,
                        )?);
                }
            }
            Some(SketchConstraintDefinitionInput::DistanceLoci {
                first,
                second,
                parameter: parameter_id,
            })
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            let axis = resolved_or_none!(profile_axis);
            let first = point(0)?;
            let second = point(1)?;
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match dynamic_point_pair
                .or(dynamic_direct_point_pair)
                .or(dynamic_roster_point_pair)
            {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => {
                        let partner = resolved_or_none!(unique_profile_axis_distance_locus(
                            ctx, sketch, &known, parameter, entities, axis,
                        )?);
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        resolved_or_none!(unique_profile_axis_distance_locus(
                            ctx, sketch, &known, parameter, entities, axis,
                        )?),
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_axis_distance_pair(
                            ctx, sketch, parameter, entities, axis
                        )?)
                    }
                },
            };
            if ctx.equal(&first, &second, ENTITY)? {
                return Ok(None);
            }
            if !entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    resolved_or_none!(parameter.value.as_ref())
                else {
                    return Ok(None);
                };
                let first_point = resolved_or_none!(locus_point(&first)?);
                let second_point = resolved_or_none!(locus_point(&second)?);
                let measured = if axis == ProfileAxis::U {
                    (second_point.u - first_point.u).abs()
                } else {
                    (second_point.v - first_point.v).abs()
                };
                if !same_relation_dimension_length(measured, expected.get()) {
                    if dynamic {
                        return Ok(None);
                    }
                    if !authoritative {
                        (first, second) =
                            resolved_or_none!(unique_repaired_profile_axis_distance_pair(
                                ctx, sketch, &first, &second, parameter, entities, axis,
                            )?);
                    }
                }
            }
            Some(if axis == ProfileAxis::U {
                SketchConstraintDefinitionInput::HorizontalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                }
            } else {
                SketchConstraintDefinitionInput::VerticalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                }
            })
        }
        PointLineDistance => {
            let point = point(0)?;
            let line = curve(1)?;
            let authoritative = point.is_some() && line.is_some();
            let (mut point, mut line) = match dynamic_point_line_pair {
                Some(pair) => pair,
                None => match (point, line) {
                    (Some(point), Some(line)) => (point, line),
                    (Some(point), None) => {
                        let partner = resolved_or_none!(unique_profile_point_line_entity(
                            ctx, sketch, &point, parameter, entities,
                        )?);
                        (point, partner)
                    }
                    (None, Some(line)) => (
                        resolved_or_none!(unique_profile_line_point_locus(
                            ctx, sketch, &line, parameter, entities
                        )?),
                        line,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_point_line_pair(
                            ctx, sketch, parameter, entities
                        )?)
                    }
                },
            };
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let point_position = resolved_or_none!(locus_point(&point)?);
            let line_entity = resolved_or_none!(entities.entity(ctx, &line, ENTITY)?);
            if !point_line_distance_value(point_position, line_entity)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
            {
                if dynamic {
                    return Ok(None);
                }
                if !authoritative {
                    (point, line) = resolved_or_none!(unique_repaired_profile_point_line_pair(
                        ctx, sketch, &point, &line, parameter, entities,
                    )?);
                }
            }
            Some(SketchConstraintDefinitionInput::DistanceLoci {
                first: point,
                second: SketchLocus::Entity(line),
                parameter: parameter_id,
            })
        }
        LineLineDistance => {
            let operand_marker = |operand: usize| -> Result<Option<&str>, CodecError> {
                match marker(operand)? {
                    Some(marker) => Ok(Some(marker)),
                    None => Ok(
                        relation_line_point_marker(ctx, relation, operand, index.markers)?
                            .map(SketchInputEntity::id),
                    ),
                }
            };
            let line_of = |operand: usize| -> Result<Option<SketchEntityId>, CodecError> {
                match curve(operand)? {
                    Some(entity) => Ok(Some(entity)),
                    None => match operand_marker(operand)? {
                        Some(marker) => single_marker_line_entity_in(
                            ctx,
                            marker,
                            markers_by_id,
                            loci_by_marker,
                            entities,
                        ),
                        None => Ok(None),
                    },
                }
            };
            let first = line_of(0)?;
            let second = line_of(1)?;
            let authoritative = match (&first, &second) {
                (Some(first), Some(second)) => !ctx.equal(first, second, ENTITY)?,
                _ => false,
            };
            let partner_of = |operand: usize,
                              known: &SketchEntityId|
             -> Result<Option<SketchEntityId>, CodecError> {
                match relation_line_point_marker(ctx, relation, operand, index.markers)? {
                    Some(marker) => unique_marker_line_distance_entity(
                        ctx,
                        marker.id(),
                        sketch,
                        known,
                        parameter,
                        index,
                    ),
                    None => {
                        unique_profile_line_distance_entity(ctx, sketch, known, parameter, entities)
                    }
                }
            };
            let (mut first, mut second) =
                match dynamic_line_distance_pair.or(dynamic_roster_line_distance_pair) {
                    Some(pair) => pair,
                    None => match (first, second) {
                        (Some(first), Some(second)) => (first, second),
                        (Some(known), None) => {
                            let partner = resolved_or_none!(partner_of(1, &known)?);
                            (known, partner)
                        }
                        (None, Some(known)) => (resolved_or_none!(partner_of(0, &known)?), known),
                        (None, None) => {
                            resolved_or_none!(unique_profile_line_distance_pair(
                                ctx, sketch, parameter, entities
                            )?)
                        }
                    },
                };
            if ctx.equal(&first, &second, ENTITY)? {
                let [first_operand, second_operand] = relation.operands.as_slice() else {
                    return Ok(None);
                };
                if first_operand.entity_index == second_operand.entity_index {
                    return Ok(None);
                }
                second = resolved_or_none!(unique_profile_line_distance_entity(
                    ctx, sketch, &first, parameter, entities,
                )?);
            }
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let first_line = resolved_or_none!(entities.entity(ctx, &first, ENTITY)?);
            let second_line = resolved_or_none!(entities.entity(ctx, &second, ENTITY)?);
            if !line_line_distance(first_line, second_line)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
            {
                if dynamic {
                    return Ok(None);
                }
                if !authoritative {
                    (first, second) =
                        resolved_or_none!(unique_repaired_profile_line_distance_pair(
                            ctx, sketch, &first, &second, parameter, entities,
                        )?);
                }
            }
            Some(SketchConstraintDefinitionInput::Distance {
                entities: vec![first, second],
                parameter: parameter_id,
            })
        }
        Angle => {
            let first = curve(0)?;
            let second = curve(1)?;
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match dynamic_angle_pair.or(dynamic_roster_angle_pair) {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => {
                        let partner = resolved_or_none!(unique_profile_line_angle_entity(
                            ctx, sketch, &known, parameter, entities,
                        )?);
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        resolved_or_none!(unique_profile_line_angle_entity(
                            ctx, sketch, &known, parameter, entities,
                        )?),
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_line_angle_pair(
                            ctx, sketch, parameter, entities
                        )?)
                    }
                },
            };
            if ctx.equal(&first, &second, ENTITY)? {
                return Ok(None);
            }
            let cadmpeg_ir::features::ParameterValue::Angle(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let first_line = resolved_or_none!(entities.entity(ctx, &first, ENTITY)?);
            let second_line = resolved_or_none!(entities.entity(ctx, &second, ENTITY)?);
            let angle = if dynamic {
                unoriented_line_line_angle(first_line, second_line)
            } else {
                line_line_angle(first_line, second_line)
            };
            if !angle.is_some_and(|measured| same_dimension_angle(measured, expected.get())) {
                if dynamic {
                    return Ok(None);
                }
                if !authoritative {
                    (first, second) = resolved_or_none!(unique_repaired_profile_line_angle_pair(
                        ctx, sketch, &first, &second, parameter, entities,
                    )?);
                }
            }
            Some(SketchConstraintDefinitionInput::Angle {
                first,
                second,
                parameter: parameter_id,
            })
        }
        CircleDiameter => {
            const SCAN: &str = "scan SLDPRT dimensional circle identities";

            if let Some(circles) =
                repeated_dimensioned_circular_entities(ctx, relation, parameter, sketch, entities)?
            {
                return Ok(Some(match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                        SketchConstraintDefinitionInput::RepeatedRadius {
                            entities: circles,
                            parameter: parameter_id,
                        }
                    }
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                        SketchConstraintDefinitionInput::RepeatedDiameter {
                            entities: circles,
                            parameter: parameter_id,
                        }
                    }
                    None => return Ok(None),
                }));
            }
            let resolved_entity = if let Some(entity) = ctx.find_by(
                entities
                    .with_geometry_ref(ctx, &relation.id, SCAN)?
                    .iter()
                    .copied(),
                |entity| Ok(in_sketch(ctx, entity, sketch, SCAN)? && is_circular(entity)),
                SCAN,
            )? {
                Some(
                    entity
                        .id()
                        .try_clone_for_decode(ctx, "retain SLDPRT dimensional circle identity")?,
                )
            } else if let Some(marker) = marker(0)? {
                match marker_center_dimensioned_entity(ctx, marker, sketch, entities, parameter)? {
                    Some(entity) => Some(entity),
                    None => {
                        if entities.is_empty() {
                            single_marker_entity(ctx, marker, markers_by_id, loci_by_marker)?
                        } else {
                            single_marker_circular_entity(
                                ctx,
                                marker,
                                markers_by_id,
                                loci_by_marker,
                                entities,
                            )?
                        }
                    }
                }
            } else {
                None
            };
            let authoritative = resolved_entity.is_some();
            let entity = resolved_or_none!(match resolved_entity {
                Some(entity) => Some(entity),
                None => unique_dimensioned_circle_entity(ctx, sketch, entities, parameter)?,
            });
            if !entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    resolved_or_none!(parameter.value.as_ref())
                else {
                    return Ok(None);
                };
                let geometry = &resolved_or_none!(entities.entity(ctx, &entity, ENTITY)?).geometry;
                let radius = match geometry.definition() {
                    SketchGeometryDefinition::Circle { radius, .. }
                    | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                    _ => return Ok(None),
                };
                let expected_radius = match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => expected.get(),
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => expected.get() * 0.5,
                    None => return Ok(None),
                };
                if !same_dimension_length(radius, expected_radius) && !authoritative {
                    return Ok(None);
                }
            }
            match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                    Some(SketchConstraintDefinitionInput::Radius {
                        entity,
                        parameter: parameter_id,
                    })
                }
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                    Some(SketchConstraintDefinitionInput::Diameter {
                        entity,
                        parameter: parameter_id,
                    })
                }
                None => None,
            }
        }
    })
}

fn solver_line_entity(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operand_index: usize,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT solver line identity";

    let Some(operand) = relation.operands.get(operand_index) else {
        return Ok(None);
    };
    if !relation_uses_solver_line_operand(relation, operand_index) {
        return Ok(None);
    }
    let sketch_line =
        |entity: &SketchEntity| Ok(in_sketch(ctx, entity, sketch, OPERATION)? && is_line(entity));
    if let Some(entity_ref) = operand.entity_ref.as_deref() {
        let candidates = entities.with_native_ref(ctx, entity_ref, OPERATION)?;
        let mut selected = None;
        if !ctx.all_by(
            candidates.iter().copied(),
            |entity| {
                if !sketch_line(entity)? {
                    return Ok(true);
                }
                if selected.is_some() {
                    return Ok(false);
                }
                selected = Some(entity);

                Ok(true)
            },
            OPERATION,
        )? {
            return Ok(None);
        }
        if let Some(entity) = selected {
            return entity.id().try_clone_for_decode(ctx, OPERATION).map(Some);
        }
        if !relation_uses_dynamic_operands(relation) {
            return Ok(None);
        }
    }
    let (geometry_ref, _geometry_ref_storage) = ctx.format_scoped(
        format_args!(
            "{}:solver-line:{}",
            relation.feature_ref, operand.entity_index
        ),
        OPERATION,
    )?;
    let candidates = entities.with_geometry_ref(ctx, &geometry_ref, OPERATION)?;
    unique_entity(ctx, candidates, OPERATION, sketch_line)?
        .map(|entity| entity.id().try_clone_for_decode(ctx, OPERATION))
        .transpose()
}

fn repeated_dimensioned_circular_entities(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
) -> Result<Option<Vec<SketchEntityId>>, CodecError> {
    const OPERATION: &str = "select SLDPRT repeated dimensioned circles";
    // Display-only scalar runs use the owner sketch's radius population.
    let repeated_display = relation.parameter_scalar_ref().is_none()
        && relation.scalar_refs().len() >= 2
        && relation.operands.len() == 1
        && parameter.native_ref.is_none()
        && super::relation_geometry::is_reference_relation_parameter_in(ctx, parameter)?
        && match ctx.get_btree_map(
            &parameter.properties,
            super::relation_geometry::RELATION_PARAMETER_ID_PROPERTY,
            OPERATION,
        )? {
            Some(relation_id) => {
                ctx.equal(relation_id.as_str(), relation.id.as_str(), OPERATION)?
            }
            None => false,
        };
    let parameter_native_ref = parameter.native_ref.as_deref();
    if !repeated_display
        && !ctx.equal(
            &relation.parameter_scalar_ref(),
            &parameter_native_ref,
            OPERATION,
        )?
    {
        return Ok(None);
    }
    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    if !(expected_radius.is_finite() && expected_radius > 0.0) {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut matched = Vec::new();
    for entity in ctx
        .admit_iter(entities.in_sketch(ctx, sketch, OPERATION)?, OPERATION)?
        .copied()
    {
        if !repeated_display
            && !ctx.equal(
                &entity.geometry_ref.as_deref(),
                &parameter_native_ref,
                OPERATION,
            )?
        {
            continue;
        }
        let radius = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { radius, .. }
            | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
            _ => continue,
        };
        if same_dimension_length(radius, expected_radius) {
            storage.with_storage(|| ctx.push_vec(&mut matched, entity, OPERATION))?;
        }
    }
    if matched.len() < 2 || matched.len() > relation.scalar_refs().len() {
        return Ok(None);
    }
    let mut circles = Vec::new();
    for entity in ctx.admit_iter(&matched, OPERATION)? {
        ctx.push_vec(
            &mut circles,
            entity.id().try_clone_for_decode(ctx, OPERATION)?,
            OPERATION,
        )?;
    }
    Ok(Some(circles))
}

// Find the unique profile locus pair whose spanned dimension, measured by
// `measure` over the two loci points, equals the parameter length. Every
// unordered pair of canonical profile loci is a candidate; `measure` is the
// only axis of variation between the distance and axis-distance resolvers.
fn unique_profile_measured_loci_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    const OPERATION: &str = "select SLDPRT measured profile locus pair";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let (loci, _loci_storage) =
        ctx.with_scoped_storage(OPERATION, || canonical_profile_loci(ctx, sketch, entities))?;
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    if !ctx.all_by(
        loci.iter().enumerate(),
        |(first_index, (first_point, first))| {
            let later = loci.get(first_index + 1..).unwrap_or_default();
            if !ctx.all_by(
                later,
                |(second_point, second)| {
                    if !same_dimension_length(measure(first_point, second_point), distance.get()) {
                        return Ok(true);
                    }
                    if let Some((selected_first, selected_second)) = selected {
                        if !(ctx.equal(selected_first, first, OPERATION)?
                            && ctx.equal(selected_second, second, OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some((first, second));

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        copy_locus(ctx, first, OPERATION)?,
        copy_locus(ctx, second, OPERATION)?,
    )))
}

// Repair a candidate pair by resolving each supplied locus to its unique
// partner via `partner`, forming the sorted pair, and keeping it only when the
// two starting loci agree on exactly one pair.
fn unique_repaired_profile_pair(
    ctx: &DecodeContext<'_>,
    first: &SketchLocus,
    second: &SketchLocus,
    partner: impl Fn(&SketchLocus) -> Result<Option<SketchLocus>, CodecError>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    const OPERATION: &str = "repair SLDPRT measured profile locus pair";
    let mut selected: Option<(SketchLocus, SketchLocus)> = None;
    for known in [first, second] {
        let Some(partner) = partner(known)? else {
            continue;
        };
        let known = copy_locus(ctx, known, OPERATION)?;
        let mut pair = [known, partner];
        ctx.sort_unstable_by(
            &mut pair,
            |value| value,
            |left, right| locus_key(left).cmp(&locus_key(right)),
            OPERATION,
        )?;
        let [first, second] = pair;
        if let Some((selected_first, selected_second)) = &selected {
            if !(ctx.equal(selected_first, &first, OPERATION)?
                && ctx.equal(selected_second, &second, OPERATION)?)
            {
                return Ok(None);
            }
        }
        selected = Some((first, second));
    }
    Ok(selected)
}

// Find the unique profile locus at a given dimension from `known`, where
// `measure` reports the dimension between the known point and a candidate
// point. The known locus is excluded and, as for the pair resolvers, `measure`
// is the sole axis of variation between the straight-distance and axis-distance
// forms.
fn unique_profile_measured_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Result<Option<SketchLocus>, CodecError> {
    const OPERATION: &str = "select SLDPRT measured profile locus";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(known_point) = profile_locus_point_charged(ctx, known, entities, OPERATION)? else {
        return Ok(None);
    };
    let (loci, _loci_storage) =
        ctx.with_scoped_storage(OPERATION, || canonical_profile_loci(ctx, sketch, entities))?;
    let mut selected: Option<&SketchLocus> = None;
    if !ctx.all_by(
        &loci,
        |(candidate_point, candidate)| {
            if ctx.equal(candidate, known, OPERATION)?
                || !same_dimension_length(measure(&known_point, candidate_point), distance.get())
            {
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
    selected
        .map(|locus| copy_locus(ctx, locus, OPERATION))
        .transpose()
}

#[cfg(test)]
pub(super) fn unique_profile_distance_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, CodecError> {
    let (entities, _entities_storage) = ctx
        .with_scoped_storage("index SLDPRT relation sketch entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    unique_profile_distance_locus_in(ctx, sketch, known, parameter, &entities)
}

fn unique_profile_distance_locus_in(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchLocus>, CodecError> {
    unique_profile_measured_locus(
        ctx,
        sketch,
        known,
        parameter,
        entities,
        |known, candidate| (candidate.u - known.u).hypot(candidate.v - known.v),
    )
}

#[cfg(test)]
pub(super) fn doubled_profile_distance_loci(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operands: (usize, usize),
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    let (entities, _entities_storage) = ctx
        .with_scoped_storage("index SLDPRT relation sketch entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    let loci_by_marker = HashMap::new();
    doubled_profile_distance_loci_in(
        ctx,
        relation,
        operands,
        sketch,
        parameter,
        RelationIndex {
            entities: &entities,
            markers: &markers,
            loci_by_marker: &loci_by_marker,
        },
    )
}

fn doubled_profile_distance_loci_in(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operands: (usize, usize),
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    index: RelationIndex<'_, '_>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const OPERATION: &str = "resolve SLDPRT doubled profile distance";
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(line_marker_id) =
        relation_operand_marker_in(ctx, relation, operands.0, sketch, index.markers)?
    else {
        return Ok(None);
    };
    let Some(center_marker_id) =
        relation_operand_marker_in(ctx, relation, operands.1, sketch, index.markers)?
    else {
        return Ok(None);
    };
    let Some(line_marker) = index.markers.get(ctx, line_marker_id, OPERATION)? else {
        return Ok(None);
    };
    let Some(center_marker) = index.markers.get(ctx, center_marker_id, OPERATION)? else {
        return Ok(None);
    };
    let feature = Some(relation.feature_ref.as_str());
    if !ctx.equal(&line_marker.feature_ref.as_deref(), &feature, OPERATION)?
        || !ctx.equal(&center_marker.feature_ref.as_deref(), &feature, OPERATION)?
    {
        return Ok(None);
    }
    let center_is_distance_handle = ctx.any_by(
        index
            .markers
            .of_feature(ctx, &relation.feature_ref, OPERATION)?
            .iter()
            .copied(),
        |marker| {
            if marker.kind()
                != SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
            {
                return Ok(false);
            }
            let [link] = marker.links() else {
                return Ok(false);
            };
            ctx.equal(link.entity_ref.as_str(), center_marker.id(), OPERATION)
        },
        OPERATION,
    )?;
    if !center_is_distance_handle {
        return Ok(None);
    }
    let Some(line_coordinates) = line_marker.coordinates_m else {
        return Ok(None);
    };
    let Some(center_coordinates) = center_marker.coordinates_m else {
        return Ok(None);
    };
    let [line_u, line_v] = line_coordinates.get();
    let [center_u, center_v] = center_coordinates.get();
    if ![
        (center_u - line_u).abs() * NATIVE_TO_IR * 2.0,
        (center_v - line_v).abs() * NATIVE_TO_IR * 2.0,
    ]
    .into_iter()
    .any(|distance| same_dimension_length(distance, expected.get()))
    {
        return Ok(None);
    }
    let candidates = index
        .entities
        .with_native_ref(ctx, line_marker_id, OPERATION)?;
    let Some(entity) = unique_entity(ctx, candidates, OPERATION, |entity| {
        if !in_sketch(ctx, entity, sketch, OPERATION)? {
            return Ok(false);
        }
        let SketchGeometryDefinition::Line { start, end } = *entity.geometry.definition() else {
            return Ok(false);
        };
        Ok(same_dimension_length(
            (end.u - start.u).hypot(end.v - start.v),
            expected.get(),
        ))
    })?
    else {
        return Ok(None);
    };
    Ok(Some((
        role_locus(ctx, SketchLocusRole::Start, entity.id(), OPERATION)?,
        role_locus(ctx, SketchLocusRole::End, entity.id(), OPERATION)?,
    )))
}

fn unique_repaired_profile_distance_loci_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    unique_repaired_profile_pair(ctx, first, second, |known| {
        unique_profile_distance_locus_in(ctx, sketch, known, parameter, entities)
    })
}

fn unique_profile_axis_distance_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
    axis: ProfileAxis,
) -> Result<Option<SketchLocus>, CodecError> {
    unique_profile_measured_locus(
        ctx,
        sketch,
        known,
        parameter,
        entities,
        |known, candidate| {
            if axis == ProfileAxis::U {
                (candidate.u - known.u).abs()
            } else {
                (candidate.v - known.v).abs()
            }
        },
    )
}

fn unique_repaired_profile_axis_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
    axis: ProfileAxis,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    unique_repaired_profile_pair(ctx, first, second, |known| {
        unique_profile_axis_distance_locus(ctx, sketch, known, parameter, entities, axis)
    })
}

fn unique_profile_axis_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
    axis: ProfileAxis,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    unique_profile_measured_loci_pair(ctx, sketch, parameter, entities, |first, second| {
        if axis == ProfileAxis::U {
            (second.u - first.u).abs()
        } else {
            (second.v - first.v).abs()
        }
    })
}

fn unique_profile_distance_loci_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    unique_profile_measured_loci_pair(ctx, sketch, parameter, entities, |first, second| {
        (second.u - first.u).hypot(second.v - first.v)
    })
}

/// The loci of the sketch's entities, one per quantized position, ordered by
/// position and then by locus identity.
pub(super) fn canonical_profile_loci(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
) -> Result<Vec<(Point2, SketchLocus)>, CodecError> {
    const QUANTUM: f64 = 1e-8;
    const OPERATION: &str = "collect SLDPRT canonical profile loci";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut indexed = Vec::new();
    for (source_index, entity) in ctx
        .admit_iter(entities.in_sketch(ctx, sketch, OPERATION)?, OPERATION)?
        .copied()
        .enumerate()
    {
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            let locus = role_locus(ctx, role, entity.id(), OPERATION)?;
            storage.with_storage(|| {
                ctx.push_vec(&mut indexed, (source_index, point, locus), OPERATION)
            })?;
        }
    }
    // Source order breaks equal geometric and identity keys without sort scratch.
    ctx.sort_unstable_by(
        &mut indexed,
        |value| value,
        |(left_index, left_point, left_locus), (right_index, right_point, right_locus)| {
            quantize(*left_point, QUANTUM)
                .cmp(&quantize(*right_point, QUANTUM))
                .then_with(|| locus_key(left_locus).cmp(&locus_key(right_locus)))
                .then_with(|| left_index.cmp(right_index))
        },
        OPERATION,
    )?;
    ctx.dedup_by(
        &mut indexed,
        |(_, left_point, _), (_, right_point, _)| {
            Ok(quantize(*left_point, QUANTUM) == quantize(*right_point, QUANTUM))
        },
        "deduplicate SLDPRT canonical profile loci",
    )?;
    let mut loci = Vec::new();
    for (_, point, locus) in ctx.admit_iter(indexed, OPERATION)? {
        ctx.push_vec(&mut loci, (point, locus), OPERATION)?;
    }
    Ok(loci)
}

// Find the unique sketch entity, other than `known`, for which `matches`
// accepts the ordered pair `(known, candidate)`. The parameter kind guard and
// the measurement both live in `matches`, so the straight-distance and angle
// resolvers differ only in the closure they pass.
fn unique_profile_matched_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    entities: &ProfileEntities<'_>,
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT matched sketch entity";
    let Some(known) = entities.entity(ctx, known, OPERATION)? else {
        return Ok(None);
    };
    let mut selected: Option<&SketchEntityId> = None;
    if !ctx.all_by(
        entities.in_sketch(ctx, sketch, OPERATION)?.iter().copied(),
        |entity| {
            if ctx.equal(entity.id(), known.id(), OPERATION)? || !matches(known, entity) {
                return Ok(true);
            }
            if let Some(selected) = selected {
                if !ctx.equal(selected, entity.id(), OPERATION)? {
                    return Ok(false);
                }
            }
            selected = Some(entity.id());

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    selected
        .map(|entity| entity.try_clone_for_decode(ctx, OPERATION))
        .transpose()
}

// Select one distinct ordered identity pair from unordered line positions.
// Only line entities participate; `matches` supplies the measurement and comparison.
fn unique_profile_matched_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT matched sketch line pair";
    let roster = entities.in_sketch(ctx, sketch, OPERATION)?;
    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
    if !ctx.all_by(
        roster.iter().copied().enumerate(),
        |(first_index, first)| {
            if !is_line(first) {
                return Ok(true);
            }
            let later = roster.get(first_index + 1..).unwrap_or_default();
            if !ctx.all_by(
                later.iter().copied(),
                |second| {
                    if !is_line(second) || !matches(first, second) {
                        return Ok(true);
                    }
                    if let Some((selected_first, selected_second)) = selected {
                        if !(ctx.equal(selected_first, first.id(), OPERATION)?
                            && ctx.equal(selected_second, second.id(), OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some((first.id(), second.id()));

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        first.try_clone_for_decode(ctx, OPERATION)?,
        second.try_clone_for_decode(ctx, OPERATION)?,
    )))
}

// Resolve each supplied entity to its unique partner and sort its two identities.
// Keep one distinct pair across the two resolutions.
fn unique_repaired_entity_pair(
    ctx: &DecodeContext<'_>,
    first: &SketchEntityId,
    second: &SketchEntityId,
    partner: impl Fn(&SketchEntityId) -> Result<Option<SketchEntityId>, CodecError>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT repaired sketch entity pair";
    let mut selected: Option<(SketchEntityId, SketchEntityId)> = None;
    for known in [first, second] {
        let Some(partner) = partner(known)? else {
            continue;
        };
        let mut pair = [known.try_clone_for_decode(ctx, OPERATION)?, partner];
        ctx.stable_sort_by(&mut pair, |value| value, Ord::cmp, OPERATION)?;
        let [first, second] = pair;
        if let Some((selected_first, selected_second)) = &selected {
            if !(ctx.equal(selected_first, &first, OPERATION)?
                && ctx.equal(selected_second, &second, OPERATION)?)
            {
                return Ok(None);
            }
        }
        selected = Some((first, second));
    }
    Ok(selected)
}

fn unique_profile_line_distance_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, entities, |known, candidate| {
        line_line_distance(known, candidate)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_marker_line_distance_entity(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    index: RelationIndex<'_, '_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(marker_locus) =
        marker_point_locus(ctx, marker, index.markers_by_id(), index.loci_by_marker)?
    else {
        return Ok(None);
    };
    let Some(marker_point) = profile_locus_point_charged(
        ctx,
        &marker_locus,
        index.entities,
        "resolve SLDPRT profile locus",
    )?
    else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, index.entities, |known, candidate| {
        is_line(candidate)
            && sketch_entity_contains_point(candidate, marker_point)
            && line_line_distance(known, candidate)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_profile_line_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_line_pair(ctx, sketch, entities, |first, second| {
        line_line_distance(first, second)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_repaired_profile_line_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    unique_repaired_entity_pair(ctx, first, second, |known| {
        unique_profile_line_distance_entity(ctx, sketch, known, parameter, entities)
    })
}

pub(super) fn line_line_distance(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometryDefinition::Line {
        start: first_start,
        end: first_end,
    } = first.geometry.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Line {
        start: second_start,
        end: second_end,
    } = second.geometry.definition()
    else {
        return None;
    };
    super::relation_records::line_line_distance(
        [[first_start.u, first_start.v], [first_end.u, first_end.v]],
        [
            [second_start.u, second_start.v],
            [second_end.u, second_end.v],
        ],
    )
}

fn unique_profile_line_angle_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, entities, |known, candidate| {
        line_line_angle(known, candidate)
            .is_some_and(|measured| same_dimension_angle(measured, angle.get()))
    })
}

#[derive(Clone, Copy)]
enum LinePairOrdering {
    Operand,
    Identity,
}

/// The first entity with this identity, found by a linear search of `entities`.
pub(super) fn find_profile_entity<'a>(
    ctx: &DecodeContext<'_>,
    entities: &'a [SketchEntity],
    id: &SketchEntityId,
    operation: &'static str,
) -> Result<Option<&'a SketchEntity>, CodecError> {
    ctx.find_by(
        entities,
        |entity| ctx.equal(entity.id(), id, operation),
        operation,
    )
}

fn select_dynamic_line_pair(
    ctx: &DecodeContext<'_>,
    first_candidates: &[SketchEntityId],
    second_candidates: &[SketchEntityId],
    entities: &ProfileEntities<'_>,
    ordering: LinePairOrdering,
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic line pair";
    if first_candidates.is_empty() || second_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
    if !ctx.all_by(
        first_candidates,
        |first| {
            let Some(first_entity) = entities.entity(ctx, first, OPERATION)? else {
                return Ok(false);
            };
            if !ctx.all_by(
                second_candidates,
                |second| {
                    if ctx.equal(first, second, OPERATION)? {
                        return Ok(true);
                    }
                    let Some(second_entity) = entities.entity(ctx, second, OPERATION)? else {
                        return Ok(false);
                    };
                    if !matches(first_entity, second_entity) {
                        return Ok(true);
                    }
                    let pair = match ordering {
                        LinePairOrdering::Operand => (first, second),
                        LinePairOrdering::Identity => {
                            if ctx.compare(first, second, OPERATION)?.is_le() {
                                (first, second)
                            } else {
                                (second, first)
                            }
                        }
                    };
                    if let Some((selected_first, selected_second)) = selected {
                        if !(ctx.equal(selected_first, pair.0, OPERATION)?
                            && ctx.equal(selected_second, pair.1, OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some(pair);

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        first.try_clone_for_decode(ctx, OPERATION)?,
        second.try_clone_for_decode(ctx, OPERATION)?,
    )))
}

fn unique_dynamic_marker_line_angle_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    index: RelationIndex<'_, '_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let first_candidates = dynamic_line_operand_candidates(ctx, relation, 0, sketch, index)?;
    let second_candidates = dynamic_line_operand_candidates(ctx, relation, 1, sketch, index)?;
    select_dynamic_line_pair(
        ctx,
        &first_candidates,
        &second_candidates,
        index.entities,
        LinePairOrdering::Operand,
        |first, second| {
            unoriented_line_line_angle(first, second)
                .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
        },
    )
}

fn unique_dynamic_profile_line_angle_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, entities, |known, candidate| {
        unoriented_line_line_angle(known, candidate)
            .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
    })
}

fn unique_dynamic_roster_line_angle_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_line_pair(ctx, sketch, entities, |first, second| {
        unoriented_line_line_angle(first, second)
            .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
    })
}

/// The point distance a dynamic point relation measures between two points.
fn dynamic_point_distance(
    family: FeatureInputRelationFamily,
    profile_axis: Option<ProfileAxis>,
    first: Point2,
    second: Point2,
) -> Option<f64> {
    match family {
        FeatureInputRelationFamily::PointPointDistance => {
            Some((second.u - first.u).hypot(second.v - first.v))
        }
        FeatureInputRelationFamily::PointPointHorizontalDistance
        | FeatureInputRelationFamily::PointPointVerticalDistance => {
            Some(if profile_axis == Some(ProfileAxis::U) {
                (second.u - first.u).abs()
            } else {
                (second.v - first.v).abs()
            })
        }
        _ => None,
    }
}

fn unique_dynamic_marker_point_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    known: (Option<SketchLocus>, Option<SketchLocus>),
    index: RelationIndex<'_, '_>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic point pairs";

    let (known_first, known_second) = known;
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let measure = |first: &SketchLocus, second: &SketchLocus| -> Result<Option<f64>, CodecError> {
        let Some(first_point) = profile_locus_point_charged(ctx, first, index.entities, OPERATION)?
        else {
            return Ok(None);
        };
        let Some(second_point) =
            profile_locus_point_charged(ctx, second, index.entities, OPERATION)?
        else {
            return Ok(None);
        };
        Ok(dynamic_point_distance(
            relation.family,
            profile_axis,
            first_point,
            second_point,
        ))
    };
    if let (Some(first), Some(second)) = (&known_first, &known_second) {
        if measure(first, second)?
            .is_some_and(|value| same_relation_dimension_length(value, expected.get()))
        {
            return Ok(known_first.zip(known_second));
        }
    }
    let candidates =
        |operand: usize, known: Option<SketchLocus>| -> Result<Vec<SketchLocus>, CodecError> {
            let mut candidates = Vec::new();
            if let Some(known) = known {
                ctx.push_vec(&mut candidates, known, OPERATION)?;
            }
            if let Some(marker) =
                relation_operand_marker_in(ctx, relation, operand, sketch, index.markers)?
            {
                let additions = dynamic_marker_point_candidates(ctx, marker, sketch, index)?;
                ctx.extend_vec(
                    &mut candidates,
                    additions,
                    "append SLDPRT dynamic point candidates",
                )?;
            }
            Ok(candidates)
        };
    let mut first_candidates = candidates(0, known_first)?;
    let mut second_candidates = candidates(1, known_second)?;
    for candidates in [&mut first_candidates, &mut second_candidates] {
        deduplicate_physical_loci(ctx, candidates, |locus| {
            profile_locus_point_charged(ctx, locus, index.entities, "resolve SLDPRT physical locus")
        })?;
    }
    if first_candidates.is_empty() || second_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    if !ctx.all_by(
        &first_candidates,
        |first| {
            if !ctx.all_by(
                &second_candidates,
                |second| {
                    if ctx.equal(first, second, OPERATION)? {
                        return Ok(true);
                    }
                    if !measure(first, second)?
                        .is_some_and(|value| same_relation_dimension_length(value, expected.get()))
                    {
                        return Ok(true);
                    }
                    let pair = if ctx
                        .compare(&locus_key(first), &locus_key(second), OPERATION)?
                        .is_le()
                    {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    if let Some((selected_first, selected_second)) = selected {
                        if !(ctx.equal(selected_first, pair.0, OPERATION)?
                            && ctx.equal(selected_second, pair.1, OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some(pair);

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        copy_locus(ctx, first, OPERATION)?,
        copy_locus(ctx, second, OPERATION)?,
    )))
}

fn unique_dynamic_direct_point_roster_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    index: RelationIndex<'_, '_>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<(SketchLocus, SketchLocus)>, CodecError> {
    const OPERATION: &str = "select SLDPRT direct marker point roster";
    if ctx.any_by(
        &relation.operands,
        |operand| Ok(operand.entity_ref.is_some()),
        OPERATION,
    )? {
        return Ok(None);
    }
    let feature = Some(relation.feature_ref.as_str());
    let is_direct_point = |marker: &SketchInputEntity| -> Result<bool, CodecError> {
        Ok(
            ctx.equal(&marker.feature_ref.as_deref(), &feature, OPERATION)?
                && marker.coordinates_m.is_some()
                && marker.links().is_empty()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                ),
        )
    };
    for operand in 0..2 {
        let Some(marker_id) =
            relation_operand_marker_in(ctx, relation, operand, sketch, index.markers)?
        else {
            return Ok(None);
        };
        let Some(marker) = index.markers.get(ctx, marker_id, OPERATION)? else {
            return Ok(None);
        };
        if !is_direct_point(marker)? {
            return Ok(None);
        }
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut direct_marker_ids = HashSet::new();
    for marker in ctx
        .admit_iter(
            index
                .markers
                .of_feature(ctx, &relation.feature_ref, OPERATION)?,
            OPERATION,
        )?
        .copied()
    {
        if is_direct_point(marker)? {
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut direct_marker_ids, marker.id(), OPERATION)
            })?;
        }
    }
    let mut loci = Vec::new();
    for entity in ctx
        .admit_iter(index.entities.in_sketch(ctx, sketch, OPERATION)?, OPERATION)?
        .copied()
    {
        if !is_point(entity) {
            continue;
        }
        let Some(reference) = entity.native_ref.as_deref() else {
            continue;
        };
        if !ctx.contains_hash_set(&direct_marker_ids, reference, OPERATION)? {
            continue;
        }
        let locus = role_locus(ctx, SketchLocusRole::Entity, entity.id(), OPERATION)?;
        storage.with_storage(|| ctx.push_vec(&mut loci, locus, OPERATION))?;
    }
    sort_profile_loci(ctx, &mut loci, OPERATION)?;
    deduplicate_physical_loci(ctx, &mut loci, |locus| {
        profile_locus_point_charged(ctx, locus, index.entities, OPERATION)
    })?;
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let measure = |first: &SketchLocus, second: &SketchLocus| -> Result<Option<f64>, CodecError> {
        let Some(first) = profile_locus_point_charged(ctx, first, index.entities, OPERATION)?
        else {
            return Ok(None);
        };
        let Some(second) = profile_locus_point_charged(ctx, second, index.entities, OPERATION)?
        else {
            return Ok(None);
        };
        Ok(dynamic_point_distance(
            relation.family,
            profile_axis,
            first,
            second,
        ))
    };
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    if !ctx.all_by(
        loci.iter().enumerate(),
        |(first_index, first)| {
            let later = loci.get(first_index + 1..).unwrap_or_default();
            if !ctx.all_by(
                later,
                |second| {
                    if !measure(first, second)?
                        .is_some_and(|value| same_relation_dimension_length(value, expected.get()))
                    {
                        return Ok(true);
                    }
                    if let Some((selected_first, selected_second)) = selected {
                        if !(ctx.equal(selected_first, first, OPERATION)?
                            && ctx.equal(selected_second, second, OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some((first, second));

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        copy_locus(ctx, first, OPERATION)?,
        copy_locus(ctx, second, OPERATION)?,
    )))
}

const DYNAMIC_POINT_LOCUS_QUANTUM: f64 = 1.0e-8;

/// Keep the first candidate at each quantized point, and every candidate with no point.
fn deduplicate_physical_loci<T>(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<T>,
    point: impl Fn(&T) -> Result<Option<Point2>, CodecError>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "deduplicate SLDPRT physical loci";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut points = HashSet::new();
    ctx.retain_vec(
        candidates,
        |candidate| {
            let Some(point) = point(candidate)? else {
                return Ok(true);
            };
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut points,
                    quantize(point, DYNAMIC_POINT_LOCUS_QUANTUM),
                    OPERATION,
                )
            })
        },
        OPERATION,
    )
}

/// The point-line operands a dynamic roster relation already knows.
enum KnownOperands {
    Point(SketchLocus),
    Line(SketchEntityId),
    Both(SketchLocus, SketchEntityId),
}

impl KnownOperands {
    fn resolve(point: Option<SketchLocus>, line: Option<SketchEntityId>) -> Option<Self> {
        match (point, line) {
            (Some(point), Some(line)) => Some(Self::Both(point, line)),
            (Some(point), None) => Some(Self::Point(point)),
            (None, Some(line)) => Some(Self::Line(line)),
            (None, None) => None,
        }
    }
}

fn unique_dynamic_roster_point_line_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    known_point: Option<SketchLocus>,
    known_line: Option<SketchEntityId>,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic roster point-line pair";
    let explicit_point = relation
        .operands
        .first()
        .is_some_and(|operand| operand.entity_ref.is_some());
    let explicit_line = relation
        .operands
        .get(1)
        .is_some_and(|operand| operand.entity_ref.is_some());
    if (explicit_point && known_point.is_none()) || (explicit_line && known_line.is_none()) {
        return Ok(None);
    }
    // Explicit identities narrow the geometry roster to the other operand.
    let Some(known) = KnownOperands::resolve(
        explicit_point.then_some(known_point).flatten(),
        explicit_line.then_some(known_line).flatten(),
    ) else {
        if explicit_point || explicit_line {
            return Ok(None);
        }
        return unique_roster_point_line_pair(ctx, sketch, parameter, entities);
    };
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let lines = storage.with_storage(|| collect_profile_lines(ctx, sketch, entities))?;
    let mut points = storage.with_storage(|| collect_profile_loci(ctx, sketch, entities))?;
    deduplicate_physical_loci(ctx, &mut points, |(_, locus)| {
        profile_locus_point_charged(ctx, locus, entities, OPERATION)
    })?;
    match known {
        KnownOperands::Point(point) => {
            let Some(position) = profile_locus_point_charged(ctx, &point, entities, OPERATION)?
            else {
                return Ok(None);
            };
            select_profile_point_line_pairs(ctx, *distance, &[(position, point)], &lines)
        }
        KnownOperands::Line(line) => {
            let Some(line_entity) = entities.entity(ctx, &line, OPERATION)? else {
                return Ok(None);
            };
            resolve_profile_locus_positions(ctx, &mut points, entities)?;
            select_profile_point_line_pairs(ctx, *distance, &points, &[line_entity])
        }
        KnownOperands::Both(point, line) => {
            let Some(line_entity) = entities.entity(ctx, &line, OPERATION)? else {
                return Ok(None);
            };
            let Some(position) = profile_locus_point_charged(ctx, &point, entities, OPERATION)?
            else {
                return Ok(None);
            };
            Ok(point_line_distance_value(position, line_entity)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
                .then_some((point, line)))
        }
    }
}

fn unique_roster_point_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT roster locus";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut loci = storage.with_storage(|| collect_profile_loci(ctx, sketch, entities))?;
    deduplicate_physical_loci(ctx, &mut loci, |(_, locus)| {
        profile_locus_point_charged(ctx, locus, entities, OPERATION)
    })?;
    // The owning sketch roster supplies line witnesses for family-scoped operands.
    let lines = storage.with_storage(|| collect_profile_lines(ctx, sketch, entities))?;
    resolve_profile_locus_positions(ctx, &mut loci, entities)?;
    select_profile_point_line_pairs(ctx, *distance, &loci, &lines)
}

fn unique_point_line_candidate_pair(
    ctx: &DecodeContext<'_>,
    expected: cadmpeg_ir::scalar::Length,
    point_candidates: &[SketchLocus],
    line_candidates: &[SketchEntityId],
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic point-line pair";
    if point_candidates.is_empty() || line_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchLocus, &SketchEntityId)> = None;
    if !ctx.all_by(
        point_candidates,
        |point| {
            let Some(point_position) =
                profile_locus_point_charged(ctx, point, entities, "resolve SLDPRT profile locus")?
            else {
                return Ok(false);
            };
            if !ctx.all_by(
                line_candidates,
                |line| {
                    let Some(line_entity) = entities.entity(ctx, line, OPERATION)? else {
                        return Ok(false);
                    };
                    if !point_line_distance_value(point_position, line_entity).is_some_and(
                        |measured| same_relation_dimension_length(measured, expected.get()),
                    ) {
                        return Ok(true);
                    }
                    if let Some((selected_point, selected_line)) = selected {
                        if !(ctx.equal(selected_point, point, OPERATION)?
                            && ctx.equal(selected_line, line, OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some((point, line));

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((point, line)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        copy_locus(ctx, point, OPERATION)?,
        line.try_clone_for_decode(ctx, OPERATION)?,
    )))
}

fn unique_dynamic_marker_line_distance_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    index: RelationIndex<'_, '_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let first_candidates = dynamic_line_operand_candidates(ctx, relation, 0, sketch, index)?;
    let second_candidates = dynamic_line_operand_candidates(ctx, relation, 1, sketch, index)?;
    select_dynamic_line_pair(
        ctx,
        &first_candidates,
        &second_candidates,
        index.entities,
        LinePairOrdering::Identity,
        |first, second| {
            line_line_distance(first, second)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
        },
    )
}

fn unique_dynamic_roster_line_distance_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    if ctx.any_by(
        &relation.operands,
        |operand| Ok(operand.entity_ref.is_some()),
        "scan SLDPRT roster line relation operands",
    )? {
        return Ok(None);
    }
    unique_profile_line_distance_pair(ctx, sketch, parameter, entities)
}

fn dynamic_line_operand_candidates(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operand: usize,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Vec<SketchEntityId>, CodecError> {
    const OPERATION: &str = "filter SLDPRT dynamic line operand candidates";

    let mut entities =
        if let Some(entity) = solver_line_entity(ctx, relation, operand, sketch, index.entities)? {
            let mut entities = Vec::new();
            ctx.push_vec(&mut entities, entity, OPERATION)?;
            entities
        } else if let Some(marker) =
            match relation_operand_marker_in(ctx, relation, operand, sketch, index.markers)? {
                Some(marker) => Some(marker),
                None => relation_line_point_marker(ctx, relation, operand, index.markers)?
                    .map(SketchInputEntity::id),
            }
        {
            dynamic_marker_line_candidates(ctx, marker, index)?
        } else {
            Vec::new()
        };
    ctx.retain_vec(
        &mut entities,
        |candidate| {
            Ok(match index.entities.entity(ctx, candidate, OPERATION)? {
                Some(entity) => in_sketch(ctx, entity, sketch, OPERATION)? && is_line(entity),
                None => false,
            })
        },
        OPERATION,
    )?;
    if entities.len() > 1 {
        ctx.sort_unstable_by(&mut entities, |value| value, Ord::cmp, OPERATION)?;
        ctx.dedup_vec(&mut entities, "deduplicate SLDPRT marker entities")?;
    }
    Ok(entities)
}

/// The marker identities reachable from one marker through its links, in
/// visiting order.
struct MarkerIdentities<'a> {
    ordered: Vec<&'a str>,
    set: HashSet<&'a str>,
}

impl<'a> MarkerIdentities<'a> {
    fn collect(
        ctx: &DecodeContext<'_>,
        marker_id: &'a str,
        markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    ) -> Result<Self, CodecError> {
        let mut identities = Self {
            ordered: Vec::new(),
            set: HashSet::new(),
        };
        identities.visit(ctx, marker_id, markers_by_id)?;
        Ok(identities)
    }

    fn visit(
        &mut self,
        ctx: &DecodeContext<'_>,
        marker_id: &'a str,
        markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "collect SLDPRT dynamic marker identities";
        let _depth = ctx.enter_nested(OPERATION)?;
        if !ctx.insert_hash_set(&mut self.set, marker_id, OPERATION)? {
            return Ok(());
        }
        ctx.push_vec(&mut self.ordered, marker_id, OPERATION)?;
        let Some(marker) = ctx
            .get_hash_map(markers_by_id, marker_id, OPERATION)?
            .copied()
        else {
            return Ok(());
        };
        for link in ctx.admit_iter(marker.links(), OPERATION)? {
            if ctx.equal(link.entity_ref.as_str(), marker_id, OPERATION)? {
                continue;
            }
            if matches!(marker.kind(), SketchInputKind::Relation(_))
                && owner_link(ctx, marker, link)?
            {
                continue;
            }
            self.visit(ctx, &link.entity_ref, markers_by_id)?;
        }
        Ok(())
    }

    /// Whether the entity carries one of these identities as its native,
    /// geometry or endpoint reference.
    fn identify(
        &self,
        ctx: &DecodeContext<'_>,
        entity: &SketchEntity,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        for reference in entity
            .native_ref
            .as_deref()
            .into_iter()
            .chain(entity.geometry_ref.as_deref())
        {
            if ctx.contains_hash_set(&self.set, reference, operation)? {
                return Ok(true);
            }
        }
        ctx.any_by(
            &entity.endpoint_refs,
            |reference| ctx.contains_hash_set(&self.set, reference.as_str(), operation),
            operation,
        )
    }
}

fn dynamic_marker_point_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Vec<SketchLocus>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker points";
    let (identities, _identity_storage) = ctx.with_scoped_storage(OPERATION, || {
        MarkerIdentities::collect(ctx, marker, index.markers_by_id())
    })?;
    let centers = dynamic_marker_center_loci(ctx, &identities, sketch, index)?;
    if let Some(centers) = centers.filter(|centers| !centers.is_empty()) {
        return Ok(if centers.len() == 1 {
            centers
        } else {
            Vec::new()
        });
    }
    let mut candidates = Vec::new();
    if let Some(locus) =
        marker_point_locus(ctx, marker, index.markers_by_id(), index.loci_by_marker)?
    {
        if let Some(entity) = index
            .entities
            .entity(ctx, locus_entity(&locus), OPERATION)?
        {
            if in_sketch(ctx, entity, sketch, OPERATION)?
                && profile_locus_point_charged(
                    ctx,
                    &locus,
                    index.entities,
                    "resolve SLDPRT profile locus",
                )?
                .is_some()
            {
                ctx.push_vec(&mut candidates, locus, OPERATION)?;
            }
        }
    }
    for entity in ctx
        .admit_iter(index.entities.in_sketch(ctx, sketch, OPERATION)?, OPERATION)?
        .copied()
    {
        if is_point(entity) && identities.identify(ctx, entity, OPERATION)? {
            let locus = role_locus(ctx, SketchLocusRole::Entity, entity.id(), OPERATION)?;
            ctx.push_vec(&mut candidates, locus, OPERATION)?;
        }
    }
    sort_profile_loci(ctx, &mut candidates, OPERATION)?;
    Ok(candidates)
}

fn dynamic_marker_point_locus(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<SketchLocus>, CodecError> {
    match dynamic_marker_center_candidates(ctx, marker, sketch, index)? {
        Some(candidates) if !candidates.is_empty() => Ok(if candidates.len() == 1 {
            candidates.into_iter().next()
        } else {
            None
        }),
        Some(_) | None => {
            marker_point_locus(ctx, marker, index.markers_by_id(), index.loci_by_marker)
        }
    }
}

fn dynamic_marker_center_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker centers";
    let (identities, _identity_storage) = ctx.with_scoped_storage(OPERATION, || {
        MarkerIdentities::collect(ctx, marker, index.markers_by_id())
    })?;
    dynamic_marker_center_loci(ctx, &identities, sketch, index)
}

/// The arc centers in the sketch that the identities' arc markers resolve to,
/// or `None` when no identity is an arc marker.
fn dynamic_marker_center_loci(
    ctx: &DecodeContext<'_>,
    identities: &MarkerIdentities<'_>,
    sketch: &SketchId,
    index: RelationIndex<'_, '_>,
) -> Result<Option<Vec<SketchLocus>>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker centers";
    let mut has_arc_marker = false;
    let mut centers = Vec::new();
    for marker_id in ctx.admit_iter(&identities.ordered, OPERATION)?.copied() {
        let Some(marker) = index.markers.get(ctx, marker_id, OPERATION)? else {
            continue;
        };
        if !matches!(marker.kind(), SketchInputKind::Arc) {
            continue;
        }
        has_arc_marker = true;
        let Some(locus) =
            marker_point_locus(ctx, marker_id, index.markers_by_id(), index.loci_by_marker)?
        else {
            continue;
        };
        let Some(entity) = index
            .entities
            .entity(ctx, locus_entity(&locus), OPERATION)?
        else {
            continue;
        };
        if in_sketch(ctx, entity, sketch, OPERATION)?
            && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Arc { .. }
            )
        {
            let center = role_locus(ctx, SketchLocusRole::Center, entity.id(), OPERATION)?;
            ctx.push_vec(&mut centers, center, OPERATION)?;
        }
    }
    if !has_arc_marker {
        return Ok(None);
    }
    sort_profile_loci(ctx, &mut centers, OPERATION)?;
    Ok(Some(centers))
}

fn dynamic_marker_line_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    index: RelationIndex<'_, '_>,
) -> Result<Vec<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker lines";
    let (identities, _identity_storage) = ctx.with_scoped_storage(OPERATION, || {
        MarkerIdentities::collect(ctx, marker, index.markers_by_id())
    })?;
    let mut candidates = marker_entities(
        ctx,
        marker,
        index.markers_by_id(),
        index.loci_by_marker,
        MarkerEntityFilter::Lines(index.entities.all()),
    )?;
    // A line that carries several identities is added once per identity; the
    // sort below removes the repeats.
    for identity in ctx.admit_iter(&identities.ordered, OPERATION)?.copied() {
        for entity in ctx
            .admit_iter(
                index.entities.with_reference(ctx, identity, OPERATION)?,
                OPERATION,
            )?
            .copied()
        {
            if is_line(entity) {
                let id = entity.id().try_clone_for_decode(ctx, OPERATION)?;
                ctx.push_vec(&mut candidates, id, OPERATION)?;
            }
        }
    }
    if candidates.is_empty() {
        if let Some(entity) = single_marker_line_entity_in(
            ctx,
            marker,
            index.markers_by_id(),
            index.loci_by_marker,
            index.entities,
        )? {
            ctx.push_vec(&mut candidates, entity, OPERATION)?;
        }
    }
    ctx.sort_unstable_by(&mut candidates, |value| value, Ord::cmp, OPERATION)?;
    ctx.dedup_vec(&mut candidates, "deduplicate SLDPRT marker line candidates")?;
    Ok(candidates)
}

fn unique_profile_line_angle_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    unique_profile_matched_line_pair(ctx, sketch, entities, |first, second| {
        line_line_angle(first, second)
            .is_some_and(|measured| same_dimension_angle(measured, angle.get()))
    })
}

fn unique_repaired_profile_line_angle_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
    unique_repaired_entity_pair(ctx, first, second, |known| {
        unique_profile_line_angle_entity(ctx, sketch, known, parameter, entities)
    })
}

pub(super) fn line_line_angle(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometryDefinition::Line {
        start: first_start,
        end: first_end,
    } = first.geometry.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Line {
        start: second_start,
        end: second_end,
    } = second.geometry.definition()
    else {
        return None;
    };
    super::relation_records::line_line_angle(
        [[first_start.u, first_start.v], [first_end.u, first_end.v]],
        [
            [second_start.u, second_start.v],
            [second_end.u, second_end.v],
        ],
    )
}

pub(super) fn unoriented_line_line_angle(
    first: &SketchEntity,
    second: &SketchEntity,
) -> Option<f64> {
    let angle = line_line_angle(first, second)?;
    Some(angle.min(std::f64::consts::PI - angle))
}

pub(super) fn same_dimension_angle(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= EPS_RELATION_LOCI_SAME_DIMENSION_ANGLE_E9 * left.abs().max(right.abs()).max(1.0)
}

fn collect_profile_loci(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
) -> Result<Vec<(Point2, SketchLocus)>, CodecError> {
    const OPERATION: &str = "collect SLDPRT profile locus candidates";
    let mut result = Vec::new();
    for entity in ctx
        .admit_iter(entities.in_sketch(ctx, sketch, OPERATION)?, OPERATION)?
        .copied()
    {
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            let locus = role_locus(ctx, role, entity.id(), OPERATION)?;
            ctx.push_vec(&mut result, (point, locus), OPERATION)?;
        }
    }
    Ok(result)
}

fn collect_profile_lines<'a>(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &ProfileEntities<'a>,
) -> Result<Vec<&'a SketchEntity>, CodecError> {
    const OPERATION: &str = "collect SLDPRT profile line candidates";
    let mut result = Vec::new();
    for entity in ctx
        .admit_iter(entities.in_sketch(ctx, sketch, OPERATION)?, OPERATION)?
        .copied()
    {
        if is_line(entity) {
            ctx.push_vec(&mut result, entity, OPERATION)?;
        }
    }
    Ok(result)
}

/// Replace each locus position by its resolved profile point, dropping loci
/// with no point.
fn resolve_profile_locus_positions(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<(Point2, SketchLocus)>,
    entities: &ProfileEntities<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "resolve SLDPRT profile candidate coordinates";
    ctx.retain_mut(
        loci,
        |(position, locus)| {
            let Some(point) = profile_locus_point_charged(ctx, locus, entities, OPERATION)? else {
                return Ok(false);
            };
            *position = point;
            Ok(true)
        },
        OPERATION,
    )
}

fn select_profile_point_line_pairs(
    ctx: &DecodeContext<'_>,
    distance: cadmpeg_ir::scalar::Length,
    loci: &[(Point2, SketchLocus)],
    lines: &[&SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT profile point-line pairs";
    let mut selected: Option<(&SketchLocus, &SketchEntityId)> = None;
    if !ctx.all_by(
        loci,
        |(point, locus)| {
            if !ctx.all_by(
                lines.iter().copied(),
                |line| {
                    if !point_line_distance_value(*point, line)
                        .is_some_and(|measured| same_dimension_length(measured, distance.get()))
                    {
                        return Ok(true);
                    }
                    if let Some((selected_point, selected_line)) = selected {
                        if !(ctx.equal(selected_point, locus, OPERATION)?
                            && ctx.equal(selected_line, line.id(), OPERATION)?)
                        {
                            return Ok(false);
                        }
                    }
                    selected = Some((locus, line.id()));

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((point, line)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        copy_locus(ctx, point, OPERATION)?,
        line.try_clone_for_decode(ctx, OPERATION)?,
    )))
}

fn unique_profile_point_line_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    point: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT profile point-line entity";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(point) = profile_locus_point_charged(ctx, point, entities, OPERATION)? else {
        return Ok(None);
    };
    let mut selected: Option<&SketchEntityId> = None;
    if !ctx.all_by(
        entities.in_sketch(ctx, sketch, OPERATION)?.iter().copied(),
        |line| {
            if !point_line_distance_value(point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
            {
                return Ok(true);
            }
            if let Some(selected) = selected {
                if !ctx.equal(selected, line.id(), OPERATION)? {
                    return Ok(false);
                }
            }
            selected = Some(line.id());

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    selected
        .map(|id| id.try_clone_for_decode(ctx, OPERATION))
        .transpose()
}

fn unique_profile_line_point_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchLocus>, CodecError> {
    const OPERATION: &str = "select SLDPRT profile line-point locus";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(line) = entities.entity(ctx, line, OPERATION)? else {
        return Ok(None);
    };
    let (loci, _loci_storage) =
        ctx.with_scoped_storage(OPERATION, || collect_profile_loci(ctx, sketch, entities))?;
    let mut selected: Option<&SketchLocus> = None;
    if !ctx.all_by(
        &loci,
        |(point, locus)| {
            if !point_line_distance_value(*point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
            {
                return Ok(true);
            }
            if let Some(selected) = selected {
                if !ctx.equal(selected, locus, OPERATION)? {
                    return Ok(false);
                }
            }
            selected = Some(locus);

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    selected
        .map(|locus| copy_locus(ctx, locus, OPERATION))
        .transpose()
}

fn unique_profile_point_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "select SLDPRT profile point-line pair";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let loci = storage.with_storage(|| collect_profile_loci(ctx, sketch, entities))?;
    let lines = storage.with_storage(|| collect_profile_lines(ctx, sketch, entities))?;
    select_profile_point_line_pairs(ctx, *distance, &loci, &lines)
}

fn unique_repaired_profile_point_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    point: &SketchLocus,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    entities: &ProfileEntities<'_>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, CodecError> {
    const OPERATION: &str = "repair SLDPRT profile point-line pair";
    let mut selected = None;
    if let Some(candidate_line) =
        unique_profile_point_line_entity(ctx, sketch, point, parameter, entities)?
    {
        selected = Some((copy_locus(ctx, point, OPERATION)?, candidate_line));
    }
    if let Some(candidate_point) =
        unique_profile_line_point_locus(ctx, sketch, line, parameter, entities)?
    {
        if let Some((selected_point, selected_line)) = &selected {
            if !(ctx.equal(selected_point, &candidate_point, OPERATION)?
                && ctx.equal(selected_line, line, OPERATION)?)
            {
                return Ok(None);
            }
        }
        selected = Some((candidate_point, line.try_clone_for_decode(ctx, OPERATION)?));
    }
    Ok(selected)
}

/// The profile point of a locus, read from its entity's geometry.
pub(super) fn profile_locus_point_charged(
    ctx: &DecodeContext<'_>,
    locus: &SketchLocus,
    entities: &ProfileEntities<'_>,
    operation: &'static str,
) -> Result<Option<Point2>, CodecError> {
    let Some(entity) = entities.entity(ctx, locus_entity(locus), operation)? else {
        return Ok(None);
    };
    Ok(entity_locus_point(entity, locus))
}

/// The point of a locus on the entity it names.
pub(super) fn entity_locus_point(entity: &SketchEntity, locus: &SketchLocus) -> Option<Point2> {
    sketch_entity_locus_points(entity)
        .into_iter()
        .flatten()
        .find_map(|(point, role)| role.matches(locus).then_some(point))
}

/// Collapse loci that all resolve to one quantized point into the locus with
/// the least identity key.
fn canonicalize_physical_loci(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    entities: &ProfileEntities<'_>,
    quantum: f64,
) -> Result<(), CodecError> {
    const OPERATION: &str = "canonicalize SLDPRT physical sketch loci";
    if loci.len() < 2 {
        return Ok(());
    }
    let mut first_point = None;
    let mut coincident = true;
    let mut minimum = 0;
    if !ctx.all_by(
        loci.iter().enumerate(),
        |(index, locus)| {
            let Some(point) =
                profile_locus_point_charged(ctx, locus, entities, "resolve SLDPRT profile locus")?
            else {
                return Ok(false);
            };
            let point = quantize(point, quantum);
            if let Some(first) = first_point {
                coincident &= point == first;
            } else {
                first_point = Some(point);
            }
            if let Some(current) = loci.get(minimum) {
                if ctx
                    .compare(&locus_key(locus), &locus_key(current), OPERATION)?
                    .is_lt()
                {
                    minimum = index;
                }
            }

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(());
    }
    if coincident {
        loci.swap(0, minimum);
        ctx.truncate_vec(loci, 1, "select SLDPRT relation locus")?;
    }
    Ok(())
}

pub(super) fn point_line_distance_value(point: Point2, line: &SketchEntity) -> Option<f64> {
    let SketchGeometryDefinition::Line { start, end } = line.geometry.definition() else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    (length > SKETCH_POINT_TOLERANCE).then(|| {
        ((point.u - start.u) * direction[1] - (point.v - start.v) * direction[0]).abs() / length
    })
}

/// The markers of the relation's feature that `matches` accepts, in source order.
fn collect_relation_marker_candidates<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    markers: &RelationMarkers<'a>,
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "collect SLDPRT relation operand markers";
    let mut candidates = Vec::new();
    for marker in ctx
        .admit_iter(
            markers.of_feature(ctx, &relation.feature_ref, OPERATION)?,
            OPERATION,
        )?
        .copied()
    {
        if matches(marker) {
            ctx.push_vec(&mut candidates, marker, OPERATION)?;
        }
    }
    Ok(candidates)
}

#[cfg(test)]
pub(super) fn relation_operand_marker<'a>(
    ctx: &DecodeContext<'_>,
    relation: &'a FeatureInputRelationInstance,
    operand: usize,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<&'a str>, CodecError> {
    let markers = RelationMarkers::from_map(ctx, markers_by_id)?;
    relation_operand_marker_in(ctx, relation, operand, sketch, &markers)
}

pub(super) fn relation_operand_marker_in<'a>(
    ctx: &DecodeContext<'_>,
    relation: &'a FeatureInputRelationInstance,
    operand: usize,
    sketch: &SketchId,
    markers: &RelationMarkers<'a>,
) -> Result<Option<&'a str>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT relation operand marker";
    let Some(relation_operand) = relation.operands.get(operand) else {
        return Ok(None);
    };
    if relation_operand.kind == FeatureInputOperandKind::D6
        && ctx.contains_text(sketch.as_str(), "sketch#compact:", OPERATION)?
    {
        let mut coordinate_handles =
            collect_relation_marker_candidates(ctx, relation, markers, |marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            })?;
        ctx.stable_sort_by_key(
            &mut coordinate_handles,
            |value| value.offset(),
            Ord::cmp,
            OPERATION,
        )?;
        return Ok(coordinate_handles
            .get(usize::from(relation_operand.entity_index))
            .map(|marker| marker.id()));
    }
    match relation_operand.entity_ref.as_deref() {
        Some(marker) => Ok(Some(marker)),
        None => dynamic_relation_marker(ctx, relation, operand, markers),
    }
}

fn dynamic_point_operand(relation: &FeatureInputRelationInstance, operand: usize) -> bool {
    matches!(
        (relation.family, operand),
        (
            FeatureInputRelationFamily::PointPointDistance
                | FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance,
            0 | 1
        ) | (FeatureInputRelationFamily::PointLineDistance, 0)
    )
}

fn dynamic_relation_marker<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operand: usize,
    markers: &RelationMarkers<'a>,
) -> Result<Option<&'a str>, CodecError> {
    if !relation_uses_dynamic_operands(relation) {
        return Ok(None);
    }
    let point_role = dynamic_point_operand(relation, operand);
    let line_role = matches!(
        (relation.family, operand),
        (
            FeatureInputRelationFamily::LineLineDistance | FeatureInputRelationFamily::Angle,
            0 | 1
        ) | (FeatureInputRelationFamily::PointLineDistance, 1)
    );
    if !point_role && !line_role {
        return Ok(None);
    }
    let Some(relation_operand) = relation.operands.get(operand) else {
        return Ok(None);
    };
    let address = u32::from(relation_operand.entity_index);
    let direct_kind = |marker: &SketchInputEntity| {
        if point_role {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        } else {
            matches!(
                marker.kind(),
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        }
    };
    let address_kind = |marker: &SketchInputEntity| {
        direct_kind(marker)
            || (point_role && matches!(marker.kind(), SketchInputKind::Arc))
            || matches!(marker.kind(), SketchInputKind::Relation(_))
    };
    let arc = |marker: &SketchInputEntity| matches!(marker.kind(), SketchInputKind::Arc);
    let select = |addressed: &[&'a SketchInputEntity]| -> Result<Option<&'a str>, CodecError> {
        if point_role {
            if let Some(marker) = unique_dynamic_marker(ctx, addressed, direct_kind)? {
                return Ok(Some(marker));
            }
            if let Some(marker) = unique_dynamic_marker(ctx, addressed, arc)? {
                return Ok(Some(marker));
            }
        }
        unique_dynamic_marker(ctx, addressed, |_| true)
    };
    let by_object = collect_relation_marker_candidates(ctx, relation, markers, |marker| {
        marker.object_index() == Some(address) && address_kind(marker)
    })?;
    if !by_object.is_empty() {
        return select(&by_object);
    }
    let by_local = collect_relation_marker_candidates(ctx, relation, markers, |marker| {
        marker.local_id() == Some(address) && address_kind(marker)
    })?;
    if !by_local.is_empty() {
        return select(&by_local);
    }
    let mut ordinal = collect_relation_marker_candidates(ctx, relation, markers, direct_kind)?;
    ctx.stable_sort_by(
        &mut ordinal,
        |value| value.id(),
        Ord::cmp,
        "sort SLDPRT ordinal operand markers",
    )?;
    ctx.stable_sort_by_key(
        &mut ordinal,
        |value| value.ordinal(),
        Ord::cmp,
        "sort SLDPRT ordinal operand markers",
    )?;
    ctx.stable_sort_by_key(
        &mut ordinal,
        |value| value.offset(),
        Ord::cmp,
        "sort SLDPRT ordinal operand markers",
    )?;
    Ok(ordinal
        .get(usize::from(relation_operand.entity_index))
        .map(|marker| marker.id()))
}

fn unique_dynamic_marker<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &[&'a SketchInputEntity],
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Result<Option<&'a str>, CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic operand marker";
    let mut selected: Option<&'a str> = None;
    if !ctx.all_by(
        candidates.iter().copied(),
        |candidate| {
            if !matches(candidate) {
                return Ok(true);
            }
            if let Some(selected) = selected {
                if !ctx.equal(selected, candidate.id(), OPERATION)? {
                    return Ok(false);
                }
            }
            selected = Some(candidate.id());

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    Ok(selected)
}

fn relation_line_point_marker<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operand: usize,
    markers: &RelationMarkers<'a>,
) -> Result<Option<&'a SketchInputEntity>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT relation line point marker";
    let Some(relation_operand) = relation.operands.get(operand) else {
        return Ok(None);
    };
    if relation_operand.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386)
        || relation_operand.entity_ref.is_some()
    {
        return Ok(None);
    }
    let mut selected = None;
    if !ctx.all_by(
        markers
            .of_feature(ctx, &relation.feature_ref, OPERATION)?
            .iter()
            .copied(),
        |marker| {
            if marker.local_id() != Some(u32::from(relation_operand.entity_index))
                || marker.coordinates_m.is_none()
                || !matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            {
                return Ok(true);
            }
            if selected.is_some() {
                return Ok(false);
            }
            selected = Some(marker);

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    Ok(selected)
}

fn marker_center_dimensioned_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT marker-centered dimensioned circle";

    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    let Some(center_entity) = unique_entity(
        ctx,
        entities.with_native_ref(ctx, marker_id, OPERATION)?,
        OPERATION,
        |entity| Ok(in_sketch(ctx, entity, sketch, OPERATION)? && is_point(entity)),
    )?
    else {
        return Ok(None);
    };
    let SketchGeometryDefinition::Point { position: center } = center_entity.geometry.definition()
    else {
        return Ok(None);
    };
    let center = quantize(
        center.get(),
        EPS_RELATION_LOCI_MARKER_CENTER_DIMENSIONED_ENTITY_E8,
    );
    let selected = unique_entity(
        ctx,
        entities.in_sketch(ctx, sketch, OPERATION)?,
        OPERATION,
        |entity| {
            let (candidate_center, radius) = match entity.geometry.definition() {
                SketchGeometryDefinition::Circle { center, radius }
                | SketchGeometryDefinition::Arc { center, radius, .. } => {
                    (center.get(), radius.get())
                }
                _ => return Ok(false),
            };
            Ok(quantize(
                candidate_center,
                EPS_RELATION_LOCI_MARKER_CENTER_DIMENSIONED_ENTITY_E8,
            ) == center
                && same_dimension_length(radius, expected_radius))
        },
    )?;
    selected
        .map(|entity| entity.id().try_clone_for_decode(ctx, OPERATION))
        .transpose()
}

fn unique_dimensioned_circle_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT dimensioned circle";

    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    let selected = unique_entity(
        ctx,
        entities.in_sketch(ctx, sketch, OPERATION)?,
        OPERATION,
        |entity| {
            let radius = match entity.geometry.definition() {
                SketchGeometryDefinition::Circle { radius, .. }
                | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                _ => return Ok(false),
            };
            Ok(same_dimension_length(radius, expected_radius))
        },
    )?;
    selected
        .map(|entity| entity.id().try_clone_for_decode(ctx, OPERATION))
        .transpose()
}

pub(super) fn same_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9 * left.abs().max(right.abs()).max(1.0)
}

pub(super) fn profile_axis_for_relation(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    transforms: Option<&[MarkerTransform]>,
) -> Result<Option<ProfileAxis>, CodecError> {
    let (native_axis, default_axis) = match relation.family {
        FeatureInputRelationFamily::PointPointHorizontalDistance => (0, ProfileAxis::U),
        FeatureInputRelationFamily::PointPointVerticalDistance => (1, ProfileAxis::V),
        _ => return Ok(None),
    };
    let Some(transforms) = transforms else {
        return Ok(Some(default_axis));
    };
    if transforms.is_empty() {
        return Ok(Some(default_axis));
    }
    let mut first = None;
    let mut disagreement = false;
    if !ctx.all_by(
        transforms,
        |transform| {
            let Some(axis) = transform.profile_axis_for_native(native_axis) else {
                return Ok(false);
            };
            if let Some(first) = first {
                if axis != first {
                    disagreement = true;
                }
            } else {
                first = Some(axis);
            }

            Ok(true)
        },
        "resolve SLDPRT relation profile axis",
    )? {
        return Ok(None);
    }
    Ok(first.filter(|_| !disagreement))
}

/// The loci stored for the qualified point of a marker.
fn qualified_point_loci<'a>(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    loci_by_marker: &'a HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<&'a [SketchLocus]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT qualified point loci";
    let (key, _key_storage) = ctx.format_scoped(
        format_args!("{marker_id}{QUALIFIED_POINT_SUFFIX}"),
        OPERATION,
    )?;
    Ok(ctx
        .get_hash_map(loci_by_marker, key.as_str(), OPERATION)?
        .map(Vec::as_slice))
}

pub(super) fn marker_point_locus(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchLocus>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker point locus";
    if let Some(loci) = qualified_point_loci(ctx, marker_id, loci_by_marker)? {
        if let Some(locus) = unique_locus(ctx, loci)? {
            return Ok(Some(locus));
        }
    }
    let mut visited = HashSet::new();
    let mut visited_storage = ctx.reserve_scoped(0, OPERATION)?;
    resolved_marker_locus(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        &mut visited,
        &mut visited_storage,
    )
}

fn qualified_or_linked_point_locus(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchLocus>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT qualified or linked point locus";
    let Some(marker) = ctx.get_hash_map(markers_by_id, marker_id, OPERATION)? else {
        return Ok(None);
    };
    if matches!(
        marker.kind(),
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) {
        let mut selected = None;
        let mut ambiguous = false;
        for link in ctx.admit_iter(marker.links(), OPERATION)? {
            if ctx.equal(link.entity_ref.as_str(), marker_id, OPERATION)? {
                continue;
            }
            if matches!(
                ctx.get_hash_map(markers_by_id, link.entity_ref.as_str(), OPERATION)?
                    .map(|marker| marker.kind()),
                Some(SketchInputKind::Relation(_))
            ) {
                continue;
            }
            let mut visited = HashSet::new();
            let mut visited_storage = ctx.reserve_scoped(0, OPERATION)?;
            let Some(locus) = resolved_marker_locus(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                &mut visited,
                &mut visited_storage,
            )?
            else {
                continue;
            };
            let Some(entity) = entities.entity(ctx, locus_entity(&locus), OPERATION)? else {
                continue;
            };
            if !is_point(entity) {
                continue;
            }
            if let Some(selected) = &selected {
                if !ctx.equal(selected, &locus, OPERATION)? {
                    ambiguous = true;
                }
            }
            selected = Some(locus);
        }
        if selected.is_some() && !ambiguous {
            return Ok(selected);
        }
    }
    if let Some(loci) = qualified_point_loci(ctx, marker_id, loci_by_marker)? {
        return unique_locus(ctx, loci);
    }
    let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    let Some(entity) = entities.entity(ctx, locus_entity(&locus), OPERATION)? else {
        return Ok(None);
    };
    Ok(is_point(entity).then_some(locus))
}

#[cfg(test)]
pub(super) fn qualified_point_marker_key(marker_id: &str) -> String {
    format!("{marker_id}{QUALIFIED_POINT_SUFFIX}")
}

fn resolved_marker_locus<'a>(
    ctx: &DecodeContext<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<&'a str>,
    visited_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<SketchLocus>, CodecError> {
    let Some(locus) = resolved_marker_locus_inner(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        visited,
        visited_storage,
    )?
    else {
        return Ok(None);
    };
    copy_locus(ctx, locus, "retain SLDPRT resolved marker locus").map(Some)
}

/// The single locus a marker resolves to: its own singleton locus, or the one
/// locus its non-relation links agree on. `visited` holds the markers on the
/// current link path, which a cycle does not re-enter.
fn resolved_marker_locus_inner<'a, 'loci>(
    ctx: &DecodeContext<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &'loci HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<&'a str>,
    visited_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<&'loci SketchLocus>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked point locus";
    let _nesting = ctx.enter_nested(OPERATION)?;
    if let Some([locus]) = ctx
        .get_hash_map(loci_by_marker, marker_id, OPERATION)?
        .map(Vec::as_slice)
    {
        return Ok(Some(locus));
    }
    if !visited_storage.with_storage(|| ctx.insert_hash_set(visited, marker_id, OPERATION))? {
        return Ok(None);
    }
    let result = (|| -> Result<Option<&'loci SketchLocus>, CodecError> {
        let Some(marker) = ctx
            .get_hash_map(markers_by_id, marker_id, OPERATION)?
            .copied()
        else {
            return Ok(None);
        };
        let mut selected: Option<&SketchLocus> = None;
        let mut ambiguous = false;
        for link in ctx.admit_iter(marker.links(), OPERATION)? {
            if ctx.equal(link.entity_ref.as_str(), marker_id, OPERATION)? {
                continue;
            }
            if matches!(
                ctx.get_hash_map(markers_by_id, link.entity_ref.as_str(), OPERATION)?
                    .map(|marker| marker.kind()),
                Some(SketchInputKind::Relation(_))
            ) {
                continue;
            }
            let Some(locus) = resolved_marker_locus_inner(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                visited,
                visited_storage,
            )?
            else {
                continue;
            };
            if let Some(selected) = selected {
                if !ctx.equal(selected, locus, OPERATION)? {
                    ambiguous = true;
                }
            }
            selected = Some(locus);
        }
        Ok(selected.filter(|_| !ambiguous))
    })();
    let locus = result?;
    ctx.remove_hash_set(visited, marker_id, OPERATION)?;
    Ok(locus)
}

fn unique_locus(
    ctx: &DecodeContext<'_>,
    loci: &[SketchLocus],
) -> Result<Option<SketchLocus>, CodecError> {
    let [locus] = loci else {
        return Ok(None);
    };
    copy_locus(ctx, locus, "retain SLDPRT singleton marker locus").map(Some)
}

fn single_marker_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchEntityId>, CodecError> {
    let entities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::All,
    )?;
    Ok(if entities.len() == 1 {
        entities.into_iter().next()
    } else {
        None
    })
}

fn single_marker_circular_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT single circular marker entity";
    let mut identities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::All,
    )?;
    ctx.retain_vec(
        &mut identities,
        |identity| {
            Ok(entities
                .entity(ctx, identity, OPERATION)?
                .is_some_and(is_circular))
        },
        OPERATION,
    )?;
    sort_marker_entity_ids(ctx, &mut identities, OPERATION)?;
    Ok(if identities.len() == 1 {
        identities.into_iter().next()
    } else {
        None
    })
}

#[cfg(test)]
pub(super) fn single_marker_line_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, CodecError> {
    let (entities, _entities_storage) = ctx
        .with_scoped_storage("index SLDPRT relation sketch entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    single_marker_line_entity_in(ctx, marker_id, markers_by_id, loci_by_marker, &entities)
}

pub(super) fn single_marker_line_entity_in(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT single marker line";
    let mut lines = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::Lines(entities.all()),
    )?;
    ctx.sort_unstable_by(
        &mut lines,
        |value| value,
        Ord::cmp,
        "sort SLDPRT single marker line entities",
    )?;
    ctx.dedup_vec(&mut lines, "deduplicate SLDPRT marker entities")?;
    if lines.len() == 1 {
        return Ok(lines.into_iter().next());
    }
    let Some(marker) = ctx.get_hash_map(markers_by_id, marker_id, OPERATION)? else {
        return Ok(None);
    };
    let fallback = || {
        unique_line_containing_marker_point(ctx, marker_id, markers_by_id, loci_by_marker, entities)
    };
    let mut links = [None, None];
    let mut other = false;
    for link in ctx.admit_iter(marker.links(), OPERATION)? {
        if ctx.equal(link.entity_ref.as_str(), marker_id, OPERATION)?
            || (matches!(marker.kind(), SketchInputKind::Relation(_))
                && owner_link(ctx, marker, link)?)
        {
            continue;
        }
        if links[0].is_none() {
            links[0] = Some(link);
        } else if links[1].is_none() {
            links[1] = Some(link);
        } else {
            other = true;
        }
    }
    let [Some(first_link), Some(second_link)] = links else {
        return fallback();
    };
    if other {
        return fallback();
    }
    let Some(first_locus) =
        marker_point_locus(ctx, &first_link.entity_ref, markers_by_id, loci_by_marker)?
    else {
        return fallback();
    };
    let Some(second_locus) =
        marker_point_locus(ctx, &second_link.entity_ref, markers_by_id, loci_by_marker)?
    else {
        return fallback();
    };
    let Some(first_entity) = entities.entity(ctx, locus_entity(&first_locus), OPERATION)? else {
        return fallback();
    };
    let sketch = &first_entity.sketch;
    let Some(second_entity) = entities.entity(ctx, locus_entity(&second_locus), OPERATION)? else {
        return fallback();
    };
    if !in_sketch(ctx, second_entity, sketch, OPERATION)? {
        return fallback();
    }
    let Some(first) = profile_locus_point_charged(ctx, &first_locus, entities, OPERATION)? else {
        return Ok(None);
    };
    let Some(second) = profile_locus_point_charged(ctx, &second_locus, entities, OPERATION)? else {
        return Ok(None);
    };
    if same_dimension_length(first.u, second.u) && same_dimension_length(first.v, second.v) {
        return fallback();
    }
    match unique_profile_line_through_points(ctx, sketch, entities, &[first, second])? {
        Some(identity) => Ok(Some(identity)),
        None => fallback(),
    }
}

fn unique_line_containing_marker_point(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities: &ProfileEntities<'_>,
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT line through marker point";
    let Some(marker) = ctx.get_hash_map(markers_by_id, marker_id, OPERATION)? else {
        return Ok(None);
    };
    if !matches!(
        marker.kind(),
        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
    ) {
        return Ok(None);
    }
    let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    let Some(point) = profile_locus_point_charged(ctx, &locus, entities, OPERATION)? else {
        return Ok(None);
    };
    let Some(entity) = entities.entity(ctx, locus_entity(&locus), OPERATION)? else {
        return Ok(None);
    };
    unique_profile_line_through_points(ctx, &entity.sketch, entities, &[point])
}

fn unique_profile_line_through_points(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &ProfileEntities<'_>,
    points: &[Point2],
) -> Result<Option<SketchEntityId>, CodecError> {
    const OPERATION: &str = "select SLDPRT profile line through points";
    let mut remaining = entities.in_sketch(ctx, sketch, OPERATION)?.iter().copied();
    let accepts = |entity: &SketchEntity| -> Result<bool, CodecError> {
        Ok(is_line(entity)
            && ctx.all_by(
                points,
                |point| Ok(sketch_entity_contains_point(entity, *point)),
                OPERATION,
            )?)
    };
    let Some(first) = ctx.find_by(&mut remaining, |entity| accepts(entity), OPERATION)? else {
        return Ok(None);
    };
    if ctx.any_by(
        &mut remaining,
        |entity| Ok(accepts(entity)? && !ctx.equal(first.id(), entity.id(), OPERATION)?),
        OPERATION,
    )? {
        return Ok(None);
    }
    first.id().try_clone_for_decode(ctx, OPERATION).map(Some)
}

fn sort_profile_loci(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    operation: &'static str,
) -> Result<(), CodecError> {
    // Equal sort keys identify equal locus values.
    ctx.sort_unstable_by(
        loci.as_mut_slice(),
        |value| value,
        |left, right| locus_key(left).cmp(&locus_key(right)),
        operation,
    )?;
    ctx.dedup_vec(loci, "deduplicate SLDPRT profile loci")?;
    Ok(())
}

fn append_profile_endpoint_locus(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    entity: &SketchEntityId,
    role: SketchLocusRole,
    operation: &'static str,
) -> Result<(), CodecError> {
    if ctx.any_by(
        loci.iter(),
        |locus| Ok(role.matches(locus) && ctx.equal(locus_entity(locus), entity, operation)?),
        operation,
    )? {
        return Ok(());
    }
    let locus = role_locus(ctx, role, entity, operation)?;
    ctx.push_vec(loci, locus, operation)
}

fn qualified_point_key(
    ctx: &DecodeContext<'_>,
    marker: &str,
    qualified: bool,
    operation: &'static str,
) -> Result<String, CodecError> {
    if qualified {
        ctx.format_retained(format_args!("{marker}{QUALIFIED_POINT_SUFFIX}"), operation)
    } else {
        ctx.copy_retained_text(marker, operation)
    }
}

#[cfg(test)]
pub(super) fn profile_loci_by_marker(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<String, Vec<SketchLocus>>, CodecError> {
    let (entities, _entities_storage) = ctx
        .with_scoped_storage("index SLDPRT relation sketch entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    let (markers, _markers_storage) = ctx
        .with_scoped_storage("index SLDPRT relation markers", || {
            RelationMarkers::new(ctx, lanes)
        })?;
    let transforms = marker_transform_candidates_in(ctx, features, sketches, &entities, lanes)?;
    profile_loci_in(ctx, features, &entities, &markers, lanes, &transforms)
}

/// The profile loci each sketch marker resolves to, keyed by marker identity,
/// or by marker identity and the qualified-point suffix for a marker a relation
/// names as a qualified point.
pub(super) fn profile_loci_in(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    entities: &ProfileEntities<'_>,
    markers: &RelationMarkers<'_>,
    lanes: &[FeatureInputLane],
    transforms: &HashMap<&str, Vec<MarkerTransform>>,
) -> Result<HashMap<String, Vec<SketchLocus>>, CodecError> {
    const BUILD_OPERATION: &str = "build SLDPRT profile marker loci";
    const RESULT_OPERATION: &str = "build SLDPRT native marker locus results";
    const ENDPOINT_OPERATION: &str = "build SLDPRT endpoint marker locus results";
    const GROUP_OPERATION: &str = "group SLDPRT transformed profile markers";
    const TRANSFORM_OPERATION: &str = "resolve SLDPRT transformed marker loci";
    const PAIR_OPERATION: &str = "resolve SLDPRT endpoint marker profile entity";
    const LINKED_OPERATION: &str = "collect SLDPRT linked endpoint loci";
    const INDEX_OPERATION: &str = "index SLDPRT profile marker identities";

    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1e-8;

    let mut storage = ctx.reserve_scoped(0, BUILD_OPERATION)?;
    let mut qualified_point_markers = HashSet::new();
    for lane in ctx.admit_iter(lanes, INDEX_OPERATION)? {
        for relation in ctx.admit_iter(&lane.relation_instances, INDEX_OPERATION)? {
            for operand in ctx.admit_iter(&relation.operands, INDEX_OPERATION)? {
                if !matches!(
                    operand.kind,
                    FeatureInputOperandKind::D6
                        | FeatureInputOperandKind::Native(
                            NativeOperandTag::TAG_80CC
                                | NativeOperandTag::TAG_8152
                                | NativeOperandTag::TAG_81B2
                                | NativeOperandTag::TAG_837B
                                | NativeOperandTag::TAG_8AB6
                                | NativeOperandTag::TAG_8DCB
                                | NativeOperandTag::TAG_929D
                                | NativeOperandTag::TAG_BC7C
                                | NativeOperandTag::TAG_BD69
                        )
                ) {
                    continue;
                }
                if let Some(marker) = operand.entity_ref.as_deref() {
                    storage.with_storage(|| {
                        ctx.insert_hash_set(&mut qualified_point_markers, marker, INDEX_OPERATION)
                    })?;
                }
            }
        }
    }
    let sketches_by_feature = planar_sketches_by_feature(ctx, &mut storage, features)?;
    let mut profile_loci = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let mut line_midpoints = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let mut nonpoint_carriers = HashSet::new();
    for entity in ctx.admit_iter(entities.all(), BUILD_OPERATION)? {
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            let locus = role_locus(ctx, role, entity.id(), BUILD_OPERATION)?;
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut profile_loci,
                    &entity.sketch,
                    (point, locus),
                    BUILD_OPERATION,
                    BUILD_OPERATION,
                )
            })?;
        }
        if let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() {
            let midpoint = Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5);
            let locus = role_locus(ctx, SketchLocusRole::Entity, entity.id(), BUILD_OPERATION)?;
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut line_midpoints,
                    &entity.sketch,
                    (midpoint, locus),
                    BUILD_OPERATION,
                    BUILD_OPERATION,
                )
            })?;
        }
        if let Some(marker) = entity.native_ref.as_deref().filter(|_| !is_point(entity)) {
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut nonpoint_carriers, marker, BUILD_OPERATION)
            })?;
        }
    }

    // A marker carried by an entity resolves to that entity; a later carrier
    // replaces an earlier one.
    let mut result = HashMap::<String, Vec<SketchLocus>>::new();
    for entity in ctx.admit_iter(entities.all(), RESULT_OPERATION)? {
        let (marker, qualified_point) = if let Some(marker) = entity.native_ref.as_deref() {
            (
                marker,
                is_point(entity)
                    && ctx.contains_hash_set(&nonpoint_carriers, marker, RESULT_OPERATION)?,
            )
        } else {
            let Some(reference) = entity.geometry_ref.as_deref() else {
                continue;
            };
            if ctx
                .strip_prefix(
                    reference,
                    "sldprt:feature-input:sketch-entity#",
                    RESULT_OPERATION,
                )?
                .is_none()
            {
                continue;
            }
            (reference, is_point(entity))
        };
        let Some(marker_record) = markers.get(ctx, marker, RESULT_OPERATION)? else {
            continue;
        };
        let role = if is_line(entity)
            && ctx.contains_text(
                entity.id().as_str(),
                "sketch-entity#compact:",
                RESULT_OPERATION,
            )? {
            SketchLocusRole::Start
        } else if matches!(
            marker_record.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        ) && matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Arc { .. }
                | SketchGeometryDefinition::Ellipse { .. }
        ) {
            SketchLocusRole::Center
        } else {
            SketchLocusRole::Entity
        };
        let key = qualified_point_key(ctx, marker, qualified_point, RESULT_OPERATION)?;
        let mut loci = Vec::new();
        ctx.push_vec(
            &mut loci,
            role_locus(ctx, role, entity.id(), RESULT_OPERATION)?,
            RESULT_OPERATION,
        )?;
        ctx.insert_hash_map(&mut result, key, loci, RESULT_OPERATION)?;
    }

    // Endpoint references add the start and end loci of their bounded entity.
    let mut endpoint_marker_keys = BTreeSet::new();
    for entity in ctx.admit_iter(entities.all(), ENDPOINT_OPERATION)? {
        let [start, end] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        for (marker, role) in [(start, SketchLocusRole::Start), (end, SketchLocusRole::End)] {
            if markers.get(ctx, marker, ENDPOINT_OPERATION)?.is_none() {
                continue;
            }
            let qualified = ctx.contains_hash_set(
                &qualified_point_markers,
                marker.as_str(),
                ENDPOINT_OPERATION,
            )?;
            for qualified_key in [false, true] {
                if qualified_key && !qualified {
                    continue;
                }
                let key = qualified_point_key(ctx, marker, qualified_key, ENDPOINT_OPERATION)?;
                if !ctx.contains_btree_set(
                    &endpoint_marker_keys,
                    key.as_str(),
                    ENDPOINT_OPERATION,
                )? {
                    let copy = ctx.copy_scoped_text(&key, &mut storage, ENDPOINT_OPERATION)?;
                    storage.with_storage(|| {
                        ctx.insert_btree_set(&mut endpoint_marker_keys, copy, ENDPOINT_OPERATION)
                    })?;
                }
                let loci = ctx
                    .entry_hash_map(&mut result, key, ENDPOINT_OPERATION)?
                    .or_default();
                append_profile_endpoint_locus(ctx, loci, entity.id(), role, ENDPOINT_OPERATION)?;
            }
        }
    }
    for key in ctx.admit_iter(&endpoint_marker_keys, ENDPOINT_OPERATION)? {
        if let Some(loci) = ctx.get_mut_hash_map(
            &mut result,
            key.as_str(),
            "lookup SLDPRT marker endpoint loci",
        )? {
            canonicalize_physical_loci(ctx, loci, entities, QUANTUM)?;
        }
    }

    // A marker with coordinates and no carrier resolves through the feature's
    // marker transforms to the profile loci at its transformed positions.
    for lane in ctx.admit_iter(lanes, GROUP_OPERATION)? {
        let mut markers_by_feature = BTreeMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, GROUP_OPERATION)? {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some()
                && ctx.contains_key_hash_map(&sketches_by_feature, feature, GROUP_OPERATION)?
            {
                storage.with_storage(|| {
                    ctx.push_btree_group(
                        &mut markers_by_feature,
                        feature,
                        marker,
                        GROUP_OPERATION,
                        GROUP_OPERATION,
                    )
                })?;
            }
        }
        for (feature, feature_markers) in ctx.admit_iter(&markers_by_feature, GROUP_OPERATION)? {
            let Some(sketch) = ctx
                .get_hash_map(&sketches_by_feature, *feature, GROUP_OPERATION)?
                .copied()
            else {
                continue;
            };
            let Some(loci) = ctx.get_hash_map(&profile_loci, sketch, GROUP_OPERATION)? else {
                continue;
            };
            let feature_transforms = ctx
                .get_hash_map(transforms, *feature, GROUP_OPERATION)?
                .map_or(&[][..], Vec::as_slice);
            let mut loci_by_point = HashMap::<GridPoint, Vec<&SketchLocus>>::new();
            for (point, locus) in ctx.admit_iter(loci, GROUP_OPERATION)? {
                storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut loci_by_point,
                        quantize(*point, QUANTUM),
                        locus,
                        GROUP_OPERATION,
                        GROUP_OPERATION,
                    )
                })?;
            }
            for marker in ctx
                .admit_iter(feature_markers, TRANSFORM_OPERATION)?
                .copied()
            {
                let qualified_point = ctx.contains_hash_set(
                    &qualified_point_markers,
                    marker.id(),
                    TRANSFORM_OPERATION,
                )?;
                let result_key =
                    qualified_point_key(ctx, marker.id(), qualified_point, TRANSFORM_OPERATION)?;
                if ctx.contains_key_hash_map(&result, result_key.as_str(), TRANSFORM_OPERATION)? {
                    continue;
                }
                if qualified_point
                    && ctx.contains_text(sketch.as_str(), "sketch#compact:", TRANSFORM_OPERATION)?
                {
                    continue;
                }
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let primary_geometry_locus = usize::try_from(marker.offset())
                    .ok()
                    .is_some_and(|offset| marker_is_geometry_locus(&lane.native_payload, offset));
                let point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let translated_points = storage.with_storage(|| {
                    ctx.collect_btree_set(
                        ctx.admit_iter(feature_transforms, TRANSFORM_OPERATION)?
                            .filter_map(|transform| transform.apply(point)),
                        TRANSFORM_OPERATION,
                    )
                })?;
                let mut marker_loci = Vec::new();
                for translated in ctx
                    .admit_iter(&translated_points, TRANSFORM_OPERATION)?
                    .copied()
                {
                    let mut translated_loci = Vec::new();
                    let at_point = ctx
                        .get_hash_map(
                            &loci_by_point,
                            &GridPoint::from(translated),
                            TRANSFORM_OPERATION,
                        )?
                        .map_or(&[][..], Vec::as_slice);
                    for locus in ctx.admit_iter(at_point, TRANSFORM_OPERATION)?.copied() {
                        if !entities
                            .entity(ctx, locus_entity(locus), TRANSFORM_OPERATION)?
                            .is_some_and(|entity| {
                                marker_accepts_locus(marker.kind(), &entity.geometry)
                            })
                        {
                            continue;
                        }
                        let role = if !qualified_point
                            && matches!(
                                marker.kind(),
                                SketchInputKind::LineOrCircle | SketchInputKind::Arc
                            ) {
                            SketchLocusRole::Entity
                        } else {
                            SketchLocusRole::of_locus(locus)
                        };
                        let locus =
                            role_locus(ctx, role, locus_entity(locus), TRANSFORM_OPERATION)?;
                        ctx.push_vec(&mut translated_loci, locus, TRANSFORM_OPERATION)?;
                    }
                    if translated_loci.is_empty() && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        let midpoints = ctx
                            .get_hash_map(&line_midpoints, sketch, TRANSFORM_OPERATION)?
                            .map_or(&[][..], Vec::as_slice);
                        for (midpoint, locus) in ctx.admit_iter(midpoints, TRANSFORM_OPERATION)? {
                            if quantize(*midpoint, QUANTUM) == translated {
                                let locus = copy_locus(ctx, locus, TRANSFORM_OPERATION)?;
                                ctx.push_vec(&mut translated_loci, locus, TRANSFORM_OPERATION)?;
                            }
                        }
                    }
                    if translated_loci.is_empty()
                        && primary_geometry_locus
                        && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        for entity in ctx
                            .admit_iter(
                                entities.in_sketch(ctx, sketch, TRANSFORM_OPERATION)?,
                                TRANSFORM_OPERATION,
                            )?
                            .copied()
                        {
                            let SketchGeometryDefinition::Line { start, end } =
                                entity.geometry.definition()
                            else {
                                continue;
                            };
                            if point_on_quantized_segment(
                                translated,
                                quantize(start.get(), QUANTUM),
                                quantize(end.get(), QUANTUM),
                            ) {
                                let locus = role_locus(
                                    ctx,
                                    SketchLocusRole::Entity,
                                    entity.id(),
                                    TRANSFORM_OPERATION,
                                )?;
                                ctx.push_vec(&mut translated_loci, locus, TRANSFORM_OPERATION)?;
                            }
                        }
                    }
                    sort_profile_loci(ctx, &mut translated_loci, TRANSFORM_OPERATION)?;
                    if qualified_point {
                        canonicalize_physical_loci(ctx, &mut translated_loci, entities, QUANTUM)?;
                    }
                    if !translated_loci.is_empty() {
                        storage.with_storage(|| {
                            ctx.push_vec(&mut marker_loci, translated_loci, TRANSFORM_OPERATION)
                        })?;
                    }
                }
                let Some(first) = marker_loci.first() else {
                    continue;
                };
                if ctx.all_by(
                    &marker_loci,
                    |candidate| ctx.equal(candidate, first, TRANSFORM_OPERATION),
                    TRANSFORM_OPERATION,
                )? {
                    if let Some(loci) = marker_loci.into_iter().next() {
                        ctx.insert_hash_map(&mut result, result_key, loci, TRANSFORM_OPERATION)?;
                    }
                }
            }
        }
    }

    // A line marker whose two endpoint markers transform onto exactly one
    // profile line resolves to that line.
    for lane in ctx.admit_iter(lanes, PAIR_OPERATION)? {
        for marker in ctx.admit_iter(&lane.sketch_entities, PAIR_OPERATION)? {
            if marker.kind() != SketchInputKind::LineOrCircle
                || !markers.is_indexed(ctx, marker)?
                || ctx.contains_key_hash_map(&result, marker.id(), PAIR_OPERATION)?
            {
                continue;
            }
            let endpoints = line_endpoint_markers_in(ctx, marker, markers)?;
            let (Some(feature), [first, second]) =
                (marker.feature_ref.as_deref(), endpoints.as_slice())
            else {
                continue;
            };
            let (Some(sketch_id), Some(first), Some(second)) = (
                ctx.get_hash_map(&sketches_by_feature, feature, PAIR_OPERATION)?
                    .copied(),
                first.coordinates_m,
                second.coordinates_m,
            ) else {
                continue;
            };
            let first_native = quantize(
                Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
                QUANTUM,
            );
            let second_native = quantize(
                Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
                QUANTUM,
            );
            let candidates = ctx
                .get_hash_map(transforms, feature, PAIR_OPERATION)?
                .map_or(&[][..], Vec::as_slice);
            let endpoint_pairs = storage.with_storage(|| {
                ctx.collect_btree_set(
                    ctx.admit_iter(candidates, PAIR_OPERATION)?
                        .filter_map(|transform| {
                            Some((
                                transform.apply(first_native)?,
                                transform.apply(second_native)?,
                            ))
                        }),
                    PAIR_OPERATION,
                )
            })?;
            if endpoint_pairs.is_empty() {
                continue;
            }
            let roster = entities.in_sketch(ctx, sketch_id, PAIR_OPERATION)?;
            let mut selected: Option<&SketchEntityId> = None;
            let complete = ctx.all_by(
                endpoint_pairs.iter().copied(),
                |(start, end)| {
                    let candidate = unique_entity(ctx, roster, PAIR_OPERATION, |entity| {
                        let SketchGeometryDefinition::Line {
                            start: candidate_start,
                            end: candidate_end,
                        } = entity.geometry.definition()
                        else {
                            return Ok(false);
                        };
                        let candidate_start = quantize(candidate_start.get(), QUANTUM);
                        let candidate_end = quantize(candidate_end.get(), QUANTUM);
                        Ok((candidate_start == start && candidate_end == end)
                            || (candidate_start == end && candidate_end == start))
                    })?;
                    let Some(candidate) = candidate else {
                        return Ok(false);
                    };
                    match selected {
                        Some(previous) => {
                            if !ctx.equal(previous, candidate.id(), PAIR_OPERATION)? {
                                return Ok(false);
                            }
                        }
                        None => selected = Some(candidate.id()),
                    }
                    Ok(true)
                },
                PAIR_OPERATION,
            )?;
            let (true, Some(entity)) = (complete, selected) else {
                continue;
            };
            let key = ctx.copy_retained_text(marker.id(), PAIR_OPERATION)?;
            let mut loci = Vec::new();
            ctx.push_vec(
                &mut loci,
                role_locus(ctx, SketchLocusRole::Entity, entity, PAIR_OPERATION)?,
                PAIR_OPERATION,
            )?;
            ctx.insert_hash_map(&mut result, key, loci, PAIR_OPERATION)?;
        }
    }

    // A point marker without coordinates resolves through its links to the one
    // endpoint they share; each pass can resolve markers the previous pass
    // made reachable.
    let (mut pending, _pending_storage) = ctx.with_scoped_storage(LINKED_OPERATION, || {
        let mut pending = Vec::new();
        for lane in ctx.admit_iter(lanes, LINKED_OPERATION)? {
            for marker in ctx.admit_iter(&lane.sketch_entities, LINKED_OPERATION)? {
                if marker.coordinates_m.is_none()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                    && markers.is_indexed(ctx, marker)?
                    && !ctx.contains_key_hash_map(&result, marker.id(), LINKED_OPERATION)?
                {
                    ctx.push_vec(&mut pending, marker, LINKED_OPERATION)?;
                }
            }
        }
        Ok::<_, CodecError>(pending)
    })?;
    loop {
        let mut additions_storage = ctx.reserve_scoped(0, LINKED_OPERATION)?;
        let mut additions = Vec::new();
        ctx.retain_vec(
            &mut pending,
            |marker| {
                let Some(locus) = unique_linked_endpoint_locus(
                    ctx,
                    marker,
                    markers.by_id(),
                    &result,
                    entities.by_id(),
                    QUANTUM,
                )?
                else {
                    return Ok(true);
                };
                additions_storage.with_storage(|| {
                    ctx.push_vec(&mut additions, (marker.id(), locus), LINKED_OPERATION)
                })?;
                Ok(false)
            },
            LINKED_OPERATION,
        )?;
        if additions.is_empty() {
            break;
        }
        for (marker, locus) in ctx.admit_iter(additions, LINKED_OPERATION)? {
            let key = ctx.copy_retained_text(marker, LINKED_OPERATION)?;
            let mut loci = Vec::new();
            ctx.push_vec(&mut loci, locus, LINKED_OPERATION)?;
            ctx.insert_hash_map(&mut result, key, loci, LINKED_OPERATION)?;
        }
    }
    Ok(result)
}

/// Each feature's planar sketch, by feature native reference; a later feature
/// with the same reference replaces an earlier one.
fn planar_sketches_by_feature<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    features: &'a [cadmpeg_ir::features::Feature],
) -> Result<HashMap<&'a str, &'a SketchId>, CodecError> {
    const OPERATION: &str = "index SLDPRT feature profile sketches";
    let mut sketches_by_feature = HashMap::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut sketches_by_feature, native_ref, sketch, OPERATION)
        })?;
    }
    Ok(sketches_by_feature)
}

pub(super) fn unique_linked_endpoint_locus(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities_by_id: &HashMap<&SketchEntityId, &SketchEntity>,
    quantum: f64,
) -> Result<Option<SketchLocus>, CodecError> {
    const OPERATION: &str = "select SLDPRT linked endpoint locus";
    if marker.links().len() < 2 {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut groups = Vec::<BTreeMap<GridPoint, Vec<(&SketchEntityId, SketchLocusRole)>>>::new();
    let mut sketch: Option<&SketchId> = None;
    if !ctx.all_by(
        marker.links(),
        |link| {
            let identities = marker_entities(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                MarkerEntityFilter::All,
            )?;
            if identities.is_empty() {
                return Ok(false);
            }
            let mut endpoints = BTreeMap::new();
            if !ctx.all_by(
                &identities,
                |identity| {
                    let Some(entity) = ctx
                        .get_hash_map(entities_by_id, identity, OPERATION)?
                        .copied()
                    else {
                        return Ok(false);
                    };
                    match sketch {
                        Some(previous) => {
                            if !ctx.equal(previous, &entity.sketch, OPERATION)? {
                                return Ok(false);
                            }
                        }
                        None => sketch = Some(&entity.sketch),
                    }
                    for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
                        if matches!(role, SketchLocusRole::Center) {
                            continue;
                        }
                        storage.with_storage(|| {
                            ctx.push_btree_group(
                                &mut endpoints,
                                quantize(point, quantum),
                                (entity.id(), role),
                                OPERATION,
                                OPERATION,
                            )
                        })?;
                    }

                    Ok(true)
                },
                OPERATION,
            )? {
                return Ok(false);
            }
            if endpoints.is_empty() {
                return Ok(false);
            }
            storage.with_storage(|| ctx.push_vec(&mut groups, endpoints, OPERATION))?;

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some((first, rest)) = groups.split_first() else {
        return Ok(None);
    };
    let mut shared = None;
    if !ctx.all_by(
        first,
        |(point, _)| {
            if !ctx.all_by(
                rest,
                |group| ctx.contains_key_btree_map(group, point, OPERATION),
                OPERATION,
            )? {
                return Ok(true);
            }
            if shared.is_some() {
                return Ok(false);
            }
            shared = Some(*point);

            Ok(true)
        },
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some(point) = shared else {
        return Ok(None);
    };
    let mut selected: Option<(&SketchEntityId, SketchLocusRole)> = None;
    for group in ctx.admit_iter(&groups, OPERATION)? {
        let at_point = ctx
            .get_btree_map(group, &point, OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        for &(entity, role) in ctx.admit_iter(at_point, OPERATION)? {
            let earlier = match selected {
                None => true,
                Some((other_entity, other_role)) => ctx
                    .compare(entity.as_str(), other_entity.as_str(), OPERATION)?
                    .then_with(|| role.cmp(&other_role))
                    .is_lt(),
            };
            if earlier {
                selected = Some((entity, role));
            }
        }
    }
    let Some((entity, role)) = selected else {
        return Ok(None);
    };
    Ok(Some(role_locus(ctx, role, entity, OPERATION)?))
}

fn point_on_quantized_segment(point: (i64, i64), start: GridPoint, end: GridPoint) -> bool {
    let (Some(start), Some(end)) = (start.cells(), end.cells()) else {
        return false;
    };
    let ab = (
        i128::from(end.0) - i128::from(start.0),
        i128::from(end.1) - i128::from(start.1),
    );
    let ap = (
        i128::from(point.0) - i128::from(start.0),
        i128::from(point.1) - i128::from(start.1),
    );
    let Some(cross) =
        ab.0.checked_mul(ap.1)
            .and_then(|left| left.checked_sub(ab.1.checked_mul(ap.0)?))
    else {
        return false;
    };
    let Some(projection) =
        ab.0.checked_mul(ap.0)
            .and_then(|left| left.checked_add(ab.1.checked_mul(ap.1)?))
    else {
        return false;
    };
    let Some(squared_length) =
        ab.0.checked_mul(ab.0)
            .and_then(|left| left.checked_add(ab.1.checked_mul(ab.1)?))
    else {
        return false;
    };
    squared_length != 0 && cross == 0 && (0..=squared_length).contains(&projection)
}

pub(super) fn marker_transform_candidates_by_feature<'a>(
    ctx: &DecodeContext<'_>,
    features: &'a [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<&'a str, Vec<MarkerTransform>>, CodecError> {
    let (entities, _entity_storage) = ctx
        .with_scoped_storage("index SLDPRT marker transform entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    marker_transform_candidates_in(ctx, features, sketches, &entities, lanes)
}

/// The transforms from each feature's marker coordinates to its sketch profile.
pub(super) fn marker_transform_candidates_in<'a>(
    ctx: &DecodeContext<'_>,
    features: &'a [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    entities: &ProfileEntities<'_>,
    lanes: &[FeatureInputLane],
) -> Result<HashMap<&'a str, Vec<MarkerTransform>>, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = RELATION_GEOMETRY_QUANTUM_MM;
    const OPERATION: &str = "index SLDPRT marker transform candidates";

    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let sketches_by_feature = planar_sketches_by_feature(ctx, &mut storage, features)?;
    // The sketch records are indexed when a feature first needs its frame.
    let mut sketch_records: Option<HashMap<&SketchId, &cadmpeg_ir::sketches::Sketch>> = None;
    let mut result = HashMap::<&'a str, Vec<MarkerTransform>>::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        let mut markers_by_feature = BTreeMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some()
                && ctx.contains_key_hash_map(&sketches_by_feature, feature, OPERATION)?
            {
                storage.with_storage(|| {
                    ctx.push_btree_group(
                        &mut markers_by_feature,
                        feature,
                        marker,
                        OPERATION,
                        OPERATION,
                    )
                })?;
            }
        }
        for (feature, markers) in ctx.admit_iter(&markers_by_feature, OPERATION)? {
            let Some((canonical_feature, sketch)) =
                ctx.get_key_value_hash_map(&sketches_by_feature, *feature, OPERATION)?
            else {
                continue;
            };
            let roster = entities.in_sketch(ctx, sketch, OPERATION)?;
            if roster.is_empty() {
                continue;
            }
            let mut directly_bound = BTreeMap::<GridPoint, BTreeSet<GridPoint>>::new();
            for marker in ctx.admit_iter(markers, OPERATION)?.copied() {
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                for entity in ctx
                    .admit_iter(
                        entities.with_native_ref(ctx, marker.id(), OPERATION)?,
                        OPERATION,
                    )?
                    .copied()
                {
                    if !in_sketch(ctx, entity, sketch, OPERATION)? {
                        continue;
                    }
                    let anchors = match *entity.geometry.definition() {
                        SketchGeometryDefinition::Point { position } => {
                            [Some(position.get()), None, None]
                        }
                        _ => marker_geometry_anchors(marker.kind(), &entity.geometry),
                    };
                    for anchor in anchors.into_iter().flatten() {
                        insert_compatible_locus(
                            ctx,
                            &mut storage,
                            &mut directly_bound,
                            native,
                            quantize(anchor, QUANTUM),
                        )?;
                    }
                }
            }
            let direct = compatible_marker_transform_candidates(ctx, &directly_bound)?;
            let candidates = if direct.len() == 1 {
                direct
            } else {
                let primary = compatible_marker_transform_candidates(
                    ctx,
                    &compatible_marker_loci(
                        ctx,
                        &mut storage,
                        markers,
                        roster,
                        &lane.native_payload,
                        true,
                    )?,
                )?;
                if primary.len() == 1 {
                    primary
                } else {
                    let fallback = compatible_marker_transform_candidates(
                        ctx,
                        &compatible_marker_loci(
                            ctx,
                            &mut storage,
                            markers,
                            roster,
                            &lane.native_payload,
                            false,
                        )?,
                    )?;
                    if fallback.is_empty() {
                        primary
                    } else {
                        fallback
                    }
                }
            };
            if sketch_records.is_none() {
                let mut records = HashMap::new();
                for record in ctx.admit_iter(sketches, OPERATION)? {
                    storage.with_storage(|| {
                        ctx.entry_hash_map(&mut records, &record.id, OPERATION)?
                            .or_insert(record);
                        Ok::<_, CodecError>(())
                    })?;
                }
                sketch_records = Some(records);
            }
            let record = match &sketch_records {
                Some(records) => ctx.get_hash_map(records, *sketch, OPERATION)?.copied(),
                None => None,
            };
            let mut candidates = candidates;
            if candidates.is_empty() {
                if let Some(transform) =
                    record.and_then(|record| sketch_frame_marker_transform(record, QUANTUM))
                {
                    ctx.push_vec(&mut candidates, transform, OPERATION)?;
                }
            }
            if !candidates.is_empty() {
                ctx.insert_hash_map(&mut result, *canonical_feature, candidates, OPERATION)?;
            }
        }
    }
    Ok(result)
}

/// The profile loci each marker position of one feature may correspond to:
/// for `primary_only`, the loci of the entities a geometry-locus marker accepts;
/// otherwise the anchors of every entity in the sketch.
fn compatible_marker_loci(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    markers: &[&SketchInputEntity],
    roster: &[&SketchEntity],
    payload: &[u8],
    primary_only: bool,
) -> Result<BTreeMap<GridPoint, BTreeSet<GridPoint>>, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = RELATION_GEOMETRY_QUANTUM_MM;
    const OPERATION: &str = "index SLDPRT marker transform candidates";
    let mut points = BTreeMap::<GridPoint, BTreeSet<GridPoint>>::new();
    for marker in ctx.admit_iter(markers, OPERATION)?.copied() {
        if !matches!(
            marker.kind(),
            SketchInputKind::Point
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
                | SketchInputKind::ConstrainedPoint
        ) {
            continue;
        }
        let Some([u, v]) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        if primary_only
            && index_from_u64(marker.offset())
                .is_none_or(|offset| !marker_is_geometry_locus(payload, offset))
        {
            continue;
        }
        let marker_point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
        for entity in ctx.admit_iter(roster, OPERATION)?.copied() {
            if primary_only {
                if !marker_accepts_locus(marker.kind(), &entity.geometry) {
                    continue;
                }
                for (point, _) in sketch_entity_locus_points(entity).into_iter().flatten() {
                    insert_compatible_locus(
                        ctx,
                        storage,
                        &mut points,
                        marker_point,
                        quantize(point, QUANTUM),
                    )?;
                }
            } else {
                for point in marker_geometry_anchors(marker.kind(), &entity.geometry)
                    .into_iter()
                    .flatten()
                {
                    insert_compatible_locus(
                        ctx,
                        storage,
                        &mut points,
                        marker_point,
                        quantize(point, QUANTUM),
                    )?;
                }
            }
        }
    }
    Ok(points)
}

fn insert_compatible_locus(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    points: &mut BTreeMap<GridPoint, BTreeSet<GridPoint>>,
    marker: GridPoint,
    locus: GridPoint,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT compatible marker loci";
    storage.with_storage(|| {
        let loci = ctx.entry_btree_map(points, marker, OPERATION)?.or_default();
        ctx.insert_btree_set(loci, locus, OPERATION).map(|_| ())
    })
}

fn marker_geometry_anchors(
    kind: SketchInputKind,
    geometry: &SketchGeometry,
) -> [Option<Point2>; 3] {
    match (kind, geometry.definition()) {
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometryDefinition::Point { position },
        ) => [Some(position.get()), None, None],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometryDefinition::Line { start, end },
        ) => [Some(start.get()), Some(end.get()), None],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Arc { center, .. }
            | SketchGeometryDefinition::Ellipse { center, .. },
        ) => [Some(center.get()), None, None],
        (SketchInputKind::LineOrCircle, SketchGeometryDefinition::Line { start, end }) => [
            Some(start.get()),
            Some(end.get()),
            Some(Point2::new(
                (start.u + end.u) * 0.5,
                (start.v + end.v) * 0.5,
            )),
        ],
        (
            SketchInputKind::LineOrCircle,
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Ellipse { center, .. },
        )
        | (SketchInputKind::Arc, SketchGeometryDefinition::Arc { center, .. }) => {
            [Some(center.get()), None, None]
        }
        _ => [None; 3],
    }
}

pub(super) fn marker_accepts_locus(kind: SketchInputKind, geometry: &SketchGeometry) -> bool {
    match kind {
        SketchInputKind::Arc => {
            matches!(geometry.definition(), SketchGeometryDefinition::Arc { .. })
        }
        SketchInputKind::LineOrCircle => matches!(
            geometry.definition(),
            SketchGeometryDefinition::Line { .. }
                | SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Ellipse { .. }
        ),
        SketchInputKind::Point
        | SketchInputKind::ConstrainedPoint
        | SketchInputKind::Relation(_)
        | SketchInputKind::Native(_)
        | SketchInputKind::NativeHandle(_) => true,
    }
}

#[cfg(test)]
mod relation_loci_tests;
