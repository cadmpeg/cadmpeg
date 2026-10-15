// SPDX-License-Identifier: Apache-2.0
//! Project dimension constraint relations.

use crate::design::constraints::scalar_close;
use crate::design::feature_project::{design_dimension_unit, design_length};
use crate::design::geometry::{angle_in_sweep, sketch_entity_endpoints};
use crate::ids::native_stream_charged;
use crate::records::{
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionLocusGroup, DesignDimensionLocusPair,
        DesignDimensionRecipeRecord,
    },
    parameters::{
        DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    },
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation, SketchRelationOperand},
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::evaluator::PcurveEvaluatorLanes;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::NativeOperandField;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

macro_rules! dimension_resource {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        }
    };
}

fn copy_spatial_entity_members(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Result<Vec<cadmpeg_ir::sketches::SpatialSketchEntityId>, CodecError> {
    let mut members = Vec::new();
    for entity in ctx.admit_iter(entities, "f3d spatial carrier output source")? {
        let id = (entity.id()).try_clone_for_decode(ctx, "f3d spatial carrier output entity id")?;
        ctx.push_vec(&mut members, id, "f3d spatial carrier output member")?;
    }
    Ok(members)
}

fn copy_dimension_source_kind(
    ctx: &DecodeContext<'_>,
    parameter: &DesignParameter,
    operation: &'static str,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    let text = ctx.copy_retained_text(parameter.source_kind(), operation)?;
    cadmpeg_core::text::NonBlankString::for_decode(ctx, text, "validate nonblank text")?
        .ok_or_else(|| CodecError::malformed("validated dimension source kind is blank"))
}

fn copy_dimension_locus(
    ctx: &DecodeContext<'_>,
    locus: &cadmpeg_ir::sketches::SketchLocus,
    operation: &'static str,
) -> Result<cadmpeg_ir::sketches::SketchLocus, CodecError> {
    use cadmpeg_ir::sketches::SketchLocus;
    Ok(match locus {
        SketchLocus::Entity(id) => SketchLocus::Entity((id).try_clone_for_decode(ctx, operation)?),
        SketchLocus::Start(id) => SketchLocus::Start((id).try_clone_for_decode(ctx, operation)?),
        SketchLocus::End(id) => SketchLocus::End((id).try_clone_for_decode(ctx, operation)?),
        SketchLocus::Center(id) => SketchLocus::Center((id).try_clone_for_decode(ctx, operation)?),
    })
}

fn copy_dimension_entity_pair(
    ctx: &DecodeContext<'_>,
    first: &cadmpeg_ir::sketches::SketchEntityId,
    second: &cadmpeg_ir::sketches::SketchEntityId,
) -> Result<
    (
        cadmpeg_ir::sketches::SketchEntityId,
        cadmpeg_ir::sketches::SketchEntityId,
    ),
    CodecError,
> {
    Ok((
        (first).try_clone_for_decode(ctx, "f3d atomic first entity id")?,
        (second).try_clone_for_decode(ctx, "f3d atomic second entity id")?,
    ))
}

fn dimension_entity_ids_distinct(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d atomic entity uniqueness storage")?;
    let mut unique = HashSet::new();
    let mut source = entities.iter();
    while let Some(entity) = ctx.next_charged(&mut source, "f3d atomic entity uniqueness source")? {
        if ctx.contains_hash_set(&unique, entity.id(), "f3d atomic entity uniqueness lookup")? {
            return Ok(false);
        }
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut unique, entity.id(), "f3d atomic entity uniqueness")
                .map(|_| ())
        })?;
    }
    Ok(true)
}

fn copy_dimension_entity_members(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Result<Vec<cadmpeg_ir::sketches::SketchEntityId>, CodecError> {
    let mut members = Vec::new();
    for entity in ctx.admit_iter(entities, "f3d atomic output member source")? {
        ctx.push_vec(
            &mut members,
            (entity.id()).try_clone_for_decode(ctx, "f3d atomic member entity id")?,
            "f3d atomic member",
        )?;
    }
    Ok(members)
}

const EPS_DIMENSIONS_OWNER_SCOPED_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OWNER_SCOPED_LINE_LENGTH_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_UNIQUE_POINT_CLASS_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_LINE_LENGTH_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_COUNTED_OFFSET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RADIAL_DIMENSION_DEFINITION_AT_TOLERANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RADIAL_LOCUS_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_COINCIDENT_LOCI_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_MIDPOINT_CONSTRAINT_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RECIPE_LINEAR_DIMENSION_CANDIDATES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_POINT_LINE_SEPARATION_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_LINEAR_MEASUREMENT_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_COUNTED_OFFSET_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OFFSET_PARAMETER_FACTOR_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_LINE_ANGLE_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_OFFSET_CONSTRAINT_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OFFSET_SOURCE_REVERSED_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_PLANAR_POINT_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9: f64 = 1.0e-9;

const PRESENTATION_MIN_LINE_LENGTH: f64 = 1.0e-12;

struct DimensionPointClass<'entity, 'ctx> {
    members: Vec<(&'entity cadmpeg_ir::sketches::SketchEntity, Point2)>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

/// Record slices shared by every dimension-constraint projection: the sketch
/// placements, parameter and companion tables, the locus/group/annotation
/// dimension records, and the sketch geometry the loci reference.
pub(crate) struct DimensionConstraintInputs<'a> {
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) pairs: &'a [DesignDimensionLocusPair],
    pub(crate) groups: &'a [DesignDimensionLocusGroup],
    pub(crate) annotation_frames: &'a [DesignDimensionAnnotationFrame],
    pub(crate) null_pairs: &'a [DesignDimensionLocusPair],
    pub(crate) companions: &'a [DesignParameterCompanion],
    pub(crate) recipe_records: &'a [DesignDimensionRecipeRecord],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
}

pub(crate) fn container_only_dimension_companions<'a>(
    ctx: &DecodeContext<'_>,
    pairs: &'a [DesignDimensionLocusPair],
    null_pairs: &'a [DesignDimensionLocusPair],
    annotation_frames: &'a [DesignDimensionAnnotationFrame],
    groups: &'a [DesignDimensionLocusGroup],
    recipe_records: &'a [DesignDimensionRecipeRecord],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d dimension companion index storage")?;
    let (physical, governed) = storage.with_storage(|| -> Result<_, CodecError> {
        let mut physical = BTreeSet::new();
        let mut governed = HashSet::new();
        let mut entries = pairs
            .iter()
            .map(|pair| {
                (
                    pair.id.as_str(),
                    Some(pair.companion_record_index),
                    pair.governing_companion_record_index,
                )
            })
            .chain(null_pairs.iter().map(|pair| {
                (
                    pair.id.as_str(),
                    Some(pair.companion_record_index),
                    pair.governing_companion_record_index,
                )
            }))
            .chain(annotation_frames.iter().map(|frame| {
                (
                    frame.id.as_str(),
                    frame.companion_record_index,
                    frame.governing_companion_record_index,
                )
            }))
            .chain(groups.iter().map(|group| {
                (
                    group.id.as_str(),
                    Some(group.companion_record_index),
                    group.companion_record_index,
                )
            }))
            .chain(recipe_records.iter().map(|record| {
                (
                    record.id.as_str(),
                    Some(record.companion_record_index),
                    record.companion_record_index,
                )
            }));
        while let Some((id, companion, owner)) =
            ctx.next_charged(&mut entries, "f3d dimension companion source scan")?
        {
            let Some(stream) = native_stream_charged(ctx, id, "resolve F3D dimension stream")?
            else {
                continue;
            };
            if let Some(companion) = companion {
                ctx.insert_btree_set(
                    &mut physical,
                    (stream, companion),
                    "f3d physical dimension companion",
                )?;
            }
            ctx.insert_hash_set(
                &mut governed,
                (stream, owner),
                "f3d governed dimension companion",
            )?;
        }
        Ok((physical, governed))
    })?;
    let mut container_only = HashSet::new();
    for entry in ctx.admit_iter(physical, "f3d dimension companion difference scan")? {
        if !ctx.contains_hash_set(&governed, &entry, "f3d governed dimension companion lookup")? {
            ctx.insert_hash_set(
                &mut container_only,
                entry,
                "f3d container-only dimension companion",
            )?;
        }
    }
    Ok(container_only)
}

/// Project dimensional parameter companions into parameter-backed sketch
/// constraints. Solved linear measurements use the source kernel's absolute
/// resolution. Two-locus dimensions have neutral semantics; aggregate and
/// role-dependent forms remain explicit native constraints.
pub(crate) fn project_dimension_constraints(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    linear_tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    let constraints = project_all_dimension_constraints(ctx, inputs, &[], linear_tolerance)?;
    retain_planar_dimension_constraints(ctx, inputs.placements, spatial_sketches, constraints)
}

/// Project planar dimensions with direct Fusion presentation frames. The
/// presentation carrier is kept separate from the established locus inputs so
/// callers that construct dimension fixtures do not need to synthesize an
/// unrelated native arena.
pub(crate) fn project_dimension_constraints_with_presentations(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionConstraintInputs<'_>,
    presentation_frames: &[crate::records::dimensions::DesignDimensionPresentationFrame],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    linear_tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    let constraints =
        project_all_dimension_constraints(ctx, inputs, presentation_frames, linear_tolerance)?;
    retain_planar_dimension_constraints(ctx, inputs.placements, spatial_sketches, constraints)
}

fn retain_planar_dimension_constraints(
    ctx: &DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    mut constraints: Vec<cadmpeg_ir::sketches::SketchConstraint>,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    let mut spatial_storage = ctx.reserve_scoped(0, "f3d planar spatial sketch storage")?;
    let mut spatial_sketch_ids = HashSet::new();
    for sketch in ctx.admit_iter(spatial_sketches, "f3d planar spatial sketch source")? {
        spatial_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut spatial_sketch_ids,
                &sketch.id,
                "f3d planar spatial sketch index",
            )
        })?;
    }
    let mut index_storage = ctx.reserve_scoped(0, "f3d planar excluded sketch storage")?;
    let mut excluded = HashMap::new();
    if !constraints.is_empty() && !spatial_sketch_ids.is_empty() {
        for placement in ctx.admit_iter(placements, "f3d planar placement source")? {
            let mut temporary = ctx.reserve_scoped(0, "f3d planar spatial identity storage")?;
            let spatial_id = temporary.with_storage(|| {
                crate::design::identity::neutral_spatial_sketch_id(ctx, placement)
            })?;
            let is_spatial = ctx.contains_hash_set(
                &spatial_sketch_ids,
                &spatial_id,
                "f3d planar spatial sketch lookup",
            )?;
            drop(spatial_id);
            drop(temporary);
            if !is_spatial {
                continue;
            }
            let mut identity_storage =
                ctx.reserve_scoped(0, "f3d planar sketch identity storage")?;
            let sketch_id = identity_storage
                .with_storage(|| crate::design::identity::neutral_sketch_id(ctx, placement))?;
            if ctx.contains_key_hash_map(
                &excluded,
                &sketch_id,
                "f3d planar excluded sketch lookup",
            )? {
                continue;
            }
            // The reservation stays with the owned index key until its drop.
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut excluded,
                    sketch_id,
                    identity_storage,
                    "f3d planar excluded sketch index",
                )
            })?;
        }
    }
    ctx.retain_vec(
        &mut constraints,
        |constraint| {
            Ok(!ctx.contains_key_hash_map(
                &excluded,
                &constraint.sketch,
                "f3d planar excluded sketch lookup",
            )?)
        },
        "f3d planar dimension source",
    )?;
    Ok(constraints)
}

fn project_all_dimension_constraints(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionConstraintInputs<'_>,
    presentation_frames: &[crate::records::dimensions::DesignDimensionPresentationFrame],
    linear_tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
        SketchNativeOperand,
    };

    let &DimensionConstraintInputs {
        placements,
        parameters: parameters_source,
        owners,
        pairs,
        groups,
        annotation_frames,
        null_pairs,
        companions,
        recipe_records,
        points,
        curves,
        entities,
    } = inputs;

    let mut index_storage = ctx.reserve_scoped(0, "f3d dimension index storage")?;
    let mut sketches = HashMap::new();
    let mut sketches_by_scope = HashMap::new();
    for placement in ctx.admit_iter(placements, "f3d dimension placement source")? {
        let Some(scope) =
            native_stream_charged(ctx, &placement.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        if let Ok(suffix) = u32::try_from(placement.entity_id.suffix()) {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut sketches,
                    (scope, suffix),
                    placement,
                    "f3d dimension sketch index",
                )
            })?;
        }
        if let Some(scope_record_index) = placement.scope_record_index {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut sketches_by_scope,
                    (scope, scope_record_index),
                    placement,
                    "f3d dimension scope sketch index",
                )
            })?;
        }
    }
    let mut parameters = HashMap::new();
    for parameter in ctx.admit_iter(parameters_source, "f3d dimension parameter source")? {
        if let Some(scope) =
            native_stream_charged(ctx, &parameter.id, "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut parameters,
                    (scope, parameter.record_index),
                    parameter,
                    "f3d dimension parameter index",
                )
            })?;
        }
    }
    let mut parameter_by_companion = HashMap::new();
    for owner in ctx.admit_iter(owners, "f3d dimension owner source")? {
        if let Some(scope) = native_stream_charged(ctx, owner.id(), "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut parameter_by_companion,
                    (scope, owner.companion_record_index()),
                    owner.parameter_record_index(),
                    "f3d dimension companion index",
                )
            })?;
        }
    }
    let mut native_geometry = BTreeMap::new();
    for point in ctx.admit_iter(points, "f3d dimension point source")? {
        if let Some(scope) = native_stream_charged(ctx, &point.id, "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_geometry,
                    (scope, point.record_index),
                    (
                        cadmpeg_core::nonblank_literal!("point"),
                        point.owner_reference,
                        point.id.as_str(),
                    ),
                    "f3d dimension native geometry index",
                )
            })?;
        }
    }
    for curve in ctx.admit_iter(curves, "f3d dimension curve source")? {
        if let Some(scope) = native_stream_charged(ctx, &curve.id, "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_geometry,
                    (scope, curve.record_index),
                    (
                        cadmpeg_core::nonblank_literal!("curve"),
                        curve.owner_reference,
                        curve.id.as_str(),
                    ),
                    "f3d dimension native geometry index",
                )
            })?;
        }
    }
    let mut record_indices_by_native_ref = HashMap::new();
    for (key, (_, _, native_ref)) in
        ctx.admit_iter(&native_geometry, "f3d dimension native reference source")?
    {
        index_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut record_indices_by_native_ref,
                *native_ref,
                *key,
                "f3d dimension native reference index",
            )
        })?;
    }
    let mut projected = BTreeMap::new();
    for entity in ctx.admit_iter(entities, "f3d dimension projected entity source")? {
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        if let Some(key) = ctx
            .get_hash_map(
                &record_indices_by_native_ref,
                native_ref,
                "f3d dimension native reference lookup",
            )?
            .copied()
        {
            index_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut projected,
                    key,
                    entity,
                    "f3d dimension projected entity index",
                )
            })?;
        }
    }
    let mut curve_secondary_ids = HashMap::new();
    for curve in ctx.admit_iter(curves, "f3d dimension secondary curve source")? {
        if let Some(scope) = native_stream_charged(ctx, &curve.id, "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut curve_secondary_ids,
                    (scope, curve.record_index),
                    curve.secondary_id,
                    "f3d dimension curve secondary index",
                )
            })?;
        }
    }

    let parameter_for = |scope: &str, companion_record_index: u32| {
        let record_index = *dimension_resource!(ctx.get_hash_map(
            &parameter_by_companion,
            &(scope, companion_record_index),
            "f3d dimension companion parameter lookup",
        ))?;
        let parameter = *dimension_resource!(ctx.get_hash_map(
            &parameters,
            &(scope, record_index),
            "f3d dimension parameter lookup",
        ))?;
        Some(
            crate::design::identity::neutral_parameter_id_with_operation(
                ctx,
                parameter,
                "f3d parameter identifier",
            )
            .map(|id| (parameter, id)),
        )
    };
    let presentation_for_owner = |scope: &str,
                                  owner_record_index: u32|
     -> Result<
        Option<&crate::records::dimensions::DesignDimensionPresentationFrame>,
        CodecError,
    > {
        let matches = |frame: &crate::records::dimensions::DesignDimensionPresentationFrame|
            -> Result<bool, CodecError> {
            Ok(frame.governing_owner_record_index == owner_record_index
                && match native_stream_charged(ctx, &frame.id, "resolve F3D dimension stream")? {
                    Some(candidate) => ctx.equal(candidate, scope, "f3d dimension presentation stream")?,
                    None => false,
                })
        };
        let Some(first) = ctx.position_by(
            presentation_frames,
            matches,
            "f3d dimension presentation source",
        )?
        else {
            return Ok(None);
        };
        if ctx.any_by(
            &presentation_frames[first + 1..],
            matches,
            "f3d dimension presentation source",
        )? {
            return Ok(None);
        }
        Ok(Some(&presentation_frames[first]))
    };
    let sketch_for_geometry = |scope: &str,
                               indices: &[u32],
                               operation|
     -> Result<Option<cadmpeg_ir::sketches::SketchId>, CodecError> {
        let projected_sketch = ctx.find_map(
            indices,
            |record_index| {
                Ok(ctx
                    .get_btree_map(
                        &projected,
                        &(scope, *record_index),
                        "f3d dimension sketch entity lookup",
                    )?
                    .map(|entity| &entity.sketch))
            },
            "f3d dimension projected sketch source",
        )?;
        if let Some(sketch) = projected_sketch {
            if ctx.all_by(
                indices,
                |record_index| {
                    let Some(entity) = ctx.get_btree_map(
                        &projected,
                        &(scope, *record_index),
                        "f3d dimension sketch entity lookup",
                    )?
                    else {
                        return Ok(false);
                    };
                    ctx.equal(&entity.sketch, sketch, "f3d dimension sketch identity")
                },
                "f3d dimension projected sketch agreement",
            )? {
                return sketch.try_clone_for_decode(ctx, operation).map(Some);
            }
        }
        let owner = ctx.find_map(
            indices,
            |record_index| {
                Ok(ctx
                    .get_btree_map(
                        &native_geometry,
                        &(scope, *record_index),
                        "f3d dimension native owner lookup",
                    )?
                    .and_then(|geometry| geometry.1))
            },
            "f3d dimension native owner source",
        )?;
        let Some(owner) = owner else {
            return Ok(None);
        };
        if !ctx.all_by(
            indices,
            |record_index| {
                Ok(ctx
                    .get_btree_map(
                        &native_geometry,
                        &(scope, *record_index),
                        "f3d dimension native owner lookup",
                    )?
                    .and_then(|geometry| geometry.1)
                    .is_none_or(|candidate| candidate == owner))
            },
            "f3d dimension native owner agreement",
        )? {
            return Ok(None);
        }
        ctx.get_hash_map(
            &sketches,
            &(scope, owner),
            "f3d dimension owner sketch lookup",
        )?
        .map(|placement| {
            crate::design::identity::neutral_sketch_id_with_operation(ctx, placement, operation)
        })
        .transpose()
    };
    let sketch_for_owner_or_geometry =
        |scope: &str,
         owner: u32,
         indices: &[u32],
         operation|
         -> Result<Option<cadmpeg_ir::sketches::SketchId>, CodecError> {
            if let Some(placement) = ctx.get_hash_map(
                &sketches,
                &(scope, owner),
                "f3d dimension owner sketch lookup",
            )? {
                return crate::design::identity::neutral_sketch_id_with_operation(
                    ctx, placement, operation,
                )
                .map(Some);
            }
            sketch_for_geometry(scope, indices, operation)
        };
    let native_operand = |scope: &str,
                          field: cadmpeg_core::text::NonBlankString,
                          role: Option<u32>,
                          record_index: u32|
     -> Result<SketchNativeOperand, CodecError> {
        let geometry = ctx.get_btree_map(
            &native_geometry,
            &(scope, record_index),
            "f3d dimension native operand lookup",
        )?;
        let native_kind = match geometry {
            Some((kind, _, _)) => kind.try_clone_for_decode(ctx, "f3d dimension native kind")?,
            None => cadmpeg_core::nonblank_literal!("record"),
        };
        let native_ref = if let Some((_, _, native_ref)) = geometry {
            if ctx.contains_key_btree_map(
                &projected,
                &(scope, record_index),
                "f3d dimension native projected lookup",
            )? {
                None
            } else {
                Some(ctx.copy_retained_text(native_ref, "f3d dimension native operand reference")?)
            }
        } else {
            None
        };
        Ok(SketchNativeOperand {
            native_kind,
            field: Some(NativeOperandField { name: field, role }),
            object_index: Some(record_index),
            native_ref,
        })
    };
    let native_definition =
        |scope: &str,
         source_kind: cadmpeg_core::text::NonBlankString,
         state: Option<u64>,
         operands: &[(cadmpeg_core::text::NonBlankString, Option<u32>, u32)],
         parameter|
         -> Result<Definition, CodecError> {
            let mut entity_ids = Vec::new();
            let mut native_operands = Vec::new();
            for (field, role, record_index) in
                ctx.admit_iter(operands, "f3d native dimension operand source")?
            {
                if let Some(entity) = ctx.get_btree_map(
                    &projected,
                    &(scope, *record_index),
                    "f3d native dimension entity lookup",
                )? {
                    let id = entity
                        .id()
                        .try_clone_for_decode(ctx, "f3d native dimension entity id")?;
                    ctx.push_vec(&mut entity_ids, id, "f3d native dimension entity")?;
                }
                let operand = native_operand(
                    scope,
                    field.try_clone_for_decode(ctx, "f3d dimension operand field")?,
                    *role,
                    *record_index,
                )?;
                ctx.push_vec(
                    &mut native_operands,
                    operand,
                    "f3d native dimension operand",
                )?;
            }
            Ok(Definition::Native {
                native_kind: source_kind,
                native_state: state,
                native_flags: None,
                native_properties: BTreeMap::new(),
                entities: entity_ids,
                parameter: Some(parameter),
                operands: native_operands,
            })
        };
    let exact_definition = |scope: &str,
                            source_parameter: &DesignParameter,
                            indices: &[u32],
                            parameter: cadmpeg_ir::features::ParameterId|
     -> Result<Option<Definition>, CodecError> {
        if !design_dimension_unit(source_parameter) {
            return Ok(None);
        }
        let source_kind = source_parameter.source_kind();
        let evaluated_value = source_parameter.evaluated_value().get();
        let mut storage = ctx.reserve_scoped(0, "f3d exact dimension storage")?;
        let mut entities = Vec::new();
        let mut source = indices.iter();
        while let Some(record_index) =
            ctx.next_charged(&mut source, "f3d exact dimension source")?
        {
            let Some(entity) = ctx
                .get_btree_map(
                    &projected,
                    &(scope, *record_index),
                    "f3d exact dimension entity lookup",
                )?
                .copied()
            else {
                return Ok(None);
            };
            storage.with_storage(|| {
                ctx.push_vec(&mut entities, entity, "f3d exact dimension entity")
            })?;
        }
        if let [entity] = entities.as_slice() {
            if let Some(definition) = radial_dimension_definition(
                ctx,
                entity,
                source_kind,
                evaluated_value,
                (&parameter, "f3d exact radial parameter id"),
            )
            .transpose()?
            {
                return Ok(Some(definition));
            }
        }
        if let [first, second] = entities.as_slice() {
            if ctx.equal(first.id(), second.id(), "f3d exact dimension identity")? {
                return Ok(None);
            }
        }
        if source_kind.starts_with("Linear Dimension") && entities.len() == 2 {
            let evaluated_mm = evaluated_value * 10.0;
            if let Some(definition) = directional_point_dimension(
                ctx,
                &entities,
                evaluated_mm,
                (&parameter, "f3d exact directional parameter id"),
                linear_tolerance,
            )
            .transpose()?
            {
                return Ok(Some(definition));
            }
            if point_line_separation(entities[0], entities[1], evaluated_mm, linear_tolerance)
                || parallel_line_separation(
                    entities[0],
                    entities[1],
                    evaluated_mm,
                    linear_tolerance,
                )
                || concentric_circle_separation(
                    entities[0],
                    entities[1],
                    evaluated_mm,
                    linear_tolerance,
                )
            {
                let mut ids = Vec::new();
                for entity in &entities {
                    let id =
                        (entity.id()).try_clone_for_decode(ctx, "f3d exact distance entity id")?;
                    ctx.push_vec(&mut ids, id, "f3d exact distance entity")?;
                }
                return Ok(Some(Definition::Distance {
                    entities: ids,
                    parameter,
                }));
            }
            let (
                SketchGeometryDefinition::Point {
                    position: first_position,
                },
                SketchGeometryDefinition::Point {
                    position: second_position,
                },
            ) = (
                entities[0].geometry.definition(),
                entities[1].geometry.definition(),
            )
            else {
                return Ok(None);
            };
            let measured =
                (first_position.u - second_position.u).hypot(first_position.v - second_position.v);
            if linear_measurement_matches(measured, evaluated_mm, linear_tolerance) {
                return Ok(Some(Definition::DistanceLoci {
                    first: cadmpeg_ir::sketches::SketchLocus::Entity(
                        (entities[0].id())
                            .try_clone_for_decode(ctx, "f3d exact first distance locus id")?,
                    ),
                    second: cadmpeg_ir::sketches::SketchLocus::Entity(
                        (entities[1].id())
                            .try_clone_for_decode(ctx, "f3d exact second distance locus id")?,
                    ),
                    parameter,
                }));
            }
            return Ok(None);
        }
        if source_kind.starts_with("Angular Dimension")
            && entities.len() == 2
            && entities.iter().all(|entity| {
                matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            })
            && line_angle_matches(
                &entities[0].geometry,
                &entities[1].geometry,
                evaluated_value,
            )
        {
            return Ok(Some(Definition::Angle {
                first: (entities[0].id())
                    .try_clone_for_decode(ctx, "f3d exact first angle entity id")?,
                second: (entities[1].id())
                    .try_clone_for_decode(ctx, "f3d exact second angle entity id")?,
                parameter,
            }));
        }
        if source_kind.starts_with("Angular Dimension") && entities.len() == 2 {
            let Some((first, second)) =
                indirect_angular_lines(ctx, scope, &entities, evaluated_value, &projected)?
            else {
                return Ok(None);
            };
            return Ok(Some(Definition::Angle {
                first,
                second,
                parameter,
            }));
        }
        Ok(None)
    };
    let exact_group_definition = |scope: &str,
                                  group: &DesignDimensionLocusGroup,
                                  parameter: &DesignParameter,
                                  parameter_id: &cadmpeg_ir::features::ParameterId|
     -> Option<Result<Definition, CodecError>> {
        if !design_dimension_unit(parameter) {
            return None;
        }
        let mut storage = dimension_resource!(ctx.reserve_scoped(0, "f3d exact group storage"));
        let mut locus_entities = Vec::new();
        let mut source = group.loci.iter();
        while let Some(locus) =
            dimension_resource!(ctx.next_charged(&mut source, "f3d exact group locus source"))
        {
            let entity = dimension_resource!(ctx.get_btree_map(
                &projected,
                &(scope, locus.geometry_record_index),
                "f3d exact group locus lookup",
            ))
            .copied()?;
            dimension_resource!(storage.with_storage(|| ctx.push_vec(
                &mut locus_entities,
                entity,
                "f3d exact group locus entity"
            )));
        }
        if parameter.source_kind().starts_with("Angular Dimension") {
            let mut angular_storage =
                dimension_resource!(ctx.reserve_scoped(0, "f3d exact group angular storage"));
            let mut indices = Vec::new();
            for locus in dimension_resource!(ctx
                .admit_iter(&group.loci, "f3d exact group angular source")
                .map_err(CodecError::from))
            {
                dimension_resource!(angular_storage.with_storage(|| ctx.push_vec(
                    &mut indices,
                    locus.geometry_record_index,
                    "f3d exact group angular index",
                )));
            }
            let copied = match parameter_id
                .try_clone_for_decode(ctx, "f3d exact group angular parameter id")
            {
                Ok(copied) => copied,
                Err(error) => return Some(Err(error)),
            };
            match exact_definition(scope, parameter, &indices, copied) {
                Ok(Some(definition)) => return Some(Ok(definition)),
                Ok(None) => {}
                Err(error) => return Some(Err(error)),
            }
        }
        if group.state == 0 {
            let counted_definition = dimension_resource!(counted_role_relation_at_tolerance(
                ctx,
                &locus_entities,
                &group.owner_kinds(),
                linear_tolerance,
            )
            .transpose());
            if let Some(definition) = counted_definition {
                return Some(Ok(definition));
            }
            if let Some(definition) =
                exact_counted_dimension_relation(ctx, &locus_entities).transpose()
            {
                return Some(definition);
            }
        }
        if let Some(definition) = dimension_resource!(radial_locus_dimension_definition(
            ctx,
            &locus_entities,
            entities,
            parameter.source_kind(),
            parameter.evaluated_value().get(),
            parameter_id,
        )
        .transpose())
        {
            return Some(Ok(definition));
        }
        if parameter.source_kind().starts_with("Linear Dimension") {
            if group.state == 0x20 {
                let mut index_storage =
                    dimension_resource!(ctx.reserve_scoped(0, "f3d exact group offset storage"));
                let mut entities_by_record = HashMap::new();
                let mut secondary_ids = HashMap::new();
                for (locus, entity) in dimension_resource!(ctx
                    .admit_iter(&group.loci, "f3d exact group offset source")
                    .map_err(CodecError::from))
                .zip(&locus_entities)
                {
                    dimension_resource!(index_storage.with_storage(|| ctx
                        .insert_hash_map(
                            &mut entities_by_record,
                            locus.geometry_record_index,
                            *entity,
                            "f3d exact group offset entity index",
                        )
                        .map(|_| ())));
                    if let Some(secondary_id) = dimension_resource!(ctx.get_hash_map(
                        &curve_secondary_ids,
                        &(scope, locus.geometry_record_index),
                        "f3d exact group offset secondary lookup",
                    ))
                    .copied()
                    {
                        dimension_resource!(index_storage.with_storage(|| ctx
                            .insert_hash_map(
                                &mut secondary_ids,
                                locus.geometry_record_index,
                                secondary_id,
                                "f3d exact group offset secondary index",
                            )
                            .map(|_| ())));
                    }
                }
                let counted = exact_counted_offset(
                    ctx,
                    &group.loci,
                    &entities_by_record,
                    &secondary_ids,
                    linear_tolerance,
                )?;
                let CountedOffset { pairs, distance } = match counted {
                    Ok(counted) => counted,
                    Err(error) => return Some(Err(error)),
                };
                let parameter = offset_parameter_factor(
                    distance.get(),
                    parameter.evaluated_value().get() * 10.0,
                )
                .map(|factor| -> Result<_, CodecError> {
                    Ok(cadmpeg_ir::sketches::OffsetParameter {
                        id: parameter_id
                            .try_clone_for_decode(ctx, "f3d exact group offset parameter id")?,
                        negated: factor.is_sign_negative(),
                    })
                })
                .transpose();
                let parameter = dimension_resource!(parameter);
                return Some(Ok(Definition::Offset {
                    pairs,
                    distance,
                    parameter,
                }));
            }
            if let Some(definition) = dimension_resource!(directional_point_dimension(
                ctx,
                &locus_entities,
                parameter.evaluated_value().get() * 10.0,
                (parameter_id, "f3d exact group directional parameter id"),
                linear_tolerance,
            )
            .transpose())
            {
                return Some(Ok(definition));
            }
            if group.state == 0 {
                return dimension_resource!(two_locus_distance_dimension(
                    ctx,
                    &locus_entities,
                    parameter_id
                )
                .transpose())
                .map(Ok);
            }
        }
        None
    };
    let mut radial_extension_annotation_groups = HashSet::new();
    for group in ctx.admit_iter(groups, "f3d dimension classification source")? {
        let (candidate, classification_storage) =
            ctx.with_scoped_storage("f3d radial group classification storage", || {
                (|| {
                    let scope = dimension_resource!(native_stream_charged(
                        ctx,
                        &group.id,
                        "resolve F3D dimension stream"
                    ))?;
                    if group.state != 0 {
                        return None;
                    }
                    let (parameter, parameter_id) =
                        dimension_resource!(parameter_for(scope, group.companion_record_index)?);
                    match exact_group_definition(scope, group, parameter, &parameter_id) {
                        Some(Ok(_)) => return None,
                        Some(Err(error)) => return Some(Err(error)),
                        None => {}
                    }
                    let mut locus_entities = Vec::new();
                    for locus in dimension_resource!(ctx
                        .admit_iter(group.loci.as_slice(), "f3d dimension group locus source")
                        .map_err(CodecError::from))
                    {
                        let entity = dimension_resource!(ctx.get_btree_map(
                            &projected,
                            &(scope, locus.geometry_record_index),
                            "f3d radial group locus lookup",
                        ))
                        .copied()?;
                        if let Err(error) = ctx.push_vec(
                            &mut locus_entities,
                            entity,
                            "f3d radial group locus entity",
                        ) {
                            return Some(Err(error));
                        }
                    }
                    if !radial_extension_annotation_group(&locus_entities, parameter) {
                        return None;
                    }
                    let mut locus_indices = Vec::new();
                    for locus in dimension_resource!(ctx
                        .admit_iter(group.loci.as_slice(), "f3d dimension group locus source")
                        .map_err(CodecError::from))
                    {
                        if let Err(error) = ctx.push_vec(
                            &mut locus_indices,
                            locus.geometry_record_index,
                            "f3d radial dimension locus index",
                        ) {
                            return Some(Err(error));
                        }
                    }
                    let sketch = match sketch_for_owner_or_geometry(
                        scope,
                        group.owner_reference,
                        &locus_indices,
                        "f3d dimension radial sketch id",
                    ) {
                        Ok(Some(sketch)) => sketch,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    dimension_resource!(owner_scoped_radial_dimension_definition(
                        ctx,
                        entities,
                        &sketch,
                        parameter,
                        &parameter_id,
                        linear_tolerance,
                    )
                    .transpose())?;
                    Some(Ok((scope, group.record_index)))
                })()
                .transpose()
            })?;
        drop(classification_storage);
        if let Some(result) = candidate {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut radial_extension_annotation_groups,
                        result,
                        "f3d radial extension group",
                    )
                })
                .map(|_| ())?;
        }
    }
    let mut exact_pair_companions = HashSet::new();
    for pair in ctx.admit_iter(pairs, "f3d dimension classification source")? {
        let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let (exact, classification_storage) = ctx.with_scoped_storage(
            "f3d exact pair classification storage",
            || -> Result<bool, CodecError> {
                let Some((parameter, parameter_id)) =
                    parameter_for(scope, pair.governing_companion_record_index).transpose()?
                else {
                    return Ok(false);
                };
                let indices = [
                    pair.loci()[0].geometry_index(),
                    pair.loci()[1].geometry_index(),
                ];
                Ok(exact_definition(scope, parameter, &indices, parameter_id)?.is_some())
            },
        )?;
        drop(classification_storage);
        if exact {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut exact_pair_companions,
                        (scope, pair.governing_companion_record_index),
                        "f3d exact pair companion",
                    )
                })
                .map(|_| ())?;
        }
    }
    let mut parameterized_offset_companions = HashSet::new();
    for group in ctx.admit_iter(groups, "f3d dimension classification source")? {
        let Some(scope) = native_stream_charged(ctx, &group.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let (parameterized_offset, classification_storage) = ctx.with_scoped_storage(
            "f3d offset group classification storage",
            || -> Result<bool, CodecError> {
                let Some((parameter, parameter_id)) =
                    parameter_for(scope, group.companion_record_index).transpose()?
                else {
                    return Ok(false);
                };
                Ok(matches!(
                    exact_group_definition(scope, group, parameter, &parameter_id).transpose()?,
                    Some(Definition::Offset {
                        parameter: Some(_),
                        ..
                    })
                ))
            },
        )?;
        drop(classification_storage);
        if parameterized_offset {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut parameterized_offset_companions,
                        (scope, group.companion_record_index),
                        "f3d parameterized offset companion",
                    )
                })
                .map(|_| ())?;
        }
    }
    let mut projected_dimension_companions = HashSet::new();
    for pair in ctx.admit_iter(pairs, "f3d dimension classification source")? {
        if let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")? {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut projected_dimension_companions,
                        (scope, pair.governing_companion_record_index),
                        "f3d projected pair companion",
                    )
                })
                .map(|_| ())?;
        }
    }
    for frame in ctx.admit_iter(annotation_frames, "f3d dimension classification source")? {
        if let Some(scope) = native_stream_charged(ctx, &frame.id, "resolve F3D dimension stream")?
        {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut projected_dimension_companions,
                        (scope, frame.governing_companion_record_index),
                        "f3d projected annotation companion",
                    )
                })
                .map(|_| ())?;
        }
    }
    for pair in ctx.admit_iter(null_pairs, "f3d dimension classification source")? {
        if let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")? {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut projected_dimension_companions,
                        (scope, pair.governing_companion_record_index),
                        "f3d projected null-pair companion",
                    )
                })
                .map(|_| ())?;
        }
    }
    for group in ctx.admit_iter(groups, "f3d dimension classification source")? {
        let Some(scope) = native_stream_charged(ctx, &group.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        if ctx.contains_hash_set(
            &radial_extension_annotation_groups,
            &(scope, group.record_index),
            "f3d radial group classification lookup",
        )? {
            continue;
        }
        let (projects_parameter, classification_storage) =
            ctx.with_scoped_storage("f3d projected group classification storage", || {
                let Some((parameter, parameter_id)) =
                    parameter_for(scope, group.companion_record_index).transpose()?
                else {
                    return Ok(false);
                };
                let definition =
                    exact_group_definition(scope, group, parameter, &parameter_id).transpose()?;
                match definition {
                    None => Ok(true),
                    Some(definition) => ctx.any_by(
                        constraint_parameters(&definition),
                        |id| ctx.equal(id, &parameter_id, "f3d projected group parameter equality"),
                        "f3d projected group parameter source",
                    ),
                }
            })?;
        drop(classification_storage);
        if projects_parameter {
            index_storage
                .with_storage(|| {
                    ctx.insert_hash_set(
                        &mut projected_dimension_companions,
                        (scope, group.companion_record_index),
                        "f3d projected group companion",
                    )
                })
                .map(|_| ())?;
        }
    }

    let mut group_constraints = Vec::new();
    for group in ctx.admit_iter(groups, "f3d dimension classification source")? {
        let projected = (|| {
            let scope = dimension_resource!(native_stream_charged(
                ctx,
                &group.id,
                "resolve F3D dimension stream"
            ))?;
            if dimension_resource!(ctx.contains_hash_set(
                &radial_extension_annotation_groups,
                &(scope, group.record_index),
                "f3d radial group classification lookup"
            )) {
                return None;
            }
            if dimension_resource!(ctx.contains_hash_set(
                &exact_pair_companions,
                &(scope, group.companion_record_index),
                "f3d exact pair classification lookup"
            )) {
                return None;
            }
            let (parameter, parameter_id) =
                dimension_resource!(parameter_for(scope, group.companion_record_index)?);
            let mut locus_indices = Vec::new();
            for locus in dimension_resource!(ctx
                .admit_iter(group.loci.as_slice(), "f3d dimension group locus source")
                .map_err(CodecError::from))
            {
                if let Err(error) = ctx.push_vec(
                    &mut locus_indices,
                    locus.geometry_record_index,
                    "f3d group dimension locus index",
                ) {
                    return Some(Err(error));
                }
            }
            let sketch = match sketch_for_owner_or_geometry(
                scope,
                group.owner_reference,
                &locus_indices,
                "f3d dimension group sketch id",
            ) {
                Ok(Some(sketch)) => sketch,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let exact =
                match exact_group_definition(scope, group, parameter, &parameter_id).transpose() {
                    Ok(definition) => definition,
                    Err(error) => return Some(Err(error)),
                };
            let definition = if let Some(definition) = exact {
                definition
            } else {
                let fallback = (|| -> Result<Definition, CodecError> {
                    let mut operand_storage =
                        ctx.reserve_scoped(0, "f3d native group operand storage")?;
                    let mut operands = Vec::new();
                    for locus in
                        ctx.admit_iter(group.loci.as_slice(), "f3d dimension group locus source")?
                    {
                        ctx.push_scoped_vec(
                            &mut operand_storage,
                            &mut operands,
                            (
                                cadmpeg_core::nonblank_literal!("locus"),
                                Some(locus.role),
                                locus.geometry_record_index,
                            ),
                            "f3d native group locus operand",
                        )?;
                    }
                    ctx.push_scoped_vec(
                        &mut operand_storage,
                        &mut operands,
                        (
                            cadmpeg_core::nonblank_literal!("owner"),
                            Some(group.owner_role),
                            group.owner_reference,
                        ),
                        "f3d native group owner operand",
                    )?;
                    for locus in
                        ctx.admit_iter(group.loci.as_slice(), "f3d dimension group locus source")?
                    {
                        ctx.push_scoped_vec(
                            &mut operand_storage,
                            &mut operands,
                            (
                                cadmpeg_core::nonblank_literal!("return"),
                                None,
                                locus.returned.value,
                            ),
                            "f3d native group return operand",
                        )?;
                    }
                    native_definition(
                        scope,
                        copy_dimension_source_kind(ctx, parameter, "f3d group source kind")?,
                        Some(u64::from(group.state)),
                        &operands,
                        parameter_id,
                    )
                })();
                match fallback {
                    Ok(definition) => definition,
                    Err(error) => return Some(Err(error)),
                }
            };
            let definition =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()?;
            let native_ref =
                match ctx.copy_retained_text(&group.id, "f3d dimension group native reference") {
                    Ok(native_ref) => native_ref,
                    Err(error) => return Some(Err(error)),
                };
            Some(Ok(SketchConstraint {
                id: dimension_resource!(crate::design::identity::neutral_sketch_constraint_id(
                    ctx,
                    &group.id,
                    group.record_index
                )),
                sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(native_ref),
            }))
        })();
        if let Some(result) = projected {
            ctx.push_vec(
                &mut group_constraints,
                result?,
                "f3d group dimension constraint",
            )?;
        }
    }
    let mut pair_constraints = Vec::new();
    for pair in ctx.admit_iter(pairs, "f3d dimension classification source")? {
        let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some((parameter, parameter_id)) =
            parameter_for(scope, pair.governing_companion_record_index).transpose()?
        else {
            continue;
        };
        let indices = [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ];
        let Some(sketch) = sketch_for_geometry(scope, &indices, "f3d dimension pair sketch id")?
        else {
            continue;
        };
        let constraint_id =
            crate::design::identity::neutral_dimension_constraint_id(ctx, &parameter_id, "pair")?;
        let copied = parameter_id.try_clone_for_decode(ctx, "f3d pair exact parameter id")?;
        let definition = exact_definition(scope, parameter, &indices, copied)?
            .map(Ok)
            .or_else(|| {
                let [first_index, second_index] = indices;
                let first = dimension_resource!(ctx.get_btree_map(
                    &projected,
                    &(scope, first_index),
                    "f3d symmetric pair first lookup"
                ))?;
                let second = dimension_resource!(ctx.get_btree_map(
                    &projected,
                    &(scope, second_index),
                    "f3d symmetric pair second lookup"
                ))?;
                symmetric_parallel_line_dimension_definition(
                    ctx,
                    first,
                    second,
                    (pair.loci()[0].role, pair.loci()[1].role),
                    parameter,
                    dimension_resource!(
                        parameter_id.try_clone_for_decode(ctx, "f3d symmetric pair parameter id")
                    ),
                    linear_tolerance,
                )
            })
            .transpose()?;
        let definition = if let Some(definition) = definition {
            definition
        } else {
            native_definition(
                scope,
                copy_dimension_source_kind(ctx, parameter, "f3d pair source kind")?,
                None,
                &[
                    (
                        cadmpeg_core::nonblank_literal!("first_locus"),
                        Some(pair.loci()[0].role),
                        indices[0],
                    ),
                    (
                        cadmpeg_core::nonblank_literal!("second_locus"),
                        Some(pair.loci()[1].role),
                        indices[1],
                    ),
                ],
                parameter_id,
            )?
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
        else {
            continue;
        };
        ctx.push_vec(
            &mut pair_constraints,
            SketchConstraint {
                id: constraint_id,
                sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(
                    ctx.copy_retained_text(&pair.id, "f3d dimension pair native reference")?,
                ),
            },
            "f3d pair dimension constraint",
        )?;
    }
    let mut constraints = Vec::new();
    for projected in ctx
        .admit_iter(pair_constraints, "f3d dimension pair output source")?
        .chain(ctx.admit_iter(group_constraints, "f3d dimension group output source")?)
    {
        ctx.push_vec(
            &mut constraints,
            projected,
            "f3d dimension constraint output",
        )?;
    }
    let mut frame_source = annotation_frames.iter();
    while let Some(frame) =
        ctx.next_charged(&mut frame_source, "f3d dimension annotation source")?
    {
        let projected = (|| {
            let scope = dimension_resource!(native_stream_charged(
                ctx,
                &frame.id,
                "resolve F3D dimension stream"
            ))?;
            let (parameter, parameter_id) = dimension_resource!(parameter_for(
                scope,
                frame.governing_companion_record_index
            )?);
            let mut index_storage =
                dimension_resource!(ctx.reserve_scoped(0, "f3d annotation index storage"));
            let mut indices = Vec::new();
            for operand in dimension_resource!(ctx
                .admit_iter(frame.operands(), "f3d dimension annotation operand source")
                .map_err(CodecError::from))
            {
                if let Some(index) = operand.geometry_record_index {
                    if let Err(error) = ctx.push_scoped_vec(
                        &mut index_storage,
                        &mut indices,
                        index.get(),
                        "f3d annotation dimension index",
                    ) {
                        return Some(Err(error));
                    }
                }
            }
            let placement = dimension_resource!(ctx.get_hash_map(
                &sketches,
                &(scope, frame.owner_reference),
                "f3d dimension annotation sketch lookup",
            ))?;
            let sketch = match crate::design::identity::neutral_sketch_id_with_operation(
                ctx,
                placement,
                "f3d dimension annotation sketch id",
            ) {
                Ok(sketch) => sketch,
                Err(error) => return Some(Err(error)),
            };
            let constraint_id =
                dimension_resource!(crate::design::identity::neutral_dimension_constraint_id(
                    ctx,
                    &parameter_id,
                    "annotation"
                ));
            let copied =
                match parameter_id.try_clone_for_decode(ctx, "f3d annotation exact parameter id") {
                    Ok(copied) => copied,
                    Err(error) => return Some(Err(error)),
                };
            let exact = match exact_definition(scope, parameter, &indices, copied) {
                Ok(definition) => definition,
                Err(error) => return Some(Err(error)),
            }
            .map(Ok)
            .or_else(|| {
                annotation_offset_dimension_definition(
                    ctx,
                    frame,
                    (parameter, &parameter_id),
                    scope,
                    curves,
                    &projected,
                    linear_tolerance,
                )
            });
            let exact = dimension_resource!(exact.transpose());
            let definition = if let Some(definition) = exact {
                definition
            } else {
                let fallback = (|| -> Result<Definition, CodecError> {
                    let mut operands = Vec::new();
                    for operand in
                        ctx.admit_iter(frame.operands(), "f3d dimension annotation operand source")?
                    {
                        let native = match operand.geometry_record_index {
                            None => SketchNativeOperand {
                                native_kind: cadmpeg_core::nonblank_literal!("null_locus"),
                                field: Some(NativeOperandField {
                                    name: cadmpeg_core::nonblank_literal!("locus"),
                                    role: Some(operand.role),
                                }),
                                object_index: None,
                                native_ref: None,
                            },
                            Some(index) => native_operand(
                                scope,
                                cadmpeg_core::nonblank_literal!("locus"),
                                Some(operand.role),
                                index.get(),
                            )?,
                        };
                        ctx.push_vec(&mut operands, native, "f3d annotation native operand")?;
                    }
                    let mut entity_ids = Vec::new();
                    for record_index in &indices {
                        if let Some(entity) = ctx.get_btree_map(
                            &projected,
                            &(scope, *record_index),
                            "f3d native dimension entity lookup",
                        )? {
                            let id = (entity.id())
                                .try_clone_for_decode(ctx, "f3d annotation native entity id")?;
                            ctx.push_vec(&mut entity_ids, id, "f3d annotation native entity")?;
                        }
                    }
                    Ok(Definition::Native {
                        native_kind: copy_dimension_source_kind(
                            ctx,
                            parameter,
                            "f3d annotation source kind",
                        )?,
                        native_state: None,
                        native_flags: None,
                        native_properties: std::collections::BTreeMap::new(),
                        entities: entity_ids,
                        parameter: Some(parameter_id),
                        operands,
                    })
                })();
                match fallback {
                    Ok(definition) => definition,
                    Err(error) => return Some(Err(error)),
                }
            };
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            else {
                return None;
            };
            let native_ref = match ctx
                .copy_retained_text(&frame.id, "f3d dimension annotation native reference")
            {
                Ok(native_ref) => native_ref,
                Err(error) => return Some(Err(error)),
            };
            Some(Ok(SketchConstraint {
                id: constraint_id,
                sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(native_ref),
            }))
        })();
        if let Some(projected) = projected {
            ctx.push_vec(
                &mut constraints,
                projected?,
                "f3d dimension constraint output",
            )?;
        }
    }
    let mut null_pair_source = null_pairs.iter();
    while let Some(pair) =
        ctx.next_charged(&mut null_pair_source, "f3d dimension null pair source")?
    {
        let projected = (|| {
            let scope = dimension_resource!(native_stream_charged(
                ctx,
                &pair.id,
                "resolve F3D dimension stream"
            ))?;
            if dimension_resource!(ctx.contains_hash_set(
                &parameterized_offset_companions,
                &(scope, pair.governing_companion_record_index),
                "f3d offset companion classification lookup"
            )) {
                return None;
            }
            let (parameter, parameter_id) =
                dimension_resource!(parameter_for(scope, pair.governing_companion_record_index)?);
            let indices = [pair.loci()[1].geometry_index()];
            let sketch =
                match sketch_for_geometry(scope, &indices, "f3d dimension null pair sketch id") {
                    Ok(Some(sketch)) => sketch,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
            let constraint_id =
                dimension_resource!(crate::design::identity::neutral_dimension_constraint_id(
                    ctx,
                    &parameter_id,
                    "null-pair"
                ));
            if design_dimension_unit(parameter) {
                if let Some(entity) = dimension_resource!(ctx.get_btree_map(
                    &projected,
                    &(scope, pair.loci()[1].geometry_index()),
                    "f3d dimension null pair entity lookup",
                )) {
                    if let Some(definition) = dimension_resource!(null_locus_dimension_definition(
                        ctx,
                        pair,
                        entity,
                        parameter.source_kind(),
                        parameter.evaluated_value().get(),
                        (&parameter_id, "f3d null pair exact parameter id"),
                        linear_tolerance,
                    )
                    .transpose())
                    {
                        let definition =
                            cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                                .ok()?;
                        let native_ref = match ctx.copy_retained_text(
                            &pair.id,
                            "f3d dimension null pair native reference",
                        ) {
                            Ok(native_ref) => native_ref,
                            Err(error) => return Some(Err(error)),
                        };
                        return Some(Ok(SketchConstraint {
                            id: constraint_id,
                            sketch,
                            definition,
                            name: None,
                            driving: None,
                            active: None,
                            virtual_space: None,
                            visible: None,
                            orientation: None,
                            label_distance: None,
                            label_position: None,
                            metadata: None,
                            native_ref: Some(native_ref),
                        }));
                    }
                }
            }
            let fallback = (|| -> Result<Option<SketchConstraint>, CodecError> {
                let mut operands = Vec::new();
                ctx.push_vec(
                    &mut operands,
                    SketchNativeOperand {
                        native_kind: cadmpeg_core::nonblank_literal!("null_locus"),
                        field: Some(NativeOperandField {
                            name: cadmpeg_core::nonblank_literal!("locus"),
                            role: Some(pair.loci()[0].role),
                        }),
                        object_index: None,
                        native_ref: None,
                    },
                    "f3d null pair native operand",
                )?;
                let native = native_operand(
                    scope,
                    cadmpeg_core::nonblank_literal!("locus"),
                    Some(pair.loci()[1].role),
                    pair.loci()[1].geometry_index(),
                )?;
                ctx.push_vec(&mut operands, native, "f3d null pair native operand")?;
                let mut entity_ids = Vec::new();
                for record_index in &indices {
                    if let Some(entity) = ctx.get_btree_map(
                        &projected,
                        &(scope, *record_index),
                        "f3d native dimension entity lookup",
                    )? {
                        let id = (entity.id())
                            .try_clone_for_decode(ctx, "f3d null pair native entity id")?;
                        ctx.push_vec(&mut entity_ids, id, "f3d null pair native entity")?;
                    }
                }
                let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                    Definition::Native {
                        native_kind: copy_dimension_source_kind(
                            ctx,
                            parameter,
                            "f3d null pair source kind",
                        )?,
                        native_state: None,
                        native_flags: None,
                        native_properties: std::collections::BTreeMap::new(),
                        entities: entity_ids,
                        parameter: Some(parameter_id),
                        operands,
                    },
                ) else {
                    return Ok(None);
                };
                Ok(Some(SketchConstraint {
                    id: constraint_id,
                    sketch,
                    definition,
                    name: None,
                    driving: None,
                    active: None,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(ctx.copy_retained_text(
                        &pair.id,
                        "f3d dimension null pair native reference",
                    )?),
                }))
            })();
            match fallback {
                Ok(Some(constraint)) => Some(Ok(constraint)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })();
        if let Some(projected) = projected {
            ctx.push_vec(
                &mut constraints,
                projected?,
                "f3d dimension constraint output",
            )?;
        }
    }
    let mut companion_storage = ctx.reserve_scoped(0, "f3d dimension companion index storage")?;
    let mut companions_by_key = HashMap::new();
    for companion in ctx.admit_iter(companions, "f3d dimension companion source")? {
        if let Some(scope) =
            native_stream_charged(ctx, companion.id(), "resolve F3D dimension stream")?
        {
            companion_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut companions_by_key,
                    (scope, companion.record_index()),
                    companion,
                    "f3d dimension recipe companion index",
                )
            })?;
        }
    }
    let mut owners_by_companion = HashMap::new();
    for owner in ctx.admit_iter(owners, "f3d dimension companion owner source")? {
        if let Some(scope) = native_stream_charged(ctx, owner.id(), "resolve F3D dimension stream")?
        {
            companion_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut owners_by_companion,
                    (scope, owner.companion_record_index()),
                    owner,
                    "f3d dimension recipe owner index",
                )
            })?;
        }
    }
    let mut recipe_storage = ctx.reserve_scoped(0, "f3d dimension recipe storage")?;
    let mut recipes_by_companion = BTreeMap::<(&str, u32), Vec<_>>::new();
    for record in ctx.admit_iter(recipe_records, "f3d dimension recipe record source")? {
        let Some(scope) = native_stream_charged(ctx, &record.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        if !ctx.contains_hash_set(
            &projected_dimension_companions,
            &(scope, record.companion_record_index),
            "f3d dimension projected companion lookup",
        )? {
            recipe_storage.with_storage(|| {
                ctx.push_btree_group(
                    &mut recipes_by_companion,
                    (scope, record.companion_record_index),
                    record,
                    "f3d dimension recipe group",
                    "f3d dimension recipe group member",
                )
            })?;
        }
    }
    for (_, records) in ctx.admit_iter(
        &mut recipes_by_companion,
        "f3d dimension recipe sort source",
    )? {
        ctx.stable_sort_by_key(
            &mut records[..],
            |record| record.recipe_ordinal,
            Ord::cmp,
            "sort f3d design dimensions 1",
        )?;
    }
    for (&(scope, companion_record_index), records) in
        ctx.admit_iter(&recipes_by_companion, "f3d dimension recipe group source")?
    {
        let Some(companion) = ctx.get_hash_map(
            &companions_by_key,
            &(scope, companion_record_index),
            "f3d dimension recipe companion lookup",
        )?
        else {
            continue;
        };
        let Some(owner) = ctx.get_hash_map(
            &owners_by_companion,
            &(scope, companion_record_index),
            "f3d dimension recipe owner lookup",
        )?
        else {
            continue;
        };
        let Some((parameter, parameter_id)) =
            parameter_for(scope, companion_record_index).transpose()?
        else {
            continue;
        };
        let constraint_id = crate::design::identity::neutral_dimension_constraint_id(
            ctx,
            &parameter_id,
            "recipe-group",
        )?;
        let Some(placement) = ctx.get_hash_map(
            &sketches_by_scope,
            &(scope, owner.scope_record_index()),
            "f3d dimension recipe sketch lookup",
        )?
        else {
            continue;
        };
        let sketch = crate::design::identity::neutral_sketch_id_with_operation(
            ctx,
            placement,
            "f3d recipe dimension sketch id",
        )?;
        let linear_candidates = if parameter.source_kind().starts_with("Linear Dimension")
            && design_dimension_unit(parameter)
        {
            recipe_linear_dimension_candidates(
                ctx,
                entities,
                &sketch,
                parameter.evaluated_value().get() * 10.0,
                &parameter_id,
                linear_tolerance,
            )?
        } else {
            Vec::default()
        };
        let copied = parameter_id.try_clone_for_decode(ctx, "f3d recipe repeated parameter id")?;
        let repeated = repeated_linear_dimension(ctx, &linear_candidates, copied).transpose()?;
        let extension =
            recipe_extension_point_dimension(ctx, &linear_candidates, entities, &sketch)
                .transpose()?;
        let radial = owner_scoped_radial_dimension_definition(
            ctx,
            entities,
            &sketch,
            parameter,
            &parameter_id,
            linear_tolerance,
        )
        .transpose()?;
        let concentric = concentric_circle_dimension_definition(
            ctx,
            entities,
            &sketch,
            parameter,
            &parameter_id,
            linear_tolerance,
        )
        .transpose()?;
        let definition = if let Some(radial) = radial {
            radial
        } else if linear_candidates.len() == 1 {
            let Some(definition) = linear_candidates.into_iter().next() else {
                continue;
            };
            definition
        } else if let Some(repeated) = repeated {
            repeated
        } else if let Some(extension) = extension {
            extension
        } else if let Some(concentric) = concentric {
            concentric
        } else {
            let mut operands = Vec::new();
            for record in ctx.admit_iter(records, "f3d dimension recipe operand source")? {
                let native_ref =
                    ctx.copy_retained_text(&record.id, "f3d recipe native operand reference")?;
                ctx.push_vec(
                    &mut operands,
                    SketchNativeOperand {
                        native_kind: cadmpeg_core::nonblank_literal!("construction_recipe"),
                        field: Some(NativeOperandField {
                            name: cadmpeg_core::nonblank_literal!("recipe"),
                            role: None,
                        }),
                        object_index: Some(record.record_index),
                        native_ref: Some(native_ref),
                    },
                    "f3d recipe native operand",
                )?;
            }
            Definition::Native {
                native_kind: copy_dimension_source_kind(ctx, parameter, "f3d recipe source kind")?,
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: recipe_dimension_candidate_entities(ctx, &linear_candidates)?,
                parameter: Some(parameter_id),
                operands,
            }
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
        else {
            continue;
        };
        let native_ref =
            ctx.copy_retained_text(companion.id(), "f3d recipe constraint native reference")?;
        ctx.push_vec(
            &mut constraints,
            SketchConstraint {
                id: constraint_id,
                sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(native_ref),
            },
            "f3d recipe dimension constraint",
        )?;
    }
    drop(recipes_by_companion);
    drop(recipe_storage);
    let mut parameter_storage =
        ctx.reserve_scoped(0, "f3d projected dimension parameter storage")?;
    let mut projected_parameters = HashSet::new();
    for constraint in ctx.admit_iter(&constraints, "f3d projected dimension constraint source")? {
        for parameter in constraint_parameters(constraint.definition.kind()) {
            if !ctx.contains_hash_set(
                &projected_parameters,
                parameter,
                "f3d projected dimension parameter lookup",
            )? {
                parameter_storage.with_storage(|| {
                    let id = parameter
                        .try_clone_for_decode(ctx, "f3d projected dimension parameter id")?;
                    ctx.insert_hash_set(
                        &mut projected_parameters,
                        id,
                        "f3d projected dimension parameter",
                    )
                })?;
            }
        }
    }
    let mut container_storage = ctx.reserve_scoped(0, "f3d container-only dimension storage")?;
    let container_only_payload_companions = container_storage.with_storage(|| {
        container_only_dimension_companions(
            ctx,
            pairs,
            null_pairs,
            annotation_frames,
            groups,
            recipe_records,
        )
    })?;
    for companion in ctx.admit_iter(companions, "f3d dimension companion projection source")? {
        let projected = (|| {
            let scope = dimension_resource!(native_stream_charged(
                ctx,
                companion.id(),
                "resolve F3D dimension stream"
            ))?;
            let owner = dimension_resource!(ctx.get_hash_map(
                &owners_by_companion,
                &(scope, companion.record_index()),
                "f3d dimension companion owner lookup",
            ))?;
            let (parameter, parameter_id) =
                dimension_resource!(parameter_for(scope, companion.record_index())?);
            if parameter.kind() != DesignParameterKind::Dimension
                || dimension_resource!(ctx.contains_hash_set(
                    &projected_parameters,
                    &parameter_id,
                    "f3d projected dimension parameter lookup"
                ))
                || dimension_resource!(ctx.contains_hash_set(
                    &container_only_payload_companions,
                    &(scope, companion.record_index()),
                    "f3d container-only dimension lookup"
                ))
            {
                return None;
            }
            let placement = dimension_resource!(ctx.get_hash_map(
                &sketches_by_scope,
                &(scope, owner.scope_record_index()),
                "f3d dimension companion sketch lookup",
            ))?;
            let sketch = match crate::design::identity::neutral_sketch_id_with_operation(
                ctx,
                placement,
                "f3d companion dimension sketch id",
            ) {
                Ok(sketch) => sketch,
                Err(error) => return Some(Err(error)),
            };
            let mut parallel_axis_angle = None;
            let mut saw_parallel_axis_angle = false;
            let mut multiple_parallel_axis_angles = false;
            for group in dimension_resource!(ctx
                .admit_iter(groups, "f3d dimension companion group source")
                .map_err(CodecError::from))
            {
                let stream = dimension_resource!(native_stream_charged(
                    ctx,
                    &group.id,
                    "resolve F3D dimension stream"
                ));
                if !dimension_resource!(ctx.equal(
                    &stream,
                    &Some(scope),
                    "f3d dimension companion group stream"
                )) || group.companion_record_index != companion.record_index()
                {
                    continue;
                }
                let mut parallel_storage = dimension_resource!(
                    ctx.reserve_scoped(0, "f3d parallel group classification storage")
                );
                let Some(Definition::Parallel { first, second }) =
                    dimension_resource!(parallel_storage.with_storage(|| exact_group_definition(
                        scope,
                        group,
                        parameter,
                        &parameter_id
                    )
                    .transpose()))
                else {
                    continue;
                };
                let members = [
                    dimension_resource!(ctx.find_by(
                        entities,
                        |entity| ctx.equal(
                            entity.id(),
                            &first,
                            "f3d parallel group entity equality"
                        ),
                        "f3d parallel group entity source"
                    )),
                    dimension_resource!(ctx.find_by(
                        entities,
                        |entity| ctx.equal(
                            entity.id(),
                            &second,
                            "f3d parallel group entity equality"
                        ),
                        "f3d parallel group entity source"
                    )),
                ];
                let [Some(first), Some(second)] = members else {
                    continue;
                };
                let Some(definition) = parallel_group_axis_angle_definition(
                    ctx,
                    &[first, second],
                    parameter,
                    &parameter_id,
                ) else {
                    continue;
                };
                let definition = dimension_resource!(definition);
                if saw_parallel_axis_angle {
                    multiple_parallel_axis_angles = true;
                    parallel_axis_angle = None;
                } else {
                    saw_parallel_axis_angle = true;
                    parallel_axis_angle = Some(definition);
                }
            }
            if multiple_parallel_axis_angles {
                parallel_axis_angle = None;
            }
            let owner_scoped_definition = owner_scoped_radial_dimension_definition(
                ctx,
                entities,
                &sketch,
                parameter,
                &parameter_id,
                linear_tolerance,
            )
            .or_else(|| {
                preceding_incident_angular_dimension_definition(
                    ctx,
                    scope,
                    points,
                    curves,
                    &projected,
                    &sketch,
                    (parameter, &parameter_id),
                )
            })
            .or_else(|| {
                owner_scoped_angular_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                )
            })
            .or_else(|| {
                owner_scoped_line_length_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                unique_parallel_line_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                owner_scoped_parallel_line_set_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                unique_point_line_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                unique_point_class_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                concentric_circle_dimension_definition(
                    ctx,
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            });
            let owner_scoped_definition = dimension_resource!(owner_scoped_definition.transpose());
            let presentation_definition =
                dimension_resource!(presentation_for_owner(scope, owner.record_index())).and_then(
                    |frame| {
                        presentation_dimension_definition(
                            ctx,
                            scope,
                            frame,
                            &projected,
                            parameter,
                            &parameter_id,
                            linear_tolerance,
                        )
                    },
                );
            let presentation_definition = dimension_resource!(presentation_definition.transpose());
            let exact_definition = presentation_definition
                .or(parallel_axis_angle)
                .or(owner_scoped_definition);
            if exact_definition.is_none()
                && companion
                    .payload()
                    .is_none_or(|payload| payload.byte_length() == 0)
            {
                return None;
            }
            let definition = if let Some(definition) = exact_definition {
                definition
            } else {
                let fallback = (|| -> Result<Definition, CodecError> {
                    let copied = parameter_id
                        .try_clone_for_decode(ctx, "f3d companion native parameter id")?;
                    let native_ref = ctx.copy_retained_text(
                        companion.id(),
                        "f3d companion native operand reference",
                    )?;
                    Ok(Definition::Native {
                        native_kind: copy_dimension_source_kind(
                            ctx,
                            parameter,
                            "f3d companion source kind",
                        )?,
                        native_state: None,
                        native_flags: None,
                        native_properties: std::collections::BTreeMap::new(),
                        entities: Vec::new(),
                        parameter: Some(copied),
                        operands: vec![SketchNativeOperand {
                            native_kind: cadmpeg_core::nonblank_literal!("dimension_companion"),
                            field: Some(NativeOperandField {
                                name: cadmpeg_core::nonblank_literal!("companion_payload"),
                                role: None,
                            }),
                            object_index: Some(companion.record_index()),
                            native_ref: Some(native_ref),
                        }],
                    })
                })();
                match fallback {
                    Ok(definition) => definition,
                    Err(error) => return Some(Err(error)),
                }
            };
            let definition =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()?;
            let native_ref = match ctx
                .copy_retained_text(companion.id(), "f3d companion constraint native reference")
            {
                Ok(native_ref) => native_ref,
                Err(error) => return Some(Err(error)),
            };
            Some(Ok(SketchConstraint {
                id: dimension_resource!(crate::design::identity::neutral_dimension_constraint_id(
                    ctx,
                    &parameter_id,
                    "companion-payload"
                )),
                sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(native_ref),
            }))
        })();
        if let Some(result) = projected {
            ctx.push_vec(
                &mut constraints,
                result?,
                "f3d companion dimension constraint",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut constraints[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimensions 2",
    )?;
    Ok(constraints)
}

/// Resolve one direct presentation carrier only when its selected geometry
/// measures the parameter value. Presentation operands are authoritative for
/// selection, while the geometric check prevents a presentation record from
/// assigning a nearby entity with the same source sketch.
fn presentation_dimension_definition(
    ctx: &DecodeContext<'_>,
    scope: &str,
    frame: &crate::records::dimensions::DesignDimensionPresentationFrame,
    projected: &BTreeMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !design_dimension_unit(parameter) {
        return None;
    }
    if !(1..=2).contains(&frame.operands.len()) {
        return None;
    }
    let first = dimension_resource!(ctx.get_btree_map(
        projected,
        &(scope, frame.operands[0].geometry_record_index.get()),
        "f3d presentation dimension entity lookup",
    ))
    .copied()?;
    let second = if frame.operands.len() == 2 {
        let second = dimension_resource!(ctx.get_btree_map(
            projected,
            &(scope, frame.operands[1].geometry_record_index.get()),
            "f3d presentation dimension entity lookup",
        ))
        .copied()?;
        if !dimension_resource!(ctx.equal(
            &first.sketch,
            &second.sketch,
            "f3d presentation sketch equality"
        )) {
            return None;
        }
        Some(second)
    } else {
        None
    };
    match (first, second) {
        (entity, None) if parameter.source_kind().starts_with("Tangent Dimension") => {
            tangent_radius_dimension_definition(
                ctx,
                entity,
                parameter,
                parameter_id,
                linear_tolerance,
            )
        }
        (entity, None) => radial_dimension_definition_at_tolerance(
            ctx,
            entity,
            parameter.source_kind(),
            parameter.evaluated_value().get(),
            (parameter_id, "f3d presentation parameter id"),
            linear_tolerance,
        )
        .or_else(|| {
            if !parameter.source_kind().starts_with("Linear Dimension") {
                return None;
            }
            let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
                return None;
            };
            let measured = (end.u - start.u).hypot(end.v - start.v);
            linear_measurement_matches(
                measured,
                parameter.evaluated_value().get() * 10.0,
                linear_tolerance,
            )
            .then(|| -> Result<_, CodecError> {
                Ok(Definition::DistanceLoci {
                    first: cadmpeg_ir::sketches::SketchLocus::Start(
                        (entity.id()).try_clone_for_decode(ctx, "f3d presentation entity id")?,
                    ),
                    second: cadmpeg_ir::sketches::SketchLocus::End(
                        (entity.id()).try_clone_for_decode(ctx, "f3d presentation entity id")?,
                    ),
                    parameter: (parameter_id)
                        .try_clone_for_decode(ctx, "f3d presentation parameter id")?,
                })
            })
        }),
        (first, Some(second)) if parameter.source_kind().starts_with("Tangent Dimension") => {
            tangent_entity_distance_definition(
                ctx,
                first,
                second,
                parameter,
                parameter_id,
                linear_tolerance,
            )
        }
        (first, Some(second)) if parameter.source_kind().starts_with("Linear Dimension") => {
            explicit_linear_dimension_definition(
                ctx,
                first,
                second,
                parameter,
                parameter_id,
                linear_tolerance,
            )
        }
        (first, Some(second)) if parameter.source_kind().starts_with("Angular Dimension") => {
            line_angle_matches(
                &first.geometry,
                &second.geometry,
                parameter.evaluated_value().get(),
            )
            .then(|| -> Result<_, CodecError> {
                Ok(Definition::Angle {
                    first: (first.id()).try_clone_for_decode(ctx, "f3d presentation first id")?,
                    second: (second.id())
                        .try_clone_for_decode(ctx, "f3d presentation second id")?,
                    parameter: (parameter_id)
                        .try_clone_for_decode(ctx, "f3d presentation parameter id")?,
                })
            })
        }
        _ => None,
    }
}

fn tangent_radius_dimension_definition(
    ctx: &DecodeContext<'_>,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let radius = match entity.geometry.definition() {
        SketchGeometryDefinition::Circle { radius, .. }
        | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
        _ => return None,
    };
    linear_measurement_matches(
        radius,
        parameter.evaluated_value().get() * 10.0,
        linear_tolerance,
    )
    .then(|| -> Result<_, CodecError> {
        Ok(Definition::Radius {
            entity: (entity.id()).try_clone_for_decode(ctx, "f3d tangent radius entity id")?,
            parameter: (parameter_id)
                .try_clone_for_decode(ctx, "f3d tangent radius parameter id")?,
        })
    })
}

fn tangent_entity_distance_definition(
    ctx: &DecodeContext<'_>,
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let circle_geometry =
        |entity: &cadmpeg_ir::sketches::SketchEntity| match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. } => Some((*center, radius.get())),
            _ => None,
        };
    let candidates = match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Line { start, end },
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. },
        ) => {
            let line = (*start, *end);
            let circle = (*center, radius.get());
            let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
            let length = direction.u.hypot(direction.v);
            if length <= PRESENTATION_MIN_LINE_LENGTH {
                return None;
            }
            let offset = Point2::new(circle.0.u - line.0.u, circle.0.v - line.0.v);
            let center_distance = (offset.u * direction.v - offset.v * direction.u).abs() / length;
            vec![
                center_distance + circle.1,
                (center_distance - circle.1).abs(),
            ]
        }
        (
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. },
            SketchGeometryDefinition::Line { start, end },
        ) => {
            let line = (*start, *end);
            let circle = (*center, radius.get());
            let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
            let length = direction.u.hypot(direction.v);
            if length <= PRESENTATION_MIN_LINE_LENGTH {
                return None;
            }
            let offset = Point2::new(circle.0.u - line.0.u, circle.0.v - line.0.v);
            let center_distance = (offset.u * direction.v - offset.v * direction.u).abs() / length;
            vec![
                center_distance + circle.1,
                (center_distance - circle.1).abs(),
            ]
        }
        _ if circle_geometry(first).is_some() && circle_geometry(second).is_some() => {
            let (first_center, first_radius) = circle_geometry(first)?;
            let (second_center, second_radius) = circle_geometry(second)?;
            let center_distance =
                (first_center.u - second_center.u).hypot(first_center.v - second_center.v);
            vec![
                center_distance + first_radius + second_radius,
                (center_distance - first_radius - second_radius).abs(),
                (center_distance + first_radius - second_radius).abs(),
                (center_distance - first_radius + second_radius).abs(),
            ]
        }
        _ => return None,
    };
    let matches = candidates
        .into_iter()
        .filter(|candidate| {
            linear_measurement_matches(
                *candidate,
                parameter.evaluated_value().get() * 10.0,
                linear_tolerance,
            )
        })
        .count();
    if matches != 1 {
        return None;
    }
    Some(Ok(Definition::Distance {
        entities: vec![
            dimension_resource!(
                (first.id()).try_clone_for_decode(ctx, "f3d tangent entity distance first id")
            ),
            dimension_resource!(
                (second.id()).try_clone_for_decode(ctx, "f3d tangent entity distance second id")
            ),
        ],
        parameter: dimension_resource!(
            (parameter_id).try_clone_for_decode(ctx, "f3d tangent entity distance parameter id")
        ),
    }))
}

fn explicit_linear_dimension_definition(
    ctx: &DecodeContext<'_>,
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    let expected = parameter.evaluated_value().get() * 10.0;
    if let Some(definition) = directional_point_dimension(
        ctx,
        &[first, second],
        expected,
        (parameter_id, "f3d explicit linear parameter id"),
        linear_tolerance,
    ) {
        return Some(definition);
    }
    if point_line_separation(first, second, expected, linear_tolerance)
        || parallel_line_separation(first, second, expected, linear_tolerance)
        || concentric_circle_separation(first, second, expected, linear_tolerance)
    {
        return Some(Ok(Definition::Distance {
            entities: vec![
                dimension_resource!(
                    (first.id()).try_clone_for_decode(ctx, "f3d explicit linear first id")
                ),
                dimension_resource!(
                    (second.id()).try_clone_for_decode(ctx, "f3d explicit linear second id")
                ),
            ],
            parameter: dimension_resource!(
                (parameter_id).try_clone_for_decode(ctx, "f3d explicit linear parameter id")
            ),
        }));
    }
    let (
        SketchGeometryDefinition::Point {
            position: first_position,
        },
        SketchGeometryDefinition::Point {
            position: second_position,
        },
    ) = (first.geometry.definition(), second.geometry.definition())
    else {
        return None;
    };
    let measured =
        (first_position.u - second_position.u).hypot(first_position.v - second_position.v);
    linear_measurement_matches(measured, expected, linear_tolerance).then(
        || -> Result<_, CodecError> {
            Ok(Definition::DistanceLoci {
                first: SketchLocus::Entity(
                    (first.id()).try_clone_for_decode(ctx, "f3d explicit linear first id")?,
                ),
                second: SketchLocus::Entity(
                    (second.id()).try_clone_for_decode(ctx, "f3d explicit linear second id")?,
                ),
                parameter: (parameter_id)
                    .try_clone_for_decode(ctx, "f3d explicit linear parameter id")?,
            })
        },
    )
}

/// Resolve an angular parameter from the unique two-line point incidence
/// serialized before the parameter in its owning sketch.
fn preceding_incident_angular_dimension_definition(
    ctx: &DecodeContext<'_>,
    scope: &str,
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    projected: &BTreeMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: (&DesignParameter, &cadmpeg_ir::features::ParameterId),
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let (parameter, parameter_id) = parameter;

    if !parameter.source_kind().starts_with("Angular Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let mut curve_storage =
        dimension_resource!(ctx.reserve_scoped(0, "f3d preceding incident curve storage"));
    let mut preceding_records = None;
    let mut matched = None::<(
        u32,
        u32,
        &cadmpeg_ir::sketches::SketchEntity,
        &cadmpeg_ir::sketches::SketchEntity,
    )>;
    let mut source = points.iter();
    while let Some(point) =
        dimension_resource!(ctx.next_charged(&mut source, "f3d preceding incident point source"))
    {
        let Some(stream) = dimension_resource!(native_stream_charged(
            ctx,
            &point.id,
            "resolve F3D dimension stream"
        )) else {
            continue;
        };
        if !dimension_resource!(ctx.equal(stream, scope, "f3d preceding incident point stream"))
            || point.byte_offset >= parameter.byte_offset()
        {
            continue;
        }
        let companion = point.companion();
        let [first_record, second_record] = companion.incident_curves else {
            continue;
        };
        if preceding_records.is_none() {
            let mut records = HashSet::new();
            for curve in dimension_resource!(ctx
                .admit_iter(curves, "f3d preceding incident curve source")
                .map_err(CodecError::from))
            {
                if curve.byte_offset >= parameter.byte_offset() {
                    continue;
                }
                let Some(stream) = dimension_resource!(native_stream_charged(
                    ctx,
                    &curve.id,
                    "resolve F3D dimension stream"
                )) else {
                    continue;
                };
                if dimension_resource!(ctx.equal(
                    stream,
                    scope,
                    "f3d preceding incident curve stream"
                )) {
                    dimension_resource!(curve_storage.with_storage(|| ctx.insert_hash_set(
                        &mut records,
                        curve.record_index,
                        "f3d preceding incident curve index",
                    )));
                }
            }
            preceding_records = Some(records);
        }
        let records = preceding_records.as_ref()?;
        if !dimension_resource!(ctx.contains_hash_set(
            records,
            first_record,
            "f3d preceding incident curve lookup"
        )) || !dimension_resource!(ctx.contains_hash_set(
            records,
            second_record,
            "f3d preceding incident curve lookup"
        )) {
            continue;
        }
        let first = dimension_resource!(ctx.get_btree_map(
            projected,
            &(scope, *first_record),
            "f3d preceding incident entity lookup"
        ))
        .copied();
        let second = dimension_resource!(ctx.get_btree_map(
            projected,
            &(scope, *second_record),
            "f3d preceding incident entity lookup"
        ))
        .copied();
        let (Some(first), Some(second)) = (first, second) else {
            continue;
        };
        if !dimension_resource!(ctx.equal(
            &first.sketch,
            sketch,
            "f3d preceding incident sketch equality"
        )) || !dimension_resource!(ctx.equal(
            &second.sketch,
            sketch,
            "f3d preceding incident sketch equality"
        )) || !matches!(
            *first.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) || !matches!(
            *second.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) || !line_angle_matches(
            &first.geometry,
            &second.geometry,
            parameter.evaluated_value().get(),
        ) {
            continue;
        }
        let key = if first_record < second_record {
            (*first_record, *second_record)
        } else {
            (*second_record, *first_record)
        };
        if matched
            .as_ref()
            .is_some_and(|(left, right, _, _)| (*left, *right) != key)
        {
            return None;
        }
        matched = Some((key.0, key.1, first, second));
    }
    let (_, _, first, second) = matched?;
    Some(Ok(Definition::Angle {
        first: dimension_resource!(
            (first.id()).try_clone_for_decode(ctx, "f3d preceding incident angular first id")
        ),
        second: dimension_resource!(
            (second.id()).try_clone_for_decode(ctx, "f3d preceding incident angular second id")
        ),
        parameter: dimension_resource!(
            (parameter_id).try_clone_for_decode(ctx, "f3d preceding incident angular parameter id")
        ),
    }))
}

/// Resolve an angular parameter when exactly one unordered line pair in its
/// owning sketch has the evaluated supporting-line angle.
fn owner_scoped_angular_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Angular Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let mut matched = None;
    let mut first_source = entities.iter().enumerate();
    while let Some((first_index, first)) =
        dimension_resource!(ctx.next_charged(&mut first_source, "f3d angular first entity source"))
    {
        if !dimension_resource!(ctx.equal(
            &first.sketch,
            sketch,
            "f3d angular first sketch equality"
        )) || !matches!(
            *first.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) {
            continue;
        }
        let mut second_source = entities[first_index + 1..].iter();
        while let Some(second) = dimension_resource!(
            ctx.next_charged(&mut second_source, "f3d angular second entity source")
        ) {
            if !dimension_resource!(ctx.equal(
                &second.sketch,
                sketch,
                "f3d angular second sketch equality"
            )) || !matches!(
                *second.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            ) {
                continue;
            }
            if !line_angle_matches(
                &first.geometry,
                &second.geometry,
                parameter.evaluated_value().get(),
            ) {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((first, second));
        }
    }
    let (first, second) = matched?;
    Some(Ok(Definition::Angle {
        first: dimension_resource!(
            (first.id()).try_clone_for_decode(ctx, "f3d owner scoped angular first id")
        ),
        second: dimension_resource!(
            (second.id()).try_clone_for_decode(ctx, "f3d owner scoped angular second id")
        ),
        parameter: dimension_resource!(
            (parameter_id).try_clone_for_decode(ctx, "f3d owner scoped angular parameter id")
        ),
    }))
}

/// Bind an angular parameter to the common direction of one exact parallel
/// relation carried by the same dimension companion.
fn parallel_group_axis_angle_definition(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchAxis, SketchConstraintDefinitionInput as Definition, SketchGeometry,
        SketchGeometryDefinition,
    };

    let [first, second] = entities else {
        return None;
    };
    if parameter.source_kind() != "Angular Dimension-2"
        || !design_dimension_unit(parameter)
        || parallel_line_distance(first, second).is_none()
    {
        return None;
    }
    let horizontal_axis = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .ok()?;
    (line_angle_matches(
        &first.geometry,
        &horizontal_axis,
        parameter.evaluated_value().get(),
    ) && line_angle_matches(
        &second.geometry,
        &horizontal_axis,
        parameter.evaluated_value().get(),
    ))
    .then(|| -> Result<_, CodecError> {
        Ok(Definition::AngleToAxis {
            entity: (first.id())
                .try_clone_for_decode(ctx, "f3d parallel group axis angle first id")?,
            axis: SketchAxis::Horizontal,
            parameter: (parameter_id)
                .try_clone_for_decode(ctx, "f3d parallel group axis angle parameter id")?,
        })
    })
}

/// Resolve one or more disjoint owner-scoped concentric-circle separations
/// controlled by one linear parameter.
fn concentric_circle_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchDistanceMeasurement as Measurement,
        SketchGeometryDefinition, SketchLocus,
    };

    if !parameter.source_kind().starts_with("Linear Dimension") || !design_dimension_unit(parameter)
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d concentric circle scratch"));
    let mut circles = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d concentric circle source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d concentric circle sketch identity"
        )) && matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
        ) {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut circles,
                entity,
                "f3d concentric circle candidate"
            )));
        }
    }
    let mut pairs = Vec::new();
    let mut paired_entities = HashSet::new();
    let mut first_source = 0..circles.len();
    while let Some(first) = dimension_resource!(
        ctx.next_charged(&mut first_source, "f3d concentric circle pair source")
    ) {
        let mut second_source = first + 1..circles.len();
        while let Some(second) = dimension_resource!(
            ctx.next_charged(&mut second_source, "f3d concentric circle pair source")
        ) {
            if !concentric_circle_separation(
                circles[first],
                circles[second],
                evaluated_mm,
                linear_tolerance,
            ) {
                continue;
            }
            let first_id = circles[first].id();
            let second_id = circles[second].id();
            if dimension_resource!(ctx.contains_hash_set(
                &paired_entities,
                first_id,
                "f3d concentric first member lookup"
            )) || dimension_resource!(ctx.contains_hash_set(
                &paired_entities,
                second_id,
                "f3d concentric second member lookup"
            )) {
                return None;
            }
            dimension_resource!(scratch.with_storage(|| ctx
                .insert_hash_set(&mut paired_entities, first_id, "f3d concentric used first")
                .map(|_| ())));
            dimension_resource!(scratch.with_storage(|| ctx
                .insert_hash_set(
                    &mut paired_entities,
                    second_id,
                    "f3d concentric used second"
                )
                .map(|_| ())));
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut pairs,
                (circles[first], circles[second]),
                "f3d concentric pair",
            )));
        }
    }
    match pairs.as_slice() {
        [] => None,
        [(first, second)] => Some(Ok(Definition::Distance {
            entities: vec![
                dimension_resource!(
                    (first.id()).try_clone_for_decode(ctx, "f3d concentric circle first id")
                ),
                dimension_resource!(
                    (second.id()).try_clone_for_decode(ctx, "f3d concentric circle second id")
                ),
            ],
            parameter: dimension_resource!(
                (parameter_id).try_clone_for_decode(ctx, "f3d concentric circle parameter id")
            ),
        })),
        _ => {
            let mut measurements = Vec::new();
            for (first, second) in dimension_resource!(ctx
                .admit_iter(&pairs, "f3d concentric output pair source")
                .map_err(CodecError::from))
            {
                let measurement = Measurement::Distance {
                    first: SketchLocus::Entity(dimension_resource!(
                        (first.id()).try_clone_for_decode(ctx, "f3d concentric repeated first id")
                    )),
                    second: SketchLocus::Entity(dimension_resource!((second.id())
                        .try_clone_for_decode(ctx, "f3d concentric repeated second id"))),
                };
                dimension_resource!(ctx.push_vec(
                    &mut measurements,
                    measurement,
                    "f3d concentric measurement"
                ));
            }
            Some(Ok(Definition::RepeatedDistance {
                measurements,
                parameter: dimension_resource!((parameter_id)
                    .try_clone_for_decode(ctx, "f3d concentric repeated parameter id")),
            }))
        }
    }
}

/// Resolve an owner-scoped linear dimension when exactly one point-line pair
/// has the evaluated perpendicular separation.
fn unique_point_line_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Linear Dimension") || !design_dimension_unit(parameter)
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d point line scratch"));
    let mut points = Vec::new();
    let mut lines = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d point line source")
        .map_err(CodecError::from))
    {
        if !dimension_resource!(ctx.equal(&entity.sketch, sketch, "f3d point line sketch identity"))
        {
            continue;
        }
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { .. } => dimension_resource!(scratch.with_storage(
                || ctx.push_vec(&mut points, entity, "f3d point line point candidate")
            )),
            SketchGeometryDefinition::Line { .. } => dimension_resource!(scratch.with_storage(
                || ctx.push_vec(&mut lines, entity, "f3d point line line candidate")
            )),
            _ => (),
        }
    }
    let mut matched = None;
    let mut point_source = points.iter().copied();
    while let Some(point) =
        dimension_resource!(ctx.next_charged(&mut point_source, "f3d point line pair source"))
    {
        let mut line_source = lines.iter().copied();
        while let Some(line) =
            dimension_resource!(ctx.next_charged(&mut line_source, "f3d point line pair source"))
        {
            if point_line_separation(point, line, evaluated_mm, linear_tolerance) {
                if matched.is_some() {
                    return None;
                }
                matched = Some((point, line));
            }
        }
    }
    let (point, line) = matched?;
    Some(Ok(Definition::Distance {
        entities: vec![
            dimension_resource!(
                (point.id()).try_clone_for_decode(ctx, "f3d unique point line point id")
            ),
            dimension_resource!(
                (line.id()).try_clone_for_decode(ctx, "f3d unique point line line id")
            ),
        ],
        parameter: dimension_resource!(
            (parameter_id).try_clone_for_decode(ctx, "f3d unique point line parameter id")
        ),
    }))
}

/// Resolve an owner-scoped linear dimension when exactly one parallel-line
/// pair has the evaluated supporting-line separation.
fn unique_parallel_line_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Linear Dimension") || !design_dimension_unit(parameter)
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d parallel line scratch"));
    let mut lines = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d parallel line source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d parallel line sketch identity"
        )) && matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut lines,
                entity,
                "f3d parallel line candidate"
            )));
        }
    }
    let mut matched = None;
    let mut first_source = lines.iter().enumerate();
    while let Some((first_ordinal, &first)) =
        dimension_resource!(ctx.next_charged(&mut first_source, "f3d parallel line pair source"))
    {
        let mut second_source = lines[first_ordinal + 1..].iter().copied();
        while let Some(second) = dimension_resource!(
            ctx.next_charged(&mut second_source, "f3d parallel line pair source")
        ) {
            if parallel_line_separation(first, second, evaluated_mm, linear_tolerance) {
                if matched.is_some() {
                    return None;
                }
                matched = Some((first, second));
            }
        }
    }
    let (first, second) = matched?;
    Some(Ok(Definition::Distance {
        entities: vec![
            dimension_resource!(
                (first.id()).try_clone_for_decode(ctx, "f3d unique parallel line first id")
            ),
            dimension_resource!(
                (second.id()).try_clone_for_decode(ctx, "f3d unique parallel line second id")
            ),
        ],
        parameter: dimension_resource!(
            (parameter_id).try_clone_for_decode(ctx, "f3d unique parallel line parameter id")
        ),
    }))
}

/// Resolve an owner-scoped distance between fragmented parallel line carriers.
fn owner_scoped_parallel_line_set_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d parallel line set scratch"));
    let mut lines = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d parallel line set source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d parallel line set sketch identity"
        )) && matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut lines,
                entity,
                "f3d parallel line set candidate"
            )));
        }
    }
    let collinear = |first: &cadmpeg_ir::sketches::SketchEntity,
                     second: &cadmpeg_ir::sketches::SketchEntity| {
        parallel_line_distance(first, second).is_some_and(|distance| distance <= linear_tolerance)
    };
    let mut carriers = Vec::<Vec<&cadmpeg_ir::sketches::SketchEntity>>::new();
    let mut line_source = lines.iter().copied();
    while let Some(line) =
        dimension_resource!(ctx.next_charged(&mut line_source, "f3d planar line carrier source"))
    {
        let matches_carrier = |carrier: &Vec<&cadmpeg_ir::sketches::SketchEntity>| {
            ctx.all_by(
                carrier,
                |member| Ok(collinear(member, line)),
                "f3d planar line carrier member agreement",
            )
        };
        let first = dimension_resource!(ctx.position_by(
            &carriers,
            matches_carrier,
            "f3d parallel line carrier candidate"
        ));
        if let Some(index) = first {
            if dimension_resource!(ctx.any_by(
                &carriers[index + 1..],
                matches_carrier,
                "f3d parallel line carrier candidate"
            )) {
                return None;
            }
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut carriers[index],
                line,
                "f3d planar carrier member"
            )));
        } else {
            let mut carrier = Vec::new();
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut carrier,
                line,
                "f3d planar carrier member"
            )));
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut carriers,
                carrier,
                "f3d planar carrier"
            )));
        }
    }
    let mut matched = None;
    let mut first_source = 0..carriers.len();
    while let Some(first) = dimension_resource!(
        ctx.next_charged(&mut first_source, "f3d planar line carrier pair source")
    ) {
        let mut second_source = first + 1..carriers.len();
        while let Some(second) = dimension_resource!(
            ctx.next_charged(&mut second_source, "f3d planar line carrier pair source")
        ) {
            if carriers[first].len() == 1 && carriers[second].len() == 1 {
                continue;
            }
            let measured = dimension_resource!(ctx.find_map(
                &carriers[first],
                |first| ctx.find_map(
                    &carriers[second],
                    |second| Ok(parallel_line_span_distance(first, second, linear_tolerance)),
                    "f3d planar line carrier second span search"
                ),
                "f3d planar line carrier first span search",
            ));
            let Some(measured) = measured else {
                continue;
            };
            let expected = evaluated_mm.abs();
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_OWNER_SCOPED_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            if (measured - expected).abs() > tolerance {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((first, second));
        }
    }
    let (first, second) = matched?;
    Some(Ok(Definition::ParallelLineSetDistance {
        first: dimension_resource!(copy_dimension_entity_members(ctx, &carriers[first])),
        second: dimension_resource!(copy_dimension_entity_members(ctx, &carriers[second])),
        parameter: dimension_resource!((parameter_id)
            .try_clone_for_decode(ctx, "f3d owner scoped parallel line set parameter id")),
    }))
}

/// Resolve the owner-scoped line lengths governed by one linear parameter.
fn owner_scoped_line_length_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    if !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = parameter.evaluated_value().get() * 10.0;
    if !expected.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d line length scratch"));
    let mut matches = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d line length source")
        .map_err(CodecError::from))
    {
        if !dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d line length sketch identity"
        )) {
            continue;
        }
        let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
            continue;
        };
        let measured = (end.u - start.u).hypot(end.v - start.v);
        let tolerance = linear_tolerance.max(
            EPS_DIMENSIONS_OWNER_SCOPED_LINE_LENGTH_DIMENSION_DEFINITION_E9
                * (1.0 + measured.abs().max(expected.abs())),
        );
        if (measured - expected.abs()).abs() <= tolerance {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut matches,
                entity,
                "f3d line length candidate"
            )));
        }
    }
    match matches.as_slice() {
        [] => None,
        [entity] => {
            Some(Ok(Definition::DistanceLoci {
                first: SketchLocus::Start(dimension_resource!((entity.id())
                    .try_clone_for_decode(ctx, "f3d owner scoped line length entity id"))),
                second: SketchLocus::End(dimension_resource!((entity.id())
                    .try_clone_for_decode(ctx, "f3d owner scoped line length entity id"))),
                parameter: dimension_resource!((parameter_id)
                    .try_clone_for_decode(ctx, "f3d owner scoped line length parameter id")),
            }))
        }
        _ => Some(Ok(Definition::RepeatedLength {
            entities: dimension_resource!(copy_dimension_entity_members(ctx, &matches)),
            parameter: dimension_resource!((parameter_id)
                .try_clone_for_decode(ctx, "f3d owner scoped line length parameter id")),
        })),
    }
}

/// Resolve the unique owner-scoped distance between solved point loci.
fn unique_point_class_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    if !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d point class scratch"));
    let mut points = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d point class source")
        .map_err(CodecError::from))
    {
        if !dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d point class sketch identity"
        )) {
            continue;
        }
        if let SketchGeometryDefinition::Point { position } = entity.geometry.definition() {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut points,
                (entity, position.get()),
                "f3d point class candidate"
            )));
        }
    }
    let coincident = |first: Point2, second: Point2| {
        let scale = 1.0
            + first
                .u
                .abs()
                .max(first.v.abs())
                .max(second.u.abs().max(second.v.abs()));
        (second.u - first.u).hypot(second.v - first.v)
            <= linear_tolerance
                .max(EPS_DIMENSIONS_UNIQUE_POINT_CLASS_DIMENSION_DEFINITION_E9 * scale)
    };
    let mut classes = Vec::<Option<DimensionPointClass<'_, '_>>>::new();
    for &point in dimension_resource!(ctx
        .admit_iter(&points, "f3d point class candidate source")
        .map_err(CodecError::from))
    {
        let mut match_storage =
            dimension_resource!(ctx.reserve_scoped(0, "f3d point class matches"));
        let mut matches = Vec::new();
        for (index, class) in dimension_resource!(ctx
            .admit_iter(&classes, "f3d point class match source")
            .map_err(CodecError::from))
        .enumerate()
        {
            let Some(class) = class else {
                continue;
            };
            if dimension_resource!(ctx.any_by(
                &class.members,
                |member| Ok(coincident(member.1, point.1)),
                "f3d point class member search"
            )) {
                dimension_resource!(match_storage.with_storage(|| ctx.push_vec(
                    &mut matches,
                    index,
                    "f3d point class match"
                )));
            }
        }
        let Some((&first, rest)) = matches.split_first() else {
            let mut class = DimensionPointClass {
                members: Vec::new(),
                storage: dimension_resource!(ctx.reserve_scoped(0, "f3d point class members")),
            };
            dimension_resource!(class.storage.with_storage(|| ctx.push_vec(
                &mut class.members,
                point,
                "f3d point class member"
            )));
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut classes,
                Some(class),
                "f3d point class"
            )));
            continue;
        };
        let target = classes[first].as_mut()?;
        dimension_resource!(target.storage.with_storage(|| ctx.push_vec(
            &mut target.members,
            point,
            "f3d point class member"
        )));
        for &index in dimension_resource!(ctx
            .admit_iter(rest, "f3d point class merge source")
            .map_err(CodecError::from))
        .rev()
        {
            let merged = classes[index].take()?;
            let target = classes[first].as_mut()?;
            for member in dimension_resource!(ctx
                .admit_iter(merged.members, "f3d point class merged member source")
                .map_err(CodecError::from))
            {
                dimension_resource!(target.storage.with_storage(|| ctx.push_vec(
                    &mut target.members,
                    member,
                    "f3d point class merged member"
                )));
            }
        }
    }
    let mut matched = None;
    let mut first_source = 0..classes.len();
    while let Some(first) =
        dimension_resource!(ctx.next_charged(&mut first_source, "f3d point class pair source"))
    {
        let Some(first_class) = &classes[first] else {
            continue;
        };
        let mut second_source = first + 1..classes.len();
        while let Some(second) =
            dimension_resource!(ctx.next_charged(&mut second_source, "f3d point class pair source"))
        {
            let Some(second_class) = &classes[second] else {
                continue;
            };
            let first_position = first_class.members[0].1;
            let second_position = second_class.members[0].1;
            let du = second_position.u - first_position.u;
            let dv = second_position.v - first_position.v;
            let measured = du.hypot(dv);
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_UNIQUE_POINT_CLASS_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            if (measured - expected).abs() > tolerance {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((
                first_class.members[0].0,
                second_class.members[0].0,
                du,
                dv,
                tolerance,
            ));
        }
    }
    let (first, second, du, dv, tolerance) = matched?;
    let first = SketchLocus::Entity(dimension_resource!(
        (first.id()).try_clone_for_decode(ctx, "f3d unique point class first id")
    ));
    let second = SketchLocus::Entity(dimension_resource!(
        (second.id()).try_clone_for_decode(ctx, "f3d unique point class second id")
    ));
    Some(Ok(if du.abs() <= tolerance {
        Definition::VerticalDistance {
            first,
            second,
            parameter: dimension_resource!(
                (parameter_id).try_clone_for_decode(ctx, "f3d unique point class parameter id")
            ),
        }
    } else if dv.abs() <= tolerance {
        Definition::HorizontalDistance {
            first,
            second,
            parameter: dimension_resource!(
                (parameter_id).try_clone_for_decode(ctx, "f3d unique point class parameter id")
            ),
        }
    } else {
        Definition::DistanceLoci {
            first,
            second,
            parameter: dimension_resource!(
                (parameter_id).try_clone_for_decode(ctx, "f3d unique point class parameter id")
            ),
        }
    }))
}

/// Resolve the owner-scoped circular measurements governed by one radial
/// parameter.
fn owner_scoped_radial_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    if !design_dimension_unit(parameter) {
        return None;
    }

    let kind = RadialDimensionKind::from_source_kind(parameter.source_kind())?;
    let mut storage = dimension_resource!(ctx.reserve_scoped(0, "F3D owner radial candidates"));
    let mut selected = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d owner radial source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(&entity.sketch, sketch, "f3d owner radial sketch"))
            && kind.matches(entity, parameter.evaluated_value().get(), linear_tolerance)
        {
            dimension_resource!(storage.with_storage(|| ctx.push_vec(
                &mut selected,
                entity,
                "f3d owner radial definition",
            )));
        }
    }
    match selected.as_slice() {
        [] => None,
        [entity] => Some(kind.single_definition(
            ctx,
            entity,
            (parameter_id, "f3d owner scoped radial parameter id"),
        )),
        _ => {
            let mut members = Vec::new();
            for entity in dimension_resource!(ctx
                .admit_iter(&selected, "f3d owner radial output source")
                .map_err(CodecError::from))
            {
                let id = dimension_resource!(entity
                    .id()
                    .try_clone_for_decode(ctx, "f3d radial at tolerance entity id",));
                dimension_resource!(ctx.push_vec(&mut members, id, "f3d owner radial member"));
            }
            let parameter = dimension_resource!(parameter_id
                .try_clone_for_decode(ctx, "f3d owner scoped radial repeated parameter id",));
            Some(Ok(kind.repeated_definition(members, parameter)))
        }
    }
}

pub(crate) fn constraint_parameters(
    definition: &cadmpeg_ir::sketches::SketchConstraintDefinitionInput,
) -> impl Iterator<Item = &cadmpeg_ir::features::ParameterId> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let parameters = match definition {
        Definition::Offset { parameter, .. } => [
            parameter.as_ref().map(|parameter| &parameter.id),
            None,
            None,
            None,
        ],
        Definition::Native { parameter, .. } => [parameter.as_ref(), None, None, None],
        Definition::PolarDistance {
            distance_parameter, ..
        } => [distance_parameter.as_ref(), None, None, None],
        Definition::DistanceLociValue { parameter, .. } => [parameter.as_ref(), None, None, None],
        Definition::Distance { parameter, .. }
        | Definition::DistanceLoci { parameter, .. }
        | Definition::HorizontalDistance { parameter, .. }
        | Definition::VerticalDistance { parameter, .. }
        | Definition::RepeatedDistance { parameter, .. }
        | Definition::RepeatedLength { parameter, .. }
        | Definition::ParallelLineSetDistance { parameter, .. }
        | Definition::Angle { parameter, .. }
        | Definition::AngleToAxis { parameter, .. }
        | Definition::Radius { parameter, .. }
        | Definition::RepeatedRadius { parameter, .. }
        | Definition::Diameter { parameter, .. }
        | Definition::RepeatedDiameter { parameter, .. }
        | Definition::SnellsLaw { parameter, .. }
        | Definition::Weight { parameter, .. } => [Some(parameter), None, None, None],
        Definition::RectangularPattern { pattern } => {
            let [first, second] = pattern.directions();
            [
                first
                    .distance
                    .as_ref()
                    .map(cadmpeg_ir::sketches::SketchPatternDistance::parameter),
                first.count_parameter.as_ref(),
                second
                    .distance
                    .as_ref()
                    .map(cadmpeg_ir::sketches::SketchPatternDistance::parameter),
                second.count_parameter.as_ref(),
            ]
        }
        Definition::CircularPattern { pattern } => [
            pattern.angle_parameter(),
            pattern.count_parameter(),
            None,
            None,
        ],
        Definition::Disabled {}
        | Definition::Coincident { .. }
        | Definition::ProjectedCopy { .. }
        | Definition::Polygon { .. }
        | Definition::SplineGroup { .. }
        | Definition::TextFrame { .. }
        | Definition::TextPath { .. }
        | Definition::CoincidentLoci { .. }
        | Definition::SameCoordinate { .. }
        | Definition::PointSymmetric { .. }
        | Definition::TangentLoci { .. }
        | Definition::AtIntersection { .. }
        | Definition::Midpoint { .. }
        | Definition::PointCoordinateValues { .. }
        | Definition::MidpointCoordinate { .. }
        | Definition::AngleDifference { .. }
        | Definition::ScalarEquality { .. }
        | Definition::Concentric { .. }
        | Definition::Coradial { .. }
        | Definition::Collinear { .. }
        | Definition::Symmetric { .. }
        | Definition::Horizontal { .. }
        | Definition::Vertical { .. }
        | Definition::Parallel { .. }
        | Definition::Perpendicular { .. }
        | Definition::Tangent { .. }
        | Definition::Curvature { .. }
        | Definition::Equal { .. }
        | Definition::EqualDistance { .. }
        | Definition::ArcAngle { .. }
        | Definition::EllipseAngle { .. }
        | Definition::Fixed { .. }
        | Definition::PointOnObject { .. }
        | Definition::InternalAlignment { .. }
        | Definition::Group { .. }
        | Definition::Text { .. } => [None; 4],
    };
    parameters.into_iter().flatten()
}

/// Attach single-locus offset dimensions to uniquely matching typed offset
/// relations and remove their redundant native annotation constraints.
pub(crate) fn bind_offset_dimension_parameters(
    ctx: &DecodeContext<'_>,
    constraints: &mut Vec<cadmpeg_ir::sketches::SketchConstraint>,
    parameters: &[DesignParameter],
) -> Result<(), CodecError> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let mut storage = ctx.reserve_scoped(0, "f3d offset binding storage")?;
    let mut parameter_values = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "f3d offset parameter source")? {
        if let Some(value) = design_length(parameter) {
            let mut identity_storage = ctx.reserve_scoped(0, "f3d offset parameter identity")?;
            let id = identity_storage.with_storage(|| {
                crate::design::identity::neutral_parameter_id_with_operation(
                    ctx,
                    parameter,
                    "f3d parameter identifier",
                )
            })?;
            if let Some(previous) = ctx.get_mut_hash_map(
                &mut parameter_values,
                &id,
                "f3d offset parameter value duplicate",
            )? {
                let (previous_value, _) = previous;
                *previous_value = value.get();
            } else {
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut parameter_values,
                        id,
                        (value.get(), identity_storage),
                        "f3d offset parameter value index",
                    )
                })?;
            }
        }
    }
    let mut candidate_storage = ctx.reserve_scoped(0, "f3d offset candidate storage")?;
    let mut offsets_by_source = HashMap::new();
    let mut remaining_constraints = Some(constraints.as_slice());
    let mut unique_offset_for = |sketch: &cadmpeg_ir::sketches::SketchId,
                                 entity: &cadmpeg_ir::sketches::SketchEntityId,
                                 parameter_value: f64|
     -> Result<Option<usize>, CodecError> {
        if let Some(source) = remaining_constraints.take() {
            for (index, constraint) in ctx
                .admit_iter(source, "f3d offset candidate source")?
                .enumerate()
            {
                let Definition::Offset {
                    pairs,
                    distance,
                    parameter: None,
                } = constraint.definition.kind()
                else {
                    continue;
                };
                for pair in ctx.admit_iter(pairs, "f3d offset candidate pair source")? {
                    let key = (&constraint.sketch, &pair.source);
                    if let Some(candidates) = ctx.get_mut_hash_map(
                        &mut offsets_by_source,
                        &key,
                        "f3d offset candidate duplicate",
                    )? {
                        candidate_storage.with_storage(|| {
                            ctx.push_vec(
                                candidates,
                                (index, distance.get()),
                                "f3d offset candidate",
                            )
                        })?;
                    } else {
                        let mut candidates = Vec::new();
                        candidate_storage.with_storage(|| {
                            ctx.push_vec(
                                &mut candidates,
                                (index, distance.get()),
                                "f3d offset candidate",
                            )?;
                            ctx.insert_hash_map(
                                &mut offsets_by_source,
                                key,
                                candidates,
                                "f3d offset candidate index",
                            )
                        })?;
                    }
                }
            }
        }
        let Some(candidates) = ctx.get_hash_map(
            &offsets_by_source,
            &(sketch, entity),
            "f3d offset candidate lookup",
        )?
        else {
            return Ok(None);
        };
        let matches =
            |(_, distance): &(usize, f64)| Ok(scalar_close(*distance, parameter_value.abs()));
        let Some(position) = ctx.position_by(candidates, matches, "f3d offset first source")?
        else {
            return Ok(None);
        };
        if ctx.any_by(
            &candidates[position + 1..],
            matches,
            "f3d offset unique suffix",
        )? {
            return Ok(None);
        }
        Ok(Some(candidates[position].0))
    };
    let mut bindings = Vec::new();
    for (dimension_index, dimension) in ctx
        .admit_iter(constraints.as_slice(), "f3d offset dimension source")?
        .enumerate()
    {
        let Definition::Native {
            native_kind,
            entities,
            parameter: Some(parameter),
            operands,
            ..
        } = dimension.definition.kind()
        else {
            continue;
        };
        let [entity] = entities.as_slice() else {
            continue;
        };
        if !native_kind.as_str().starts_with("Linear Dimension")
            || operands.len() != 2
            || operands[0].native_kind.as_str() != "null_locus"
            || operands[1].native_kind.as_str() != "curve"
        {
            continue;
        }
        let Some(parameter_value) = ctx
            .get_hash_map(
                &parameter_values,
                parameter,
                "f3d offset parameter value lookup",
            )?
            .map(|(value, _)| *value)
        else {
            continue;
        };
        let Some(offset_index) = unique_offset_for(&dimension.sketch, entity, parameter_value)?
        else {
            continue;
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut bindings,
                (dimension_index, offset_index, parameter_value),
                "f3d offset dimension binding",
            )
        })?;
    }
    drop(offsets_by_source);
    drop(candidate_storage);
    let mut offset_counts = HashMap::new();
    for binding in ctx.admit_iter(&bindings, "f3d offset binding count source")? {
        if let Some(count) = ctx.get_mut_hash_map(
            &mut offset_counts,
            &binding.1,
            "f3d offset binding count lookup",
        )? {
            *count += 1;
        } else {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut offset_counts,
                    binding.1,
                    1usize,
                    "f3d offset binding count",
                )
            })?;
        }
    }
    ctx.retain_vec(
        &mut bindings,
        |binding| {
            Ok(ctx.get_hash_map(
                &offset_counts,
                &binding.1,
                "f3d offset binding count lookup",
            )? == Some(&1))
        },
        "f3d offset binding uniqueness source",
    )?;
    let mut removed = HashSet::new();
    for &(dimension_index, offset_index, parameter_value) in
        ctx.admit_iter(&bindings, "f3d offset binding mutation source")?
    {
        let Definition::Native {
            parameter: Some(parameter),
            ..
        } = constraints[dimension_index].definition.kind()
        else {
            return Err(CodecError::malformed(
                "offset dimension binding source is absent",
            ));
        };
        let parameter = parameter.try_clone_for_decode(ctx, "f3d offset driving parameter id")?;
        constraints[offset_index].definition.set_offset_parameter(
            cadmpeg_ir::sketches::OffsetParameter {
                id: parameter,
                negated: parameter_value.is_sign_negative(),
            },
        );
        storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut removed,
                dimension_index,
                "f3d offset removed dimension",
            )
        })?;
    }
    let mut index = 0usize;
    ctx.retain_vec(
        constraints,
        |_| {
            let keep =
                !ctx.contains_hash_set(&removed, &index, "f3d offset removed dimension lookup")?;
            index += 1;
            Ok(keep)
        },
        "f3d offset constraint removal source",
    )?;
    Ok(())
}

/// Project dimensions owned by model-space sketches without assigning them
/// planar relation semantics.
pub(crate) fn project_spatial_dimension_constraints(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    linear_tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SpatialSketchConstraint>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchNativeOperand, SpatialSketchConstraint,
        SpatialSketchConstraintDefinitionInput,
    };

    let &DimensionConstraintInputs {
        placements,
        parameters,
        owners,
        companions,
        points,
        curves,
        ..
    } = inputs;

    let mut index_storage = ctx.reserve_scoped(0, "f3d spatial dimension indexes")?;
    let mut spatial_ids = HashSet::new();
    if !placements.is_empty() {
        for sketch in ctx.admit_iter(spatial_sketches, "f3d spatial sketch source")? {
            index_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut spatial_ids,
                    &sketch.id,
                    "f3d spatial sketch identity index",
                )
            })?;
        }
    }
    let mut spatial_by_planar_id = HashMap::new();
    let mut spatial_by_scope = HashMap::new();
    for placement in ctx.admit_iter(placements, "f3d spatial placement source")? {
        let mut temporary = ctx.reserve_scoped(0, "f3d spatial placement identity")?;
        let spatial_id = temporary
            .with_storage(|| crate::design::identity::neutral_spatial_sketch_id(ctx, placement))?;
        let sketch = ctx
            .get_hash_set(&spatial_ids, &spatial_id, "f3d spatial sketch lookup")?
            .copied();
        drop(spatial_id);
        drop(temporary);
        let Some(sketch) = sketch else {
            continue;
        };
        let mut identity_storage = ctx.reserve_scoped(0, "f3d spatial planar identity")?;
        let planar_id = identity_storage
            .with_storage(|| crate::design::identity::neutral_sketch_id(ctx, placement))?;
        if ctx.contains_key_hash_map(
            &spatial_by_planar_id,
            &planar_id,
            "f3d spatial planar sketch lookup",
        )? {
            drop(planar_id);
            drop(identity_storage);
        } else {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut spatial_by_planar_id,
                    planar_id,
                    (sketch, identity_storage),
                    "f3d spatial planar sketch index",
                )
            })?;
        }
        if let (Some(scope), Some(scope_record_index)) = (
            native_stream_charged(ctx, &placement.id, "resolve F3D dimension stream")?,
            placement.scope_record_index,
        ) {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut spatial_by_scope,
                    (scope, scope_record_index),
                    sketch,
                    "f3d spatial scope sketch index",
                )
            })?;
        }
    }
    let mut native_record_indices = HashMap::new();
    for point in ctx.admit_iter(points, "f3d spatial native point source")? {
        let Some(scope) = native_stream_charged(ctx, &point.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        index_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut native_record_indices,
                point.id.as_str(),
                (scope, point.record_index),
                "f3d spatial native record index",
            )
        })?;
    }
    for curve in ctx.admit_iter(curves, "f3d spatial native curve source")? {
        let Some(scope) = native_stream_charged(ctx, &curve.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        index_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut native_record_indices,
                curve.id.as_str(),
                (scope, curve.record_index),
                "f3d spatial native record index",
            )
        })?;
    }
    let mut spatial_by_record = HashMap::new();
    for entity in ctx.admit_iter(spatial_entities, "f3d spatial projected entity source")? {
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        if let Some(key) = ctx.get_hash_map(
            &native_record_indices,
            native_ref,
            "f3d spatial native record lookup",
        )? {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut spatial_by_record,
                    *key,
                    entity,
                    "f3d spatial projected record index",
                )
            })?;
        }
    }
    let mut parameters_by_id = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "f3d spatial parameter source")? {
        index_storage.with_storage(|| {
            let id = crate::design::identity::neutral_parameter_id_with_operation(
                ctx,
                parameter,
                "f3d parameter identifier",
            )?;
            ctx.insert_hash_map(
                &mut parameters_by_id,
                id,
                (
                    parameter,
                    design_length(parameter).map(|length| length.get().abs()),
                ),
                "f3d spatial parameter index",
            )
        })?;
    }
    let source_constraints = project_all_dimension_constraints(ctx, inputs, &[], linear_tolerance)?;
    let mut parameter_constraint_counts = HashMap::new();
    for parameter in ctx
        .admit_iter(&source_constraints, "f3d spatial source constraint scan")?
        .flat_map(|constraint| constraint_parameters(constraint.definition.kind()))
    {
        let Some((parameter, _)) = ctx.get_key_value_hash_map(
            &parameters_by_id,
            parameter,
            "f3d spatial source parameter lookup",
        )?
        else {
            continue;
        };
        if let Some(count) = ctx.get_mut_hash_map(
            &mut parameter_constraint_counts,
            parameter,
            "f3d spatial parameter count lookup",
        )? {
            *count += 1;
        } else {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut parameter_constraint_counts,
                    parameter,
                    1usize,
                    "f3d spatial parameter count index",
                )
                .map(|_| ())
            })?;
        }
    }
    let mut source_parameters = BTreeSet::new();
    let mut projected = Vec::new();
    for constraint in ctx.admit_iter(
        source_constraints,
        "f3d spatial dimension projection source",
    )? {
        let projected_constraint = (|| -> Result<Option<SpatialSketchConstraint>, CodecError> {
            let Some((sketch, _storage)) = ctx.get_hash_map(
                &spatial_by_planar_id,
                &constraint.sketch,
                "f3d spatial projected sketch lookup",
            )?
            else {
                return Ok(None);
            };
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d projected spatial sketch id")?;
            for parameter in constraint_parameters(constraint.definition.kind()) {
                let Some((parameter, _)) = ctx.get_key_value_hash_map(
                    &parameters_by_id,
                    parameter,
                    "f3d spatial source parameter lookup",
                )?
                else {
                    continue;
                };
                index_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut source_parameters,
                        parameter,
                        "f3d spatial source parameter index",
                    )
                    .map(|_| ())
                })?;
            }
            let definition = match constraint.definition.into_kind() {
                SketchConstraintDefinitionInput::Native {
                    native_kind,
                    native_state,
                    parameter,
                    operands,
                    ..
                } => {
                    let symmetry = spatial_reflection_symmetry(
                        ctx,
                        native_kind.as_str(),
                        native_state,
                        &operands,
                        constraint.native_ref.as_deref(),
                        &sketch,
                        &spatial_by_record,
                    )
                    .transpose()?;
                    let offset = parameter
                        .as_ref()
                        .and_then(|parameter_id| {
                            let (parameter, expected) = *dimension_resource!(ctx.get_hash_map(
                                &parameters_by_id,
                                parameter_id,
                                "f3d spatial parameter lookup"
                            ))?;
                            let expected = expected?;
                            let signed = design_length(parameter)?.get();
                            spatial_counted_offset_dimension_definition(
                                ctx,
                                (native_kind.as_str(), native_state, &operands),
                                (parameter_id, expected, signed),
                                &sketch,
                                spatial_sketches,
                                &spatial_by_record,
                            )
                        })
                        .transpose()?;
                    let distance = parameter
                        .as_ref()
                        .and_then(|parameter| {
                            let (_, expected) = *dimension_resource!(ctx.get_hash_map(
                                &parameters_by_id,
                                parameter,
                                "f3d spatial parameter lookup"
                            ))?;
                            let expected = expected?;
                            let scope = dimension_resource!(native_stream_charged(
                                ctx,
                                constraint.native_ref.as_deref()?,
                                "resolve F3D dimension stream"
                            ))?;
                            let mut measured_storage = dimension_resource!(
                                ctx.reserve_scoped(0, "f3d spatial distance locus storage")
                            );
                            let mut measured = Vec::new();
                            for operand in dimension_resource!(ctx
                                .admit_iter(
                                    operands.as_slice(),
                                    "f3d spatial distance operand source"
                                )
                                .map_err(CodecError::from))
                            {
                                if !operand_field(operand).is_some_and(|field| {
                                    field == "locus" || field.ends_with("_locus")
                                }) || operand.object_index.is_none()
                                {
                                    continue;
                                }
                                let entity = dimension_resource!(ctx.get_hash_map(
                                    &spatial_by_record,
                                    &(scope, operand.object_index?),
                                    "f3d spatial distance entity lookup"
                                ))
                                .copied()?;
                                dimension_resource!(ctx.push_scoped_vec(
                                    &mut measured_storage,
                                    &mut measured,
                                    entity,
                                    "f3d spatial distance locus"
                                ));
                            }
                            let [first, second] = measured.as_slice() else {
                                return None;
                            };
                            if !native_kind.as_str().starts_with("Linear Dimension")
                                || !dimension_resource!(ctx.equal(
                                    &first.sketch,
                                    &sketch,
                                    "f3d spatial distance first sketch equality"
                                ))
                                || !dimension_resource!(ctx.equal(
                                    &second.sketch,
                                    &sketch,
                                    "f3d spatial distance second sketch equality"
                                ))
                                || dimension_resource!(ctx.equal(
                                    first.id(),
                                    second.id(),
                                    "f3d spatial distance entity equality"
                                ))
                            {
                                return None;
                            }
                            if spatial_point_distance_matches(
                                &first.geometry,
                                &second.geometry,
                                expected,
                            ) {
                                Some(Ok(SpatialSketchConstraintDefinitionInput::PointDistance {
                                    first: dimension_resource!((first.id()).try_clone_for_decode(
                                        ctx,
                                        "f3d spatial distance first id"
                                    )),
                                    second: dimension_resource!((second.id())
                                        .try_clone_for_decode(
                                            ctx,
                                            "f3d spatial distance second id"
                                        )),
                                    parameter: dimension_resource!((parameter)
                                        .try_clone_for_decode(
                                            ctx,
                                            "f3d spatial distance parameter id"
                                        )),
                                }))
                            } else if spatial_parallel_line_distance_matches(
                                &first.geometry,
                                &second.geometry,
                                expected,
                            ) {
                                Some(Ok(
                                    SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
                                        first: dimension_resource!((first.id())
                                            .try_clone_for_decode(
                                                ctx,
                                                "f3d spatial distance first id"
                                            )),
                                        second: dimension_resource!((second.id())
                                            .try_clone_for_decode(
                                                ctx,
                                                "f3d spatial distance second id"
                                            )),
                                        parameter: dimension_resource!((parameter)
                                            .try_clone_for_decode(
                                                ctx,
                                                "f3d spatial distance parameter id"
                                            )),
                                    },
                                ))
                            } else {
                                None
                            }
                        })
                        .transpose()?;
                    let owner_scoped = (|| -> Result<Option<SpatialSketchConstraintDefinitionInput>, CodecError> {
                        if operands.len() != 1
                            || operands[0].native_kind.as_str() != "dimension_companion"
                            || !operand_field(&operands[0]).is_some_and(|field| {
                                field == "companion" || field == "companion_payload"
                            })
                        { return Ok(None); }
                        let Some(parameter_id) = parameter.as_ref() else { return Ok(None); };
                        if ctx.get_hash_map(&parameter_constraint_counts, parameter_id, "f3d spatial parameter count lookup")? != Some(&1) {
                            return Ok(None);
                        }
                        let Some((parameter, _length)) = ctx.get_hash_map(&parameters_by_id, parameter_id, "f3d spatial parameter lookup")? else { return Ok(None); };
                        let line_length = owner_scoped_spatial_line_length_dimension_definition(
                            ctx,
                            spatial_entities,
                            &sketch,
                            parameter,
                            parameter_id,
                            linear_tolerance,
                        )?;
                        line_length.map(Ok)
                        .or_else(|| {
                            unique_spatial_parallel_line_dimension_definition(ctx,
                                spatial_entities,
                                &sketch,
                                parameter,
                                parameter_id,
                            )
                        })
                        .or_else(|| {
                            owner_scoped_spatial_repeated_profile_line_distance_definition(ctx,
                                spatial_entities,
                                spatial_sketches,
                                &sketch,
                                parameter,
                                parameter_id,
                            )
                        })
                        .or_else(|| {
                            owner_scoped_spatial_parallel_line_set_dimension_definition(ctx,
                                spatial_entities,
                                &sketch,
                                parameter,
                                parameter_id,
                                linear_tolerance,
                            )
                        }).transpose()
                    })()?;
                    symmetry.or(offset).or(distance).or(owner_scoped).unwrap_or(
                        SpatialSketchConstraintDefinitionInput::Native {
                            native_kind,
                            native_state,
                            parameter,
                            operands,
                        },
                    )
                }
                _ => return Ok(None),
            };
            let Some(definition) =
                cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(definition).ok()
            else {
                return Ok(None);
            };
            Ok(Some(SpatialSketchConstraint {
                id: constraint.id,
                sketch,
                definition,
                native_ref: constraint.native_ref,
            }))
        })()?;
        if let Some(projected_constraint) = projected_constraint {
            ctx.push_vec(
                &mut projected,
                projected_constraint,
                "f3d projected spatial dimension output",
            )?;
        }
    }

    let retained_parameter_ids = ctx
        .admit_iter(&projected, "f3d spatial retained parameter source")?
        .filter_map(|constraint| match constraint.definition.kind() {
            SpatialSketchConstraintDefinitionInput::Native {
                parameter: Some(parameter),
                ..
            }
            | SpatialSketchConstraintDefinitionInput::PointDistance { parameter, .. }
            | SpatialSketchConstraintDefinitionInput::PointLineDistance { parameter, .. }
            | SpatialSketchConstraintDefinitionInput::LineLength { parameter, .. }
            | SpatialSketchConstraintDefinitionInput::RepeatedLineLength { parameter, .. }
            | SpatialSketchConstraintDefinitionInput::ParallelLineDistance { parameter, .. }
            | SpatialSketchConstraintDefinitionInput::RepeatedParallelLineDistance {
                parameter,
                ..
            }
            | SpatialSketchConstraintDefinitionInput::ParallelLineSetDistance {
                parameter, ..
            } => Some(parameter),
            SpatialSketchConstraintDefinitionInput::Offset {
                parameter: Some(parameter),
                ..
            } => Some(&parameter.id),
            _ => None,
        });
    let mut retained_parameters = HashSet::new();
    for parameter in retained_parameter_ids {
        let Some((parameter, _)) = ctx.get_key_value_hash_map(
            &parameters_by_id,
            parameter,
            "f3d retained spatial parameter lookup",
        )?
        else {
            continue;
        };
        index_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut retained_parameters,
                parameter,
                "f3d retained spatial parameter index",
            )
            .map(|_| ())
        })?;
    }
    let mut owners_by_record = HashMap::new();
    for owner in ctx.admit_iter(owners, "f3d spatial owner source")? {
        if let Some(scope) = native_stream_charged(ctx, owner.id(), "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut owners_by_record,
                    (scope, owner.record_index()),
                    owner,
                    "f3d spatial owner record index",
                )
            })?;
        }
    }
    let mut companions_by_record = HashMap::new();
    for companion in ctx.admit_iter(companions, "f3d spatial companion source")? {
        if let Some(scope) =
            native_stream_charged(ctx, companion.id(), "resolve F3D dimension stream")?
        {
            index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut companions_by_record,
                    (scope, companion.record_index()),
                    companion,
                    "f3d spatial companion record index",
                )
            })?;
        }
    }
    for parameter_id in ctx.admit_iter(&source_parameters, "f3d missing spatial parameter scan")? {
        if ctx.contains_hash_set(
            &retained_parameters,
            parameter_id,
            "f3d retained spatial parameter lookup",
        )? {
            continue;
        }
        let Some((parameter, _length)) = ctx.get_hash_map(
            &parameters_by_id,
            *parameter_id,
            "f3d spatial parameter lookup",
        )?
        else {
            continue;
        };
        let Some(scope) =
            native_stream_charged(ctx, &parameter.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some(owner_record_index) = parameter.owner_record_index() else {
            continue;
        };
        let Some(owner) = ctx.get_hash_map(
            &owners_by_record,
            &(scope, owner_record_index),
            "f3d spatial owner record lookup",
        )?
        else {
            continue;
        };
        let Some(companion) = ctx.get_hash_map(
            &companions_by_record,
            &(scope, owner.companion_record_index()),
            "f3d spatial companion record lookup",
        )?
        else {
            continue;
        };
        let Some(sketch) = ctx.get_hash_map(
            &spatial_by_scope,
            &(scope, owner.scope_record_index()),
            "f3d spatial scope sketch lookup",
        )?
        else {
            continue;
        };
        let sketch = (sketch).try_clone_for_decode(ctx, "f3d missing spatial sketch id")?;
        let native_ref =
            ctx.copy_retained_text(companion.id(), "f3d missing spatial operand native id")?;
        let constraint_native_ref =
            ctx.copy_retained_text(companion.id(), "f3d missing spatial constraint native id")?;
        let definition = cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(
            SpatialSketchConstraintDefinitionInput::Native {
                native_kind: copy_dimension_source_kind(
                    ctx,
                    parameter,
                    "f3d spatial companion source kind",
                )?,
                native_state: None,
                parameter: Some(
                    (parameter_id)
                        .try_clone_for_decode(ctx, "f3d missing spatial output parameter id")?,
                ),
                operands: vec![SketchNativeOperand {
                    native_kind: cadmpeg_core::nonblank_literal!("dimension_companion"),
                    field: Some(NativeOperandField {
                        name: if companion
                            .payload()
                            .is_none_or(|payload| payload.byte_length() == 0)
                        {
                            cadmpeg_core::nonblank_literal!("companion")
                        } else {
                            cadmpeg_core::nonblank_literal!("companion_payload")
                        },
                        role: None,
                    }),
                    object_index: Some(companion.record_index()),
                    native_ref: Some(native_ref),
                }],
            },
        )
        .ok();
        let Some(definition) = definition else {
            continue;
        };
        ctx.push_vec(
            &mut projected,
            SpatialSketchConstraint {
                id: crate::design::identity::neutral_dimension_constraint_id(
                    ctx,
                    parameter_id,
                    "companion-payload",
                )?,
                sketch,
                definition,
                native_ref: Some(constraint_native_ref),
            },
            "f3d missing spatial constraint output",
        )?;
    }
    Ok(projected)
}

fn owner_scoped_spatial_line_length_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Result<Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return Ok(None);
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return Ok(None);
    }
    let mut matches = Vec::new();
    for entity in ctx.admit_iter(entities, "f3d spatial line length source")? {
        if !ctx.equal(
            &entity.sketch,
            sketch,
            "f3d spatial line length sketch identity",
        )? {
            continue;
        }
        let SpatialSketchGeometryDefinition::Line { start, end } = *entity.geometry.definition()
        else {
            continue;
        };
        let measured = (end.x - start.x).hypot((end.y - start.y).hypot(end.z - start.z));
        if !measured.is_finite() {
            continue;
        }
        let tolerance = linear_tolerance.max(
            EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_LINE_LENGTH_DIMENSION_DEFINITION_E9
                * (1.0 + measured.abs().max(expected.abs())),
        );
        if (measured - expected).abs() <= tolerance {
            let id = entity
                .id()
                .try_clone_for_decode(ctx, "f3d spatial line length entity id")?;
            ctx.push_vec(&mut matches, id, "f3d spatial line length match")?;
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(Definition::LineLength {
            entity: matches.remove(0),
            parameter: (parameter_id)
                .try_clone_for_decode(ctx, "f3d spatial line length parameter id")?,
        })),
        _ => Ok(Some(Definition::RepeatedLineLength {
            entities: matches,
            parameter: (parameter_id)
                .try_clone_for_decode(ctx, "f3d spatial line length parameter id")?,
        })),
    }
}

fn unique_spatial_parallel_line_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Linear Dimension") || !design_dimension_unit(parameter)
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let mut scratch =
        dimension_resource!(ctx.reserve_scoped(0, "f3d spatial parallel line scratch"));
    let mut lines = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d spatial parallel line source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d spatial parallel line sketch identity"
        )) && matches!(
            entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Line { .. }
        ) {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut lines,
                entity,
                "f3d spatial parallel line candidate"
            )));
        }
    }
    let mut matched = None;
    let mut first_source = 0..lines.len();
    while let Some(first) = dimension_resource!(
        ctx.next_charged(&mut first_source, "f3d spatial parallel line pair source")
    ) {
        let mut second_source = first + 1..lines.len();
        while let Some(second) = dimension_resource!(
            ctx.next_charged(&mut second_source, "f3d spatial parallel line pair source")
        ) {
            if spatial_parallel_line_distance_matches(
                &lines[first].geometry,
                &lines[second].geometry,
                expected,
            ) {
                if matched.is_some() {
                    return None;
                }
                matched = Some((lines[first], lines[second]));
            }
        }
    }
    let (first, second) = matched?;
    Some(Ok(Definition::ParallelLineDistance {
        first: dimension_resource!(
            (first.id()).try_clone_for_decode(ctx, "f3d unique spatial parallel line first id")
        ),
        second: dimension_resource!(
            (second.id()).try_clone_for_decode(ctx, "f3d unique spatial parallel line second id")
        ),
        parameter: dimension_resource!((parameter_id)
            .try_clone_for_decode(ctx, "f3d unique spatial parallel line parameter id")),
    }))
}

fn owner_scoped_spatial_repeated_profile_line_distance_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchEntityPair,
    };

    if !parameter.source_kind().starts_with("Linear Dimension") || !design_dimension_unit(parameter)
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d spatial repeated scratch"));
    let mut entities_by_id = HashMap::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d spatial repeated entity source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d spatial repeated sketch identity"
        )) {
            dimension_resource!(scratch.with_storage(|| ctx
                .insert_hash_map(
                    &mut entities_by_id,
                    entity.id(),
                    entity,
                    "f3d spatial repeated entity index"
                )
                .map(|_| ())));
        }
    }
    let sketch = dimension_resource!(ctx.find_by(
        sketches,
        |candidate| ctx.equal(
            &candidate.id,
            sketch,
            "f3d spatial repeated sketch identity"
        ),
        "f3d spatial repeated sketch search",
    ))?;
    let mut seen_pairs = HashSet::new();
    let mut used_entities = HashSet::new();
    let mut pairs = Vec::new();
    let mut profile_source = sketch.profiles.iter();
    while let Some(profile) = dimension_resource!(
        ctx.next_charged(&mut profile_source, "f3d spatial repeated profile source")
    ) {
        if profile.boundary().len() < 4 {
            continue;
        }
        let mut boundary_source = 0..profile.boundary().len();
        while let Some(index) = dimension_resource!(
            ctx.next_charged(&mut boundary_source, "f3d spatial repeated boundary source")
        ) {
            let first_id = &profile.boundary()[index].entity;
            let second_id = &profile.boundary()[(index + 2) % profile.boundary().len()].entity;
            let key = if dimension_resource!(ctx.compare(
                first_id,
                second_id,
                "f3d spatial repeated pair order"
            )) == std::cmp::Ordering::Less
            {
                (first_id, second_id)
            } else {
                (second_id, first_id)
            };
            if dimension_resource!(ctx.contains_hash_set(
                &seen_pairs,
                &key,
                "f3d spatial repeated pair lookup"
            )) {
                continue;
            }
            dimension_resource!(scratch.with_storage(|| ctx
                .insert_hash_set(&mut seen_pairs, key, "f3d spatial repeated pair key")
                .map(|_| ())));
            let first = dimension_resource!(ctx.get_hash_map(
                &entities_by_id,
                first_id,
                "f3d spatial repeated first entity lookup"
            ))?;
            let second = dimension_resource!(ctx.get_hash_map(
                &entities_by_id,
                second_id,
                "f3d spatial repeated second entity lookup"
            ))?;
            if !spatial_parallel_line_distance_matches(&first.geometry, &second.geometry, expected)
            {
                continue;
            }
            if dimension_resource!(ctx.contains_hash_set(
                &used_entities,
                first_id,
                "f3d spatial repeated first member lookup"
            )) || dimension_resource!(ctx.contains_hash_set(
                &used_entities,
                second_id,
                "f3d spatial repeated second member lookup"
            )) {
                return None;
            }
            dimension_resource!(scratch.with_storage(|| ctx
                .insert_hash_set(
                    &mut used_entities,
                    first_id,
                    "f3d spatial repeated first member"
                )
                .map(|_| ())));
            dimension_resource!(scratch.with_storage(|| ctx
                .insert_hash_set(
                    &mut used_entities,
                    second_id,
                    "f3d spatial repeated second member"
                )
                .map(|_| ())));
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut pairs,
                (first_id, second_id),
                "f3d spatial repeated borrowed pair",
            )));
        }
    }
    if pairs.len() < 2 {
        return None;
    }
    let mut output = Vec::new();
    for (first, second) in dimension_resource!(ctx
        .admit_iter(&pairs, "f3d spatial repeated output source")
        .map_err(CodecError::from))
    {
        dimension_resource!(ctx.push_vec(
            &mut output,
            SpatialSketchEntityPair {
                first: dimension_resource!(
                    first.try_clone_for_decode(ctx, "f3d spatial repeated first id")
                ),
                second: dimension_resource!(
                    second.try_clone_for_decode(ctx, "f3d spatial repeated second id")
                ),
            },
            "f3d spatial repeated pair",
        ));
    }
    Some(Ok(Definition::RepeatedParallelLineDistance {
        pairs: output,
        parameter: dimension_resource!(parameter_id.try_clone_for_decode(
            ctx,
            "f3d owner scoped spatial repeated profile line distance parameter id",
        )),
    }))
}

fn owner_scoped_spatial_parallel_line_set_dimension_definition(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let mut scratch = dimension_resource!(ctx.reserve_scoped(0, "f3d spatial line set scratch"));
    let mut lines = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(entities, "f3d spatial line set source")
        .map_err(CodecError::from))
    {
        if dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d spatial line set sketch identity"
        )) && matches!(
            entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Line { .. }
        ) {
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut lines,
                entity,
                "f3d spatial line set candidate"
            )));
        }
    }
    let collinear = |first: &cadmpeg_ir::sketches::SpatialSketchEntity,
                     second: &cadmpeg_ir::sketches::SpatialSketchEntity| {
        spatial_parallel_line_distance(&first.geometry, &second.geometry)
            .is_some_and(|distance| distance <= linear_tolerance)
    };
    let mut carriers = Vec::<Vec<&cadmpeg_ir::sketches::SpatialSketchEntity>>::new();
    let mut line_source = lines.iter().copied();
    while let Some(line) =
        dimension_resource!(ctx.next_charged(&mut line_source, "f3d spatial line carrier source"))
    {
        let matches_carrier = |carrier: &Vec<&cadmpeg_ir::sketches::SpatialSketchEntity>| {
            ctx.all_by(
                carrier,
                |member| Ok(collinear(member, line)),
                "f3d spatial line carrier member agreement",
            )
        };
        let first = dimension_resource!(ctx.position_by(
            &carriers,
            matches_carrier,
            "f3d spatial line carrier candidate"
        ));
        if let Some(index) = first {
            if dimension_resource!(ctx.any_by(
                &carriers[index + 1..],
                matches_carrier,
                "f3d spatial line carrier candidate"
            )) {
                return None;
            }
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut carriers[index],
                line,
                "f3d spatial carrier member",
            )));
        } else {
            let mut carrier = Vec::new();
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut carrier,
                line,
                "f3d spatial carrier member"
            )));
            dimension_resource!(scratch.with_storage(|| ctx.push_vec(
                &mut carriers,
                carrier,
                "f3d spatial carrier"
            )));
        }
    }
    let mut matched = None;
    let mut first_source = 0..carriers.len();
    while let Some(first) = dimension_resource!(
        ctx.next_charged(&mut first_source, "f3d spatial line carrier pair source")
    ) {
        let mut second_source = first + 1..carriers.len();
        while let Some(second) = dimension_resource!(
            ctx.next_charged(&mut second_source, "f3d spatial line carrier pair source")
        ) {
            if carriers[first].len() == 1 && carriers[second].len() == 1 {
                continue;
            }
            let measured = dimension_resource!(ctx.find_map(
                &carriers[first],
                |first| ctx.find_map(
                    &carriers[second],
                    |second| Ok(spatial_parallel_line_span_distance(
                        &first.geometry,
                        &second.geometry,
                        linear_tolerance
                    )),
                    "f3d spatial line carrier second span search",
                ),
                "f3d spatial line carrier first span search",
            ));
            let Some(measured) = measured else {
                continue;
            };
            if !measured.is_finite() {
                continue;
            }
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            if (measured - expected).abs() > tolerance {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((first, second));
        }
    }
    let (first, second) = matched?;
    Some(Ok(Definition::ParallelLineSetDistance {
        first: dimension_resource!(copy_spatial_entity_members(ctx, &carriers[first])),
        second: dimension_resource!(copy_spatial_entity_members(ctx, &carriers[second])),
        parameter: dimension_resource!((parameter_id).try_clone_for_decode(
            ctx,
            "f3d owner scoped spatial parallel line set parameter id"
        )),
    }))
}

fn operand_field(operand: &cadmpeg_ir::sketches::SketchNativeOperand) -> Option<&str> {
    operand.field.as_ref().map(|field| field.name.as_str())
}

fn operand_role(operand: &cadmpeg_ir::sketches::SketchNativeOperand) -> Option<u32> {
    operand.field.as_ref().and_then(|field| field.role)
}

fn spatial_reflection_symmetry(
    ctx: &DecodeContext<'_>,
    native_kind: &str,
    native_state: Option<u64>,
    operands: &[cadmpeg_ir::sketches::SketchNativeOperand],
    native_ref: Option<&str>,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    spatial_by_record: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SpatialSketchEntity>,
) -> Option<Result<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !native_kind.starts_with("Linear Dimension") || native_state != Some(0) {
        return None;
    }
    let mut owners = operands.iter();
    let owner = dimension_resource!(ctx.find_by(
        owners.by_ref(),
        |operand| Ok(operand_field(operand) == Some("owner")),
        "f3d spatial reflection owner search",
    ))?;
    if dimension_resource!(ctx.any_by(
        owners,
        |operand| Ok(operand_field(operand) == Some("owner")),
        "f3d spatial reflection owner search"
    )) {
        return None;
    }
    if !operand_role(owner).is_some_and(|role| {
        crate::records::sketch_relations::constraint_kinds_iter(u64::from(role))
            .any(|kind| kind == SketchConstraintKind::Symmetry)
    }) {
        return None;
    }
    let scope = dimension_resource!(native_stream_charged(
        ctx,
        native_ref?,
        "resolve F3D dimension stream"
    ))?;
    let mut loci = operands.iter();
    let mut entity_for =
        || -> Result<Option<&cadmpeg_ir::sketches::SpatialSketchEntity>, CodecError> {
            let Some(operand) = ctx.find_by(
                loci.by_ref(),
                |operand| Ok(operand_field(operand) == Some("locus")),
                "f3d spatial reflection locus search",
            )?
            else {
                return Ok(None);
            };
            let Some(record) = operand.object_index else {
                return Ok(None);
            };
            Ok(ctx
                .get_hash_map(
                    spatial_by_record,
                    &(scope, record),
                    "f3d spatial reflection entity lookup",
                )?
                .copied())
        };
    let entities = [
        dimension_resource!(entity_for())?,
        dimension_resource!(entity_for())?,
        dimension_resource!(entity_for())?,
    ];
    if dimension_resource!(ctx.any_by(
        loci,
        |operand| Ok(operand_field(operand) == Some("locus")),
        "f3d spatial reflection locus search"
    )) {
        return None;
    }
    let [first, second, third] = entities;
    for entity in entities {
        if !dimension_resource!(ctx.equal(
            &entity.sketch,
            sketch,
            "f3d spatial reflection sketch identity"
        )) {
            return None;
        }
    }
    if dimension_resource!(ctx.equal(
        first.id(),
        second.id(),
        "f3d spatial reflection entity identity"
    )) || dimension_resource!(ctx.equal(
        first.id(),
        third.id(),
        "f3d spatial reflection entity identity"
    )) || dimension_resource!(ctx.equal(
        second.id(),
        third.id(),
        "f3d spatial reflection entity identity"
    )) {
        return None;
    }
    let mut points = [None; 2];
    let mut point_count = 0;
    let mut axis = None;
    for entity in entities {
        match *entity.geometry.definition() {
            SpatialSketchGeometryDefinition::Point { position } => {
                let slot = points.get_mut(point_count)?;
                *slot = Some((entity, position.get()));
                point_count += 1;
            }
            SpatialSketchGeometryDefinition::Line { start, end } if axis.is_none() => {
                axis = Some((entity, start.get(), end.get()));
            }
            _ => return None,
        }
    }
    let [Some((first, first_position)), Some((second, second_position))] = points else {
        return None;
    };
    let (axis, axis_start, axis_end) = axis?;
    cadmpeg_ir::eval::spatial_points_are_reflections(
        first_position,
        second_position,
        axis_start,
        axis_end,
    )
    .then(|| -> Result<_, CodecError> {
        Ok(Definition::Symmetric {
            first: (first.id())
                .try_clone_for_decode(ctx, "f3d spatial reflection symmetry first id")?,
            second: (second.id())
                .try_clone_for_decode(ctx, "f3d spatial reflection symmetry second id")?,
            axis: (axis.id())
                .try_clone_for_decode(ctx, "f3d spatial reflection symmetry axis id")?,
        })
    })
}

fn spatial_counted_offset_dimension_definition(
    ctx: &DecodeContext<'_>,
    native: (
        &str,
        Option<u64>,
        &[cadmpeg_ir::sketches::SketchNativeOperand],
    ),
    measurement: (&cadmpeg_ir::features::ParameterId, f64, f64),
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_by_record: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SpatialSketchEntity>,
) -> Option<Result<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::scalar::Length;

    use cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput as Definition;

    let (native_kind, native_state, operands) = native;
    let (parameter, distance, signed_parameter) = measurement;

    if !native_kind.starts_with("Linear Dimension")
        || native_state != Some(0x20)
        || !distance.is_finite()
        || distance <= EPS_DIMENSIONS_SPATIAL_COUNTED_OFFSET_DIMENSION_DEFINITION_E9
    {
        return None;
    }
    let parameter_factor = offset_parameter_factor(distance, signed_parameter)?;
    let scope = dimension_resource!(ctx.find_map(
        operands,
        |operand| match operand.native_ref.as_deref() {
            Some(native_ref) =>
                native_stream_charged(ctx, native_ref, "resolve F3D spatial offset stream"),
            None => Ok(None),
        },
        "f3d spatial offset stream search",
    ))?;
    let owner_position = dimension_resource!(ctx.position_by(
        operands,
        |operand| Ok(operand_field(operand) != Some("locus")),
        "f3d spatial offset locus prefix",
    ))
    .unwrap_or(operands.len());
    let loci = &operands[..owner_position];
    let owner = operands.get(owner_position)?;
    let returns = operands.get(owner_position + 1..)?;
    if operand_field(owner) != Some("owner")
        || owner.native_kind.as_str() != "record"
        || operand_role(owner) != Some(0)
        || dimension_resource!(ctx.any_by(
            loci,
            |operand| Ok(operand.native_kind.as_str() != "curve"),
            "f3d spatial offset locus kind",
        ))
        || dimension_resource!(ctx.any_by(
            returns,
            |operand| Ok(operand.native_kind.as_str() != "curve"
                || operand_field(operand) != Some("return")
                || operand_role(operand).is_some()),
            "f3d spatial offset return shape",
        ))
        || loci.len() < 4
        || !loci.len().is_multiple_of(2)
        || returns.len() != loci.len()
    {
        return None;
    }
    let source_count = dimension_resource!(ctx.position_by(
        loci,
        |operand| Ok(operand_role(operand) == Some(0)),
        "f3d spatial offset source role prefix",
    ))?;
    if source_count == 0
        || source_count * 2 != loci.len()
        || dimension_resource!(ctx.any_by(
            &loci[source_count..],
            |operand| Ok(operand_role(operand) != Some(0)),
            "f3d spatial offset result role suffix",
        ))
    {
        return None;
    }
    let mut role_storage = dimension_resource!(ctx.reserve_scoped(0, "f3d spatial offset roles"));
    let mut roles = HashMap::new();
    let mut locus_source = loci.iter();
    while let Some(operand) =
        dimension_resource!(ctx.next_charged(&mut locus_source, "f3d spatial offset role source",))
    {
        let record = operand.object_index?;
        let role = operand_role(operand)?;
        dimension_resource!(role_storage.with_storage(|| ctx
            .insert_hash_map(&mut roles, record, role, "f3d spatial offset role")
            .map(|_| ())));
    }
    if roles.len() != loci.len() {
        return None;
    }
    let mut record_storage =
        dimension_resource!(ctx.reserve_scoped(0, "f3d spatial offset result records"));
    let mut result_records = BTreeSet::new();
    let mut return_source = returns.chunks_exact(2);
    while let Some(pair) = dimension_resource!(ctx.next_charged(
        &mut return_source,
        "f3d spatial offset result record source",
    )) {
        let record = pair[1].object_index?;
        dimension_resource!(record_storage.with_storage(|| ctx
            .insert_btree_set(
                &mut result_records,
                record,
                "f3d spatial offset result record"
            )
            .map(|_| ())));
    }
    let mut identity_storage =
        dimension_resource!(ctx.reserve_scoped(0, "f3d spatial offset result identities"));
    let mut result_ids = BTreeSet::new();
    for record in dimension_resource!(ctx
        .admit_iter(&result_records, "f3d spatial offset result identity source")
        .map_err(CodecError::from))
    {
        if let Some(entity) = dimension_resource!(ctx.get_hash_map(
            spatial_by_record,
            &(scope, *record),
            "f3d spatial offset result entity lookup",
        )) {
            dimension_resource!(identity_storage.with_storage(|| ctx
                .insert_btree_set(
                    &mut result_ids,
                    entity.id(),
                    "f3d spatial offset result identity"
                )
                .map(|_| ())));
        }
    }
    if result_ids.len() != result_records.len() {
        return None;
    }
    let spatial = dimension_resource!(ctx.find_by(
        spatial_sketches,
        |candidate| ctx.equal(&candidate.id, sketch, "f3d spatial offset sketch identity"),
        "f3d spatial offset sketch search",
    ))?;
    let matches_profile =
        |profile: &&cadmpeg_ir::sketches::SpatialSketchProfile| -> Result<bool, CodecError> {
            Ok(profile.boundary().len() == result_ids.len()
                && ctx.all_by(
                    profile.boundary(),
                    |use_| {
                        ctx.contains_btree_set(
                            &result_ids,
                            &use_.entity,
                            "f3d spatial offset boundary membership",
                        )
                    },
                    "f3d spatial offset profile boundary search",
                )?)
        };
    let mut profile_source = spatial.profiles.iter();
    let normal = dimension_resource!(ctx.find_by(
        profile_source.by_ref(),
        matches_profile,
        "f3d spatial offset profile search",
    ))?
    .normal();
    if dimension_resource!(ctx.any_by(
        profile_source,
        |profile| matches_profile(&profile),
        "f3d spatial offset profile search",
    )) {
        return None;
    }
    let mut used_storage =
        dimension_resource!(ctx.reserve_scoped(0, "f3d spatial offset used records"));
    let mut used = HashSet::new();
    let mut pair_storage =
        dimension_resource!(ctx.reserve_scoped(0, "f3d spatial offset borrowed pairs"));
    let mut pairs = Vec::new();
    let mut return_source = returns.chunks_exact(2);
    while let Some(operands) =
        dimension_resource!(ctx.next_charged(&mut return_source, "f3d spatial offset pair source",))
    {
        let source_record = operands[0].object_index?;
        let result_record = operands[1].object_index?;
        if !matches!(dimension_resource!(ctx.get_hash_map(&roles, &source_record, "f3d spatial offset source role lookup")), Some(role) if *role != 0)
            || dimension_resource!(ctx.get_hash_map(
                &roles,
                &result_record,
                "f3d spatial offset result role lookup"
            )) != Some(&0)
            || dimension_resource!(ctx.contains_hash_set(
                &used,
                &source_record,
                "f3d spatial offset used source lookup"
            ))
            || dimension_resource!(ctx.contains_hash_set(
                &used,
                &result_record,
                "f3d spatial offset used result lookup"
            ))
        {
            return None;
        }
        dimension_resource!(used_storage.with_storage(|| ctx
            .insert_hash_set(&mut used, source_record, "f3d spatial offset used source")
            .map(|_| ())));
        dimension_resource!(used_storage.with_storage(|| ctx
            .insert_hash_set(&mut used, result_record, "f3d spatial offset used result")
            .map(|_| ())));
        let source = *dimension_resource!(ctx.get_hash_map(
            spatial_by_record,
            &(scope, source_record),
            "f3d spatial offset source entity lookup"
        ))?;
        let result = *dimension_resource!(ctx.get_hash_map(
            spatial_by_record,
            &(scope, result_record),
            "f3d spatial offset pair result entity lookup"
        ))?;
        if !dimension_resource!(ctx.equal(
            &source.sketch,
            sketch,
            "f3d spatial offset source sketch identity"
        )) || !dimension_resource!(ctx.equal(
            &result.sketch,
            sketch,
            "f3d spatial offset result sketch identity"
        )) || !spatial_curve(&source.geometry)
            || !spatial_curve(&result.geometry)
        {
            return None;
        }
        dimension_resource!(pair_storage.with_storage(|| ctx.push_vec(
            &mut pairs,
            (source, result),
            "f3d spatial offset borrowed pair",
        )));
    }
    let mut sources = Vec::new();
    let mut results = Vec::new();
    for (source, result) in dimension_resource!(ctx
        .admit_iter(&pairs, "f3d spatial offset output pair source")
        .map_err(CodecError::from))
    {
        dimension_resource!(ctx.push_vec(
            &mut sources,
            dimension_resource!(
                (source.id()).try_clone_for_decode(ctx, "f3d spatial counted offset source id")
            ),
            "f3d spatial offset source member"
        ));
        dimension_resource!(ctx.push_vec(
            &mut results,
            dimension_resource!(
                (result.id()).try_clone_for_decode(ctx, "f3d spatial counted offset result id")
            ),
            "f3d spatial offset result member"
        ));
    }
    Some(Ok(Definition::Offset {
        sources,
        results,
        normal: normal.into(),
        distance: Length::new(distance)?,
        parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
            id: dimension_resource!(
                (parameter).try_clone_for_decode(ctx, "f3d spatial counted offset parameter id")
            ),
            negated: parameter_factor.is_sign_negative(),
        }),
    }))
}

fn spatial_curve(geometry: &cadmpeg_ir::sketches::SpatialSketchGeometry) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;
    matches!(
        (geometry).definition(),
        SpatialSketchGeometryDefinition::Line { .. }
            | SpatialSketchGeometryDefinition::Circle { .. }
            | SpatialSketchGeometryDefinition::Arc { .. }
            | SpatialSketchGeometryDefinition::Nurbs { .. }
    )
}

fn spatial_point_distance_matches(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let (
        SpatialSketchGeometryDefinition::Point { position: first },
        SpatialSketchGeometryDefinition::Point { position: second },
    ) = (first.definition(), second.definition())
    else {
        return false;
    };
    let measured = first.distance(second.get());
    let scale = 1.0 + measured.max(expected.abs());
    expected.is_finite()
        && measured.is_finite()
        && (measured - expected.abs()).abs()
            <= EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9 * scale
}

fn spatial_parallel_line_distance_matches(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    expected: f64,
) -> bool {
    let Some(measured) = spatial_parallel_line_distance(first, second) else {
        return false;
    };
    let scale = 1.0 + measured.max(expected.abs());
    expected.is_finite()
        && measured.is_finite()
        && (measured - expected.abs()).abs()
            <= EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_MATCHES_E9 * scale
}

fn spatial_line_segment(
    geometry: &cadmpeg_ir::sketches::SpatialSketchGeometry,
) -> Option<[Point3; 2]> {
    match geometry.definition() {
        cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line { start, end } => {
            Some([start.get(), end.get()])
        }
        _ => None,
    }
}

fn spatial_parallel_line_distance(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
) -> Option<f64> {
    spatial_parallel_segment_distance(spatial_line_segment(first)?, spatial_line_segment(second)?)
}

fn spatial_parallel_segment_distance(
    [first_start, first_end]: [Point3; 2],
    [second_start, second_end]: [Point3; 2],
) -> Option<f64> {
    let first_direction = first_end.vector_from(first_start);
    let second_direction = second_end.vector_from(second_start);
    let first_length = first_direction.norm();
    let second_length = second_direction.norm();
    let cross = first_direction.cross(second_direction);
    if first_length <= EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E12
        || second_length <= EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E12
        || cross.norm()
            > EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E9 * first_length * second_length
    {
        return None;
    }
    let offset = second_start.vector_from(first_start);
    let area = offset.cross(first_direction).norm();
    Some(area / first_length)
}

fn spatial_parallel_line_span_distance(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    linear_tolerance: f64,
) -> Option<f64> {
    let first_segment = spatial_line_segment(first)?;
    let second_segment = spatial_line_segment(second)?;
    let distance = spatial_parallel_segment_distance(first_segment, second_segment)?;
    let [first_start, first_end] = first_segment;
    let [second_start, second_end] = second_segment;
    let direction = first_end.vector_from(first_start);
    let length = direction.norm();
    let project = |point: Point3| Vector3::new(point.x, point.y, point.z).dot(direction) / length;
    let first_interval = [project(first_start), project(first_end)];
    let second_interval = [project(second_start), project(second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    (first_min.max(second_min) <= first_max.min(second_max) + linear_tolerance).then_some(distance)
}

fn repeated_linear_dimension(
    ctx: &DecodeContext<'_>,
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinitionInput],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchDistanceMeasurement as Measurement,
        SketchLocus,
    };

    if candidates.len() < 2 {
        return None;
    }
    let unique = ctx.with_scoped_storage("f3d repeated dimension member storage", || {
        let mut entities = HashSet::new();
        ctx.all_by(
            candidates,
            |candidate| {
                let (first, second) = match candidate {
                    Definition::Distance { entities: pair, .. } => {
                        let [first, second] = pair.as_slice() else {
                            return Ok(false);
                        };
                        (first, second)
                    }
                    Definition::HorizontalDistance { first, second, .. }
                    | Definition::VerticalDistance { first, second, .. } => {
                        (locus_entity_id(first), locus_entity_id(second))
                    }
                    _ => return Ok(false),
                };
                if ctx.equal(first, second, "f3d repeated member equality")?
                    || ctx.contains_hash_set(&entities, first, "f3d repeated member lookup")?
                    || ctx.contains_hash_set(&entities, second, "f3d repeated member lookup")?
                {
                    return Ok(false);
                }
                ctx.insert_hash_set(&mut entities, first, "f3d repeated first member")?;
                ctx.insert_hash_set(&mut entities, second, "f3d repeated second member")?;
                Ok(true)
            },
            "f3d repeated dimension proof source",
        )
    });
    let (unique, member_storage) = dimension_resource!(unique);
    drop(member_storage);
    if !unique {
        return None;
    }
    let mut measurements = Vec::new();
    for candidate in dimension_resource!(ctx
        .admit_iter(candidates, "f3d repeated dimension output source")
        .map_err(CodecError::from))
    {
        let measurement = match candidate {
            Definition::Distance { entities: pair, .. } => {
                let [first, second] = pair.as_slice() else {
                    return None;
                };
                Measurement::Distance {
                    first: SketchLocus::Entity(dimension_resource!(
                        (first).try_clone_for_decode(ctx, "f3d repeated distance first id")
                    )),
                    second: SketchLocus::Entity(dimension_resource!(
                        (second).try_clone_for_decode(ctx, "f3d repeated distance second id")
                    )),
                }
            }
            Definition::HorizontalDistance { first, second, .. } => Measurement::Horizontal {
                first: dimension_resource!(copy_dimension_locus(
                    ctx,
                    first,
                    "f3d repeated directional first locus"
                )),
                second: dimension_resource!(copy_dimension_locus(
                    ctx,
                    second,
                    "f3d repeated directional second locus"
                )),
            },
            Definition::VerticalDistance { first, second, .. } => Measurement::Vertical {
                first: dimension_resource!(copy_dimension_locus(
                    ctx,
                    first,
                    "f3d repeated directional first locus"
                )),
                second: dimension_resource!(copy_dimension_locus(
                    ctx,
                    second,
                    "f3d repeated directional second locus"
                )),
            },
            _ => return None,
        };
        dimension_resource!(ctx.push_vec(
            &mut measurements,
            measurement,
            "f3d repeated distance measurement"
        ));
    }
    Some(Ok(Definition::RepeatedDistance {
        measurements,
        parameter,
    }))
}

fn locus_entity_id(
    locus: &cadmpeg_ir::sketches::SketchLocus,
) -> &cadmpeg_ir::sketches::SketchEntityId {
    use cadmpeg_ir::sketches::SketchLocus;
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

pub(super) fn null_locus_dimension_definition(
    ctx: &DecodeContext<'_>,
    pair: &DesignDimensionLocusPair,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: (&cadmpeg_ir::features::ParameterId, &'static str),
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchAxis, SketchConstraintDefinitionInput as Definition, SketchGeometry,
        SketchGeometryDefinition,
    };

    if let Some(kind) = RadialDimensionKind::from_source_kind(source_kind) {
        if kind.matches(entity, evaluated_value, linear_tolerance) {
            let parameter = dimension_resource!(parameter.0.try_clone_for_decode(ctx, parameter.1));
            let entity = dimension_resource!(entity
                .id()
                .try_clone_for_decode(ctx, "f3d radial at tolerance entity id",));
            return Some(Ok(match kind {
                RadialDimensionKind::Radius => Definition::Radius { entity, parameter },
                RadialDimensionKind::Diameter => Definition::Diameter { entity, parameter },
            }));
        }
    }
    if source_kind != "Angular Dimension-2"
        || pair.loci()[0].role != 14
        || pair.loci()[1].role != 3
        || !matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        )
    {
        return None;
    }
    let horizontal_axis = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .ok()?;
    line_angle_matches(&entity.geometry, &horizontal_axis, evaluated_value).then(
        || -> Result<_, CodecError> {
            Ok(Definition::AngleToAxis {
                entity: (entity.id()).try_clone_for_decode(ctx, "f3d null locus entity id")?,
                axis: SketchAxis::Horizontal,
                parameter: parameter.0.try_clone_for_decode(ctx, parameter.1)?,
            })
        },
    )
}

fn radial_dimension_definition(
    ctx: &DecodeContext<'_>,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: (&cadmpeg_ir::features::ParameterId, &'static str),
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    radial_dimension_definition_at_tolerance(
        ctx,
        entity,
        source_kind,
        evaluated_value,
        parameter,
        0.0,
    )
}

fn radial_dimension_definition_at_tolerance(
    ctx: &DecodeContext<'_>,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: (&cadmpeg_ir::features::ParameterId, &'static str),
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    let kind = RadialDimensionKind::from_source_kind(source_kind)?;
    kind.matches(entity, evaluated_value, linear_tolerance)
        .then(|| kind.single_definition(ctx, entity, parameter))
}

#[derive(Clone, Copy)]
enum RadialDimensionKind {
    Radius,
    Diameter,
}

impl RadialDimensionKind {
    fn from_source_kind(source_kind: &str) -> Option<Self> {
        if source_kind.starts_with("Radius Dimension")
            || source_kind.starts_with("Radial Dimension")
        {
            Some(Self::Radius)
        } else if source_kind.starts_with("Diameter Dimension") {
            Some(Self::Diameter)
        } else {
            None
        }
    }

    fn matches(
        self,
        entity: &cadmpeg_ir::sketches::SketchEntity,
        evaluated_value: f64,
        linear_tolerance: f64,
    ) -> bool {
        use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;
        let radius = match entity.geometry.definition() {
            Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. } => radius.get(),
            _ => return false,
        };
        let measured = match self {
            Self::Radius => radius,
            Self::Diameter => 2.0 * radius,
        };
        let evaluated = evaluated_value * 10.0;
        let scale = 1.0 + measured.abs().max(evaluated.abs());
        let tolerance = linear_tolerance
            .max(EPS_DIMENSIONS_RADIAL_DIMENSION_DEFINITION_AT_TOLERANCE_E9 * scale);
        if !evaluated.is_finite()
            || !tolerance.is_finite()
            || tolerance < 0.0
            || (measured - evaluated).abs() > tolerance
        {
            return false;
        }
        true
    }

    fn single_definition(
        self,
        ctx: &DecodeContext<'_>,
        entity: &cadmpeg_ir::sketches::SketchEntity,
        parameter: (&cadmpeg_ir::features::ParameterId, &'static str),
    ) -> Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError> {
        use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;
        let parameter = parameter.0.try_clone_for_decode(ctx, parameter.1)?;
        let entity = entity
            .id()
            .try_clone_for_decode(ctx, "f3d radial at tolerance entity id")?;
        Ok(match self {
            Self::Radius => Definition::Radius { entity, parameter },
            Self::Diameter => Definition::Diameter { entity, parameter },
        })
    }

    fn repeated_definition(
        self,
        entities: Vec<cadmpeg_ir::sketches::SketchEntityId>,
        parameter: cadmpeg_ir::features::ParameterId,
    ) -> cadmpeg_ir::sketches::SketchConstraintDefinitionInput {
        use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;
        match self {
            Self::Radius => Definition::RepeatedRadius {
                entities,
                parameter,
            },
            Self::Diameter => Definition::RepeatedDiameter {
                entities,
                parameter,
            },
        }
    }
}

/// Resolve a parameterized linear annotation that governs an offset pair.
///
/// Fusion stores this form without a generic sketch-relation record. The
/// explicit form returns both curves; the single-source form returns the
/// source and requires a unique generated result. In both forms, the source
/// curve has a null secondary identity, the generated result has a non-null
/// secondary identity, and the annotation's evaluated parameter selects the
/// parallel or concentric offset. Requiring all three facts avoids assigning
/// an arbitrary offset when the sketch contains several generated curves.
fn annotation_offset_dimension_definition(
    ctx: &DecodeContext<'_>,
    frame: &DesignDimensionAnnotationFrame,
    parameter: (&DesignParameter, &cadmpeg_ir::features::ParameterId),
    scope: &str,
    curves: &[SketchCurveIdentity],
    projected: &BTreeMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::scalar::Length;

    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput as Definition, SketchOffsetPair};

    let (parameter, parameter_id) = parameter;

    if !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }

    if !(2..=3).contains(&frame.operands().len()) {
        return None;
    }
    let curve_for_index = |record_index| -> Result<Option<&SketchCurveIdentity>, CodecError> {
        let matches = |curve: &SketchCurveIdentity| -> Result<bool, CodecError> {
            if curve.record_index != record_index
                || curve.owner_reference != Some(frame.owner_reference)
            {
                return Ok(false);
            }
            let Some(stream) =
                native_stream_charged(ctx, &curve.id, "resolve F3D dimension stream")?
            else {
                return Ok(false);
            };
            ctx.equal(stream, scope, "f3d annotation offset curve stream")
        };
        let Some(index) = ctx.position_by(curves, matches, "f3d annotation offset curve source")?
        else {
            return Ok(None);
        };
        if ctx.any_by(
            &curves[index + 1..],
            matches,
            "f3d annotation offset curve source",
        )? {
            return Ok(None);
        }
        Ok(Some(&curves[index]))
    };
    let mut indices = [0; 2];
    let mut count = 0;
    for operand in frame.operands() {
        if let Some(index) = operand.geometry_record_index {
            if count == indices.len() {
                return None;
            }
            indices[count] = index.get();
            count += 1;
        }
    }
    let non_null_indices = &indices[..count];
    let null_locus_count = frame.operands().len() - non_null_indices.len();

    let explicit_pair = match non_null_indices {
        [first_index, second_index]
            if frame.operands().len() == 3
                && null_locus_count == 1
                && first_index != second_index =>
        {
            let first_curve = dimension_resource!(curve_for_index(*first_index))?;
            let second_curve = dimension_resource!(curve_for_index(*second_index))?;
            let (source_curve, result_curve) = match (
                first_curve.secondary_id == 0,
                second_curve.secondary_id == 0,
            ) {
                (true, false) => (first_curve, second_curve),
                (false, true) => (second_curve, first_curve),
                _ => return None,
            };
            Some((source_curve.record_index, Some(result_curve.record_index)))
        }
        _ => None,
    };
    let (source_record_index, explicit_result_record_index) = if let Some(pair) = explicit_pair {
        pair
    } else {
        match non_null_indices {
            [source_record_index] if frame.operands().len() == 2 && null_locus_count == 1 => {
                let source_curve = dimension_resource!(curve_for_index(*source_record_index))?;
                (source_curve.secondary_id == 0).then_some((*source_record_index, None))?
            }
            _ => return None,
        }
    };

    let source = dimension_resource!(ctx.get_btree_map(
        projected,
        &(scope, source_record_index),
        "f3d annotation offset entity lookup"
    ))?;
    let expected = parameter.evaluated_value().get() * 10.0;
    if !expected.is_finite() {
        return None;
    }

    let (result, distance) = if let Some(result_record_index) = explicit_result_record_index {
        let result_curve = dimension_resource!(curve_for_index(result_record_index))?;
        if result_curve.secondary_id == 0 {
            return None;
        }
        let result = dimension_resource!(ctx.get_btree_map(
            projected,
            &(scope, result_record_index),
            "f3d annotation offset entity lookup"
        ))?;
        let distance = sketch_curve_offset(&source.geometry, &result.geometry)?;
        let tolerance = linear_tolerance.max(
            EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
                * (1.0 + distance.abs().max(expected.abs())),
        );
        (distance.abs() > EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
            && (distance.abs() - expected.abs()).abs() <= tolerance)
            .then_some((result, distance))?
    } else {
        let mut source_curves = curves.iter();
        let mut matched = None;
        while let Some(curve) = dimension_resource!(
            ctx.next_charged(&mut source_curves, "f3d annotation offset result source")
        ) {
            if curve.record_index == source_record_index
                || curve.owner_reference != Some(frame.owner_reference)
                || curve.secondary_id == 0
            {
                continue;
            }
            let Some(stream) = dimension_resource!(native_stream_charged(
                ctx,
                &curve.id,
                "resolve F3D dimension stream"
            )) else {
                continue;
            };
            if !dimension_resource!(ctx.equal(stream, scope, "f3d annotation offset curve stream"))
            {
                continue;
            }
            let Some(result) = dimension_resource!(ctx.get_btree_map(
                projected,
                &(scope, curve.record_index),
                "f3d annotation offset entity lookup"
            )) else {
                continue;
            };
            let Some(distance) = sketch_curve_offset(&source.geometry, &result.geometry) else {
                continue;
            };
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
                    * (1.0 + distance.abs().max(expected.abs())),
            );
            if distance.abs() > EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
                && (distance.abs() - expected.abs()).abs() <= tolerance
            {
                if matched.is_some() {
                    return None;
                }
                matched = Some((result, distance));
            }
        }
        matched?
    };
    let parameter_factor = offset_parameter_factor(distance.abs(), expected)?;
    let magnitude = Length::new(distance.abs())?;
    let mut pairs = Vec::new();
    dimension_resource!(ctx.push_vec(
        &mut pairs,
        SketchOffsetPair {
            source: dimension_resource!(
                (source.id()).try_clone_for_decode(ctx, "f3d annotation offset source id")
            ),
            result: dimension_resource!(
                (result.id()).try_clone_for_decode(ctx, "f3d annotation offset result id")
            ),
            source_reversed: distance.is_sign_negative(),
        },
        "f3d annotation offset pair"
    ));
    Some(Ok(Definition::Offset {
        pairs,
        distance: magnitude,
        parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
            id: dimension_resource!(
                (parameter_id).try_clone_for_decode(ctx, "f3d annotation offset parameter id")
            ),
            negated: parameter_factor.is_sign_negative(),
        }),
    }))
}

/// Resolve a radial locus group from its selected circular entity or from a
/// selected center point that uniquely identifies a measured circle or arc.
fn radial_locus_dimension_definition(
    ctx: &DecodeContext<'_>,
    loci: &[&cadmpeg_ir::sketches::SketchEntity],
    all_entities: &[cadmpeg_ir::sketches::SketchEntity],
    source_kind: &str,
    evaluated_value: f64,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let kind = RadialDimensionKind::from_source_kind(source_kind)?;
    let mut storage = dimension_resource!(ctx.reserve_scoped(0, "F3D radial locus candidates"));
    let mut direct = Vec::new();
    for entity in dimension_resource!(ctx
        .admit_iter(loci, "f3d radial locus source")
        .map_err(CodecError::from))
    {
        if kind.matches(entity, evaluated_value, 0.0) {
            dimension_resource!(storage.with_storage(|| ctx.push_vec(
                &mut direct,
                *entity,
                "f3d radial locus definition",
            )));
        }
    }
    if let [entity] = direct.as_slice() {
        return Some(kind.single_definition(
            ctx,
            entity,
            (parameter, "f3d radial locus parameter id"),
        ));
    }
    if !direct.is_empty() {
        let mut unique_storage =
            dimension_resource!(ctx.reserve_scoped(0, "F3D radial locus identity index"));
        let mut unique = HashSet::new();
        let mut candidates = direct.iter();
        while let Some(entity) = dimension_resource!(
            ctx.next_charged(&mut candidates, "f3d radial locus uniqueness source")
        ) {
            if !dimension_resource!(unique_storage.with_storage(|| ctx.insert_hash_set(
                &mut unique,
                entity.id(),
                "f3d radial locus unique member",
            ))) {
                return None;
            }
        }
        let mut ids = Vec::new();
        for entity in dimension_resource!(ctx
            .admit_iter(&direct, "f3d radial locus output source")
            .map_err(CodecError::from))
        {
            let id = dimension_resource!(entity
                .id()
                .try_clone_for_decode(ctx, "f3d radial at tolerance entity id"));
            dimension_resource!(ctx.push_vec(&mut ids, id, "f3d radial locus member"));
        }
        let parameter = dimension_resource!(
            parameter.try_clone_for_decode(ctx, "f3d radial locus repeated parameter id")
        );
        return Some(Ok(kind.repeated_definition(ids, parameter)));
    }

    let sketch = &loci.first()?.sketch;
    if !dimension_resource!(ctx.all_by(
        loci,
        |entity| ctx.equal(&entity.sketch, sketch, "f3d radial center sketch"),
        "f3d radial center sketch source",
    )) || !dimension_resource!(ctx.any_by(
        loci,
        |entity| Ok(matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Point { .. }
        )),
        "f3d radial center point source",
    )) {
        return None;
    }
    let matches = |entity: &cadmpeg_ir::sketches::SketchEntity| -> Result<bool, CodecError> {
        if !ctx.equal(&entity.sketch, sketch, "f3d radial center candidate sketch")? {
            return Ok(false);
        }
        let center = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Arc { center, .. } => *center,
            _ => return Ok(false),
        };
        if !ctx.any_by(
            loci,
            |witness| {
                let SketchGeometryDefinition::Point { position: witness } =
                    witness.geometry.definition()
                else {
                    return Ok(false);
                };
                let scale = 1.0
                    + center
                        .u
                        .abs()
                        .max(center.v.abs())
                        .max(witness.u.abs())
                        .max(witness.v.abs());
                Ok((center.u - witness.u).hypot(center.v - witness.v)
                    <= EPS_DIMENSIONS_RADIAL_LOCUS_DIMENSION_DEFINITION_E9 * scale)
            },
            "f3d radial center witness source",
        )? {
            return Ok(false);
        }
        Ok(kind.matches(entity, evaluated_value, 0.0))
    };
    let first = dimension_resource!(ctx.position_by(
        all_entities,
        matches,
        "f3d radial center candidate source",
    ))?;
    if dimension_resource!(ctx.any_by(
        &all_entities[first + 1..],
        matches,
        "f3d radial center candidate suffix",
    )) {
        return None;
    }
    Some(kind.single_definition(
        ctx,
        &all_entities[first],
        (parameter, "f3d radial center parameter id"),
    ))
}

/// Identify a point-and-line radial annotation whose point lies on the
/// line's infinite carrier. The pair locates a virtual sharp corner and does
/// not itself identify the governed circular entity.
fn radial_extension_annotation_group(
    loci: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: &DesignParameter,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if !design_dimension_unit(parameter)
        || !(parameter.source_kind().starts_with("Radius Dimension")
            || parameter.source_kind().starts_with("Radial Dimension")
            || parameter.source_kind().starts_with("Diameter Dimension"))
    {
        return false;
    }
    let [first, second] = loci else {
        return false;
    };
    let (point, start, end) = match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Point { position },
            SketchGeometryDefinition::Line { start, end },
        )
        | (
            SketchGeometryDefinition::Line { start, end },
            SketchGeometryDefinition::Point { position },
        ) => (*position, *start, *end),
        _ => return false,
    };
    let du = end.u - start.u;
    let dv = end.v - start.v;
    let length = du.hypot(dv);
    if length <= EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E12 {
        return false;
    }
    let relative_u = point.u - start.u;
    let relative_v = point.v - start.v;
    relative_u.mul_add(dv, -relative_v * du).abs()
        <= EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E9
            * (1.0 + length + relative_u.abs().max(relative_v.abs()))
}

/// Remove generic relation parses whose exact stream position is owned by a
/// typed dimension frame.
pub(crate) fn remove_dimension_frame_relations(
    ctx: &DecodeContext<'_>,
    relations: &mut Vec<SketchRelation>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    null_pairs: &[DesignDimensionLocusPair],
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d dimension frame relation storage")?;
    let mut dimension_frames = HashSet::new();
    for pair in ctx.admit_iter(pairs, "f3d dimension relation pair source")? {
        if let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")? {
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut dimension_frames,
                    (scope, pair.byte_offset()),
                    "f3d dimension frame relation index",
                )
            })?;
        }
    }
    for group in ctx.admit_iter(groups, "f3d dimension relation group source")? {
        if let Some(scope) = native_stream_charged(ctx, &group.id, "resolve F3D dimension stream")?
        {
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut dimension_frames,
                    (scope, group.byte_offset),
                    "f3d dimension frame relation index",
                )
            })?;
        }
    }
    for pair in ctx.admit_iter(null_pairs, "f3d dimension relation null pair source")? {
        if let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")? {
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut dimension_frames,
                    (scope, pair.byte_offset()),
                    "f3d dimension frame relation index",
                )
            })?;
        }
    }
    ctx.retain_vec(
        relations,
        |relation| {
            let Some(scope) =
                native_stream_charged(ctx, &relation.id, "resolve F3D dimension stream")?
            else {
                return Ok(true);
            };
            Ok(!ctx.contains_hash_set(
                &dimension_frames,
                &(scope, relation.byte_offset),
                "f3d dimension frame relation lookup",
            )?)
        },
        "f3d dimension relation source",
    )?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct DimensionLocusInputs<'a> {
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) pairs: &'a [DesignDimensionLocusPair],
    pub(crate) groups: &'a [DesignDimensionLocusGroup],
    pub(crate) annotation_frames: &'a [DesignDimensionAnnotationFrame],
    pub(crate) null_pairs: &'a [DesignDimensionLocusPair],
}

/// Bind geometry referenced only by dimensional companions to the sketch
/// reached through the parameter scope or the counted frame's explicit owner.
pub(crate) fn bind_dimension_loci(
    ctx: &DecodeContext<'_>,
    input: DimensionLocusInputs<'_>,
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
) -> Result<(), CodecError> {
    let DimensionLocusInputs {
        placements,
        owners,
        pairs,
        groups,
        annotation_frames,
        null_pairs,
    } = input;
    let mut lookup_storage = ctx.reserve_scoped(0, "f3d dimension owner lookup storage")?;
    let mut placements_by_scope = HashMap::new();
    for placement in ctx.admit_iter(placements, "f3d dimension binding placement source")? {
        if let (Some(scope), Some(record_index), Ok(owner)) = (
            native_stream_charged(ctx, &placement.id, "resolve F3D dimension stream")?,
            placement.scope_record_index,
            u32::try_from(placement.entity_id.suffix()),
        ) {
            lookup_storage
                .with_storage(|| {
                    ctx.insert_hash_map(
                        &mut placements_by_scope,
                        (scope, record_index),
                        owner,
                        "f3d dimension placement scope",
                    )
                })
                .map(|_| ())?;
        }
    }
    let mut scopes_by_companion = HashMap::new();
    for owner in ctx.admit_iter(owners, "f3d dimension binding owner source")? {
        if let Some(scope) = native_stream_charged(ctx, owner.id(), "resolve F3D dimension stream")?
        {
            lookup_storage
                .with_storage(|| {
                    ctx.insert_hash_map(
                        &mut scopes_by_companion,
                        (scope, owner.companion_record_index()),
                        owner.scope_record_index(),
                        "f3d dimension companion scope",
                    )
                })
                .map(|_| ())?;
        }
    }
    let mut binding_storage = ctx.reserve_scoped(0, "f3d dimension binding storage")?;
    let mut bindings = HashMap::new();
    let mut pair_source = pairs.iter();
    while let Some(pair) =
        ctx.next_charged(&mut pair_source, "f3d dimension binding pair source")?
    {
        let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some(parameter_scope) = ctx
            .get_hash_map(
                &scopes_by_companion,
                &(scope, pair.governing_companion_record_index),
                "f3d dimension binding companion lookup",
            )?
            .copied()
        else {
            continue;
        };
        let Some(owner) = ctx
            .get_hash_map(
                &placements_by_scope,
                &(scope, parameter_scope),
                "f3d dimension binding placement lookup",
            )?
            .copied()
        else {
            continue;
        };
        insert_dimension_binding(
            ctx,
            &mut binding_storage,
            &mut bindings,
            scope,
            pair.loci()[0].geometry_index(),
            owner,
        )?;
        insert_dimension_binding(
            ctx,
            &mut binding_storage,
            &mut bindings,
            scope,
            pair.loci()[1].geometry_index(),
            owner,
        )?;
    }
    let mut group_source = groups.iter();
    while let Some(group) =
        ctx.next_charged(&mut group_source, "f3d dimension binding group source")?
    {
        let Some(scope) = native_stream_charged(ctx, &group.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let mut locus_source = group.loci.iter();
        while let Some(locus) =
            ctx.next_charged(&mut locus_source, "f3d dimension binding locus source")?
        {
            insert_dimension_binding(
                ctx,
                &mut binding_storage,
                &mut bindings,
                scope,
                locus.geometry_record_index,
                group.owner_reference,
            )?;
        }
    }
    let mut frame_source = annotation_frames.iter();
    while let Some(frame) =
        ctx.next_charged(&mut frame_source, "f3d dimension binding annotation source")?
    {
        let Some(scope) = native_stream_charged(ctx, &frame.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let mut operand_source = frame.operands().iter();
        while let Some(operand) =
            ctx.next_charged(&mut operand_source, "f3d dimension binding operand source")?
        {
            let Some(record_index) = operand.geometry_record_index else {
                continue;
            };
            insert_dimension_binding(
                ctx,
                &mut binding_storage,
                &mut bindings,
                scope,
                record_index.get(),
                frame.owner_reference,
            )?;
        }
    }
    let mut null_pair_source = null_pairs.iter();
    while let Some(pair) = ctx.next_charged(
        &mut null_pair_source,
        "f3d dimension binding null pair source",
    )? {
        let Some(scope) = native_stream_charged(ctx, &pair.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some(parameter_scope) = ctx
            .get_hash_map(
                &scopes_by_companion,
                &(scope, pair.governing_companion_record_index),
                "f3d dimension binding companion lookup",
            )?
            .copied()
        else {
            continue;
        };
        let Some(owner) = ctx
            .get_hash_map(
                &placements_by_scope,
                &(scope, parameter_scope),
                "f3d dimension binding placement lookup",
            )?
            .copied()
        else {
            continue;
        };
        insert_dimension_binding(
            ctx,
            &mut binding_storage,
            &mut bindings,
            scope,
            pair.loci()[1].geometry_index(),
            owner,
        )?;
    }
    let mut point_source = points.iter_mut();
    while let Some(point) =
        ctx.next_charged(&mut point_source, "f3d dimension binding point source")?
    {
        let Some(scope) = native_stream_charged(ctx, &point.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some(owner) = ctx
            .get_hash_map(
                &bindings,
                &(scope, point.record_index),
                "f3d dimension binding record lookup",
            )?
            .copied()
        else {
            continue;
        };
        if point
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion sketch point {} has conflicting relation and dimension owners",
                    point.record_index
                ),
            ));
        }
    }
    let mut curve_source = curves.iter_mut();
    while let Some(curve) =
        ctx.next_charged(&mut curve_source, "f3d dimension binding curve source")?
    {
        let Some(scope) = native_stream_charged(ctx, &curve.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some(owner) = ctx
            .get_hash_map(
                &bindings,
                &(scope, curve.record_index),
                "f3d dimension binding record lookup",
            )?
            .copied()
        else {
            continue;
        };
        if curve
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion sketch curve {} has conflicting relation and dimension owners",
                    curve.record_index
                ),
            ));
        }
    }
    Ok(())
}

fn insert_dimension_binding<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    bindings: &mut HashMap<(&'a str, u32), u32>,
    scope: &'a str,
    record_index: u32,
    owner: u32,
) -> Result<(), CodecError> {
    let previous = storage.with_storage(|| {
        ctx.insert_hash_map(
            bindings,
            (scope, record_index),
            owner,
            "f3d dimension binding record",
        )
    })?;
    if previous.is_some_and(|existing| existing != owner) {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!(
                "Fusion dimensional geometry record {record_index} belongs to multiple sketches"
            ),
        ));
    }
    Ok(())
}

pub(super) fn exact_atomic_constraint(
    kind: SketchConstraintKind,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchCoordinateAxis,
        SketchGeometryDefinition as Geometry, SketchLocus,
    };

    let lines = || -> Result<Option<_>, CodecError> {
        if entities.len() == 2
            && !ctx.equal(
                entities[0].id(),
                entities[1].id(),
                "f3d atomic entity equality",
            )?
            && entities
                .iter()
                .all(|entity| matches!(*entity.geometry.definition(), Geometry::Line { .. }))
        {
            Ok(Some(copy_dimension_entity_pair(
                ctx,
                entities[0].id(),
                entities[1].id(),
            )?))
        } else {
            Ok(None)
        }
    };
    let curves = || -> Result<Option<_>, CodecError> {
        if entities.len() == 2
            && !ctx.equal(
                entities[0].id(),
                entities[1].id(),
                "f3d atomic entity equality",
            )?
            && entities.iter().all(|entity| {
                matches!(
                    *entity.geometry.definition(),
                    Geometry::Line { .. }
                        | Geometry::Circle { .. }
                        | Geometry::Arc { .. }
                        | Geometry::Ellipse { .. }
                        | Geometry::Nurbs { .. }
                )
            })
        {
            Ok(Some(copy_dimension_entity_pair(
                ctx,
                entities[0].id(),
                entities[1].id(),
            )?))
        } else {
            Ok(None)
        }
    };
    let equal_size_entities = || -> Result<Option<_>, CodecError> {
        let [first, second] = entities else {
            return Ok(None);
        };
        if !ctx.equal(first.id(), second.id(), "f3d atomic entity equality")?
            && matches!(
                (first.geometry.definition(), second.geometry.definition()),
                (Geometry::Line { .. }, Geometry::Line { .. })
                    | (
                        Geometry::Circle { .. } | Geometry::Arc { .. },
                        Geometry::Circle { .. } | Geometry::Arc { .. }
                    )
                    | (Geometry::Ellipse { .. }, Geometry::Ellipse { .. })
            )
        {
            Ok(Some(copy_dimension_entity_pair(
                ctx,
                first.id(),
                second.id(),
            )?))
        } else {
            Ok(None)
        }
    };
    Ok(match kind {
        SketchConstraintKind::Coincident
            if entities.len() >= 2 && dimension_entity_ids_distinct(ctx, entities)? =>
        {
            Some(Definition::Coincident {
                entities: copy_dimension_entity_members(ctx, entities)?,
            })
        }
        SketchConstraintKind::Colinear => {
            lines()?.map(|(first, second)| Definition::Collinear { first, second })
        }
        SketchConstraintKind::Concentric => {
            if entities.len() == 2
                && !ctx.equal(
                    entities[0].id(),
                    entities[1].id(),
                    "f3d atomic entity equality",
                )?
                && entities.iter().all(|entity| {
                    matches!(
                        *entity.geometry.definition(),
                        Geometry::Circle { .. } | Geometry::Arc { .. } | Geometry::Ellipse { .. }
                    )
                })
            {
                let (first, second) =
                    copy_dimension_entity_pair(ctx, entities[0].id(), entities[1].id())?;
                return Ok(Some(Definition::Concentric { first, second }));
            }
            let Some((first, second, axis)) = reflected_symmetry(ctx, entities)? else {
                return Ok(None);
            };
            Some(Definition::Symmetric {
                first: SketchLocus::Entity(
                    (first.id()).try_clone_for_decode(ctx, "f3d atomic symmetry first id")?,
                ),
                second: SketchLocus::Entity(
                    (second.id()).try_clone_for_decode(ctx, "f3d atomic symmetry second id")?,
                ),
                axis: (axis.id()).try_clone_for_decode(ctx, "f3d atomic symmetry axis id")?,
            })
        }
        SketchConstraintKind::Symmetry => {
            let Some((first, second, axis)) = reflected_symmetry(ctx, entities)? else {
                return Ok(None);
            };
            Some(Definition::Symmetric {
                first: SketchLocus::Entity(
                    (first.id()).try_clone_for_decode(ctx, "f3d atomic symmetry first id")?,
                ),
                second: SketchLocus::Entity(
                    (second.id()).try_clone_for_decode(ctx, "f3d atomic symmetry second id")?,
                ),
                axis: (axis.id()).try_clone_for_decode(ctx, "f3d atomic symmetry axis id")?,
            })
        }
        SketchConstraintKind::EqualLength => {
            lines()?.map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Parallel => match lines()? {
            Some((first, second)) => Some(Definition::Parallel { first, second }),
            None => midpoint_constraint(entities, ctx)?,
        },
        SketchConstraintKind::Perpendicular => {
            lines()?.map(|(first, second)| Definition::Perpendicular { first, second })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 1
                && matches!(*entities[0].geometry.definition(), Geometry::Line { .. }) =>
        {
            Some(Definition::Horizontal {
                entity: (entities[0].id())
                    .try_clone_for_decode(ctx, "f3d atomic single entity id")?,
            })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 2
                && !ctx.equal(
                    entities[0].id(),
                    entities[1].id(),
                    "f3d atomic entity equality",
                )?
                && entities.iter().all(|entity| {
                    matches!(*entity.geometry.definition(), Geometry::Point { .. })
                }) =>
        {
            let (first, second) =
                copy_dimension_entity_pair(ctx, entities[0].id(), entities[1].id())?;
            let Ok(relation) = cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Entity(first),
                SketchLocus::Entity(second),
                SketchCoordinateAxis::V,
            ) else {
                return Ok(None);
            };
            Some(Definition::SameCoordinate { relation })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 1
                && matches!(*entities[0].geometry.definition(), Geometry::Line { .. }) =>
        {
            Some(Definition::Vertical {
                entity: (entities[0].id())
                    .try_clone_for_decode(ctx, "f3d atomic single entity id")?,
            })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 2
                && !ctx.equal(
                    entities[0].id(),
                    entities[1].id(),
                    "f3d atomic entity equality",
                )?
                && entities.iter().all(|entity| {
                    matches!(*entity.geometry.definition(), Geometry::Point { .. })
                }) =>
        {
            let (first, second) =
                copy_dimension_entity_pair(ctx, entities[0].id(), entities[1].id())?;
            let Ok(relation) = cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Entity(first),
                SketchLocus::Entity(second),
                SketchCoordinateAxis::U,
            ) else {
                return Ok(None);
            };
            Some(Definition::SameCoordinate { relation })
        }
        SketchConstraintKind::Tangent => {
            curves()?.map(|(first, second)| Definition::Tangent { first, second })
        }
        SketchConstraintKind::Curvature => {
            curves()?.map(|(first, second)| Definition::Curvature { first, second })
        }
        SketchConstraintKind::Midpoint => midpoint_constraint(entities, ctx)?,
        SketchConstraintKind::Equal => {
            equal_size_entities()?.map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Polygon
            if entities.len() >= 3 && dimension_entity_ids_distinct(ctx, entities)? =>
        {
            let members = copy_dimension_entity_members(ctx, entities)?;
            let polygon = cadmpeg_ir::sketches::SketchPolygon::try_new(
                members,
                ctx,
                "f3d atomic polygon uniqueness",
            )?;
            Some(Definition::Polygon {
                polygon: match polygon {
                    Ok(polygon) => polygon,
                    Err(_) => return Ok(None),
                },
            })
        }
        SketchConstraintKind::SplineGroup
            if entities.len() >= 2 && dimension_entity_ids_distinct(ctx, entities)? =>
        {
            Some(Definition::SplineGroup {
                entities: copy_dimension_entity_members(ctx, entities)?,
            })
        }
        _ => None,
    })
}

pub(super) fn exact_coincident_loci(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
        SketchLocus,
    };
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum LocusKind {
        Entity,
        Start,
        End,
        Center,
    }
    if entities.len() < 2 {
        return Ok(None);
    }
    let mut proof_storage = ctx.reserve_scoped(0, "f3d coincident carrier storage")?;
    let loci_by_entity = proof_storage.with_storage(|| -> Result<_, CodecError> {
        let mut unique = HashSet::new();
        let mut loci_by_entity = Vec::new();
        let mut entity_source = entities.iter();
        while let Some(entity) =
            ctx.next_charged(&mut entity_source, "f3d coincident entity source")?
        {
            if ctx.contains_hash_set(&unique, entity.id(), "f3d coincident entity uniqueness")? {
                return Ok(None);
            }
            ctx.insert_hash_set(&mut unique, entity.id(), "f3d coincident entity uniqueness")?;
            let mut loci = Vec::new();
            if let Some([start, end]) = sketch_entity_endpoints(entity, ctx)? {
                ctx.push_vec(
                    &mut loci,
                    (LocusKind::Start, entity.id(), start),
                    "f3d coincident local locus",
                )?;
                ctx.push_vec(
                    &mut loci,
                    (LocusKind::End, entity.id(), end),
                    "f3d coincident local locus",
                )?;
            }
            match entity.geometry.definition() {
                Geometry::Point { position } => {
                    ctx.push_vec(
                        &mut loci,
                        (LocusKind::Entity, entity.id(), position.get()),
                        "f3d coincident local locus",
                    )?;
                }
                Geometry::Circle { center, .. }
                | Geometry::Arc { center, .. }
                | Geometry::Ellipse { center, .. }
                | Geometry::Hyperbola { center, .. } => {
                    ctx.push_vec(
                        &mut loci,
                        (LocusKind::Center, entity.id(), center.get()),
                        "f3d coincident local locus",
                    )?;
                }
                _ => {}
            }
            ctx.push_vec(&mut loci_by_entity, loci, "f3d coincident member loci")?;
        }
        Ok(Some(loci_by_entity))
    })?;
    let Some(loci_by_entity) = loci_by_entity else {
        return Ok(None);
    };
    let mut unique_solution: Option<(
        Vec<(LocusKind, &cadmpeg_ir::sketches::SketchEntityId)>,
        cadmpeg_core::decode::ScopedReservation<'_>,
    )> = None;
    // Each entity contributes at most start, end and center/point loci.
    for &(first_kind, first_id, position) in &loci_by_entity[0] {
        let mut solution_storage = ctx.reserve_scoped(0, "f3d coincident solution storage")?;
        let mut solution = Vec::new();
        ctx.push_scoped_vec(
            &mut solution_storage,
            &mut solution,
            (first_kind, first_id),
            "f3d coincident solution locus",
        )?;
        let mut member_source = loci_by_entity[1..].iter();
        while let Some(member_loci) =
            ctx.next_charged(&mut member_source, "f3d coincident member source")?
        {
            let matched = crate::design::feature_project::unique_feature_match_for_decode(
                ctx,
                member_loci.iter(),
                |(_, _, candidate)| {
                    Ok((candidate.u - position.u).hypot(candidate.v - position.v)
                        <= EPS_DIMENSIONS_EXACT_COINCIDENT_LOCI_E9)
                },
                "f3d coincident locus matching",
            )?;
            let Some(&(kind, id, _)) = matched else {
                solution.clear();
                break;
            };
            ctx.push_scoped_vec(
                &mut solution_storage,
                &mut solution,
                (kind, id),
                "f3d coincident solution locus",
            )?;
        }
        if solution.len() == entities.len() {
            if let Some((existing, _storage)) = &unique_solution {
                if !ctx.all_by(
                    existing.iter().zip(&solution),
                    |((left_kind, left), (right_kind, right))| {
                        Ok(left_kind == right_kind
                            && ctx.equal(*left, *right, "f3d coincident solution equality")?)
                    },
                    "f3d coincident solution source",
                )? {
                    return Ok(None);
                }
            } else {
                unique_solution = Some((solution, solution_storage));
            }
        }
    }
    let Some((solution, _solution_storage)) = unique_solution else {
        return Ok(None);
    };
    let loci = ctx.try_collect_vec(
        solution.into_iter().map(|(kind, id)| {
            let id = id.try_clone_for_decode(ctx, "f3d coincident solution entity id")?;
            Ok::<_, CodecError>(match kind {
                LocusKind::Entity => SketchLocus::Entity(id),
                LocusKind::Start => SketchLocus::Start(id),
                LocusKind::End => SketchLocus::End(id),
                LocusKind::Center => SketchLocus::Center(id),
            })
        }),
        "f3d coincident output locus",
    )?;
    Ok(Some(Definition::CoincidentLoci { loci }))
}

fn midpoint_constraint(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
        SketchLocus,
    };

    let [first, second] = entities else {
        return Ok(None);
    };
    let (line, point, start, end, position) =
        match (first.geometry.definition(), second.geometry.definition()) {
            (Geometry::Line { start, end }, Geometry::Point { position }) => {
                (*first, *second, start, end, position)
            }
            (Geometry::Point { position }, Geometry::Line { start, end }) => {
                (*second, *first, start, end, position)
            }
            _ => return Ok(None),
        };
    let midpoint = Point2::new(start.u.midpoint(end.u), start.v.midpoint(end.v));
    if (position.u - midpoint.u).abs() > EPS_DIMENSIONS_MIDPOINT_CONSTRAINT_E9
        || (position.v - midpoint.v).abs() > EPS_DIMENSIONS_MIDPOINT_CONSTRAINT_E9
    {
        return Ok(None);
    }
    Ok(Some(Definition::Midpoint {
        point: SketchLocus::Entity(
            (point.id()).try_clone_for_decode(ctx, "f3d midpoint point id")?,
        ),
        entity: (line.id()).try_clone_for_decode(ctx, "f3d midpoint line id")?,
    }))
}

fn indirect_angular_lines(
    ctx: &DecodeContext<'_>,
    scope: &str,
    operands: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_value: f64,
    projected: &BTreeMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Result<
    Option<(
        cadmpeg_ir::sketches::SketchEntityId,
        cadmpeg_ir::sketches::SketchEntityId,
    )>,
    CodecError,
> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let [first, second] = operands else {
        return Ok(None);
    };
    let (point_ordinal, position, explicit_line) =
        match (first.geometry.definition(), second.geometry.definition()) {
            (
                SketchGeometryDefinition::Point { position },
                SketchGeometryDefinition::Line { .. },
            ) => (0, position, *second),
            (
                SketchGeometryDefinition::Line { .. },
                SketchGeometryDefinition::Point { position },
            ) => (1, position, *first),
            _ => return Ok(None),
        };
    if !evaluated_value.is_finite() || !(0.0..=std::f64::consts::PI).contains(&evaluated_value) {
        return Ok(None);
    }
    let mut source = projected.iter();
    let mut candidate: Option<&cadmpeg_ir::sketches::SketchEntity> = None;
    while let Some(((candidate_scope, _), next)) =
        ctx.next_charged(&mut source, "f3d indirect angular candidate source")?
    {
        if !ctx.equal(
            *candidate_scope,
            scope,
            "f3d indirect angular candidate stream",
        )? || !ctx.equal(
            &next.sketch,
            &explicit_line.sketch,
            "f3d indirect angular sketch equality",
        )? || ctx.equal(
            next.id(),
            explicit_line.id(),
            "f3d indirect angular explicit equality",
        )? {
            continue;
        }
        let SketchGeometryDefinition::Line { start, end } = next.geometry.definition() else {
            continue;
        };
        if !(sketch_points_close(position.get(), start.get())
            || sketch_points_close(position.get(), end.get()))
            || !line_angle_matches(&explicit_line.geometry, &next.geometry, evaluated_value)
        {
            continue;
        }
        if let Some(first) = candidate {
            if !ctx.equal(
                first.id(),
                next.id(),
                "f3d indirect angular candidate equality",
            )? {
                return Ok(None);
            }
        } else {
            candidate = Some(*next);
        }
    }
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let (first, second) = if point_ordinal == 0 {
        (candidate.id(), explicit_line.id())
    } else {
        (explicit_line.id(), candidate.id())
    };
    Ok(Some((
        first.try_clone_for_decode(ctx, "f3d indirect angular first id")?,
        second.try_clone_for_decode(ctx, "f3d indirect angular second id")?,
    )))
}

fn directional_point_dimension(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_mm: f64,
    parameter: (&cadmpeg_ir::features::ParameterId, &'static str),
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    let [first, second] = entities else {
        return None;
    };
    let SketchGeometryDefinition::Point {
        position: first_position,
    } = first.geometry.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Point {
        position: second_position,
    } = second.geometry.definition()
    else {
        return None;
    };
    let horizontal = linear_measurement_matches(
        first_position.u - second_position.u,
        evaluated_mm,
        linear_tolerance,
    );
    let vertical = linear_measurement_matches(
        first_position.v - second_position.v,
        evaluated_mm,
        linear_tolerance,
    );
    let horizontal = match (horizontal, vertical) {
        (true, false) => true,
        (false, true) => false,
        (false, false) | (true, true) => return None,
    };
    let first_locus = SketchLocus::Entity(dimension_resource!(
        (first.id()).try_clone_for_decode(ctx, "f3d directional point dimension first id")
    ));
    let second_locus = SketchLocus::Entity(dimension_resource!(
        (second.id()).try_clone_for_decode(ctx, "f3d directional point dimension second id")
    ));
    let parameter = dimension_resource!(parameter.0.try_clone_for_decode(ctx, parameter.1));
    Some(Ok(if horizontal {
        Definition::HorizontalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        }
    } else {
        Definition::VerticalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        }
    }))
}

fn recipe_linear_dimension_candidates(
    ctx: &DecodeContext<'_>,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    evaluated_mm: f64,
    parameter: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let mut carrier_storage = ctx.reserve_scoped(0, "f3d recipe carrier storage")?;
    let (points, lines) = carrier_storage.with_storage(|| {
        let mut points = Vec::new();
        let mut lines = Vec::new();
        for entity in ctx.admit_iter(entities, "f3d recipe sketch source")? {
            if !ctx.equal(&entity.sketch, sketch, "f3d recipe sketch equality")? {
                continue;
            }
            match entity.geometry.definition() {
                SketchGeometryDefinition::Point { position } => {
                    ctx.push_vec(
                        &mut points,
                        (entity, position.get()),
                        "f3d recipe point candidate",
                    )?;
                }
                SketchGeometryDefinition::Line { .. } => {
                    ctx.push_vec(&mut lines, entity, "f3d recipe line candidate")?;
                }
                _ => {}
            }
        }
        Ok::<_, CodecError>((points, lines))
    })?;
    let mut pair_storage = ctx.reserve_scoped(0, "f3d recipe line pair storage")?;
    let mut line_pairs = Vec::new();
    for first in ctx.admit_iter(0..lines.len(), "f3d recipe first line source")? {
        for second in ctx.admit_iter(first + 1..lines.len(), "f3d recipe second line source")? {
            if parallel_line_separation(lines[first], lines[second], evaluated_mm, linear_tolerance)
            {
                ctx.push_scoped_vec(
                    &mut pair_storage,
                    &mut line_pairs,
                    (lines[first], lines[second]),
                    "f3d recipe line pair",
                )?;
            }
        }
    }
    let same_point = |left: Point2, right: Point2| {
        let scale = 1.0
            + left
                .u
                .abs()
                .max(left.v.abs())
                .max(right.u.abs())
                .max(right.v.abs());
        (left.u - right.u).abs() <= EPS_DIMENSIONS_RECIPE_LINEAR_DIMENSION_CANDIDATES_E9 * scale
            && (left.v - right.v).abs()
                <= EPS_DIMENSIONS_RECIPE_LINEAR_DIMENSION_CANDIDATES_E9 * scale
    };
    let point_on_endpoint = |position: Point2, line: &cadmpeg_ir::sketches::SketchEntity| match line
        .geometry
        .definition()
    {
        SketchGeometryDefinition::Line { start, end } => [start.get(), end.get()]
            .into_iter()
            .any(|end| same_point(position, end)),
        _ => false,
    };
    let mut candidates = Vec::new();
    for first in ctx.admit_iter(0..points.len(), "f3d recipe first point source")? {
        for second in ctx.admit_iter(first + 1..points.len(), "f3d recipe second point source")? {
            let subsumed_by_line_pair = ctx.any_by(
                &line_pairs,
                |(first_line, second_line)| {
                    Ok((point_on_endpoint(points[first].1, first_line)
                        && point_on_endpoint(points[second].1, second_line))
                        || (point_on_endpoint(points[first].1, second_line)
                            && point_on_endpoint(points[second].1, first_line)))
                },
                "f3d recipe subsuming line pair",
            )?;
            if subsumed_by_line_pair {
                continue;
            }
            if let Some(definition) = directional_point_dimension(
                ctx,
                &[points[first].0, points[second].0],
                evaluated_mm,
                (parameter, "f3d recipe directional parameter id"),
                0.0,
            )
            .transpose()?
            {
                ctx.push_vec(&mut candidates, definition, "f3d recipe point definition")?;
            }
        }
    }
    for (first, second) in ctx.admit_iter(line_pairs, "f3d recipe line output source")? {
        let definition = Definition::Distance {
            entities: copy_dimension_entity_members(ctx, &[first, second])?,
            parameter: (parameter).try_clone_for_decode(ctx, "f3d recipe line parameter id")?,
        };
        ctx.push_vec(&mut candidates, definition, "f3d recipe line definition")?;
    }
    Ok(candidates)
}

fn recipe_dimension_candidate_entities(
    ctx: &DecodeContext<'_>,
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinitionInput],
) -> Result<Vec<cadmpeg_ir::sketches::SketchEntityId>, CodecError> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let mut entities = Vec::new();
    let mut add = |entity: &cadmpeg_ir::sketches::SketchEntityId| -> Result<(), CodecError> {
        if !ctx.contains(&entities, entity, "f3d recipe native entity membership")? {
            let copied = (entity).try_clone_for_decode(ctx, "f3d recipe native entity id")?;
            ctx.push_vec(&mut entities, copied, "f3d recipe native entity")?;
        }
        Ok(())
    };
    for candidate in ctx.admit_iter(candidates, "f3d recipe native candidate source")? {
        match candidate {
            Definition::Distance {
                entities: candidate_entities,
                ..
            } => {
                for entity in ctx.admit_iter(
                    candidate_entities.as_slice(),
                    "f3d recipe native member source",
                )? {
                    add(entity)?;
                }
            }
            Definition::HorizontalDistance { first, second, .. }
            | Definition::VerticalDistance { first, second, .. } => {
                add(locus_entity_id(first))?;
                add(locus_entity_id(second))?;
            }
            _ => {}
        }
    }
    Ok(entities)
}

/// Resolve an ambiguous recipe-backed directional distance through one
/// detached point on the extension of an axis-aligned bounded line. The other
/// measured point must be an endpoint of that same line.
fn recipe_extension_point_dimension(
    ctx: &DecodeContext<'_>,
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinitionInput],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let point = |id: &cadmpeg_ir::sketches::SketchEntityId| {
        ctx.find_map(
            entities,
            |entity| {
                if !ctx.equal(
                    &entity.sketch,
                    sketch,
                    "f3d recipe extension sketch equality",
                )? {
                    return Ok(None);
                }
                match entity.geometry.definition() {
                    SketchGeometryDefinition::Point { position }
                        if ctx.equal(
                            entity.id(),
                            id,
                            "f3d recipe extension entity equality",
                        )? =>
                    {
                        Ok(Some(position.get()))
                    }
                    _ => Ok(None),
                }
            },
            "f3d recipe extension point source",
        )
    };
    let is_any_line_endpoint = |position: Point2| {
        ctx.any_by(
            entities,
            |entity| {
                if !ctx.equal(
                    &entity.sketch,
                    sketch,
                    "f3d recipe extension sketch equality",
                )? {
                    return Ok(false);
                }
                Ok(line_segment(entity).is_some_and(|[start, end]| {
                    sketch_points_close(position, start) || sketch_points_close(position, end)
                }))
            },
            "f3d recipe extension endpoint source",
        )
    };
    let mut matched = None;
    let mut candidate_source = candidates.iter();
    while let Some(candidate) = dimension_resource!(ctx.next_charged(
        &mut candidate_source,
        "f3d recipe extension candidate source"
    )) {
        let (first_id, second_id, horizontal) = match candidate {
            Definition::HorizontalDistance { first, second, .. } => {
                (locus_entity_id(first), locus_entity_id(second), true)
            }
            Definition::VerticalDistance { first, second, .. } => {
                (locus_entity_id(first), locus_entity_id(second), false)
            }
            _ => continue,
        };
        let first_position = dimension_resource!(point(first_id))?;
        let second_position = dimension_resource!(point(second_id))?;
        let mut qualifies = false;
        for (detached, endpoint) in [
            (first_position, second_position),
            (second_position, first_position),
        ] {
            if dimension_resource!(is_any_line_endpoint(detached)) {
                continue;
            }
            if dimension_resource!(ctx.any_by(
                entities,
                |entity| {
                    if !ctx.equal(
                        &entity.sketch,
                        sketch,
                        "f3d recipe extension sketch equality",
                    )? {
                        return Ok(false);
                    }
                    let Some([start, end]) = line_segment(entity) else {
                        return Ok(false);
                    };
                    let du = end.u - start.u;
                    let dv = end.v - start.v;
                    let length = du.hypot(dv);
                    let axis_aligned = if horizontal {
                        dv.abs()
                            <= EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9 * (1.0 + du.abs())
                    } else {
                        du.abs()
                            <= EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9 * (1.0 + dv.abs())
                    };
                    if !length.is_finite()
                        || length <= EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9
                        || !axis_aligned
                    {
                        return Ok(false);
                    }
                    if !sketch_points_close(endpoint, start) && !sketch_points_close(endpoint, end)
                    {
                        return Ok(false);
                    }
                    let relative_u = detached.u - start.u;
                    let relative_v = detached.v - start.v;
                    let carrier_error = relative_u
                        .mul_add(dv / length, -relative_v * (du / length))
                        .abs();
                    let carrier_tolerance = EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9
                        * (1.0 + length + relative_u.abs().max(relative_v.abs()));
                    let projection =
                        (relative_u * (du / length) + relative_v * (dv / length)) / length;
                    Ok(carrier_error <= carrier_tolerance
                        && !(-EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9
                            ..=1.0 + EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9)
                            .contains(&projection))
                },
                "f3d recipe extension line source"
            )) {
                qualifies = true;
                break;
            }
        }

        if qualifies {
            if matched.is_some() {
                return None;
            }
            matched = Some(candidate);
        }
    }
    let (first, second, parameter, horizontal) = match matched? {
        Definition::HorizontalDistance {
            first,
            second,
            parameter,
        } => (first, second, parameter, true),
        Definition::VerticalDistance {
            first,
            second,
            parameter,
        } => (first, second, parameter, false),
        _ => return None,
    };
    Some((|| -> Result<_, CodecError> {
        let first = copy_dimension_locus(ctx, first, "f3d recipe extension first locus")?;
        let second = copy_dimension_locus(ctx, second, "f3d recipe extension second locus")?;
        let parameter =
            (parameter).try_clone_for_decode(ctx, "f3d recipe extension parameter id")?;
        Ok(if horizontal {
            Definition::HorizontalDistance {
                first,
                second,
                parameter,
            }
        } else {
            Definition::VerticalDistance {
                first,
                second,
                parameter,
            }
        })
    })())
}

fn parallel_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
    linear_tolerance: f64,
) -> bool {
    let Some(separation) = parallel_line_distance(first, second) else {
        return false;
    };
    linear_measurement_matches(separation, evaluated_mm, linear_tolerance)
}

/// Resolve the symmetric line-width form of a paired linear dimension.
///
/// A nonzero role on both parallel line loci selects a width dimension: the
/// stored value is twice the perpendicular carrier separation. Zero roles use
/// the ordinary direct separation rules in `exact_definition`.
fn symmetric_parallel_line_dimension_definition(
    ctx: &DecodeContext<'_>,
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    roles: (u32, u32),
    parameter: &DesignParameter,
    parameter_id: cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let (first_role, second_role) = roles;

    if first_role == 0
        || second_role == 0
        || !parameter.source_kind().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let separation = parallel_line_distance(first, second)?;
    let expected = parameter.evaluated_value().get() * 10.0;
    linear_measurement_matches(2.0 * separation, expected, linear_tolerance).then(
        || -> Result<_, CodecError> {
            Ok(Definition::Distance {
                entities: vec![
                    (first.id())
                        .try_clone_for_decode(ctx, "f3d symmetric parallel line first id")?,
                    (second.id())
                        .try_clone_for_decode(ctx, "f3d symmetric parallel line second id")?,
                ],
                parameter: parameter_id,
            })
        },
    )
}

fn line_segment(geometry: &cadmpeg_ir::sketches::SketchEntity) -> Option<[Point2; 2]> {
    match geometry.geometry.definition() {
        cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => {
            Some([start.get(), end.get()])
        }
        _ => None,
    }
}

fn parallel_line_distance(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
) -> Option<f64> {
    parallel_segment_distance(line_segment(first)?, line_segment(second)?)
}

fn parallel_segment_distance(
    [first_start, first_end]: [Point2; 2],
    [second_start, second_end]: [Point2; 2],
) -> Option<f64> {
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if first_length <= EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E12
        || second_length <= EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E12
    {
        return None;
    }
    let cross = first_direction.u * second_direction.v - first_direction.v * second_direction.u;
    if cross.abs() > EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E9 * first_length * second_length {
        return None;
    }
    let offset = Point2::new(
        second_start.u - first_start.u,
        second_start.v - first_start.v,
    );
    Some((offset.u * first_direction.v - offset.v * first_direction.u).abs() / first_length)
}

fn parallel_line_span_distance(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    linear_tolerance: f64,
) -> Option<f64> {
    let first_segment = line_segment(first)?;
    let second_segment = line_segment(second)?;
    let distance = parallel_segment_distance(first_segment, second_segment)?;
    let [first_start, first_end] = first_segment;
    let [second_start, second_end] = second_segment;
    let direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let length = direction.u.hypot(direction.v);
    let project = |point: Point2| (point.u * direction.u + point.v * direction.v) / length;
    let first_interval = [project(first_start), project(first_end)];
    let second_interval = [project(second_start), project(second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    (first_min.max(second_min) <= first_max.min(second_max) + linear_tolerance).then_some(distance)
}

fn concentric_circle_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let (
        SketchGeometryDefinition::Circle {
            center: first_center,
            radius: first_radius,
        },
        SketchGeometryDefinition::Circle {
            center: second_center,
            radius: second_radius,
        },
    ) = (first.geometry.definition(), second.geometry.definition())
    else {
        return false;
    };
    if !evaluated_mm.is_finite() {
        return false;
    }
    let center_separation =
        (first_center.u - second_center.u).hypot(first_center.v - second_center.v);
    if !linear_measurement_matches(center_separation, 0.0, linear_tolerance) {
        return false;
    }
    let measured = (first_radius.get() - second_radius.get()).abs();
    measured > 0.0 && linear_measurement_matches(measured, evaluated_mm, linear_tolerance)
}

fn point_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let (point, line) = match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Point { position },
            SketchGeometryDefinition::Line { start, end },
        )
        | (
            SketchGeometryDefinition::Line { start, end },
            SketchGeometryDefinition::Point { position },
        ) => (*position, (*start, *end)),
        _ => return false,
    };
    let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
    let length = direction.u.hypot(direction.v);
    if length <= EPS_DIMENSIONS_POINT_LINE_SEPARATION_E12 || !evaluated_mm.is_finite() {
        return false;
    }
    let offset = Point2::new(point.u - line.0.u, point.v - line.0.v);
    let measured = (offset.u * direction.v - offset.v * direction.u).abs() / length;
    linear_measurement_matches(measured, evaluated_mm, linear_tolerance)
}

fn linear_measurement_matches(measured: f64, expected: f64, linear_tolerance: f64) -> bool {
    if !measured.is_finite()
        || !expected.is_finite()
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return false;
    }
    let expected = expected.abs();
    let scale = 1.0 + measured.abs().max(expected);
    (measured.abs() - expected).abs()
        <= linear_tolerance.max(EPS_DIMENSIONS_LINEAR_MEASUREMENT_MATCHES_E9 * scale)
}

fn two_locus_distance_dimension(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    if entities.len() != 2
        || dimension_resource!(ctx.equal(
            entities[0].id(),
            entities[1].id(),
            "f3d distance entity equality"
        ))
    {
        return None;
    }
    Some(Ok(Definition::Distance {
        entities: dimension_resource!(copy_dimension_entity_members(ctx, entities)),
        parameter: dimension_resource!(
            parameter.try_clone_for_decode(ctx, "f3d two locus parameter id")
        ),
    }))
}

#[cfg(test)]
fn counted_role_relation(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    owner_role: u64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    crate::test_support::with_decode_context(|decode_ctx| {
        counted_role_relation_at_tolerance(
            decode_ctx,
            entities,
            &crate::records::sketch_relations::constraint_kinds_from_state(owner_role).0,
            0.0,
        )
        .transpose()
        .unwrap()
    })
}

fn counted_role_relation_at_tolerance(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    owner_kinds: &[SketchConstraintKind],
    linear_tolerance: f64,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
    };
    let mut kind_source = owner_kinds.iter();
    while let Some(kind) =
        dimension_resource!(ctx.next_charged(&mut kind_source, "f3d counted owner kind source"))
    {
        let candidate = (|| match (kind, entities) {
            (SketchConstraintKind::Horizontal | SketchConstraintKind::Vertical, [entity]) => {
                let Geometry::Line { start, end } = entity.geometry.definition() else {
                    return None;
                };
                let du = end.u - start.u;
                let dv = end.v - start.v;
                let length = du.hypot(dv);
                if length <= EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E12 {
                    return None;
                }
                match kind {
                    SketchConstraintKind::Horizontal
                        if dv.abs()
                            <= EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E9 * length =>
                    {
                        Some(Ok(Definition::Horizontal {
                            entity: dimension_resource!((entity.id()).try_clone_for_decode(
                                ctx,
                                "f3d counted role relation at tolerance entity id"
                            )),
                        }))
                    }
                    SketchConstraintKind::Vertical
                        if du.abs()
                            <= EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E9 * length =>
                    {
                        Some(Ok(Definition::Vertical {
                            entity: dimension_resource!((entity.id()).try_clone_for_decode(
                                ctx,
                                "f3d counted role relation at tolerance entity id"
                            )),
                        }))
                    }
                    _ => None,
                }
            }
            (SketchConstraintKind::Tangent, [first, second])
                if exact_line_arc_tangency(entities, linear_tolerance)
                    || dimension_resource!(exact_circular_tangency(
                        ctx,
                        entities,
                        linear_tolerance
                    )) =>
            {
                Some(Ok(Definition::Tangent {
                    first: dimension_resource!((first.id()).try_clone_for_decode(
                        ctx,
                        "f3d counted role relation at tolerance first id"
                    )),
                    second: dimension_resource!((second.id()).try_clone_for_decode(
                        ctx,
                        "f3d counted role relation at tolerance second id"
                    )),
                }))
            }
            (SketchConstraintKind::Equal, [first, second])
                if dimension_resource!(exact_equal_size(ctx, entities)) =>
            {
                Some(Ok(Definition::Equal {
                    first: dimension_resource!((first.id()).try_clone_for_decode(
                        ctx,
                        "f3d counted role relation at tolerance first id"
                    )),
                    second: dimension_resource!((second.id()).try_clone_for_decode(
                        ctx,
                        "f3d counted role relation at tolerance second id"
                    )),
                }))
            }
            _ => None,
        })();
        if candidate.is_some() {
            return candidate;
        }
    }
    None
}

fn exact_line_arc_tangency(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    let [first, second] = entities else {
        return false;
    };
    let (line_start, line_end, center, radius, arc_start, arc_end) =
        match (first.geometry.definition(), second.geometry.definition()) {
            (
                Geometry::Line {
                    start: line_start,
                    end: line_end,
                },
                Geometry::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                },
            )
            | (
                Geometry::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                },
                Geometry::Line {
                    start: line_start,
                    end: line_end,
                },
            ) => (
                *line_start,
                *line_end,
                *center,
                radius.get(),
                Point2::new(
                    center.u + radius.get() * start_angle.get().cos(),
                    center.v + radius.get() * start_angle.get().sin(),
                ),
                Point2::new(
                    center.u + radius.get() * end_angle.get().cos(),
                    center.v + radius.get() * end_angle.get().sin(),
                ),
            ),
            _ => return false,
        };
    let line_direction = Point2::new(line_end.u - line_start.u, line_end.v - line_start.v);
    let line_length = line_direction.u.hypot(line_direction.v);
    if line_length <= EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E12 {
        return false;
    }
    [line_start, line_end].into_iter().any(|line_point| {
        [arc_start, arc_end].into_iter().any(|arc_point| {
            let scale = 1.0
                + line_point
                    .u
                    .abs()
                    .max(line_point.v.abs())
                    .max(arc_point.u.abs())
                    .max(arc_point.v.abs())
                    .max(radius);
            let point_tolerance = linear_tolerance.max(EPS_CIRCULAR_TANGENCY * scale.max(1.0));
            let radius_direction = Point2::new(arc_point.u - center.u, arc_point.v - center.v);
            (line_point.u - arc_point.u).abs() <= point_tolerance
                && (line_point.v - arc_point.v).abs() <= point_tolerance
                && (line_direction.u * radius_direction.u + line_direction.v * radius_direction.v)
                    .abs()
                    <= EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E9 * line_length * radius
        })
    })
}

fn exact_circular_tangency(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    let [first, second] = entities else {
        return Ok(false);
    };
    if ctx.equal(first.id(), second.id(), "f3d geometry entity equality")? {
        return Ok(false);
    }
    Ok((|| {
        let circular = |geometry: &cadmpeg_ir::sketches::SketchGeometry| match geometry.definition()
        {
            Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
                Some((*center, radius.get()))
            }
            _ => None,
        };
        let Some((first_center, first_radius)) = circular(&first.geometry) else {
            return false;
        };
        let Some((second_center, second_radius)) = circular(&second.geometry) else {
            return false;
        };
        let center_delta = Point2::new(
            second_center.u - first_center.u,
            second_center.v - first_center.v,
        );
        let center_distance = center_delta.u.hypot(center_delta.v);
        if !center_distance.is_finite() || center_distance <= EPS_CIRCULAR_TANGENCY_LENGTH {
            return false;
        }
        let tangent_tolerance =
            linear_tolerance.max(EPS_CIRCULAR_TANGENCY * (1.0 + first_radius.max(second_radius)));
        let close = |left: f64, right: f64| (left - right).abs() <= tangent_tolerance;
        if !close(center_distance, first_radius + second_radius)
            && !close(center_distance, (first_radius - second_radius).abs())
        {
            return false;
        }
        let unit = Point2::new(
            center_delta.u / center_distance,
            center_delta.v / center_distance,
        );
        let candidate_points = [
            Point2::new(
                first_center.u + first_radius * unit.u,
                first_center.v + first_radius * unit.v,
            ),
            Point2::new(
                first_center.u - first_radius * unit.u,
                first_center.v - first_radius * unit.v,
            ),
        ];
        candidate_points.iter().any(|point| {
            let second_radius_error =
                (point.u - second_center.u).hypot(point.v - second_center.v) - second_radius;
            if second_radius_error.abs() > tangent_tolerance {
                return false;
            }
            circular_entity_contains_point(&first.geometry, *point, linear_tolerance)
                && circular_entity_contains_point(&second.geometry, *point, linear_tolerance)
        })
    })())
}

fn circular_entity_contains_point(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    point: Point2,
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    match geometry.definition() {
        Geometry::Circle { .. } => true,
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let scale = 1.0 + radius.get().abs().max(point.u.abs().max(point.v.abs()));
            let point_tolerance = linear_tolerance.max(EPS_CIRCULAR_TANGENCY * scale.max(1.0));
            // Not a stated value: the floored quantity is the length-to-angle ratio
            // computed here, and the constant is the double-precision angular
            // resolution of that division, never read from the file.
            let angular_tolerance =
                (point_tolerance / radius.get().abs()).max(EPS_CIRCULAR_TANGENCY);
            let angle = relative.v.atan2(relative.u);
            let angle_close = |left: f64, right: f64| {
                let distance = (left - right).abs().rem_euclid(std::f64::consts::TAU);
                distance.min(std::f64::consts::TAU - distance) <= angular_tolerance
            };
            (relative.u.hypot(relative.v) - radius.get()).abs() <= point_tolerance
                && (angle_close(angle, start_angle.get())
                    || angle_close(angle, end_angle.get())
                    || angle_in_sweep(angle, start_angle.get(), end_angle.get(), angular_tolerance))
        }
        _ => false,
    }
}

const EPS_CIRCULAR_TANGENCY: f64 = 1.0e-9;
const EPS_CIRCULAR_TANGENCY_LENGTH: f64 = 1.0e-12;

fn exact_equal_size(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    let [first, second] = entities else {
        return Ok(false);
    };
    if ctx.equal(first.id(), second.id(), "f3d geometry entity equality")? {
        return Ok(false);
    }
    Ok({
        let close = |first: f64, second: f64| {
            (first - second).abs()
                <= EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E9 * (1.0 + first.abs().max(second.abs()))
        };
        match (first.geometry.definition(), second.geometry.definition()) {
            (
                Geometry::Line {
                    start: first_start,
                    end: first_end,
                },
                Geometry::Line {
                    start: second_start,
                    end: second_end,
                },
            ) => {
                let first_length = (first_end.u - first_start.u).hypot(first_end.v - first_start.v);
                let second_length =
                    (second_end.u - second_start.u).hypot(second_end.v - second_start.v);
                first_length > EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E12
                    && second_length > EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E12
                    && close(first_length, second_length)
            }
            (
                Geometry::Circle { radius: first, .. } | Geometry::Arc { radius: first, .. },
                Geometry::Circle { radius: second, .. } | Geometry::Arc { radius: second, .. },
            ) => close(first.get(), second.get()),
            (
                Geometry::Ellipse {
                    radii: first_radii, ..
                },
                Geometry::Ellipse {
                    radii: second_radii,
                    ..
                },
            ) => {
                close(first_radii.major().get(), second_radii.major().get())
                    && close(first_radii.minor().get(), second_radii.minor().get())
            }
            _ => false,
        }
    })
}

const EPS_CENTERED_RELATION: f64 = 1.0e-9;
const EPS_OFFSET_SWEEP: f64 = 1.0e-12;

fn exact_centered_entity_relation(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let [first, second] = entities else {
        return None;
    };
    if dimension_resource!(ctx.equal(first.id(), second.id(), "f3d centered entity equality")) {
        return None;
    }
    let centered_geometry =
        |entity: &cadmpeg_ir::sketches::SketchEntity| match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. } => {
                Some((center.get(), Some(radius.get())))
            }
            SketchGeometryDefinition::Ellipse { center, .. } => Some((center.get(), None)),
            _ => None,
        };
    let (first_center, first_radius) = centered_geometry(first)?;
    let (second_center, second_radius) = centered_geometry(second)?;
    if !sketch_points_close(first_center, second_center) {
        return None;
    }
    if let (Some(first_radius), Some(second_radius)) = (first_radius, second_radius) {
        let scale = 1.0 + first_radius.abs().max(second_radius.abs());
        if (first_radius - second_radius).abs() <= EPS_CENTERED_RELATION * scale {
            return Some(Ok(Definition::Coradial {
                first: dimension_resource!((first.id())
                    .try_clone_for_decode(ctx, "f3d exact centered entity relation first id")),
                second: dimension_resource!((second.id())
                    .try_clone_for_decode(ctx, "f3d exact centered entity relation second id")),
            }));
        }
    }
    Some(Ok(Definition::Concentric {
        first: dimension_resource!(
            (first.id()).try_clone_for_decode(ctx, "f3d exact centered entity relation first id")
        ),
        second: dimension_resource!(
            (second.id()).try_clone_for_decode(ctx, "f3d exact centered entity relation second id")
        ),
    }))
}

fn exact_counted_dimension_relation(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    if let Some(definition) = exact_centered_entity_relation(ctx, entities).transpose()? {
        return Ok(Some(definition));
    }
    if let Some((first, second, axis)) = reflected_symmetry(ctx, entities)? {
        return Ok(Some(Definition::Symmetric {
            first: SketchLocus::Entity(
                (first.id()).try_clone_for_decode(ctx, "f3d exact counted first id")?,
            ),
            second: SketchLocus::Entity(
                (second.id()).try_clone_for_decode(ctx, "f3d exact counted second id")?,
            ),
            axis: (axis.id()).try_clone_for_decode(ctx, "f3d exact counted axis id")?,
        }));
    }
    let [first, second] = entities else {
        return Ok(None);
    };
    if ctx.equal(first.id(), second.id(), "f3d counted entity equality")? {
        return Ok(None);
    }
    let point_on_geometry = |point: &cadmpeg_ir::sketches::SketchEntity,
                             geometry: &cadmpeg_ir::sketches::SketchEntity|
     -> Result<bool, CodecError> {
        let SketchGeometryDefinition::Point { position } = *point.geometry.definition() else {
            return Ok(false);
        };
        point_lies_on_sketch_geometry(ctx, position.get(), &geometry.geometry)
    };
    if point_on_geometry(first, second)? || point_on_geometry(second, first)? {
        return Ok(Some(Definition::Coincident {
            entities: vec![
                (first.id()).try_clone_for_decode(ctx, "f3d exact counted first id")?,
                (second.id()).try_clone_for_decode(ctx, "f3d exact counted second id")?,
            ],
        }));
    }
    let (
        SketchGeometryDefinition::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometryDefinition::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first.geometry.definition(), second.geometry.definition())
    else {
        return Ok(None);
    };
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if !first_length.is_finite()
        || !second_length.is_finite()
        || first_length <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9
        || second_length <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9
    {
        return Ok(None);
    }
    let first_direction = Point2::new(
        first_direction.u / first_length,
        first_direction.v / first_length,
    );
    let second_direction = Point2::new(
        second_direction.u / second_length,
        second_direction.v / second_length,
    );
    let cross = first_direction
        .u
        .mul_add(second_direction.v, -first_direction.v * second_direction.u);
    if cross.abs() <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9 {
        let Some(signed_offset) = parallel_line_offset(&first.geometry, &second.geometry) else {
            return Ok(None);
        };
        return Ok(Some(
            if signed_offset.abs()
                <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9 * (1.0 + first_length)
            {
                Definition::Collinear {
                    first: (first.id()).try_clone_for_decode(ctx, "f3d exact counted first id")?,
                    second: (second.id())
                        .try_clone_for_decode(ctx, "f3d exact counted second id")?,
                }
            } else {
                Definition::Parallel {
                    first: (first.id()).try_clone_for_decode(ctx, "f3d exact counted first id")?,
                    second: (second.id())
                        .try_clone_for_decode(ctx, "f3d exact counted second id")?,
                }
            },
        ));
    }
    let dot = first_direction
        .u
        .mul_add(second_direction.u, first_direction.v * second_direction.v);
    (dot.abs() <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9)
        .then(|| -> Result<_, CodecError> {
            Ok(Definition::Perpendicular {
                first: (first.id()).try_clone_for_decode(ctx, "f3d exact counted first id")?,
                second: (second.id()).try_clone_for_decode(ctx, "f3d exact counted second id")?,
            })
        })
        .transpose()
}

pub(super) fn point_lies_on_sketch_geometry(
    ctx: &DecodeContext<'_>,
    point: Point2,
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() {
        if curve.periodic() {
            return Ok(false);
        }
        let tolerance = EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
            * (1.0 + point.u.abs().max(point.v.abs()));
        let lanes = PcurveEvaluatorLanes::new(
            ctx,
            curve.pole_rows(),
            "f3d nurbs evaluator poles",
            "f3d nurbs evaluator weights",
        )?;
        return cadmpeg_ir::eval::nurbs_pcurve_contains_point(
            ctx,
            curve.degree(),
            curve.knots(),
            lanes.points(),
            lanes.weights(),
            point,
            tolerance,
        )
        .map(|contained| contained.unwrap_or(false))
        .map_err(CodecError::ResourceLimit);
    }

    let close = |left: f64, right: f64| {
        left.is_finite()
            && right.is_finite()
            && (left - right).abs()
                <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    * (1.0 + left.abs().max(right.abs()))
    };
    Ok((|| match geometry.definition() {
        SketchGeometryDefinition::Point { position } => sketch_points_close(point, position.get()),
        SketchGeometryDefinition::Line { start, end } => {
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let length = direction.u.hypot(direction.v);
            if !length.is_finite() || length <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 {
                return false;
            }
            let unit = Point2::new(direction.u / length, direction.v / length);
            let relative = Point2::new(point.u - start.u, point.v - start.v);
            let parameter = relative.u.mul_add(unit.u, relative.v * unit.v) / length;
            let perpendicular = relative.u.mul_add(unit.v, -relative.v * unit.u);
            (-EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                ..=1.0 + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9)
                .contains(&parameter)
                && perpendicular.is_finite()
                && perpendicular.abs() <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            if !length.is_finite() || length <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 {
                return false;
            }
            let relative = Point2::new(point.u - origin.u, point.v - origin.v);
            let perpendicular = relative
                .u
                .mul_add(direction.v / length, -relative.v * (direction.u / length));
            perpendicular.is_finite()
                && perpendicular.abs() <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            close((point.u - center.u).hypot(point.v - center.v), radius.get())
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            close(relative.u.hypot(relative.v), radius.get())
                && angle_in_sweep(
                    relative.v.atan2(relative.u),
                    start_angle.get(),
                    end_angle.get(),
                    EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9,
                )
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / radii.major().get();
            let y = (-relative.u).mul_add(sin, relative.v * cos) / radii.minor().get();
            close(x.mul_add(x, y * y), 1.0)
                && match bounds {
                    Some([start, end]) => angle_in_sweep(
                        y.atan2(x),
                        start.get(),
                        end.get(),
                        EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9,
                    ),
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
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.get();
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.get();
            let parameter = y.asinh();
            close(x, parameter.cosh())
                && match bounds {
                    Some([start, end]) => {
                        parameter >= start.get() - EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                            && parameter
                                <= end.get() + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    }
                    None => true,
                }
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let relative = Point2::new(point.u - vertex.u, point.v - vertex.v);
            let (sin, cos) = axis_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin);
            let y = (-relative.u).mul_add(sin, relative.v * cos);
            let parameter = y;
            let Some(axial) = cadmpeg_ir::math::product_quotient(
                [parameter, parameter],
                [4.0, focal_length.get()],
            ) else {
                return false;
            };
            close(x, axial.get())
                && match bounds {
                    Some([start, end]) => {
                        parameter
                            >= start.get().min(end.get())
                                - EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                            && parameter
                                <= start.get().max(end.get())
                                    + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    }
                    None => true,
                }
        }
        SketchGeometryDefinition::Nurbs { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => false,
    })())
}

struct CountedOffset {
    pairs: Vec<cadmpeg_ir::sketches::SketchOffsetPair>,
    distance: cadmpeg_ir::scalar::Length,
}

fn exact_counted_offset(
    ctx: &DecodeContext<'_>,
    loci: &[crate::records::dimensions::DesignDimensionLocus],
    entities: &HashMap<u32, &cadmpeg_ir::sketches::SketchEntity>,
    secondary_ids: &HashMap<u32, u64>,
    linear_tolerance: f64,
) -> Option<Result<CountedOffset, CodecError>> {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::SketchOffsetPair;
    macro_rules! resource {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }
        };
    }

    if loci.len() != entities.len() || loci.len() < 2 || !loci.len().is_multiple_of(2) {
        return None;
    }
    let source_count = loci.len() / 2;
    let role_partition = resource!(ctx.all_by(
        &loci[..source_count],
        |locus| Ok(locus.role != 0),
        "f3d counted offset source role"
    )) && resource!(ctx.all_by(
        &loci[source_count..],
        |locus| Ok(locus.role == 0),
        "f3d counted offset result role"
    ));
    let identity_partition = resource!(ctx.all_by(
        loci,
        |locus| Ok(locus.role != 0),
        "f3d counted offset identity role"
    )) && resource!(ctx.all_by(
        &loci[..source_count],
        |locus| Ok(ctx
            .get_hash_map(
                secondary_ids,
                &locus.geometry_record_index,
                "f3d counted offset source identity lookup"
            )?
            .copied()
            == Some(0)),
        "f3d counted offset source identity"
    )) && resource!(ctx.all_by(
        &loci[source_count..],
        |locus| Ok(ctx
            .get_hash_map(
                secondary_ids,
                &locus.geometry_record_index,
                "f3d counted offset result identity lookup"
            )?
            .is_some_and(|id| *id != 0)),
        "f3d counted offset result identity"
    ));
    if !role_partition && !identity_partition {
        return None;
    }
    let mut index_storage = resource!(ctx.reserve_scoped(0, "f3d counted offset index storage"));
    let mut source_records = HashSet::new();
    let mut result_records = HashSet::new();
    for locus in resource!(ctx
        .admit_iter(&loci[..source_count], "f3d counted offset source records")
        .map_err(CodecError::from))
    {
        resource!(index_storage
            .with_storage(|| ctx.insert_hash_set(
                &mut source_records,
                locus.geometry_record_index,
                "f3d counted offset source record"
            ))
            .map(|_| ()));
    }
    for locus in resource!(ctx
        .admit_iter(&loci[source_count..], "f3d counted offset result records")
        .map_err(CodecError::from))
    {
        resource!(index_storage
            .with_storage(|| ctx.insert_hash_set(
                &mut result_records,
                locus.geometry_record_index,
                "f3d counted offset result record"
            ))
            .map(|_| ()));
    }
    if source_records.len() != source_count || result_records.len() != source_count {
        return None;
    }
    let mut used_members = HashSet::new();
    let mut pairs = Vec::new();
    let mut canonical_distance: Option<f64> = None;
    let mut pair_source = loci.as_chunks::<2>().0.iter();
    while let Some([source_locus, result_locus]) =
        resource!(ctx.next_charged(&mut pair_source, "f3d counted offset pair source"))
    {
        let source_record_index = source_locus.returned.value;
        let result_record_index = result_locus.returned.value;
        if !resource!(ctx.contains_hash_set(
            &source_records,
            &source_record_index,
            "f3d counted offset record lookup"
        )) || !resource!(ctx.contains_hash_set(
            &result_records,
            &result_record_index,
            "f3d counted offset record lookup"
        )) || resource!(ctx.contains_hash_set(
            &used_members,
            &source_record_index,
            "f3d counted offset record lookup"
        )) || resource!(ctx.contains_hash_set(
            &used_members,
            &result_record_index,
            "f3d counted offset record lookup"
        )) {
            return None;
        }
        resource!(index_storage
            .with_storage(|| ctx.insert_hash_set(
                &mut used_members,
                source_record_index,
                "f3d counted offset used source"
            ))
            .map(|_| ()));
        resource!(index_storage
            .with_storage(|| ctx.insert_hash_set(
                &mut used_members,
                result_record_index,
                "f3d counted offset used result"
            ))
            .map(|_| ()));
        let source = resource!(ctx.get_hash_map(
            entities,
            &source_record_index,
            "f3d counted offset entity lookup"
        ))?;
        let result = resource!(ctx.get_hash_map(
            entities,
            &result_record_index,
            "f3d counted offset entity lookup"
        ))?;
        let distance = match sketch_curve_offset(&source.geometry, &result.geometry) {
            Some(distance) => distance,
            None => resource!(cadmpeg_ir::eval::fitted_nurbs_offset_frame_distance(
                ctx,
                &source.geometry,
                &result.geometry,
                linear_tolerance,
            )
            .map_err(CodecError::from))
            .map(cadmpeg_ir::scalar::FiniteReal::get)?,
        };
        if distance.abs() <= EPS_DIMENSIONS_EXACT_COUNTED_OFFSET_E9 {
            return None;
        }
        let source_reversed = offset_source_reversed(distance, &mut canonical_distance)?;
        resource!(ctx.push_vec(
            &mut pairs,
            SketchOffsetPair {
                source: resource!(
                    (source.id()).try_clone_for_decode(ctx, "f3d counted offset source id")
                ),
                result: resource!(
                    (result.id()).try_clone_for_decode(ctx, "f3d counted offset result id")
                ),
                source_reversed,
            },
            "f3d counted offset pair"
        ));
    }
    Some(Ok(CountedOffset {
        pairs,
        distance: Length::new(canonical_distance?)?,
    }))
}

fn offset_parameter_factor(distance: f64, parameter_value: f64) -> Option<f64> {
    let scale = 1.0 + distance.abs().max(parameter_value.abs());
    (distance.is_finite()
        && parameter_value.is_finite()
        && (distance - parameter_value.abs()).abs()
            <= scale * EPS_DIMENSIONS_OFFSET_PARAMETER_FACTOR_E9)
        .then(|| {
            if parameter_value.is_sign_positive() {
                1.0
            } else {
                -1.0
            }
        })
}

fn line_angle_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchGeometryDefinition::Line {
        start: first_start,
        end: first_end,
    } = first.definition()
    else {
        return false;
    };
    let SketchGeometryDefinition::Line {
        start: second_start,
        end: second_end,
    } = second.definition()
    else {
        return false;
    };
    let first_du = first_end.u - first_start.u;
    let first_dv = first_end.v - first_start.v;
    let second_du = second_end.u - second_start.u;
    let second_dv = second_end.v - second_start.v;
    let unit = |du, dv| {
        cadmpeg_ir::features::FiniteVector3::new(cadmpeg_ir::math::Vector3::new(du, dv, 0.0))
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    };
    let Some(first) = unit(first_du, first_dv) else {
        return false;
    };
    let Some(second) = unit(second_du, second_dv) else {
        return false;
    };
    let angle = first.cross(second).norm().atan2(first.dot(second));
    let supplementary = std::f64::consts::PI - angle;
    let scale = 1.0 + expected.abs();
    (angle - expected).abs() <= scale * EPS_DIMENSIONS_LINE_ANGLE_MATCHES_E9
        || (supplementary - expected).abs() <= scale * EPS_DIMENSIONS_LINE_ANGLE_MATCHES_E9
}

pub(super) fn exact_offset_constraint(
    ctx: &DecodeContext<'_>,
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<Result<cadmpeg_ir::sketches::SketchConstraintDefinitionInput, CodecError>> {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput as Definition, SketchOffsetPair};

    if crate::design::relation_kinds::unknown_constraint_bits(relation.definition.state()) != 0
        || !matches!(
            crate::design::relation_kinds::sole_constraint_kind(relation),
            Some(SketchConstraintKind::Perpendicular | SketchConstraintKind::Offset)
        )
        || relation.return_members().len() < 4
        || !relation.return_members().len().is_multiple_of(2)
        || relation.return_members().len() != relation.members().len()
        || dimension_resource!(ctx.any_by(
            relation.return_members().iter(),
            |member| Ok(member.reference.resolved().is_none()),
            "f3d relation offset resolved source"
        ))
    {
        return None;
    }
    // The second reference run carries the bijection in order. A stored offset
    // relation's sources can be another offset relation's results, so their
    // secondary identities are not null and only the run order separates the
    // two sides.
    let ordered_pairs = crate::design::relation_kinds::sole_constraint_kind(relation)
        == Some(SketchConstraintKind::Offset);
    let mut index_storage =
        dimension_resource!(ctx.reserve_scoped(0, "f3d relation offset index storage"));
    let mut pairs = Vec::new();
    let mut used_entities = HashSet::new();
    let mut canonical_distance: Option<f64> = None;
    let mut member_source = relation.return_members().chunks_exact(2);
    while let Some(members) = dimension_resource!(
        ctx.next_charged(&mut member_source, "f3d relation offset member source")
    ) {
        let (first_record_index, first_secondary_id, second_record_index, second_secondary_id) =
            match (&members[0].reference, &members[1].reference) {
                (
                    crate::records::sketch_relations::SketchRelationReference::Resolved(
                        SketchRelationOperand::Curve {
                            record_index: first_record_index,
                            secondary_id: first_secondary_id,
                            ..
                        },
                    ),
                    crate::records::sketch_relations::SketchRelationReference::Resolved(
                        SketchRelationOperand::Curve {
                            record_index: second_record_index,
                            secondary_id: second_secondary_id,
                            ..
                        },
                    ),
                ) => (
                    *first_record_index,
                    *first_secondary_id,
                    *second_record_index,
                    *second_secondary_id,
                ),
                _ => return None,
            };
        let (source_record_index, result_record_index) =
            if ordered_pairs || (first_secondary_id == 0 && second_secondary_id != 0) {
                (first_record_index, second_record_index)
            } else {
                return None;
            };
        let source = dimension_resource!(ctx.get_hash_map(
            projected,
            &(scope, source_record_index),
            "f3d relation offset entity lookup"
        ))?;
        let result = dimension_resource!(ctx.get_hash_map(
            projected,
            &(scope, result_record_index),
            "f3d relation offset entity lookup"
        ))?;
        if dimension_resource!(ctx.contains_hash_set(
            &used_entities,
            source.id(),
            "f3d relation offset used lookup"
        )) || dimension_resource!(ctx.contains_hash_set(
            &used_entities,
            result.id(),
            "f3d relation offset used lookup"
        )) {
            return None;
        }
        dimension_resource!(index_storage
            .with_storage(|| ctx.insert_hash_set(
                &mut used_entities,
                source.id(),
                "f3d relation offset used source"
            ))
            .map(|_| ()));
        dimension_resource!(index_storage
            .with_storage(|| ctx.insert_hash_set(
                &mut used_entities,
                result.id(),
                "f3d relation offset used result"
            ))
            .map(|_| ()));
        let distance = parallel_line_offset(&source.geometry, &result.geometry)?;
        if distance.abs() <= EPS_DIMENSIONS_EXACT_OFFSET_CONSTRAINT_E9 {
            return None;
        }
        let source_reversed = offset_source_reversed(distance, &mut canonical_distance)?;
        dimension_resource!(ctx.push_vec(
            &mut pairs,
            SketchOffsetPair {
                source: dimension_resource!(
                    (source.id()).try_clone_for_decode(ctx, "f3d relation offset source id")
                ),
                result: dimension_resource!(
                    (result.id()).try_clone_for_decode(ctx, "f3d relation offset result id")
                ),
                source_reversed,
            },
            "f3d relation offset pair"
        ));
    }
    Some(Ok(Definition::Offset {
        pairs,
        distance: Length::new(canonical_distance?)?,
        parameter: None,
    }))
}

fn offset_source_reversed(distance: f64, canonical: &mut Option<f64>) -> Option<bool> {
    let magnitude = distance.abs();
    let Some(expected) = *canonical else {
        *canonical = Some(magnitude);
        return Some(distance.is_sign_negative());
    };
    let scale = 1.0 + magnitude.max(expected);
    if (magnitude - expected).abs() > scale * EPS_DIMENSIONS_OFFSET_SOURCE_REVERSED_E9 {
        return None;
    }
    Some(distance.is_sign_negative())
}

fn sketch_curve_offset(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match (source.definition(), result.definition()) {
        (
            SketchGeometryDefinition::Circle {
                center: source_center,
                radius: source_radius,
            },
            SketchGeometryDefinition::Circle {
                center: result_center,
                radius: result_radius,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            ((source_center.u - result_center.u).abs() <= EPS_CENTERED_RELATION * scale
                && (source_center.v - result_center.v).abs() <= EPS_CENTERED_RELATION * scale)
                .then_some(source_radius.get() - result_radius.get())
        }
        (
            SketchGeometryDefinition::Circle {
                center: source_center,
                radius: source_radius,
            },
            SketchGeometryDefinition::Arc {
                center: result_center,
                radius: result_radius,
                start_angle: result_start,
                end_angle: result_end,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            let result_sweep = result_end.get() - result_start.get();
            (result_sweep.abs() > EPS_OFFSET_SWEEP
                && (source_center.u - result_center.u).abs() <= EPS_CENTERED_RELATION * scale
                && (source_center.v - result_center.v).abs() <= EPS_CENTERED_RELATION * scale)
                .then_some(source_radius.get() - result_radius.get())
        }
        (
            SketchGeometryDefinition::Arc {
                center: source_center,
                radius: source_radius,
                start_angle: source_start,
                end_angle: source_end,
            },
            SketchGeometryDefinition::Circle {
                center: result_center,
                radius: result_radius,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            let source_sweep = source_end.get() - source_start.get();
            (source_sweep.abs() > EPS_OFFSET_SWEEP
                && (source_center.u - result_center.u).abs() <= EPS_CENTERED_RELATION * scale
                && (source_center.v - result_center.v).abs() <= EPS_CENTERED_RELATION * scale)
                .then_some(source_sweep.signum() * (source_radius.get() - result_radius.get()))
        }
        (
            SketchGeometryDefinition::Arc {
                center: source_center,
                radius: source_radius,
                start_angle: source_start,
                end_angle: source_end,
            },
            SketchGeometryDefinition::Arc {
                center: result_center,
                radius: result_radius,
                start_angle: result_start,
                end_angle: result_end,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            let source_sweep = source_end.get() - source_start.get();
            let result_sweep = result_end.get() - result_start.get();
            let angular_overlap = [source_start.get(), source_end.get()]
                .into_iter()
                .any(|angle| {
                    angle_in_sweep(
                        angle,
                        result_start.get(),
                        result_end.get(),
                        EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9,
                    )
                })
                || [result_start.get(), result_end.get()]
                    .into_iter()
                    .any(|angle| {
                        angle_in_sweep(
                            angle,
                            source_start.get(),
                            source_end.get(),
                            EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9,
                        )
                    });
            (source_sweep.abs() > EPS_OFFSET_SWEEP
                && result_sweep.abs() > EPS_OFFSET_SWEEP
                && source_sweep.signum() == result_sweep.signum()
                && angular_overlap
                && (source_center.u - result_center.u).abs()
                    <= EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9 * scale
                && (source_center.v - result_center.v).abs()
                    <= EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9 * scale)
                .then_some(source_sweep.signum() * (source_radius.get() - result_radius.get()))
        }
        _ => parallel_line_offset(source, result),
    }
}

fn parallel_line_offset(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchGeometryDefinition::Line {
        start: source_start,
        end: source_end,
    } = source.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Line {
        start: result_start,
        end: result_end,
    } = result.definition()
    else {
        return None;
    };
    let source_du = source_end.u - source_start.u;
    let source_dv = source_end.v - source_start.v;
    let result_du = result_end.u - result_start.u;
    let result_dv = result_end.v - result_start.v;
    let source_length = source_du.hypot(source_dv);
    let result_length = result_du.hypot(result_dv);
    if source_length <= EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E12
        || result_length <= EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E12
    {
        return None;
    }
    let source_direction = cadmpeg_ir::features::FiniteVector3::new(
        cadmpeg_ir::math::Vector3::new(source_du, source_dv, 0.0),
    )?
    .unit_nonzero()?;
    let result_direction = cadmpeg_ir::features::FiniteVector3::new(
        cadmpeg_ir::math::Vector3::new(result_du, result_dv, 0.0),
    )?
    .unit_nonzero()?;
    let parallel_error = source_direction.cross(result_direction).norm();
    if parallel_error > EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E9 {
        return None;
    }
    let normal_u = -source_direction.y;
    let normal_v = source_direction.x;
    let distance_at = |point: &Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    let start_distance = distance_at(result_start);
    let end_distance = distance_at(result_end);
    let scale = 1.0 + start_distance.abs().max(end_distance.abs());
    ((start_distance - end_distance).abs() <= scale * EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E9)
        .then_some(start_distance)
}

fn reflected_symmetry<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a cadmpeg_ir::sketches::SketchEntity],
) -> Result<
    Option<(
        &'a cadmpeg_ir::sketches::SketchEntity,
        &'a cadmpeg_ir::sketches::SketchEntity,
        &'a cadmpeg_ir::sketches::SketchEntity,
    )>,
    CodecError,
> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let [first, second, third] = entities else {
        return Ok(None);
    };
    if ctx.equal(first.id(), second.id(), "f3d symmetry entity equality")?
        || ctx.equal(first.id(), third.id(), "f3d symmetry entity equality")?
        || ctx.equal(second.id(), third.id(), "f3d symmetry entity equality")?
    {
        return Ok(None);
    }
    let mut candidate = None;
    for (axis_ordinal, axis) in entities.iter().enumerate() {
        let axis = *axis;
        let SketchGeometryDefinition::Line {
            start: axis_start,
            end: axis_end,
        } = axis.geometry.definition()
        else {
            continue;
        };
        let others = match axis_ordinal {
            0 => [*second, *third],
            1 => [*first, *third],
            _ => [*first, *second],
        };
        if reflected_geometry_matches(
            &others[0].geometry,
            &others[1].geometry,
            axis_start,
            axis_end,
        ) && candidate.replace((others[0], others[1], axis)).is_some()
        {
            return Ok(None);
        }
    }
    Ok(candidate)
}

fn reflected_geometry_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    axis_start: &Point2,
    axis_end: &Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match (first.definition(), second.definition()) {
        (
            SketchGeometryDefinition::Point {
                position: first_position,
            },
            SketchGeometryDefinition::Point {
                position: second_position,
            },
        ) => reflect_point(first_position.get(), *axis_start, *axis_end)
            .is_some_and(|reflected| sketch_points_close(reflected, second_position.get())),
        (
            SketchGeometryDefinition::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometryDefinition::Line {
                start: second_start,
                end: second_end,
            },
        ) => {
            let Some(reflected_start) = reflect_point(first_start.get(), *axis_start, *axis_end)
            else {
                return false;
            };
            let Some(reflected_end) = reflect_point(first_end.get(), *axis_start, *axis_end) else {
                return false;
            };
            sketch_points_close(reflected_start, second_start.get())
                && sketch_points_close(reflected_end, second_end.get())
                || sketch_points_close(reflected_start, second_end.get())
                    && sketch_points_close(reflected_end, second_start.get())
        }
        (
            SketchGeometryDefinition::Circle {
                center: first_center,
                radius: first_radius,
            },
            SketchGeometryDefinition::Circle {
                center: second_center,
                radius: second_radius,
            },
        ) => {
            let radius_scale = 1.0 + first_radius.get().abs().max(second_radius.get().abs());
            (first_radius.get() - second_radius.get()).abs()
                <= EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9 * radius_scale
                && reflect_point(first_center.get(), *axis_start, *axis_end)
                    .is_some_and(|reflected| sketch_points_close(reflected, second_center.get()))
        }
        (
            SketchGeometryDefinition::Arc {
                center: first_center,
                radius: first_radius,
                start_angle: first_start,
                end_angle: first_end,
            },
            SketchGeometryDefinition::Arc {
                center: second_center,
                radius: second_radius,
                start_angle: second_start,
                end_angle: second_end,
            },
        ) => {
            let radius_scale = 1.0 + first_radius.get().abs().max(second_radius.get().abs());
            let first_sweep = (first_end.get() - first_start.get()).abs();
            let second_sweep = (second_end.get() - second_start.get()).abs();
            let sweep_scale = 1.0 + first_sweep.max(second_sweep);
            if (first_radius.get() - second_radius.get()).abs()
                > EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9 * radius_scale
                || (first_sweep - second_sweep).abs()
                    > EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9 * sweep_scale
                || !reflect_point(first_center.get(), *axis_start, *axis_end)
                    .is_some_and(|reflected| sketch_points_close(reflected, second_center.get()))
            {
                return false;
            }
            let arc_point = |center: Point2, radius: f64, angle: f64| {
                Point2::new(
                    center.u + radius * angle.cos(),
                    center.v + radius * angle.sin(),
                )
            };
            let Some(reflected_start) = reflect_point(
                arc_point(first_center.get(), first_radius.get(), first_start.get()),
                *axis_start,
                *axis_end,
            ) else {
                return false;
            };
            let Some(reflected_end) = reflect_point(
                arc_point(first_center.get(), first_radius.get(), first_end.get()),
                *axis_start,
                *axis_end,
            ) else {
                return false;
            };
            let second_start =
                arc_point(second_center.get(), second_radius.get(), second_start.get());
            let second_end = arc_point(second_center.get(), second_radius.get(), second_end.get());
            sketch_points_close(reflected_start, second_start)
                && sketch_points_close(reflected_end, second_end)
                || sketch_points_close(reflected_start, second_end)
                    && sketch_points_close(reflected_end, second_start)
        }
        _ => false,
    }
}

const EPS_REFLECTION_AXIS_LENGTH: f64 = 1.0e-9;

fn reflect_point(point: Point2, axis_start: Point2, axis_end: Point2) -> Option<Point2> {
    let direction =
        cadmpeg_ir::math::Vector3::new(axis_end.u - axis_start.u, axis_end.v - axis_start.v, 0.0);
    if direction.norm() <= EPS_REFLECTION_AXIS_LENGTH {
        return None;
    }
    let unit = cadmpeg_ir::features::FiniteVector3::new(direction)?.unit_nonzero()?;
    let normal = Point2::new(-unit.y, unit.x);
    let distance = normal
        .u
        .mul_add(point.u - axis_start.u, normal.v * (point.v - axis_start.v));
    if !distance.is_finite() {
        return None;
    }
    let reflected = Point2::new(
        (-2.0 * normal.u).mul_add(distance, point.u),
        (-2.0 * normal.v).mul_add(distance, point.v),
    );
    reflected.is_finite().then_some(reflected)
}

fn sketch_points_close(first: Point2, second: Point2) -> bool {
    let scale = 1.0
        + first
            .u
            .abs()
            .max(first.v.abs())
            .max(second.u.abs())
            .max(second.v.abs());
    (first.u - second.u).abs() <= scale * EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9
        && (first.v - second.v).abs() <= scale * EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9
}

pub(super) fn relation_kind_name(
    relation: &SketchRelation,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    struct Names([Option<&'static str>; 21]);
    impl std::fmt::Display for Names {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut separator = "";
            for name in self.0.iter().flatten() {
                formatter.write_str(separator)?;
                formatter.write_str(name)?;
                separator = "+";
            }
            Ok(())
        }
    }
    let mut names = [None; 21];
    for (slot, kind) in
        names
            .iter_mut()
            .zip(crate::records::sketch_relations::constraint_kinds_iter(
                relation.definition.state(),
            ))
    {
        *slot = Some(match kind {
            SketchConstraintKind::Coincident => "coincident",
            SketchConstraintKind::Colinear => "collinear",
            SketchConstraintKind::Concentric => "concentric",
            SketchConstraintKind::EqualLength => "equal_length",
            SketchConstraintKind::Parallel => "parallel",
            SketchConstraintKind::Perpendicular => "perpendicular",
            SketchConstraintKind::Horizontal => "horizontal",
            SketchConstraintKind::Vertical => "vertical",
            SketchConstraintKind::Tangent => "tangent",
            SketchConstraintKind::Curvature => "curvature",
            SketchConstraintKind::Symmetry => "symmetry",
            SketchConstraintKind::Equal => "equal",
            SketchConstraintKind::Midpoint => "midpoint",
            SketchConstraintKind::Polygon => "polygon",
            SketchConstraintKind::Offset => "offset",
            SketchConstraintKind::SplineGroup => "spline_group",
            SketchConstraintKind::CircularPattern => "circular_pattern",
            SketchConstraintKind::RectangularPattern => "rectangular_pattern",
            SketchConstraintKind::TextFrame => "text_frame",
            SketchConstraintKind::TextPath => "text_path",
        });
    }
    if crate::design::relation_kinds::unknown_constraint_bits(relation.definition.state()) != 0 {
        names[20] = Some("unknown_bits");
    }
    ctx.format_retained(
        format_args!("{}", Names(names)),
        "f3d sketch constraint native kind",
    )
}

pub(super) fn planar_point(point: &Point3) -> bool {
    point.is_finite() && point.z.abs() <= EPS_DIMENSIONS_PLANAR_POINT_E9
}

pub(super) fn sketch_normal_sign(normal: &Vector3) -> Option<f64> {
    (normal.x.abs() <= EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9
        && normal.y.abs() <= EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9
        && (normal.z.abs() - 1.0).abs() <= EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9)
        .then_some(normal.z.signum())
}

pub(super) fn expression_identifiers<'text, 'ctx>(
    ctx: &'ctx DecodeContext<'ctx>,
    expression: &'text str,
) -> impl Iterator<Item = Result<&'text str, CodecError>> + 'ctx
where
    'text: 'ctx,
{
    let identifier_character = |character: char| {
        character.is_alphanumeric() || matches!(character, '_' | '"' | '$' | '°' | 'µ')
    };
    let mut start = None;
    let mut characters = expression
        .char_indices()
        .chain(std::iter::once((expression.len(), '\0')));
    let mut ended = false;
    std::iter::from_fn(move || {
        if ended {
            return None;
        }
        let token = (|| -> Result<Option<&'text str>, CodecError> {
            while let Some((offset, character)) =
                ctx.next_charged(&mut characters, "f3d expression character scan")?
            {
                if identifier_character(character) {
                    start.get_or_insert(offset);
                    continue;
                }
                let Some(token_start) = start.take() else {
                    continue;
                };
                let token = &expression[token_start..offset];
                if !token
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_alphabetic() || character == '_')
                {
                    continue;
                }
                let next = ctx.find_by(
                    expression[offset..].chars(),
                    |character| Ok(!character.is_whitespace()),
                    "f3d expression following character",
                )?;
                if next == Some('(') {
                    continue;
                }
                if matches!(token, "mm" | "cm" | "m" | "in" | "ft" | "deg" | "rad") {
                    let previous = ctx.find_by(
                        expression[..token_start].chars().rev(),
                        |character| Ok(!character.is_whitespace()),
                        "f3d expression preceding character",
                    )?;
                    if previous
                        .is_some_and(|character| character.is_ascii_digit() || character == ')')
                    {
                        continue;
                    }
                }
                return Ok(Some(token));
            }
            Ok(None)
        })();
        match token {
            Ok(Some(token)) => Some(Ok(token)),
            Ok(None) => {
                ended = true;
                None
            }
            Err(error) => {
                ended = true;
                Some(Err(error))
            }
        }
    })
}

/// Count decoded same-stream parameter-name symbols that have no neutral
/// dependency edge.
pub(crate) fn unresolved_parameter_expression_dependency_count(
    ctx: &DecodeContext<'_>,
    native: &[DesignParameter],
    projected: &[cadmpeg_ir::features::DesignParameter],
) -> Result<usize, CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "f3d expression audit lookup storage")?;
    let (projected_by_native_ref, projected_by_id, names_by_stream) =
        lookup_storage.with_storage(|| -> Result<_, CodecError> {
            let mut projected_by_native_ref = HashMap::new();
            let mut projected_by_id = HashMap::new();
            for parameter in ctx.admit_iter(projected, "f3d expression projected parameter scan")? {
                if let Some(native_ref) = parameter.native_ref.as_deref() {
                    ctx.insert_hash_map(
                        &mut projected_by_native_ref,
                        native_ref,
                        parameter,
                        "f3d expression native parameter index",
                    )
                    .map(|_| ())?;
                }
                ctx.insert_hash_map(
                    &mut projected_by_id,
                    &parameter.id,
                    parameter,
                    "f3d expression neutral parameter index",
                )
                .map(|_| ())?;
            }
            let mut names_by_stream = HashMap::<&str, HashSet<&str>>::new();
            for parameter in ctx.admit_iter(native, "f3d expression native parameter scan")? {
                let Some(stream) =
                    native_stream_charged(ctx, &parameter.id, "resolve F3D dimension stream")?
                else {
                    continue;
                };
                let names = ctx
                    .entry_hash_map(&mut names_by_stream, stream, "f3d expression stream index")?
                    .or_default();
                ctx.insert_hash_set(names, parameter.name(), "f3d expression stream name")?;
            }
            Ok((projected_by_native_ref, projected_by_id, names_by_stream))
        })?;
    let mut unresolved = 0usize;
    for parameter in ctx.admit_iter(native, "f3d expression audit parameter scan")? {
        let Some(stream) =
            native_stream_charged(ctx, &parameter.id, "resolve F3D dimension stream")?
        else {
            continue;
        };
        let Some(names) =
            ctx.get_hash_map(&names_by_stream, stream, "f3d expression stream lookup")?
        else {
            continue;
        };
        let Some(projected) = ctx.get_hash_map(
            &projected_by_native_ref,
            parameter.id.as_str(),
            "f3d expression projected parameter lookup",
        )?
        else {
            continue;
        };
        let mut dependency_storage = ctx.reserve_scoped(0, "f3d expression dependency storage")?;
        let dependency_names = dependency_storage.with_storage(|| -> Result<_, CodecError> {
            let mut names = HashSet::new();
            for dependency in ctx.admit_iter(
                projected.dependencies.as_slice(),
                "f3d expression dependency scan",
            )? {
                if let Some(dependency) = ctx.get_hash_map(
                    &projected_by_id,
                    dependency,
                    "f3d expression dependency lookup",
                )? {
                    ctx.insert_hash_set(
                        &mut names,
                        dependency.name.as_str(),
                        "f3d expression dependency name",
                    )?;
                }
            }
            Ok(names)
        })?;
        let mut identifier_storage = ctx.reserve_scoped(0, "f3d expression identifier storage")?;
        identifier_storage.with_storage(|| -> Result<(), CodecError> {
            let mut identifiers = HashSet::new();
            for identifier in expression_identifiers(ctx, parameter.expression()) {
                let identifier = identifier?;
                if ctx.contains_hash_set(names, identifier, "f3d expression name lookup")?
                    && ctx.insert_hash_set(
                        &mut identifiers,
                        identifier,
                        "f3d expression identifier",
                    )?
                    && !ctx.contains_hash_set(
                        &dependency_names,
                        identifier,
                        "f3d expression dependency name lookup",
                    )?
                {
                    unresolved += 1;
                }
            }
            Ok(())
        })?;
    }
    Ok(unresolved)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod numerical_range_tests;
