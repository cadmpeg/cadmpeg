// SPDX-License-Identifier: Apache-2.0
//! Transfer of `FCStd` construction history into neutral design entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::KnotVector;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchAxis, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
    SketchNativeOperand,
};
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        holes::{
            HoleBottom, HoleConstruction, HoleKind, HoleProfileFilter, HoleSpecification,
            HoleThreadDepth, ThreadHand,
        },
        patterns::{PatternKind, PatternScaleCenter, PatternSeed, PatternStage, PatternTransform},
        AngularTermination, BinderConstruction, BinderCopyOnChange, BinderLifecycle, BinderOffset,
        BinderOffsetJoin, BinderPlacement, BinderSource, BinderTarget, BodySelection, BooleanOp,
        DesignParameter, DistinctMembers, EdgeSelection, ExtrudeExtent, ExtrudeSide,
        ExtrusionDirectionSource, FaceMaker, Feature, FeatureContent, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureTreeNodeRole, FuzzyTolerance, GeometryImportFormat,
        HelicalSweepConstruction, HelicalSweepLaw, HelixConstructionStyle, InnerWireTaper,
        LinearTermination, ParameterId, ParameterValue, PathRef, PlanarProfileRef, PrimitiveSolid,
        PrimitiveSolidKind, ProfileRef, RevolutionAxis, RevolutionFuseOrder, RevolveConstruction,
        RevolveExtent, RuledCurveOrientation, ScaleCenter, ScaleFactors, ShellJoin, ShellMode,
        SurfaceProjectionMode, SweepOrientation, SweepTransformation, SweepTransition,
        TreeChildren,
    },
    scalar::{FiniteReal, Length, NonZeroReal, PositiveLength, PositiveReal},
};

use crate::brep::ShapePayloadRecord;
use crate::native::{EntryRecord, ObjectRecord, PropertyRecord};

const MAX_SKETCH_RECORDS: usize = 1_000_000;
const EXTERNAL_GEO_AXIS_COUNT: usize = 2;
const EXTERNAL_GEOMETRY_MISSING_FLAG: u64 = 1 << 3;
const DEFAULT_HELICAL_SWEEP_TOLERANCE: f64 = 0.1;
const DEFAULT_PART_SPIRAL_SEGMENT_TURNS: f64 = 1.0;
const U64_UPPER_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

macro_rules! required {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

mod profiles;
mod spreadsheets;

fn malformed_design(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> CodecError {
    crate::resource::malformed_charged(ctx, message, "fcstd design diagnostic")
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    entries: &[EntryRecord],
    program_version: Option<&str>,
) -> Result<BTreeSet<String>, CodecError> {
    let (properties_by_owner, _properties_by_owner_storage) = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "fcstd design owner index",
    )?;
    let (properties_by_id, _property_id_storage) = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.id.as_str(), property)),
        "fcstd design property identity index",
    )?;
    let (payloads_by_owner, _payload_storage) =
        ctx.with_scoped_storage("fcstd design payload owner storage", || {
            let mut by_owner = BTreeMap::new();
            for payload in ctx.admit_iter(payloads, "fcstd design payload index inputs")? {
                let Some(properties) = ctx.get_btree_map(
                    &properties_by_id,
                    payload.property.as_str(),
                    "fcstd design payload property lookup",
                )?
                else {
                    continue;
                };
                let (mut owners, mut owner_storage) = ctx
                    .with_scoped_storage("fcstd design payload unique owner storage", || {
                        Ok::<_, CodecError>(BTreeSet::new())
                    })?;
                for property in
                    ctx.admit_iter(properties, "fcstd design payload property owners")?
                {
                    if owner_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut owners,
                            property.owner.as_str(),
                            "fcstd design payload unique owner",
                        )
                    })? {
                        ctx.push_btree_group(
                            &mut by_owner,
                            property.owner.as_str(),
                            payload,
                            "fcstd design payload owners",
                            "fcstd design owner payloads",
                        )?;
                    }
                }
            }
            Ok::<_, CodecError>(by_owner)
        })?;
    let (object_by_id, _object_storage) = ctx.collect_scoped_btree_map(
        objects
            .iter()
            .rev()
            .map(|object| (object.id().as_str(), object)),
        "fcstd design object index",
    )?;
    let (feature_ids, feature_ids_storage) =
        ctx.with_scoped_storage("fcstd design feature index storage", || {
            let mut feature_ids = HashMap::new();
            for object in ctx.admit_iter(objects, "fcstd design feature objects")? {
                if !is_design_object(&object.type_name) {
                    continue;
                }
                ctx.insert_hash_map(
                    &mut feature_ids,
                    object.id().as_str(),
                    feature_id(ctx, object)?,
                    "fcstd design feature ids",
                )?;
            }
            Ok::<_, CodecError>(feature_ids)
        })?;
    let (predecessors, predecessor_storage) =
        body_predecessors(ctx, objects, &feature_ids, &properties_by_owner)?;
    let (parent_by_member, parent_by_member_storage) =
        ctx.with_scoped_storage("fcstd design membership index storage", || {
            let mut parent_by_member = HashMap::new();
            for body in ctx
                .admit_iter(objects, "fcstd design body objects")?
                .filter(|object| is_body(&object.type_name))
            {
                let property = match ctx.get_btree_map(
                    &properties_by_owner,
                    body.id().as_str(),
                    "fcstd design owner index lookup",
                )? {
                    Some(properties) => body_membership_property(ctx, properties)?,
                    None => None,
                };
                let Some(property) = property else {
                    continue;
                };
                for link in
                    ctx.admit_iter(property.links(), "fcstd design body membership links")?
                {
                    let Some(member) = link.as_ref().and_then(crate::native::LinkTarget::object)
                    else {
                        continue;
                    };
                    ctx.insert_hash_map(
                        &mut parent_by_member,
                        member,
                        feature_id(ctx, body)?,
                        "fcstd design body membership",
                    )?;
                }
            }
            Ok::<_, CodecError>(parent_by_member)
        })?;
    let (mut sketch_ids, mut sketch_ids_storage) =
        ctx.with_scoped_storage("fcstd design sketch index storage", || {
            let mut sketch_ids = HashMap::new();
            for object in ctx
                .admit_iter(objects, "fcstd design sketch objects")?
                .filter(|object| is_sketch(&object.type_name))
            {
                let id = SketchId::mint(design_identity_text(
                    ctx,
                    "sketch",
                    object,
                    format_args!(""),
                    "fcstd design sketch identity",
                )?)
                .map_err(CodecError::malformed)?;
                ctx.insert_hash_map(
                    &mut sketch_ids,
                    object.id().as_str(),
                    id,
                    "fcstd design sketch ids",
                )?;
            }
            Ok::<_, CodecError>(sketch_ids)
        })?;
    let (body_ids, body_ids_storage) =
        ctx.with_scoped_storage("fcstd design body id storage", || {
            let mut body_ids =
                ctx.vector_storage(ir.model.bodies.len(), "fcstd design body ids")?;
            for body in ctx.admit_iter(&ir.model.bodies, "fcstd design body source ids")? {
                ctx.push_vec(&mut body_ids, &body.id, "fcstd design body ids")?;
            }
            Ok::<_, CodecError>(body_ids)
        })?;
    let FeatureOrdering {
        ordinals: feature_ordinals,
        mut cycle_affected,
        storage: feature_ordinal_storage,
    } = feature_ordinals(ctx, objects, &properties_by_owner, &parent_by_member)?;
    drop(parent_by_member);
    drop(parent_by_member_storage);
    let (ordinal_by_feature, ordinal_by_feature_storage) =
        ctx.with_scoped_storage("fcstd design ordinal index storage", || {
            let mut ordinal_by_feature = HashMap::new();
            for object in ctx.admit_iter(objects, "fcstd design feature objects")? {
                if !is_design_object(&object.type_name) {
                    continue;
                }
                ctx.insert_hash_map(
                    &mut ordinal_by_feature,
                    feature_id(ctx, object)?,
                    *ctx.get_hash_map(
                        &feature_ordinals,
                        object.id().as_str(),
                        "fcstd design object ordinal lookup",
                    )?
                    .ok_or_else(|| CodecError::malformed("design object lost its ordinal"))?,
                    "fcstd design feature ordinals",
                )?;
            }
            Ok::<_, CodecError>(ordinal_by_feature)
        })?;

    for object in ctx.admit_iter(objects, "fcstd design transfer objects")? {
        if !is_design_object(&object.type_name) {
            continue;
        }
        let owned = ctx
            .get_btree_map(
                &properties_by_owner,
                object.id().as_str(),
                "fcstd design selected owner properties",
            )?
            .map_or(&[][..], Vec::as_slice);
        let object_ordinal = *ctx
            .get_hash_map(
                &feature_ordinals,
                object.id().as_str(),
                "fcstd design object ordinal lookup",
            )?
            .ok_or_else(|| CodecError::malformed("design object lost its ordinal"))?;
        let id = feature_id(ctx, object)?;
        let mut definition = if is_spreadsheet(&object.type_name) {
            ctx.push_vec(
                &mut ir.model.spreadsheets,
                spreadsheets::append_spreadsheet(ctx, &mut ir.model.parameters, object, owned)?,
                "fcstd design spreadsheets",
            )?;
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: TreeChildren::default(),
            })
        } else if is_body(&object.type_name) {
            body_definition(ctx, owned, &feature_ids)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_datum(&object.type_name) {
            datum_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_sketch(&object.type_name) {
            let decoded = parse_sketch(ctx, object, owned)?;
            let sketch = decoded.sketch;
            let sketch_id = sketch
                .id
                .try_clone_for_decode(ctx, "fcstd design sketch identity")?;
            sketch_ids_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut sketch_ids,
                    object.id().as_str(),
                    sketch_id.try_clone_for_decode(ctx, "fcstd design sketch index identity")?,
                    "fcstd design sketch ids",
                )
            })?;
            ctx.push_vec(&mut ir.model.sketches, sketch, "fcstd neutral sketches")?;
            ctx.extend_vec(
                &mut ir.model.sketch_entities,
                decoded.entities,
                "fcstd neutral sketch entities",
            )?;
            ctx.extend_vec(
                &mut ir.model.sketch_constraints,
                decoded.constraints,
                "fcstd neutral sketch constraints",
            )?;
            ctx.extend_vec(
                &mut ir.model.parameters,
                decoded.parameters,
                "fcstd sketch parameters",
            )?;
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
            })
        } else if is_stored_geometry_feature(&object.type_name) {
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        } else if object.type_name == "PartDesign::FeatureBase" {
            feature_base_definition(ctx, owned, &feature_ids)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_imported_geometry(&object.type_name) {
            imported_geometry_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_part_construction_geometry(&object.type_name) {
            part_construction_geometry_definition(ctx, &object.type_name, owned, entries)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_primitive(&object.type_name) {
            primitive_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_boolean(&object.type_name) {
            match boolean_definition(ctx, &object.type_name, owned)? {
                Some(definition) => definition,
                None if object.type_name != "PartDesign::Boolean" => {
                    match cached_shape_definition(ctx, owned)? {
                        Some(definition) => definition,
                        None => native_definition(ctx, &object.type_name, owned)?,
                    }
                }
                None => native_definition(ctx, &object.type_name, owned)?,
            }
        } else if is_loft(&object.type_name) {
            match loft_definition(ctx, &object.type_name, owned, &sketch_ids)? {
                Some(definition) => definition,
                None => match cached_shape_definition(ctx, owned)? {
                    Some(definition) => definition,
                    None => native_definition(ctx, &object.type_name, owned)?,
                },
            }
        } else if is_sweep(&object.type_name) {
            sweep_definition(ctx, &object.type_name, owned, &sketch_ids)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_helical_sweep(&object.type_name) {
            helical_sweep_definition(
                ctx,
                &object.type_name,
                object.id(),
                owned,
                &sketch_ids,
                &object_by_id,
                &properties_by_owner,
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if matches!(object.type_name.as_str(), "Part::Helix" | "Part::Spiral") {
            parametric_helix_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_binder(&object.type_name) {
            binder_definition(ctx, &object.type_name, owned, &feature_ids)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_pattern(&object.type_name) {
            pattern_definition(
                ctx,
                &object.type_name,
                object.id(),
                owned,
                &feature_ids,
                PatternSources {
                    objects,
                    object_by_id: &object_by_id,
                    predecessors: &predecessors,
                    properties_by_owner: &properties_by_owner,
                    entries,
                },
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if object.type_name == "Part::Scale" {
            scale_definition(ctx, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_hole(&object.type_name) {
            hole_definition(
                ctx,
                object.id(),
                owned,
                &sketch_ids,
                &object_by_id,
                &properties_by_owner,
                program_version,
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_extrusion(&object.type_name) {
            let profile = match profile_ref(ctx, object.id(), owned, &sketch_ids)? {
                ProfileRef::Planar(PlanarProfileRef::Unresolved(_)) => {
                    let mut selected = None;
                    for name in ["Profile", "Sketch", "Base", "Source"] {
                        if let Some(candidate) = property(ctx, owned, name)? {
                            selected = Some(candidate);
                            break;
                        }
                    }
                    selected.map_or_else(
                        || {
                            ctx.copy_retained_text(object.id(), "fcstd unresolved profile identity")
                                .map(|id| ProfileRef::Planar(PlanarProfileRef::Unresolved(id)))
                        },
                        |property| {
                            ctx.copy_retained_text(&property.id, "fcstd native profile identity")
                                .map(|id| ProfileRef::Planar(PlanarProfileRef::Native(id)))
                        },
                    )?
                }
                profile => profile,
            };
            let profile_normal = match profile_target(ctx, owned)? {
                Some((_, target)) => {
                    match ctx.get_btree_map(&object_by_id, target, "fcstd profile object lookup")? {
                        Some(profile_object) => {
                            let profile_properties = ctx
                                .get_btree_map(
                                    &properties_by_owner,
                                    profile_object.id().as_str(),
                                    "fcstd profile owner properties",
                                )?
                                .map_or(&[][..], Vec::as_slice);
                            Some(sketch_frame(ctx, profile_properties)?.1)
                        }
                        None => None,
                    }
                }
                None => None,
            };
            extrusion_definition(
                ctx,
                &object.type_name,
                owned,
                profile,
                profile_normal,
                &ir.model.sketches,
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_revolution(&object.type_name) {
            revolution_definition(ctx, &object.type_name, object.id(), owned, &sketch_ids)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if matches!(
            object.type_name.as_str(),
            "PartDesign::Thickness" | "Part::Thickness"
        ) {
            thickness_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if matches!(object.type_name.as_str(), "Part::Offset" | "Part::Offset2D") {
            offset_shape_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if matches!(
            object.type_name.as_str(),
            "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
        ) {
            derived_shape_definition(ctx, &object.type_name, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if object.type_name == "Part::RuledSurface" {
            ruled_surface_definition(ctx, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if object.type_name == "Part::Section" {
            section_shape_definition(ctx, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if object.type_name == "Part::Mirroring" {
            mirror_shape_definition(ctx, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if object.type_name == "Part::ProjectOnSurface" {
            project_on_surface_definition(ctx, owned)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if object.type_name == "PartDesign::Draft" {
            draft_definition(ctx, owned, &object_by_id, &properties_by_owner)?
                .map_or_else(|| native_definition(ctx, &object.type_name, owned), Ok)?
        } else if is_fillet(&object.type_name) {
            match fillet_definition(ctx, &object.type_name, owned, entries)? {
                Some(definition) => definition,
                None => match cached_shape_definition(ctx, owned)? {
                    Some(definition) => definition,
                    None => native_definition(ctx, &object.type_name, owned)?,
                },
            }
        } else if is_chamfer(&object.type_name) {
            match chamfer_definition(ctx, &object.type_name, owned, entries, program_version)? {
                Some(definition) => definition,
                None => match cached_shape_definition(ctx, owned)? {
                    Some(definition) => definition,
                    None => native_definition(ctx, &object.type_name, owned)?,
                },
            }
        } else {
            native_definition(ctx, &object.type_name, owned)?
        };
        if ctx.contains_btree_set(
            &cycle_affected,
            object.id().as_str(),
            "fcstd design cycle object lookup",
        )? {
            definition = native_definition(ctx, &object.type_name, owned)?;
        }
        let mut semantic_dependencies = Vec::new();
        let mut semantic_storage = ctx.reserve_scoped(0, "fcstd semantic dependency storage")?;
        if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = &definition {
            for seed in ctx.admit_iter(seeds, "fcstd design pattern seeds")? {
                if let PatternSeed::Feature(feature) = seed {
                    semantic_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut semantic_dependencies,
                            feature.try_clone_for_decode(ctx, "fcstd design pattern dependency")?,
                            "fcstd design pattern dependencies",
                        )
                    })?;
                }
            }
        }
        let definition = post_processed_definition(ctx, definition, &object.type_name, owned)?;
        append_operation_parameters(ctx, &mut ir.model.parameters, object, owned)?;
        let mut outputs = Vec::new();
        let owned_payloads = ctx
            .get_btree_map(
                &payloads_by_owner,
                object.id().as_str(),
                "fcstd design owner payload lookup",
            )?
            .map_or(&[][..], Vec::as_slice);
        for payload in ctx.admit_iter(owned_payloads, "fcstd design body payloads")? {
            let prefix = BodyOutputPrefix::new(ctx, payload)?;
            for body in ctx.admit_iter(&body_ids, "fcstd design body output candidates")? {
                if !ctx.starts_with(
                    body.as_str(),
                    &prefix.text,
                    "fcstd design body output prefix",
                )? {
                    continue;
                }
                ctx.push_vec(
                    &mut outputs,
                    body.try_clone_for_decode(ctx, "fcstd design output body")?,
                    "fcstd design feature outputs",
                )?;
            }
        }
        let cycle_affected = ctx.contains_btree_set(
            &cycle_affected,
            object.id().as_str(),
            "fcstd design cycle object lookup",
        )?;
        let dependencies = if cycle_affected {
            // The native object and property arenas retain the exact cycle.
            // A neutral edge would require a decoder-owned cycle break and
            // would change when persisted declaration order changes.
            Vec::new()
        } else {
            let mut dependency_storage =
                ctx.reserve_scoped(0, "fcstd design dependency candidates")?;
            let mut dependency_objects = Vec::new();
            if !is_body(&object.type_name) {
                for dependency in
                    ctx.admit_iter(&object.dependencies, "fcstd design declared dependencies")?
                {
                    ctx.push_scoped_vec(
                        &mut dependency_storage,
                        &mut dependency_objects,
                        (dependency.as_str(), true),
                        "fcstd design dependency candidates",
                    )?;
                }
            }
            for property in ctx.admit_iter(owned, "fcstd design dependency properties")? {
                for link in ctx.admit_iter(property.links(), "fcstd design dependency links")? {
                    let Some(dependency) =
                        link.as_ref().and_then(crate::native::LinkTarget::object)
                    else {
                        continue;
                    };
                    ctx.push_scoped_vec(
                        &mut dependency_storage,
                        &mut dependency_objects,
                        (dependency, false),
                        "fcstd design dependency candidates",
                    )?;
                }
            }
            let mut seen_dependencies = BTreeSet::new();
            let mut seen_dependency_storage =
                ctx.reserve_scoped(0, "fcstd design unique dependency storage")?;
            let mut dependencies = Vec::new();
            for &(dependency, declared) in ctx.admit_iter(
                &dependency_objects,
                "fcstd design dependency candidate traversal",
            )? {
                if ctx.contains_btree_set(
                    &seen_dependencies,
                    dependency,
                    "fcstd design unique dependency search",
                )? {
                    continue;
                }
                seen_dependency_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut seen_dependencies,
                        dependency,
                        "fcstd design unique dependencies",
                    )
                })?;
                if let Some(feature) = ctx.get_hash_map(
                    &feature_ids,
                    dependency,
                    "fcstd design feature dependency lookup",
                )? {
                    let include = if declared {
                        true
                    } else {
                        ctx.get_hash_map(
                            &ordinal_by_feature,
                            feature,
                            "fcstd design dependency ordinal lookup",
                        )?
                        .is_some_and(|ordinal| *ordinal < object_ordinal)
                    };
                    if include {
                        ctx.push_vec(
                            &mut dependencies,
                            feature.try_clone_for_decode(ctx, "fcstd design feature dependency")?,
                            "fcstd design feature dependencies",
                        )?;
                    }
                }
            }
            for dependency in ctx.admit_iter(
                &semantic_dependencies,
                "fcstd semantic dependency candidates",
            )? {
                let duplicate = ctx
                    .find_by(
                        &dependencies,
                        |existing| {
                            ctx.equal(
                                existing.as_str(),
                                dependency.as_str(),
                                "fcstd design semantic dependency identity",
                            )
                        },
                        "fcstd design semantic dependency search",
                    )?
                    .is_some();
                let include = if duplicate {
                    false
                } else {
                    ctx.get_hash_map(
                        &ordinal_by_feature,
                        dependency,
                        "fcstd design semantic dependency ordinal lookup",
                    )?
                    .is_some_and(|ordinal| *ordinal < object_ordinal)
                };
                if include {
                    ctx.push_vec(
                        &mut dependencies,
                        dependency.try_clone_for_decode(
                            ctx,
                            "fcstd design semantic dependency identity",
                        )?,
                        "fcstd design feature dependencies",
                    )?;
                }
            }
            dependencies
        };
        let mut dependency_members = DistinctMembers::default();
        dependency_members.reserve_for_decode(
            ctx,
            dependencies.len(),
            "fcstd distinct feature dependencies",
        )?;
        dependency_members.append(ctx, dependencies, "fcstd distinct feature dependencies")?;
        ctx.push_vec(
            &mut ir.model.features,
            Feature {
                id,
                ordinal: object_ordinal,
                name: Some(ctx.copy_retained_text(object.name(), "fcstd feature name")?),
                suppressed: bool_property(ctx, owned, "Suppressed")?,
                dependencies: dependency_members,
                source_properties: feature_state(ctx, object.id(), owned)?,
                source_tag: Some(
                    ctx.copy_retained_text(&object.type_name, "fcstd feature source type")?,
                ),
                source_text: None,
                source_content: FeatureContent::default(),
                evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                    definition,
                    cadmpeg_ir::features::DistinctMembers::try_from(outputs, ctx)
                        .map_err(cadmpeg_core::CodecError::from)?,
                ),
                native_ref: Some(
                    ctx.copy_retained_text(object.id(), "fcstd feature native reference")?,
                ),
            },
            "fcstd neutral features",
        )?;
    }
    drop(body_ids);
    drop(body_ids_storage);
    drop(predecessors);
    drop(predecessor_storage);
    drop(feature_ids);
    drop(feature_ids_storage);
    drop(sketch_ids);
    drop(sketch_ids_storage);
    drop(ordinal_by_feature);
    drop(ordinal_by_feature_storage);
    drop(feature_ordinals);
    drop(feature_ordinal_storage);
    let mut initial_cycle_storage = ctx.reserve_scoped(0, "fcstd initial cycle feature storage")?;
    let mut initial_cycle_affected_features = BTreeSet::new();
    for object in ctx.admit_iter(objects, "fcstd design cycle objects")? {
        if !ctx.contains_btree_set(
            &cycle_affected,
            object.id().as_str(),
            "fcstd design cycle object lookup",
        )? {
            continue;
        }
        initial_cycle_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut initial_cycle_affected_features,
                feature_id(ctx, object)?,
                "fcstd design cycle feature identities",
            )
        })?;
    }
    let (parameter_cycle_features, _parameter_cycle_storage) = bind_parameter_dependencies(
        ctx,
        &mut ir.model.parameters,
        objects,
        &initial_cycle_affected_features,
    )?;
    for object in ctx.admit_iter(objects, "fcstd design parameter cycle objects")? {
        let (object_feature, _object_feature_storage) = ctx
            .with_scoped_storage("fcstd parameter cycle lookup identity storage", || {
                feature_id(ctx, object)
            })?;
        if !ctx.contains_btree_set(
            &parameter_cycle_features,
            &object_feature,
            "fcstd design parameter cycle feature lookup",
        )? {
            continue;
        }
        ctx.insert_btree_set(
            &mut cycle_affected,
            ctx.copy_retained_text(object.id(), "fcstd design parameter cycle identity")?,
            "fcstd design parameter cycle objects",
        )?;
        let feature_index = ctx.position_by(
            &ir.model.features,
            |feature| match feature.native_ref.as_deref() {
                Some(native_ref) => ctx.equal(
                    native_ref,
                    object.id().as_str(),
                    "fcstd design feature native reference",
                ),
                None => Ok(false),
            },
            "fcstd design feature native reference search",
        )?;
        if let Some(feature) = feature_index.and_then(|index| ir.model.features.get_mut(index)) {
            feature.evaluation.set_definition(native_definition(
                ctx,
                &object.type_name,
                ctx.get_btree_map(
                    &properties_by_owner,
                    object.id().as_str(),
                    "fcstd design cycle owner properties",
                )?
                .map(Vec::as_slice)
                .unwrap_or_default(),
            )?);
            feature.dependencies.clear();
        }
    }
    Ok(cycle_affected)
}

/// A body identity prefix and the reservation for its temporary text.
struct BodyOutputPrefix<'ctx> {
    text: String,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> BodyOutputPrefix<'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        payload: &crate::brep::ShapePayloadRecord,
    ) -> Result<Self, CodecError> {
        let key = ctx
            .split_once(&payload.id, "#", "fcstd design body output identity key")?
            .map_or(payload.id.as_str(), |(_, key)| key);
        let (text, storage) = ctx.format_scoped(
            format_args!("fcstd:model:body#{key}:"),
            "fcstd design body output prefix",
        )?;
        Ok(Self {
            text,
            _storage: storage,
        })
    }
}

fn body_membership_property<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    Ok(
        match (
            property(ctx, properties, "Group")?,
            property(ctx, properties, "Model")?,
        ) {
            (Some(group), None) | (None, Some(group))
                if group.type_name == "App::PropertyLinkList" =>
            {
                Some(group)
            }
            _ => None,
        },
    )
}

fn body_membership_carrier_is_valid(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<bool, CodecError> {
    Ok(
        match (
            property(ctx, properties, "Group")?,
            property(ctx, properties, "Model")?,
        ) {
            (None, None) => true,
            (Some(group), None) | (None, Some(group)) => group.type_name == "App::PropertyLinkList",
            (Some(_), Some(_)) => false,
        },
    )
}

fn body_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    if !body_membership_carrier_is_valid(ctx, properties)? {
        return Ok(None);
    }
    let mut children = Vec::new();
    if let Some(property) = body_membership_property(ctx, properties)? {
        for link in ctx.admit_iter(property.links(), "fcstd body member links")? {
            let Some(target) = link.as_ref().and_then(|link| link.object()) else {
                continue;
            };
            if let Some(feature) =
                ctx.get_hash_map(feature_ids, target, "fcstd body member feature lookup")?
            {
                ctx.push_vec(
                    &mut children,
                    feature.try_clone_for_decode(ctx, "fcstd body member feature identity")?,
                    "fcstd body member features",
                )?;
            }
        }
    }
    let active_child = match body_tip(ctx, properties, feature_ids)? {
        BodyTipResolution::Valid(active_child) => active_child,
        BodyTipResolution::Invalid => return Ok(None),
    };
    Ok(
        match cadmpeg_ir::features::TreeChildren::new(children, active_child, ctx) {
            Ok(children) => Some(children),
            Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                return Err(CodecError::ResourceLimit(limit))
            }
            Err(_) => None,
        }
        .map(|children| {
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::SolidBodies,
                children,
            })
        }),
    )
}

enum BodyTipResolution {
    Valid(Option<FeatureId>),
    Invalid,
}

fn body_tip(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Result<BodyTipResolution, CodecError> {
    let Some(property) = property(ctx, properties, "Tip")? else {
        return Ok(BodyTipResolution::Valid(None));
    };
    if property.type_name != "App::PropertyLink" {
        return Ok(BodyTipResolution::Invalid);
    }
    Ok(match property.links() {
        [] | [None] => BodyTipResolution::Valid(None),
        [Some(link)]
            if link.subelements().is_empty()
                && link.document().is_none()
                && link.document_attribute().is_none() =>
        {
            let Some(target) = link.object() else {
                return Ok(BodyTipResolution::Valid(None));
            };
            match ctx.get_hash_map(feature_ids, target, "fcstd body tip feature lookup")? {
                Some(feature) => BodyTipResolution::Valid(Some(
                    feature.try_clone_for_decode(ctx, "fcstd body tip feature identity")?,
                )),
                None => BodyTipResolution::Invalid,
            }
        }
        _ => BodyTipResolution::Invalid,
    })
}

struct FeatureOrdering<'ctx, 'a> {
    ordinals: HashMap<&'a str, u64>,
    cycle_affected: BTreeSet<String>,
    storage: ScopedReservation<'ctx>,
}

fn feature_ordinals<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    objects: &'a [ObjectRecord],
    properties_by_owner: &BTreeMap<&'a str, Vec<&'a PropertyRecord>>,
    parent_by_member: &HashMap<&'a str, FeatureId>,
) -> Result<FeatureOrdering<'ctx, 'a>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "fcstd design ordinal working storage")?;
    let mut design_objects = Vec::new();
    for object in ctx.admit_iter(objects, "fcstd design ordered objects")? {
        if is_design_object(&object.type_name) {
            storage.with_storage(|| {
                ctx.push_vec(&mut design_objects, object, "fcstd design ordered objects")
            })?;
        }
    }
    let count = design_objects.len();
    let mut object_by_id = HashMap::new();
    let mut object_by_name = HashMap::new();
    let mut object_by_feature = HashMap::new();
    let mut source_ordinals =
        storage.with_storage(|| ctx.vector_storage(count, "fcstd design source ordinals"))?;
    for object in ctx.admit_iter(&design_objects, "fcstd design source indexes")? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut object_by_id,
                object.id().as_str(),
                *object,
                "fcstd design id index",
            )?;
            ctx.insert_hash_map(
                &mut object_by_name,
                object.name().as_str(),
                *object,
                "fcstd design name index",
            )?;
            ctx.insert_hash_map(
                &mut object_by_feature,
                feature_id(ctx, object)?,
                object.id().as_str(),
                "fcstd design feature index",
            )?;
            ctx.push_vec(
                &mut source_ordinals,
                cadmpeg_core::decode::u64_from_index(object.order),
                "fcstd design source ordinals",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut source_ordinals,
        |value| value,
        Ord::cmp,
        "fcstd design source ordinals sort",
    )?;
    let mut emitted = BTreeSet::new();
    let mut ordinal_storage = ctx.reserve_scoped(0, "fcstd design ordinal result storage")?;
    let mut ordinals = HashMap::new();
    let mut cycle_affected = BTreeSet::new();

    let dependencies = storage.with_storage(|| {
        let mut dependencies =
            ctx.vector_storage(design_objects.len(), "fcstd design dependency lists")?;
        for object in ctx.admit_iter(&design_objects, "fcstd design dependency sources")? {
            let mut required = BTreeSet::new();
            if let Some(parent) = ctx.get_hash_map(
                parent_by_member,
                object.id().as_str(),
                "fcstd design membership parent lookup",
            )? {
                if let Some(parent) = ctx.get_hash_map(
                    &object_by_feature,
                    parent,
                    "fcstd design feature parent lookup",
                )? {
                    ctx.insert_btree_set(&mut required, *parent, "fcstd design required parent")?;
                }
            }
            if !is_body(&object.type_name) {
                for dependency in
                    ctx.admit_iter(&object.dependencies, "fcstd declared design dependencies")?
                {
                    if ctx.contains_key_hash_map(
                        &object_by_id,
                        dependency.as_str(),
                        "fcstd declared dependency lookup",
                    )? {
                        ctx.insert_btree_set(
                            &mut required,
                            dependency.as_str(),
                            "fcstd design required declaration",
                        )?;
                    }
                }
            }
            if let Some(properties) = ctx.get_btree_map(
                properties_by_owner,
                object.id().as_str(),
                "fcstd design dependency properties",
            )? {
                for property in ctx.admit_iter(properties, "fcstd design dependency properties")? {
                    for value in
                        ctx.admit_iter(property.values(), "fcstd design expression values")?
                    {
                        if let Some(expression) = ctx.get_btree_map(
                            &value.attributes,
                            "expression",
                            "fcstd design expression attribute",
                        )? {
                            expression_identifiers_until(
                                ctx,
                                expression,
                                "fcstd design expression identifiers",
                                |identifier| {
                                    if let Some((owner, _)) = ctx.split_once(
                                        identifier,
                                        ".",
                                        "fcstd design expression owner separator",
                                    )? {
                                        if let Some(dependency) = ctx.get_hash_map(
                                            &object_by_name,
                                            owner,
                                            "fcstd design expression owner lookup",
                                        )? {
                                            if !ctx.equal(
                                                dependency.id(),
                                                object.id(),
                                                "fcstd design expression self identity",
                                            )? {
                                                ctx.insert_btree_set(
                                                    &mut required,
                                                    dependency.id().as_str(),
                                                    "fcstd design required expression",
                                                )?;
                                            }
                                        }
                                    }
                                    Ok(true)
                                },
                            )?;
                        }
                    }
                    if !is_body(&object.type_name) {
                        for link in
                            ctx.admit_iter(property.links(), "fcstd design property links")?
                        {
                            let Some(dependency_id) =
                                link.as_ref().and_then(crate::native::LinkTarget::object)
                            else {
                                continue;
                            };
                            let Some(dependency) = ctx.get_hash_map(
                                &object_by_id,
                                dependency_id,
                                "fcstd design linked dependency lookup",
                            )?
                            else {
                                continue;
                            };
                            if matches!(
                                property.name.as_str(),
                                "Base"
                                    | "BaseFeature"
                                    | "Originals"
                                    | "Path"
                                    | "Profile"
                                    | "Sketch"
                                    | "Sections"
                                    | "Source"
                                    | "Spine"
                            ) || dependency.order < object.order
                            {
                                ctx.insert_btree_set(
                                    &mut required,
                                    dependency_id,
                                    "fcstd design required link",
                                )?;
                            }
                        }
                    }
                }
            }
            ctx.push_vec(&mut dependencies, required, "fcstd design dependency lists")?;
        }
        Ok::<_, CodecError>(dependencies)
    })?;
    for ordinal in ctx.admit_iter(&source_ordinals, "fcstd design dependency ordering passes")? {
        let mut next: Option<&ObjectRecord> = None;
        for (index, object) in ctx
            .admit_iter(&design_objects, "fcstd design dependency ordering")?
            .enumerate()
        {
            if ctx.contains_btree_set(
                &emitted,
                object.id().as_str(),
                "fcstd design emitted object lookup",
            )? {
                continue;
            }
            if !ctx.all_by(
                &dependencies[index],
                |dependency| {
                    ctx.contains_btree_set(
                        &emitted,
                        *dependency,
                        "fcstd design emitted dependency lookup",
                    )
                },
                "fcstd design dependency readiness",
            )? {
                continue;
            }
            if next.is_none_or(|current| object.order < current.order) {
                next = Some(object);
            }
        }
        let next = match next {
            Some(next) => next,
            None => {
                let mut next: Option<&ObjectRecord> = None;
                for object in ctx.admit_iter(&design_objects, "fcstd design cycle objects")? {
                    if ctx.contains_btree_set(
                        &emitted,
                        object.id().as_str(),
                        "fcstd design emitted cycle object lookup",
                    )? {
                        continue;
                    }
                    ctx.insert_btree_set(
                        &mut cycle_affected,
                        ctx.copy_retained_text(object.id(), "fcstd design cycle object")?,
                        "fcstd design cycle affected objects",
                    )?;
                    if next.is_none_or(|current| object.order < current.order) {
                        next = Some(object);
                    }
                }
                next.ok_or_else(|| {
                    CodecError::malformed(
                        "design object ordering lost an un-emitted object while resolving a cycle",
                    )
                })?
            }
        };
        storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut emitted,
                next.id().as_str(),
                "fcstd design emitted objects",
            )
        })?;
        ordinal_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut ordinals,
                next.id().as_str(),
                *ordinal,
                "fcstd design ordinals",
            )
        })?;
    }
    Ok(FeatureOrdering {
        ordinals,
        cycle_affected,
        storage: ordinal_storage,
    })
}

/// Apply an operation's shape-refinement and boolean-tolerance controls.
fn post_processed_definition(
    ctx: &DecodeContext<'_>,
    definition: FeatureDefinition,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<FeatureDefinition, CodecError> {
    Ok(match post_process_controls(ctx, properties)? {
        PostProcessControlState::Absent => definition,
        PostProcessControlState::Valid {
            refine,
            fuzzy_tolerance,
        } => match definition {
            FeatureDefinition::Operation(operation) => FeatureDefinition::PostProcess {
                operation,
                refine,
                fuzzy_tolerance,
            },
            FeatureDefinition::PostProcess { .. } => native_definition(ctx, kind, properties)?,
        },
        PostProcessControlState::Malformed => native_definition(ctx, kind, properties)?,
    })
}

enum PostProcessControlState {
    Absent,
    Valid {
        refine: bool,
        fuzzy_tolerance: FuzzyTolerance,
    },
    Malformed,
}

/// Resolve the exact persisted controls an operation carries.
fn post_process_controls(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<PostProcessControlState, CodecError> {
    let refine = match unique_named_property(ctx, properties, "Refine")? {
        NamedProperty::Present(property) => match direct_bool_value(ctx, property)? {
            Some(value) => Some(value),
            None => return Ok(PostProcessControlState::Malformed),
        },
        NamedProperty::Absent => None,
        NamedProperty::Duplicate => return Ok(PostProcessControlState::Malformed),
    };
    let fuzzy_tolerance = match unique_named_property(ctx, properties, "FuzzyTolerance")? {
        NamedProperty::Present(property) => match direct_fuzzy_tolerance(ctx, property)? {
            Some(value) => Some(value),
            None => return Ok(PostProcessControlState::Malformed),
        },
        NamedProperty::Absent => None,
        NamedProperty::Duplicate => return Ok(PostProcessControlState::Malformed),
    };
    if refine.is_none() && fuzzy_tolerance.is_none() {
        return Ok(PostProcessControlState::Absent);
    }
    Ok(PostProcessControlState::Valid {
        refine: refine.unwrap_or(false),
        fuzzy_tolerance: fuzzy_tolerance.unwrap_or(FuzzyTolerance::KernelDefault),
    })
}

enum NamedProperty<'a> {
    Absent,
    Present(&'a PropertyRecord),
    Duplicate,
}

fn unique_named_property<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<NamedProperty<'a>, CodecError> {
    Ok(
        match crate::native::sole_property_matching(ctx, properties, |property| {
            property.name == name
        })? {
            Ok(Some(property)) => NamedProperty::Present(property),
            Ok(None) => NamedProperty::Absent,
            Err(_) => NamedProperty::Duplicate,
        },
    )
}

fn append_operation_parameters(
    ctx: &DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<(), CodecError> {
    const NAMES: &[&str] = &[
        "Angle",
        "Angle2",
        "Radius",
        "Size",
        "Size2",
        "Length",
        "Length2",
        "Value",
        "Diameter",
        "Depth",
        "HoleCutDiameter",
        "HoleCutDepth",
        "HoleCutCountersinkAngle",
        "DrillPointAngle",
        "TaperedAngle",
        "ThreadPitch",
        "ThreadDiameter",
        "ThreadDepth",
        "CustomThreadClearance",
    ];
    let owner = feature_id(ctx, object)?;
    for property in ctx.admit_iter(properties, "fcstd operation parameter properties")? {
        let property = *property;
        if !NAMES.contains(&property.name.as_str()) {
            continue;
        }
        if ctx.any_by(
            parameters.as_slice(),
            |parameter| {
                let owner_matches = match parameter.owner.as_ref() {
                    Some(parameter_owner) => {
                        ctx.equal(parameter_owner, &owner, "fcstd operation parameter owner")?
                    }
                    None => false,
                };
                if !owner_matches {
                    return Ok(false);
                }
                ctx.equal(
                    &parameter.name,
                    &property.name,
                    "fcstd operation parameter duplicate name",
                )
            },
            "fcstd operation parameter duplicate search",
        )? {
            continue;
        }
        let Some(tag) = scalar_value_tag(&property.type_name) else {
            continue;
        };
        if tag == "Bool" {
            continue;
        }
        let Some((value, expression, retained)) =
            direct_root_value(ctx, property, tag, "value", |ctx, text| {
                let Some(value) = ctx
                    .parse_text::<f64>(text, "fcstd scalar property parse")?
                    .ok()
                    .and_then(FiniteReal::new)
                else {
                    return Ok(None);
                };
                let mut retained = BTreeMap::new();
                let expression = match expression_binding(ctx, properties, &property.name)? {
                    Some((native_ref, expression)) => {
                        ctx.insert_btree_map(
                            &mut retained,
                            cadmpeg_core::nonblank_literal!("expression_native_ref"),
                            native_ref,
                            "fcstd operation expression properties",
                        )?;
                        expression
                    }
                    None => ctx.copy_retained_text(text, "fcstd operation scalar expression")?,
                };
                Ok(Some((value, expression, retained)))
            })?
        else {
            continue;
        };
        let is_angle = property.type_name == "App::PropertyAngle";
        ctx.push_vec(
            parameters,
            DesignParameter {
                id: ParameterId::mint(design_identity_text(
                    ctx,
                    "parameter",
                    object,
                    format_args!(":{}", property.name),
                    "fcstd operation parameter identity",
                )?)
                .map_err(CodecError::malformed)?,
                owner: Some(owner.try_clone_for_decode(ctx, "fcstd operation parameter owner")?),
                ordinal: u32::try_from(property.order).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "FreeCAD ordinal",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(property.order),
                    )
                })?,
                name: ctx.copy_retained_text(&property.name, "fcstd operation parameter name")?,
                expression,
                display: None,
                value: if is_angle {
                    cadmpeg_ir::scalar::Angle::new(value.get().to_radians())
                        .map(ParameterValue::Angle)
                } else {
                    Some(ParameterValue::Length(Length::from_assigned_real(value)))
                },
                dependencies: DistinctMembers::default(),
                properties: retained,
                pmi: None,
                native_ref: Some(ctx.copy_retained_text(
                    &property.id,
                    "fcstd operation parameter native reference",
                )?),
            },
            "fcstd operation parameters",
        )?;
    }
    Ok(())
}

struct SketchTransfer {
    sketch: Sketch,
    entities: Vec<SketchEntity>,
    constraints: Vec<SketchConstraint>,
    parameters: Vec<DesignParameter>,
}

fn sketch_carrier<'a, 'input>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'a, 'input>,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut carrier = None;
    let mut xml_nodes_7 = node.children();
    while let Some(child) = ctx.next_charged(&mut xml_nodes_7, "FreeCAD design XML traversal")? {
        if !child.is_element() {
            continue;
        }
        let tag = child.tag_name().name();
        if (tag == "Construction") || (tag == "GeoExtensions") || (tag == "UID") {
            continue;
        }
        if carrier.is_some() {
            return Ok(None);
        }
        carrier = Some(child);
    }
    Ok(carrier)
}

fn validate_sketch_carrier(
    ctx: &DecodeContext<'_>,
    kind: &str,
    carrier: &roxmltree::Node<'_, '_>,
    ordinal: usize,
) -> Result<(), CodecError> {
    let Some(expected) = (match kind {
        "Part::GeomLine" => Some("GeomLine"),
        "Part::GeomLineSegment" => Some("LineSegment"),
        "Part::GeomCircle" => Some("Circle"),
        "Part::GeomArcOfCircle" => Some("ArcOfCircle"),
        "Part::GeomEllipse" => Some("Ellipse"),
        "Part::GeomArcOfEllipse" => Some("ArcOfEllipse"),
        "Part::GeomHyperbola" => Some("Hyperbola"),
        "Part::GeomArcOfHyperbola" => Some("ArcOfHyperbola"),
        "Part::GeomParabola" => Some("Parabola"),
        "Part::GeomArcOfParabola" => Some("ArcOfParabola"),
        "Part::GeomPoint" => Some("GeomPoint"),
        "Part::GeomBSplineCurve" => Some("BSplineCurve"),
        _ => None,
    }) else {
        return Ok(());
    };
    if carrier.tag_name().name() == expected {
        return Ok(());
    }
    Err(malformed_design(
        ctx,
        format_args!(
        "sketch Geometry record {ordinal} declares {kind} but carries <{}>, expected <{expected}>",
        carrier.tag_name().name()
    ),
    ))
}

fn external_geometry_metadata(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    ordinal: usize,
) -> Result<(Option<String>, bool), CodecError> {
    let mut extension = None;
    let mut xml_nodes_8 = node.children();
    while let Some(container) =
        ctx.next_charged(&mut xml_nodes_8, "FreeCAD design XML traversal")?
    {
        if !ctx.xml_has_tag_name(container, "GeoExtensions", "FreeCAD design XML tag")? {
            continue;
        }
        let mut xml_nodes_9 = container.children();
        while let Some(candidate) =
            ctx.next_charged(&mut xml_nodes_9, "FreeCAD design XML traversal")?
        {
            if !ctx.xml_has_tag_name(candidate, "GeoExtension", "FreeCAD design XML tag")? {
                continue;
            }
            let Some(kind) =
                ctx.xml_attribute(candidate, "type", "FreeCAD design XML attribute")?
            else {
                continue;
            };
            if kind != "Sketcher::ExternalGeometryExtension" {
                continue;
            }
            if extension.is_some() {
                return Err(malformed_design(ctx, format_args!(
                    "sketch ExternalGeo Geometry record {ordinal} has multiple ExternalGeometryExtension values"
                )));
            }
            extension = Some(candidate);
        }
    }
    let extension_ref = match extension {
        Some(extension) => ctx.xml_attribute(extension, "Ref", "FreeCAD design XML attribute")?,
        None => None,
    };
    let geometry_ref = ctx.xml_attribute(node, "ref", "FreeCAD design XML attribute")?;
    if let (Some(extension_ref), Some(geometry_ref)) = (extension_ref, geometry_ref) {
        if !ctx.equal(
            extension_ref,
            geometry_ref,
            "fcstd external geometry reference",
        )? {
            return Err(malformed_design(
                ctx,
                format_args!(
                    "sketch ExternalGeo Geometry record {ordinal} has conflicting Ref and ref values"
                ),
            ));
        }
    }
    let reference = match extension_ref.or(geometry_ref) {
        Some(value) if !value.is_empty() => {
            Some(ctx.copy_retained_text(value, "fcstd external geometry reference")?)
        }
        _ => None,
    };
    let extension_flags = match extension {
        Some(extension) => {
            match ctx.xml_attribute(extension, "Flags", "FreeCAD design XML attribute")? {
                Some(value) => Some(
                    ctx.parse_text::<u64>(value, "fcstd external geometry flags")?
                        .map_err(|_| {
                            malformed_design(
                                ctx,
                                format_args!(
                                "sketch ExternalGeo Geometry record {ordinal} has invalid Flags"
                            ),
                            )
                        })?,
                ),
                None => None,
            }
        }
        None => None,
    };
    let geometry_flags = match ctx.xml_attribute(node, "flags", "FreeCAD design XML attribute")? {
        Some(value) => Some(
            ctx.parse_text::<u64>(value, "fcstd external geometry flags")?
                .map_err(|_| {
                    malformed_design(
                        ctx,
                        format_args!(
                            "sketch ExternalGeo Geometry record {ordinal} has invalid flags"
                        ),
                    )
                })?,
        ),
        None => None,
    };
    if let (Some(extension_flags), Some(geometry_flags)) = (extension_flags, geometry_flags) {
        if extension_flags != geometry_flags {
            return Err(malformed_design(ctx, format_args!(
                "sketch ExternalGeo Geometry record {ordinal} has conflicting Flags and flags values"
            )));
        }
    }
    let flags = extension_flags.or(geometry_flags).unwrap_or_default();
    Ok((reference, flags & EXTERNAL_GEOMETRY_MISSING_FLAG != 0))
}

fn validate_external_geo_prefix(
    ctx: &DecodeContext<'_>,
    records: &[roxmltree::Node<'_, '_>],
    owner: &str,
) -> Result<(), CodecError> {
    if records.len() < EXTERNAL_GEO_AXIS_COUNT {
        return Err(malformed_design(
            ctx,
            format_args!("{owner} must contain the two reserved ExternalGeo axis records"),
        ));
    }
    for (index, (expected_value, expected_label)) in
        [(-1_i64, "-1"), (-2_i64, "-2")].into_iter().enumerate()
    {
        let node = records[index];
        let id = ctx
            .xml_attribute(node, "id", "FreeCAD design XML attribute")?
            .ok_or_else(|| {
                malformed_design(
                    ctx,
                    format_args!(
                        "{owner} reserved ExternalGeo record {} has no id",
                        index + 1
                    ),
                )
            })?;
        if ctx
            .parse_text::<i64>(id, "fcstd external geometry record id")?
            .ok()
            != Some(expected_value)
        {
            return Err(malformed_design(
                ctx,
                format_args!(
                    "{owner} reserved ExternalGeo record {} has id {id}, expected {expected_label}",
                    index + 1
                ),
            ));
        }
        let (reference, _) = external_geometry_metadata(ctx, node, index + 1)?;
        if reference.is_some() {
            return Err(malformed_design(
                ctx,
                format_args!(
                    "{owner} reserved ExternalGeo record {} has an external reference",
                    index + 1
                ),
            ));
        }
    }
    Ok(())
}

fn external_link_key(
    ctx: &DecodeContext<'_>,
    reference: &crate::native::LinkTarget,
) -> Result<Option<String>, CodecError> {
    let Some(object) = reference.object() else {
        return Ok(None);
    };
    let Some(subelement) = reference.subelements().first() else {
        return Ok(None);
    };
    let parent_key = ctx
        .split_once(object, "#", "fcstd external link parent key")?
        .map_or(object, |(_, key)| key);
    Ok(Some(ctx.join_retained(
        &[parent_key, subelement.as_str()],
        ".",
        "fcstd external link key",
    )?))
}

fn external_link_indices(
    ctx: &DecodeContext<'_>,
    references: Option<&PropertyRecord>,
) -> Result<HashMap<String, usize>, CodecError> {
    let mut indices = HashMap::new();
    if let Some(references) = references {
        for (index, reference) in ctx
            .admit_iter(references.links(), "fcstd external link index")?
            .enumerate()
        {
            let Some(key) = reference
                .as_ref()
                .map(|reference| external_link_key(ctx, reference))
                .transpose()?
                .flatten()
            else {
                continue;
            };
            if ctx.contains_key_hash_map(&indices, &key, "fcstd external link duplicate")? {
                return Err(malformed_design(
                    ctx,
                    format_args!("sketch ExternalGeometry links contain duplicate key {key}"),
                ));
            }
            ctx.insert_hash_map(&mut indices, key, index, "fcstd external link index")?;
        }
    }
    Ok(indices)
}

fn sketch_attributes(
    ctx: &DecodeContext<'_>,
    carrier: Option<roxmltree::Node<'_, '_>>,
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut attributes = BTreeMap::new();
    if let Some(carrier) = carrier {
        let mut source = carrier.attributes();
        while let Some(attribute) =
            ctx.next_charged(&mut source, "FreeCAD sketch carrier attributes")?
        {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text(attribute.name(), "fcstd sketch attribute name")?,
                ctx.copy_retained_text(attribute.value(), "fcstd sketch attribute value")?,
                "fcstd sketch carrier attributes",
            )?;
        }
    }
    Ok(attributes)
}

fn parse_sketch(
    ctx: &DecodeContext<'_>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<SketchTransfer, CodecError> {
    let id = SketchId::mint(design_identity_text(
        ctx,
        "sketch",
        object,
        format_args!(""),
        "fcstd design sketch identity",
    )?)
    .map_err(CodecError::malformed)?;
    let mut entities = Vec::new();
    let mut matched_references = BTreeSet::new();
    let mut matched_storage = ctx.reserve_scoped(0, "fcstd matched reference storage")?;
    if let Some(geometry) = property(ctx, properties, "Geometry")? {
        if geometry.type_name != "Part::PropertyGeometryList" {
            return Err(malformed_design(
                ctx,
                format_args!(
                    "{} has runtime type {}, expected Part::PropertyGeometryList",
                    geometry.id, geometry.type_name
                ),
            ));
        }
        let admitted_xml = ctx
            .parse_xml(geometry.xml.text(), "FreeCAD XML tree")
            .map_err(|error| {
                let CodecError::Malformed(error) = error else {
                    return error;
                };
                malformed_design(
                    ctx,
                    format_args!("invalid sketch geometry {}: {error}", geometry.id),
                )
            })?;
        let xml = admitted_xml.document();
        let (records, _record_storage) =
            direct_counted_records(ctx, xml, "GeometryList", "Geometry", &geometry.id)?;
        for (index, node) in ctx
            .admit_iter(&records, "FreeCAD sketch geometry records")?
            .copied()
            .enumerate()
        {
            let carrier = sketch_carrier(ctx, node)?;
            let kind = ctx.xml_attribute(node, "type", "FreeCAD design XML attribute")?;
            if let (Some(kind), Some(carrier)) = (kind, carrier.as_ref()) {
                validate_sketch_carrier(ctx, kind, carrier, index + 1)?;
            }
            let native_kind = kind
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown");
            let geometry_value = match carrier
                .map(|carrier| sketch_nurbs(ctx, native_kind, carrier))
                .transpose()?
                .flatten()
            {
                Some(nurbs) => nurbs,
                None => {
                    let (attributes, _attribute_storage) = ctx
                        .with_scoped_storage("fcstd sketch attribute storage", || {
                            sketch_attributes(ctx, carrier)
                        })?;
                    sketch_geometry(ctx, native_kind, &attributes)?
                }
            };
            let mut construction = false;
            let mut xml_nodes_10 = node.descendants();
            while let Some(child) =
                ctx.next_charged(&mut xml_nodes_10, "FreeCAD design XML traversal")?
            {
                if ctx.xml_has_tag_name(child, "Construction", "FreeCAD design XML tag")? {
                    construction = ctx
                        .xml_attribute(child, "value", "FreeCAD design XML attribute")?
                        .map(|value| Ok::<_, CodecError>(value != "0"))
                        .transpose()?
                        .unwrap_or(false);
                    if construction {
                        break;
                    }
                }
            }
            ctx.push_vec(
                &mut entities,
                SketchEntity::new(
                    SketchEntityId::mint(design_identity_text(
                        ctx,
                        "sketch-entity",
                        object,
                        format_args!(":{}", index + 1),
                        "fcstd sketch geometry identity",
                    )?)
                    .map_err(CodecError::malformed)?,
                    id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                    geometry_value,
                )
                .with_construction(construction)
                .with_native_ref(Some(
                    ctx.copy_retained_text(&geometry.id, "fcstd sketch geometry native reference")?,
                )),
                "fcstd sketch entities",
            )?;
        }
    }
    if let Some(external_geometry) = property(ctx, properties, "ExternalGeo")? {
        if external_geometry.type_name != "Part::PropertyGeometryList" {
            return Err(malformed_design(
                ctx,
                format_args!(
                    "{} has runtime type {}, expected Part::PropertyGeometryList",
                    external_geometry.id, external_geometry.type_name
                ),
            ));
        }
        let admitted_xml = ctx
            .parse_xml(external_geometry.xml.text(), "FreeCAD XML tree")
            .map_err(|error| {
                let CodecError::Malformed(error) = error else {
                    return error;
                };
                malformed_design(
                    ctx,
                    format_args!(
                        "invalid external sketch geometry {}: {error}",
                        external_geometry.id
                    ),
                )
            })?;
        let xml = admitted_xml.document();
        let (records, _record_storage) =
            direct_counted_records(ctx, xml, "GeometryList", "Geometry", &external_geometry.id)?;
        validate_external_geo_prefix(ctx, &records, &external_geometry.id)?;
        let references = property(ctx, properties, "ExternalGeometry")?;
        if let Some(references) = references {
            if references.type_name != "App::PropertyLinkSubList" {
                return Err(malformed_design(
                    ctx,
                    format_args!(
                        "{} has runtime type {}, expected App::PropertyLinkSubList",
                        references.id, references.type_name
                    ),
                ));
            }
        }
        let (link_indices, _link_storage) = ctx
            .with_scoped_storage("fcstd external link index storage", || {
                external_link_indices(ctx, references)
            })?;
        for (external_index, node) in ctx
            .admit_iter(&records, "FreeCAD external sketch geometry records")?
            .copied()
            .skip(EXTERNAL_GEO_AXIS_COUNT)
            .enumerate()
        {
            let ((cache_reference, missing), _cache_storage) = ctx
                .with_scoped_storage("fcstd external cache reference storage", || {
                    external_geometry_metadata(ctx, node, external_index + 3)
                })?;
            let reference_index = match cache_reference.as_deref() {
                Some(cache_reference) => ctx
                    .get_hash_map(
                        &link_indices,
                        cache_reference,
                        "fcstd external geometry link lookup",
                    )?
                    .copied(),
                None => None,
            };
            if let (Some(cache_reference), None) = (cache_reference.as_deref(), reference_index) {
                if !missing {
                    return Err(malformed_design(ctx, format_args!(
                        "sketch ExternalGeo Geometry record {} reference {cache_reference} has no matching ExternalGeometry link",
                        external_index + 3
                    )));
                }
            }
            if let Some(reference_index) = reference_index {
                matched_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut matched_references,
                        reference_index,
                        "fcstd sketch matched references",
                    )
                })?;
            }
            let carrier = sketch_carrier(ctx, node)?;
            let kind = ctx.xml_attribute(node, "type", "FreeCAD design XML attribute")?;
            if let (Some(kind), Some(carrier)) = (kind, carrier.as_ref()) {
                validate_sketch_carrier(ctx, kind, carrier, external_index + 3)?;
            }
            let native_kind = kind
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown");
            let geometry = match carrier
                .map(|carrier| sketch_nurbs(ctx, native_kind, carrier))
                .transpose()?
                .flatten()
            {
                Some(nurbs) => nurbs,
                None => {
                    let (attributes, _attribute_storage) = ctx
                        .with_scoped_storage("fcstd sketch attribute storage", || {
                            sketch_attributes(ctx, carrier)
                        })?;
                    sketch_geometry(ctx, native_kind, &attributes)?
                }
            };
            ctx.push_vec(
                &mut entities,
                SketchEntity::new(
                    SketchEntityId::mint(design_identity_text(
                        ctx,
                        "sketch-entity",
                        object,
                        format_args!(":external:{external_index}"),
                        "fcstd sketch external geometry identity",
                    )?)
                    .map_err(CodecError::malformed)?,
                    id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                    geometry,
                )
                .with_construction(true)
                .with_native_ref(Some(ctx.copy_retained_text(
                    &external_geometry.id,
                    "fcstd external geometry native reference",
                )?))
                .with_geometry_ref(
                    references
                        .map(|property| {
                            ctx.copy_retained_text(
                                &property.id,
                                "fcstd external geometry reference property",
                            )
                        })
                        .transpose()?,
                )
                .with_endpoint_refs(
                    reference_index
                        .and_then(|index| {
                            references.and_then(|property| property.links().get(index))
                        })
                        .and_then(Option::as_ref)
                        .map(|reference| {
                            ctx.copy_retained_strings(
                                reference.subelements(),
                                "fcstd sketch external endpoint refs",
                            )
                        })
                        .transpose()?
                        .unwrap_or_default(),
                ),
                "fcstd sketch entities",
            )?;
        }
    }
    if let Some(references) = property(ctx, properties, "ExternalGeometry")? {
        for (external_index, reference) in ctx
            .admit_iter(references.links(), "fcstd unmatched external links")?
            .enumerate()
        {
            if ctx.contains_btree_set(
                &matched_references,
                &external_index,
                "fcstd matched external link lookup",
            )? {
                continue;
            }
            let Some(reference) = reference.as_ref() else {
                continue;
            };
            let Some(target_object) = reference.object() else {
                continue;
            };
            let (numeric_suffix, _numeric_suffix_storage) = ctx.format_scoped(
                format_args!(":external:{external_index}"),
                "fcstd external link identity suffix",
            )?;
            let entity_kind = if ctx.any_by(
                &entities,
                |entity| {
                    ctx.ends_with(
                        entity.id().as_str(),
                        &numeric_suffix,
                        "fcstd external link identity suffix",
                    )
                },
                "fcstd external link identity lookup",
            )? {
                "external-link"
            } else {
                "external"
            };
            ctx.push_vec(
                &mut entities,
                SketchEntity::new(
                    SketchEntityId::mint(design_identity_text(
                        ctx,
                        "sketch-entity",
                        object,
                        format_args!(":{entity_kind}:{external_index}"),
                        "fcstd sketch external link identity",
                    )?)
                    .map_err(CodecError::malformed)?,
                    id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                    SketchGeometry::try_from(SketchGeometryDefinition::ExternalReference {
                        document: reference
                            .document_name()
                            .map(|name| {
                                ctx.copy_retained_text(name, "fcstd sketch external document")
                            })
                            .transpose()?,
                        object: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.copy_retained_text(target_object, "fcstd sketch external object")?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("object must not be empty")
                        })?,
                        subelements: ctx.copy_retained_strings(
                            reference.subelements(),
                            "fcstd sketch external subelements",
                        )?,
                    })
                    .map_err(CodecError::malformed)?,
                )
                .with_construction(true)
                .with_native_ref(Some(ctx.copy_retained_text(
                    &references.id,
                    "fcstd sketch external native reference",
                )?))
                .with_geometry_ref(Some(ctx.copy_retained_text(
                    &references.id,
                    "fcstd sketch external geometry reference",
                )?))
                .with_endpoint_refs(ctx.copy_retained_strings(
                    reference.subelements(),
                    "fcstd sketch external endpoint refs",
                )?),
                "fcstd sketch entities",
            )?;
        }
    }
    let constraint_source = constraint_xml(ctx, properties)?;
    let (horizontal_axis, vertical_axis, root_point) = builtin_reference_usage(
        ctx,
        constraint_source.as_ref().map(|(_, tree)| tree.document()),
    )?;
    if horizontal_axis {
        ctx.push_vec(
            &mut entities,
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx,
                    "sketch-entity",
                    object,
                    format_args!(":reference-horizontal-axis"),
                    "fcstd sketch horizontal axis identity",
                )?)
                .map_err(CodecError::malformed)?,
                id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                })
                .map_err(CodecError::malformed)?,
            )
            .with_construction(true)
            .with_native_ref(Some(
                ctx.copy_retained_text(object.id(), "fcstd sketch axis native reference")?,
            )),
            "fcstd sketch entities",
        )?;
    }
    if vertical_axis {
        ctx.push_vec(
            &mut entities,
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx,
                    "sketch-entity",
                    object,
                    format_args!(":reference-vertical-axis"),
                    "fcstd sketch vertical axis identity",
                )?)
                .map_err(CodecError::malformed)?,
                id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(0.0, 1.0),
                })
                .map_err(CodecError::malformed)?,
            )
            .with_construction(true)
            .with_native_ref(Some(
                ctx.copy_retained_text(object.id(), "fcstd sketch axis native reference")?,
            )),
            "fcstd sketch entities",
        )?;
    }
    if root_point {
        ctx.push_vec(
            &mut entities,
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx,
                    "sketch-entity",
                    object,
                    format_args!(":reference-root-point"),
                    "fcstd sketch root point identity",
                )?)
                .map_err(CodecError::malformed)?,
                id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(0.0, 0.0),
                })
                .map_err(CodecError::malformed)?,
            )
            .with_construction(true)
            .with_native_ref(Some(
                ctx.copy_retained_text(object.id(), "fcstd sketch axis native reference")?,
            )),
            "fcstd sketch entities",
        )?;
    }
    let (constraints, parameters) = parse_constraints(
        ctx,
        object,
        properties,
        &id,
        &entities,
        constraint_source
            .as_ref()
            .map(|(property, tree)| (*property, tree.document())),
    )?;
    let profiles = profiles::build_profiles(ctx, &entities, &constraints)?;
    let (origin, normal, u_axis) = sketch_frame(ctx, properties)?;
    Ok(SketchTransfer {
        sketch: Sketch {
            id,
            name: Some(ctx.copy_retained_text(object.name(), "fcstd sketch name")?),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, normal, u_axis)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            native_ref: Some(ctx.copy_retained_text(object.id(), "fcstd sketch native reference")?),
        },
        entities,
        constraints,
        parameters,
    })
}

fn builtin_reference_usage(
    ctx: &DecodeContext<'_>,
    source: Option<&roxmltree::Document<'_>>,
) -> Result<(bool, bool, bool), CodecError> {
    let Some(xml) = source else {
        return Ok((false, false, false));
    };
    let mut horizontal = false;
    let mut vertical = false;
    let mut root = false;
    let document_root = ctx.xml_root_element(xml, "FreeCAD design XML root_element")?;
    let mut xml_nodes_11 = document_root.descendants();
    while let Some(node) = ctx.next_charged(&mut xml_nodes_11, "FreeCAD design XML traversal")? {
        if !ctx.xml_has_tag_name(node, "Constrain", "FreeCAD design XML tag")? {
            continue;
        }
        let type_code = int_attr(ctx, node, "Type")?;
        let (operands, _operand_storage) = match ctx
            .with_scoped_storage("fcstd built-in constraint operand storage", || {
                constraint_operands(ctx, node)
            }) {
            Ok(operands) => operands,
            Err(CodecError::ResourceLimit(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(_) => continue,
        };
        root |= matches!(type_code, Some(7 | 8)) && operands.len() == 1;
        for (entity, position) in ctx
            .admit_iter(&operands, "FreeCAD sketch constraint operands")?
            .copied()
        {
            if type_code == Some(9) {
                horizontal |= entity == -1;
                vertical |= entity == -2;
                if matches!(entity, -1 | -2) {
                    continue;
                }
            }
            horizontal |= entity == -1 && position == 0;
            root |= matches!(entity, -1 | -2) && position == 1;
            vertical |= entity == -2 && position == 0;
        }
    }
    Ok((horizontal, vertical, root))
}

/// Lanes of a sketch B-spline record, as the source states them.
struct SketchNurbsLanes {
    degree: u32,
    knots: KnotVector,
    control_points: Vec<FinitePoint2>,
    weights: Option<Vec<NonZeroReal>>,
    periodic: bool,
}

/// Read a sketch B-spline record. `Ok(None)` states the record is not a
/// B-spline; `Err` states a B-spline record whose lanes the carrier refuses.
fn sketch_nurbs(
    ctx: &DecodeContext<'_>,
    kind: &str,
    node: roxmltree::Node<'_, '_>,
) -> Result<Option<SketchGeometry>, CodecError> {
    let Some(lanes) = sketch_nurbs_lanes(ctx, kind, node)? else {
        return Ok(None);
    };
    let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
        ctx,
        lanes.degree,
        lanes.knots,
        lanes.control_points,
        lanes.weights,
        lanes.periodic,
    )?;
    let curve = match curve {
        Ok(curve) => curve,
        Err(cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit)) => {
            return Err(CodecError::ResourceLimit(limit));
        }
        Err(error) => return Err(malformed_design(ctx, format_args!("{error}"))),
    };
    Ok(Some(SketchGeometry::nurbs(curve)))
}

fn sketch_nurbs_lanes(
    ctx: &DecodeContext<'_>,
    kind: &str,
    node: roxmltree::Node<'_, '_>,
) -> Result<Option<SketchNurbsLanes>, CodecError> {
    let is_spline_kind = (kind == "Part::GeomBSplineCurve") || (kind == "BSplineCurve");
    if !is_spline_kind && !ctx.xml_has_tag_name(node, "BSplineCurve", "FreeCAD design XML tag")? {
        return Ok(None);
    }
    let Some(degree_text) = ctx.xml_attribute(node, "Degree", "FreeCAD design XML attribute")?
    else {
        return Ok(None);
    };
    let Ok(degree) = ctx.parse_text::<u32>(degree_text, "fcstd sketch NURBS degree")? else {
        return Ok(None);
    };
    let Some(periodic_text) =
        ctx.xml_attribute(node, "IsPeriodic", "FreeCAD design XML attribute")?
    else {
        return Ok(None);
    };
    let periodic = (periodic_text == "1") || (periodic_text == "true") || (periodic_text == "True");
    let Some(pole_count_text) =
        ctx.xml_attribute(node, "PolesCount", "FreeCAD design XML attribute")?
    else {
        return Ok(None);
    };
    let Ok(pole_count) =
        ctx.parse_text::<usize>(pole_count_text, "fcstd sketch NURBS pole count")?
    else {
        return Ok(None);
    };
    let Some(knot_count_text) =
        ctx.xml_attribute(node, "KnotsCount", "FreeCAD design XML attribute")?
    else {
        return Ok(None);
    };
    let Ok(knot_count) =
        ctx.parse_text::<usize>(knot_count_text, "fcstd sketch NURBS knot count")?
    else {
        return Ok(None);
    };
    if pole_count == 0
        || knot_count == 0
        || pole_count > MAX_SKETCH_RECORDS
        || knot_count > MAX_SKETCH_RECORDS
    {
        return Ok(None);
    }
    let mut lane_storage = ctx.reserve_scoped(0, "fcstd sketch NURBS lane storage")?;
    let mut poles =
        lane_storage.with_storage(|| ctx.vector_storage(pole_count, "fcstd sketch NURBS poles"))?;
    let mut xml_nodes_13 = node.children();
    while let Some(pole) = ctx.next_charged(&mut xml_nodes_13, "FreeCAD design XML traversal")? {
        if !ctx.xml_has_tag_name(pole, "Pole", "FreeCAD design XML tag")? {
            continue;
        }
        let Some(x_text) = ctx.xml_attribute(pole, "X", "FreeCAD design XML attribute")? else {
            return Ok(None);
        };
        let Ok(x) = ctx.parse_text::<f64>(x_text, "fcstd sketch NURBS pole coordinate")? else {
            return Ok(None);
        };
        let Some(y_text) = ctx.xml_attribute(pole, "Y", "FreeCAD design XML attribute")? else {
            return Ok(None);
        };
        let Ok(y) = ctx.parse_text::<f64>(y_text, "fcstd sketch NURBS pole coordinate")? else {
            return Ok(None);
        };
        let Some(point) = FinitePoint2::new(Point2::new(x, y)) else {
            return Ok(None);
        };
        let Some(z_text) = ctx.xml_attribute(pole, "Z", "FreeCAD design XML attribute")? else {
            return Ok(None);
        };
        let Ok(z) = ctx.parse_text::<f64>(z_text, "fcstd sketch NURBS pole coordinate")? else {
            return Ok(None);
        };
        let Some(z) = FiniteReal::new(z) else {
            return Ok(None);
        };
        if z.get().abs() > f64::EPSILON {
            return Ok(None);
        }
        let Some(weight_text) =
            ctx.xml_attribute(pole, "Weight", "FreeCAD design XML attribute")?
        else {
            return Ok(None);
        };
        let Ok(weight) = ctx.parse_text::<f64>(weight_text, "fcstd sketch NURBS pole weight")?
        else {
            return Ok(None);
        };
        let Some(weight) = PositiveReal::new(weight) else {
            return Ok(None);
        };
        lane_storage.with_storage(|| {
            ctx.push_vec(&mut poles, (point, weight), "fcstd sketch NURBS poles")
        })?;
    }
    let mut knots =
        lane_storage.with_storage(|| ctx.vector_storage(knot_count, "fcstd sketch NURBS knots"))?;
    let mut xml_nodes_15 = node.children();
    while let Some(knot) = ctx.next_charged(&mut xml_nodes_15, "FreeCAD design XML traversal")? {
        if !ctx.xml_has_tag_name(knot, "Knot", "FreeCAD design XML tag")? {
            continue;
        }
        let Some(value_text) = ctx.xml_attribute(knot, "Value", "FreeCAD design XML attribute")?
        else {
            return Ok(None);
        };
        let Ok(value) = ctx.parse_text::<f64>(value_text, "fcstd sketch NURBS knot value")? else {
            return Ok(None);
        };
        let Some(value) = FiniteReal::new(value) else {
            return Ok(None);
        };
        let Some(multiplicity_text) =
            ctx.xml_attribute(knot, "Mult", "FreeCAD design XML attribute")?
        else {
            return Ok(None);
        };
        let Ok(multiplicity) =
            ctx.parse_text::<usize>(multiplicity_text, "fcstd sketch NURBS knot multiplicity")?
        else {
            return Ok(None);
        };
        lane_storage.with_storage(|| {
            ctx.push_vec(
                &mut knots,
                (value, multiplicity),
                "fcstd sketch NURBS knots",
            )
        })?;
    }
    if poles.len() != pole_count
        || knots.len() != knot_count
        || degree == 0
        || usize::try_from(degree)
            .ok()
            .is_none_or(|degree| degree >= pole_count)
        || ctx.any_by(
            &knots,
            |(_, multiplicity)| Ok(*multiplicity == 0 || *multiplicity > MAX_SKETCH_RECORDS),
            "fcstd sketch NURBS knot multiplicities",
        )?
    {
        return Ok(None);
    }
    let Some(first_knot) = knots.first() else {
        return Ok(None);
    };
    let mut previous_knot = first_knot.0;
    if ctx.any_by(
        &knots[1..],
        |(knot, _)| {
            let unordered = previous_knot.get() >= knot.get();
            previous_knot = *knot;
            Ok(unordered)
        },
        "fcstd sketch NURBS knot order",
    )? {
        return Ok(None);
    }
    let mut expanded_count = 0_usize;
    for (_, multiplicity) in ctx.admit_iter(&knots, "fcstd sketch NURBS expanded knot count")? {
        let Some(next) = expanded_count.checked_add(*multiplicity) else {
            return Ok(None);
        };
        expanded_count = next;
    }
    if expanded_count > MAX_SKETCH_RECORDS {
        return Ok(None);
    }
    if !periodic {
        let Some(expected) = usize::try_from(degree)
            .ok()
            .and_then(|degree| pole_count.checked_add(degree))
            .and_then(|count| count.checked_add(1))
        else {
            return Ok(None);
        };
        if expanded_count != expected {
            return Ok(None);
        }
    }
    let mut full_knots = ctx.vector_storage(expanded_count, "fcstd sketch NURBS expanded knots")?;
    for (value, multiplicity) in ctx.admit_iter(&knots, "fcstd sketch NURBS knot expansion")? {
        for _ in ctx.admit_iter(&(0..*multiplicity), "fcstd sketch NURBS knot repetition")? {
            ctx.push_vec(&mut full_knots, *value, "fcstd sketch NURBS expanded knots")?;
        }
    }
    let mut control_points = ctx.vector_storage(pole_count, "fcstd sketch NURBS control points")?;
    let mut weights = lane_storage
        .with_storage(|| ctx.vector_storage(pole_count, "fcstd sketch NURBS weights"))?;
    for (point, weight) in ctx.admit_iter(&poles, "fcstd sketch NURBS pole lanes")? {
        ctx.push_vec(
            &mut control_points,
            *point,
            "fcstd sketch NURBS control points",
        )?;
        lane_storage
            .with_storage(|| ctx.push_vec(&mut weights, *weight, "fcstd sketch NURBS weights"))?;
    }
    let has_non_unit_weights = ctx.any_by(
        &weights,
        |weight| Ok((weight.get() - 1.0).abs() > f64::EPSILON),
        "fcstd sketch NURBS weight check",
    )?;
    let weights = if has_non_unit_weights {
        let mut converted = ctx.vector_storage(pole_count, "fcstd sketch NURBS nonzero weights")?;
        for weight in ctx.admit_iter(&weights, "fcstd sketch NURBS nonzero weights")? {
            ctx.push_vec(
                &mut converted,
                NonZeroReal::from(*weight),
                "fcstd sketch NURBS nonzero weights",
            )?;
        }
        Some(converted)
    } else {
        None
    };
    let Some(knots) = KnotVector::from_finite_lanes(ctx, full_knots)?.ok() else {
        return Ok(None);
    };
    Ok(Some(SketchNurbsLanes {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    }))
}

fn sketch_frame(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<(Point3, Vector3, Vector3), CodecError> {
    validate_sketch_placement(ctx, properties)?;
    Ok(placement_frame(ctx, properties)?.map_or_else(
        || {
            (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
        },
        |(origin, normal, x_axis, _)| (origin, normal, x_axis),
    ))
}

fn placement_frame(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<(Point3, Vector3, Vector3, Vector3)>, CodecError> {
    let property = match property(ctx, properties, "Placement")? {
        Some(property) => Some(property),
        None => property(ctx, properties, "AttachmentOffset")?,
    };
    let Some(property) = property else {
        return Ok(None);
    };
    let Some(matrix) = crate::placement::placement_matrix_unreported(property) else {
        return Ok(None);
    };
    let matrix = matrix.rows();
    let column = |index| Vector3::new(matrix[0][index], matrix[1][index], matrix[2][index]);
    Ok(Some((
        Point3::new(matrix[0][3], matrix[1][3], matrix[2][3]),
        column(2),
        column(0),
        column(1),
    )))
}

fn validate_sketch_placement(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<(), CodecError> {
    let property = match property(ctx, properties, "Placement")? {
        Some(property) => Some(property),
        None => property(ctx, properties, "AttachmentOffset")?,
    };
    let Some(property) = property else {
        return Ok(());
    };
    let error = if property.type_name != "App::PropertyPlacement" {
        Some(ctx.format_retained(
            format_args!(
                "sketch {} placement carrier has runtime type {}",
                property.name, property.type_name
            ),
            "FreeCAD sketch placement error",
        )?)
    } else if property.values().len() != 1 || property.values()[0].tag != "PropertyPlacement" {
        Some(ctx.format_retained(
            format_args!(
                "sketch {} placement carrier requires one PropertyPlacement value",
                property.name
            ),
            "FreeCAD sketch placement error",
        )?)
    } else if placement_frame(ctx, properties)?.is_none() {
        Some(ctx.format_retained(
            format_args!(
                "sketch {} placement carrier has incomplete or invalid components",
                property.name
            ),
            "FreeCAD sketch placement error",
        )?)
    } else {
        None
    };
    if let Some(message) = error {
        return Err(CodecError::Malformed(message));
    }
    Ok(())
}

fn feature_state(
    ctx: &DecodeContext<'_>,
    object: &str,
    properties: &[&PropertyRecord],
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    const STATE_NAMES: &[&str] = &[
        "Active",
        "Frozen",
        "Invalid",
        "MapMode",
        "Support",
        "Suppressed",
        "Tip",
        "Touched",
        "Visibility",
    ];
    let mut state = BTreeMap::new();
    for property in ctx.admit_iter(properties, "fcstd feature state properties")? {
        if !STATE_NAMES.contains(&property.name.as_str()) {
            continue;
        }
        let value = if let Some(link) = property
            .links()
            .first()
            .and_then(|link| link.as_ref()?.object())
        {
            ctx.copy_retained_text(link, "fcstd feature state value")?
        } else if let Some(value) = scalar_text(ctx, property, |ctx, text| {
            Ok(Some(
                ctx.copy_retained_text(text, "fcstd feature state value")?,
            ))
        })? {
            value
        } else {
            ctx.copy_retained_text(property.xml.text(), "fcstd feature state value")?
        };
        let name = ctx.copy_retained_text(&property.name, "fcstd feature state name")?;
        let Some(name) = NonBlankString::for_decode(ctx, name, "validate nonblank text")? else {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("{object} states a property with a blank key"),
                "fcstd feature state blank key error",
            ));
        };
        if ctx.contains_key_btree_map(&state, &name, "fcstd feature state duplicate key")? {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("{object} states the property {name} a second time"),
                "fcstd feature state duplicate key error",
            ));
        }
        ctx.insert_btree_map(&mut state, name, value, "fcstd feature state properties")?;
    }
    Ok(state)
}

fn bool_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<bool>, CodecError> {
    scalar_text(
        ctx,
        required!(property(ctx, properties, name)?),
        |_ctx, value| {
            if (value == "1") || value.eq_ignore_ascii_case("true") {
                Ok(Some(true))
            } else if (value == "0") || value.eq_ignore_ascii_case("false") {
                Ok(Some(false))
            } else {
                Ok(None)
            }
        },
    )
}

/// Read an operation enumeration while keeping absence distinct from malformed persistence.
/// `FreeCAD` constructors provide the legacy default for an absent property; a present property
/// must use the exact enumeration carrier before its value can select neutral semantics.
fn enumeration_selector(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: u64,
) -> Result<Option<u64>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(Some(absent_default));
    };
    if property.type_name != "App::PropertyEnumeration" {
        return Ok(None);
    }
    let value = required!(direct_root_value(
        ctx,
        property,
        "Integer",
        "value",
        |ctx, value| Ok(ctx
            .parse_text::<i64>(value, "fcstd enumeration value")?
            .ok())
    )?);
    Ok(u64::try_from(value).ok())
}

/// Read a persisted boolean while keeping absence distinct from malformed persistence.
/// `FreeCAD` constructors provide the legacy default for an absent property; a present property
/// must use the exact boolean carrier before its value can select neutral semantics.
fn bool_selector(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: bool,
) -> Result<Option<bool>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(Some(absent_default));
    };
    direct_bool_value(ctx, property)
}

fn finite_float_selector(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    runtime_type: &str,
    absent_default: FiniteReal,
) -> Result<Option<FiniteReal>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(Some(absent_default));
    };
    if property.type_name.as_str() != runtime_type {
        return Ok(None);
    }
    let value = required!(direct_root_value(
        ctx,
        property,
        "Float",
        "value",
        |ctx, value| Ok(ctx
            .parse_text::<f64>(value, "fcstd numeric property value")?
            .ok())
    )?);
    Ok(FiniteReal::new(value))
}

fn direct_bool_value(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<bool>, CodecError> {
    if property.type_name != "App::PropertyBool" {
        return Ok(None);
    }
    direct_root_value(ctx, property, "Bool", "value", |_ctx, value| {
        if value == "true" {
            Ok(Some(true))
        } else if value == "false" {
            Ok(Some(false))
        } else {
            Ok(None)
        }
    })
}

fn direct_fuzzy_tolerance(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<FuzzyTolerance>, CodecError> {
    if property.type_name != "App::PropertyFloatConstraint" {
        return Ok(None);
    }
    let value = required!(direct_root_value(
        ctx,
        property,
        "Float",
        "value",
        |ctx, value| {
            Ok(ctx
                .parse_text::<f64>(value, "fcstd fuzzy tolerance value")?
                .ok()
                .and_then(FiniteReal::new))
        }
    )?);
    Ok(Some(
        match cadmpeg_ir::scalar::PositiveLength::from_assigned_real(value) {
            Some(explicit) => FuzzyTolerance::Explicit(explicit),
            None if value.get() < 0.0 => FuzzyTolerance::Automatic,
            None => FuzzyTolerance::KernelDefault,
        },
    ))
}

fn constraint_xml<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
) -> Result<
    Option<(
        &'a PropertyRecord,
        cadmpeg_core::decode::tree::AdmittedXml<'a, 'ctx>,
    )>,
    CodecError,
> {
    let Some(property) = property(ctx, properties, "Constraints")? else {
        return Ok(None);
    };
    if property.type_name != "Sketcher::PropertyConstraintList" {
        return Err(malformed_design(
            ctx,
            format_args!(
                "{} has runtime type {}, expected Sketcher::PropertyConstraintList",
                property.id, property.type_name
            ),
        ));
    }
    let admitted_xml = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            malformed_design(
                ctx,
                format_args!("invalid sketch constraints {}: {error}", property.id),
            )
        })?;
    Ok(Some((property, admitted_xml)))
}

fn parse_constraints(
    ctx: &DecodeContext<'_>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    sketch: &SketchId,
    entities: &[SketchEntity],
    source: Option<(&PropertyRecord, &roxmltree::Document<'_>)>,
) -> Result<(Vec<SketchConstraint>, Vec<DesignParameter>), CodecError> {
    let Some((property, xml)) = source else {
        return Ok((Vec::new(), Vec::new()));
    };
    let (records, _record_storage) =
        direct_counted_records(ctx, xml, "ConstraintList", "Constrain", &property.id)?;
    let mut constraints = Vec::new();
    let mut parameters = Vec::new();
    for (index, node) in ctx
        .admit_iter(&records, "FreeCAD sketch constraints")?
        .copied()
        .enumerate()
    {
        let (type_code, native_kind) =
            match ctx.xml_attribute(node, "Type", "FreeCAD design XML attribute")? {
                None => (
                    None,
                    ctx.copy_retained_text("missing_type", "fcstd sketch constraint type")?,
                ),
                Some(value) => {
                    match ctx.parse_text::<i64>(value, "fcstd sketch constraint type")? {
                        Ok(type_code) => (
                            Some(type_code),
                            ctx.copy_retained_text(
                                constraint_kind(type_code),
                                "fcstd sketch constraint type",
                            )?,
                        ),
                        Err(_) => (
                            None,
                            ctx.copy_retained_text(
                                "malformed_type",
                                "fcstd sketch constraint type",
                            )?,
                        ),
                    }
                }
            };
        let (operands, _operand_storage) = ctx
            .with_scoped_storage("fcstd constraint operand storage", || {
                constraint_operands(ctx, node)
            })
            .map_err(|error| match error {
                CodecError::Malformed(message) => malformed_design(
                    ctx,
                    format_args!("{} constraint {}: {message}", property.id, index + 1),
                ),
                error => error,
            })?;
        let resolve = |entity, position| {
            if type_code == Some(9) {
                match entity {
                    -1 => return resolve_operand(ctx, -1, 0, entities),
                    -2 => return resolve_operand(ctx, -2, 0, entities),
                    _ => {}
                }
            }
            resolve_operand(ctx, entity, position, entities)
        };
        let mut resolved_storage = ctx.reserve_scoped(0, "fcstd resolved constraint storage")?;
        let mut resolved = resolved_storage.with_storage(|| {
            ctx.vector_storage(operands.len(), "fcstd resolved constraint operands")
        })?;
        for (entity, position) in ctx
            .admit_iter(&operands, "fcstd resolved constraint operands")?
            .copied()
        {
            if let Some(locus) = resolved_storage.with_storage(|| resolve(entity, position))? {
                resolved_storage.with_storage(|| {
                    ctx.push_vec(&mut resolved, locus, "fcstd resolved constraint operands")
                })?;
            }
        }
        let all_resolved = resolved.len() == operands.len();
        if matches!(type_code, Some(7 | 8)) && operands.len() == 1 && resolved.len() == 1 {
            if let Some(root) = ctx.find_by(
                entities,
                |entity| Ok(entity.id().as_str().ends_with(":reference-root-point")),
                "fcstd reference root entity search",
            )? {
                resolved_storage.with_storage(|| {
                    ctx.insert_vec(
                        &mut resolved,
                        0,
                        SketchLocus::Entity(
                            root.id()
                                .try_clone_for_decode(ctx, "fcstd constraint root entity")?,
                        ),
                        "fcstd resolved constraint operands",
                    )
                })?;
            }
        }
        let parameter_value = if matches!(type_code, Some(6..=9 | 11 | 16 | 18 | 19)) {
            match ctx.xml_attribute(node, "Value", "FreeCAD design XML attribute")? {
                Some(value) => ctx
                    .parse_text::<f64>(value, "fcstd sketch constraint value")?
                    .ok(),
                None => None,
            }
        } else {
            None
        };
        let parameter = parameter_value
            .map(|value| {
                let id = ParameterId::mint(design_identity_text(
                    ctx,
                    "parameter",
                    object,
                    format_args!(":constraint:{}", index + 1),
                    "fcstd constraint parameter identity",
                )?)
                .map_err(CodecError::malformed)?;
                let value = match type_code {
                    Some(9) => {
                        ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(value).ok_or_else(
                            || CodecError::malformed("constraint angle must be finite"),
                        )?)
                    }
                    Some(16 | 19) => ParameterValue::Real(
                        cadmpeg_ir::scalar::FiniteReal::new(value).ok_or_else(|| {
                            CodecError::malformed("constraint real must be finite")
                        })?,
                    ),
                    _ => ParameterValue::Length(Length::new(value).ok_or_else(|| {
                        CodecError::malformed("constraint length must be finite")
                    })?),
                };
                let (path, _path_storage) = ctx.format_scoped(
                    format_args!("Constraints[{index}]"),
                    "fcstd constraint expression path",
                )?;
                let expression = expression_binding(ctx, properties, &path)?;
                let mut parameter_properties = BTreeMap::new();
                let driving = ctx
                    .xml_attribute(node, "IsDriving", "FreeCAD design XML attribute")?
                    .unwrap_or("1");
                ctx.insert_btree_map(
                    &mut parameter_properties,
                    cadmpeg_core::nonblank_literal!("is_driving"),
                    ctx.copy_retained_text(driving, "fcstd constraint driving flag")?,
                    "fcstd constraint parameter properties",
                )?;
                if let Some(name) = ctx
                    .xml_attribute(node, "Name", "FreeCAD design XML attribute")?
                    .filter(|name| !name.is_empty())
                {
                    ctx.insert_btree_map(
                        &mut parameter_properties,
                        cadmpeg_core::nonblank_literal!("source_name"),
                        ctx.copy_retained_text(name, "fcstd constraint source name")?,
                        "fcstd constraint parameter properties",
                    )?;
                }
                let expression = match expression {
                    Some((native_ref, expression)) => {
                        ctx.insert_btree_map(
                            &mut parameter_properties,
                            cadmpeg_core::nonblank_literal!("expression_native_ref"),
                            native_ref,
                            "fcstd constraint parameter properties",
                        )?;
                        expression
                    }
                    None => ctx.copy_retained_text(
                        ctx.xml_attribute(node, "Value", "FreeCAD design XML attribute")?
                            .unwrap_or_default(),
                        "fcstd constraint expression",
                    )?,
                };
                ctx.push_vec(
                    &mut parameters,
                    DesignParameter {
                        id: id.try_clone_for_decode(ctx, "fcstd constraint parameter identity")?,
                        owner: Some(feature_id(ctx, object)?),
                        ordinal: u32::try_from(index).map_err(|_| {
                            ctx.refuse_codec_limit(
                                "FreeCAD ordinal",
                                u64::from(u32::MAX),
                                cadmpeg_core::decode::u64_from_index(index),
                            )
                        })?,
                        name: ctx.format_retained(
                            format_args!("Constraint{}", index + 1),
                            "fcstd constraint parameter name",
                        )?,
                        expression,
                        display: None,
                        value: Some(value),
                        dependencies: DistinctMembers::default(),
                        properties: parameter_properties,
                        pmi: None,
                        native_ref: Some(ctx.copy_retained_text(
                            &property.id,
                            "fcstd constraint parameter native reference",
                        )?),
                    },
                    "fcstd constraint parameters",
                )?;
                Ok::<_, CodecError>(id)
            })
            .transpose()?;
        let internal_alignment =
            || -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
                use cadmpeg_ir::sketches::SketchInternalAlignment as Alignment;
                let Some(alignment_type) = int_attr(ctx, node, "InternalAlignmentType")? else {
                    return Ok(None);
                };
                let alignment = match alignment_type {
                    1 => Alignment::EllipseMajorDiameter,
                    2 => Alignment::EllipseMinorDiameter,
                    3 => Alignment::EllipseFocus1,
                    4 => Alignment::EllipseFocus2,
                    5 => Alignment::HyperbolaMajor,
                    6 => Alignment::HyperbolaMinor,
                    7 => Alignment::HyperbolaFocus,
                    8 => Alignment::ParabolaFocus,
                    9 | 10 => {
                        let Some(index) = ctx.xml_attribute(
                            node,
                            "InternalAlignmentIndex",
                            "FreeCAD design XML attribute",
                        )?
                        else {
                            return Ok(None);
                        };
                        let Ok(index) =
                            ctx.parse_text::<u32>(index, "fcstd sketch alignment index")?
                        else {
                            return Ok(None);
                        };
                        if alignment_type == 9 {
                            Alignment::BsplineControlPoint(index)
                        } else {
                            Alignment::BsplineKnotPoint(index)
                        }
                    }
                    11 => Alignment::ParabolaFocalAxis,
                    _ => return Ok(None),
                };
                let Some(helper) = resolved.first() else {
                    return Ok(None);
                };
                let Some(parent) = resolved.get(1) else {
                    return Ok(None);
                };
                Ok(Some(SketchConstraintDefinitionInput::InternalAlignment {
                    helper: locus_entity(helper)
                        .try_clone_for_decode(ctx, "fcstd constraint entity identity")?,
                    parent: locus_entity(parent)
                        .try_clone_for_decode(ctx, "fcstd constraint entity identity")?,
                    alignment,
                }))
            };
        let grouped_geometry = || -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
            if !all_resolved || resolved.is_empty() {
                return Ok(None);
            }
            match type_code {
                Some(20) => Ok(Some(SketchConstraintDefinitionInput::Group {
                    elements: copy_constraint_loci(ctx, &resolved)?,
                })),
                Some(21) => {
                    let Some(metadata) =
                        ctx.xml_attribute(node, "MetaData", "FreeCAD design XML attribute")?
                    else {
                        return Ok(None);
                    };
                    let (_reservation, metadata) = match ctx
                        .parse_json_value(metadata, "fcstd constraint text metadata parse")
                    {
                        Ok((metadata, reservation)) => (reservation, metadata),
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => return Ok(None),
                    };
                    let Some(text) = metadata.get("text").and_then(serde_json::Value::as_str)
                    else {
                        return Ok(None);
                    };
                    Ok(Some(SketchConstraintDefinitionInput::Text {
                        elements: copy_constraint_loci(ctx, &resolved)?,
                        text: ctx.copy_retained_text(text, "fcstd constraint text")?,
                        font: metadata
                            .get("font")
                            .and_then(serde_json::Value::as_str)
                            .map(|font| ctx.copy_retained_text(font, "fcstd constraint font"))
                            .transpose()?,
                        is_text_height: metadata
                            .get("isTextHeight")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true),
                    }))
                }
                _ => Ok(None),
            }
        };
        let native_kind = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            native_kind,
            "validate nonblank text",
        )?
        .ok_or_else(|| CodecError::malformed("empty native constraint kind"))?;
        let mut native_operands = Vec::new();
        for (entity, position) in ctx
            .admit_iter(&operands, "fcstd native constraint operands")?
            .copied()
        {
            if entity >= 0
                && resolved_storage
                    .with_storage(|| resolve(entity, position))?
                    .is_some()
            {
                continue;
            }
            let native_kind = cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                ctx.format_retained(
                    format_args!("position:{position}"),
                    "fcstd native operand position kind",
                )?,
                "validate nonblank text",
            )?
            .ok_or_else(|| {
                malformed_design(
                    ctx,
                    format_args!(
                        "{} constraint {} has an empty source operand kind",
                        property.id,
                        index + 1
                    ),
                )
            })?;
            ctx.push_vec(
                &mut native_operands,
                SketchNativeOperand {
                    native_kind,
                    field: None,
                    object_index: u32::try_from(entity).ok(),
                    native_ref: None,
                },
                "fcstd native constraint operands",
            )?;
        }
        let mut definition = if type_code == Some(15) && all_resolved {
            internal_alignment()?
        } else {
            None
        };
        if definition.is_none() {
            definition = grouped_geometry()?;
        }
        if definition.is_none() {
            definition = type_code
                .map(|kind| midpoint_constraint(ctx, kind, &operands, entities))
                .transpose()?
                .flatten();
        }
        if definition.is_none() {
            if let Some(type_code) = type_code {
                definition = neutral_constraint(
                    ctx,
                    type_code,
                    &resolved,
                    parameter.as_ref(),
                    all_resolved,
                )?;
            }
        }
        let definition = if let Some(definition) = definition {
            definition
        } else {
            let mut entities =
                ctx.vector_storage(resolved.len(), "fcstd native constraint entities")?;
            for locus in ctx.admit_iter(&resolved, "fcstd native constraint entities")? {
                ctx.push_vec(
                    &mut entities,
                    locus_entity(locus)
                        .try_clone_for_decode(ctx, "fcstd constraint entity identity")?,
                    "fcstd native constraint entities",
                )?;
            }
            SketchConstraintDefinitionInput::Native {
                native_kind,
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities,
                parameter,
                operands: native_operands,
            }
        };
        let orientation =
            match ctx.xml_attribute(node, "Orientation", "FreeCAD design XML attribute")? {
                Some(value) => ctx
                    .parse_text::<u32>(value, "fcstd constraint orientation")?
                    .ok(),
                None => None,
            };
        ctx.push_vec(
            &mut constraints,
            SketchConstraint {
                id: SketchConstraintId::mint(design_identity_text(
                    ctx,
                    "sketch-constraint",
                    object,
                    format_args!(":{}", index + 1),
                    "fcstd sketch constraint identity",
                )?)
                .map_err(CodecError::malformed)?,
                sketch: sketch.try_clone_for_decode(ctx, "fcstd constraint sketch identity")?,
                definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                name: nonempty_attr(ctx, node, "Name")?,
                driving: bool_attr(ctx, node, "IsDriving")?,
                active: bool_attr(ctx, node, "IsActive")?,
                virtual_space: bool_attr(ctx, node, "IsInVirtualSpace")?,
                visible: bool_attr(ctx, node, "IsVisible")?,
                orientation,
                label_distance: label_attr(ctx, node, "LabelDistance")?,
                label_position: label_attr(ctx, node, "LabelPosition")?,
                metadata: nonempty_attr(ctx, node, "MetaData")?,
                native_ref: Some(
                    ctx.copy_retained_text(&property.id, "fcstd constraint native reference")?,
                ),
            },
            "fcstd sketch constraints",
        )?;
    }
    Ok((constraints, parameters))
}

fn midpoint_constraint(
    ctx: &DecodeContext<'_>,
    kind: i64,
    operands: &[(i64, i64)],
    entities: &[SketchEntity],
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    if kind != 1 || operands.len() != 2 {
        return Ok(None);
    }
    for (midpoint_index, point_index) in [(0, 1), (1, 0)] {
        let (entity, position) = operands[midpoint_index];
        if position != 3 {
            continue;
        }
        let Some(midpoint) = resolve_operand(ctx, entity, position, entities)? else {
            return Ok(None);
        };
        let Some(bounded) = ctx.find_by(
            entities,
            |candidate| {
                ctx.equal(
                    candidate.id(),
                    locus_entity(&midpoint),
                    "fcstd midpoint entity identity",
                )
            },
            "fcstd midpoint line search",
        )?
        else {
            return Ok(None);
        };
        if !matches!(
            *bounded.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) {
            continue;
        }
        let (entity, position) = operands[point_index];
        let Some(point) = resolve_operand(ctx, entity, position, entities)? else {
            return Ok(None);
        };
        let Some(point_entity) = ctx.find_by(
            entities,
            |candidate| {
                ctx.equal(
                    candidate.id(),
                    locus_entity(&point),
                    "fcstd midpoint entity identity",
                )
            },
            "fcstd midpoint point search",
        )?
        else {
            return Ok(None);
        };
        if !matches!(
            *point_entity.geometry.definition(),
            SketchGeometryDefinition::Point { .. }
        ) {
            continue;
        }
        return Ok(Some(SketchConstraintDefinitionInput::Midpoint {
            point,
            entity: bounded
                .id()
                .try_clone_for_decode(ctx, "fcstd midpoint line identity")?,
        }));
    }
    Ok(None)
}

fn bool_attr(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
) -> Result<Option<bool>, CodecError> {
    let Some(value) = ctx.xml_attribute(node, name, "FreeCAD design XML attribute")? else {
        return Ok(None);
    };
    if (value == "1") || value.eq_ignore_ascii_case("true") {
        Ok(Some(true))
    } else if (value == "0") || value.eq_ignore_ascii_case("false") {
        Ok(Some(false))
    } else {
        Ok(None)
    }
}

fn label_attr(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
) -> Result<Option<cadmpeg_ir::sketches::SketchLabelValue>, CodecError> {
    let Some(value) = ctx.xml_attribute(node, name, "FreeCAD design XML attribute")? else {
        return Ok(None);
    };
    let Ok(value) = ctx.parse_text::<f64>(value, "fcstd constraint label value")? else {
        return Ok(None);
    };
    Ok(cadmpeg_ir::sketches::SketchLabelValue::try_from(value).ok())
}

fn nonempty_attr(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
) -> Result<Option<String>, CodecError> {
    match ctx.xml_attribute(node, name, "FreeCAD design XML attribute")? {
        Some(value) if !value.is_empty() => Ok(Some(
            ctx.copy_retained_text(value, "fcstd constraint attribute")?,
        )),
        _ => Ok(None),
    }
}

fn expression_binding(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    path: &str,
) -> Result<Option<(String, String)>, CodecError> {
    let Some(engine) = property(ctx, properties, "ExpressionEngine")? else {
        return Ok(None);
    };
    let Some(value) = ctx.find_by(
        engine.values(),
        |value| {
            if value.tag.as_str() != "Expression" {
                return Ok(false);
            }
            let Some(value_path) =
                ctx.get_btree_map(&value.attributes, "path", "fcstd expression binding path")?
            else {
                return Ok(false);
            };
            ctx.equal(value_path.as_str(), path, "fcstd expression binding path")
        },
        "fcstd expression binding values",
    )?
    else {
        return Ok(None);
    };
    let Some(expression) = ctx.get_btree_map(
        &value.attributes,
        "expression",
        "fcstd expression binding text",
    )?
    else {
        return Ok(None);
    };
    Ok(Some((
        ctx.copy_retained_text(&engine.id, "fcstd expression engine reference")?,
        ctx.copy_retained_text(expression, "fcstd expression text")?,
    )))
}

fn bind_parameter_dependencies<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    objects: &[ObjectRecord],
    cycle_affected_features: &BTreeSet<FeatureId>,
) -> Result<(BTreeSet<FeatureId>, ScopedReservation<'ctx>), CodecError> {
    let mut dependency_storage =
        ctx.reserve_scoped(0, "fcstd parameter dependency result storage")?;
    let (dependencies, candidate_storage) =
        ctx.with_scoped_storage("fcstd parameter dependency candidates", || {
            let mut object_names = HashMap::new();
            for object in ctx.admit_iter(objects, "fcstd parameter dependency objects")? {
                ctx.insert_hash_map(
                    &mut object_names,
                    feature_id(ctx, object)?,
                    object.name().as_str(),
                    "fcstd parameter dependency object names",
                )?;
            }
            let mut candidates =
                ctx.collection_vec(parameters.len(), "fcstd parameter dependency candidates")?;
            for parameter in ctx.admit_iter(parameters.as_slice(), "fcstd dependency parameters")? {
                let source_name = match ctx.get_btree_map(
                    &parameter.properties,
                    "source_name",
                    "fcstd parameter source name",
                )? {
                    Some(name)
                        if !ctx.equal(name, &parameter.name, "fcstd parameter source name")? =>
                    {
                        Some(name)
                    }
                    _ => None,
                };
                let mut names = ctx.collection_vec(
                    1 + usize::from(source_name.is_some()),
                    "fcstd parameter candidate names",
                )?;
                names.push(parameter.name.as_str());
                if let Some(source_name) = source_name {
                    names.push(source_name.as_str());
                }
                candidates.push((&parameter.id, parameter.owner.as_ref(), names));
            }
            let mut local_candidates = BTreeMap::<(&FeatureId, &str), Vec<&ParameterId>>::new();
            let mut qualified_candidates = BTreeMap::<String, Vec<&ParameterId>>::new();
            for (id, owner, names) in
                ctx.admit_iter(&candidates, "fcstd parameter candidate groups")?
            {
                let Some(owner) = owner else { continue };
                for name in ctx.admit_iter(names, "fcstd parameter candidate names")? {
                    let key = (*owner, *name);
                    ctx.push_btree_group(
                        &mut local_candidates,
                        key,
                        *id,
                        "fcstd local candidate keys",
                        "fcstd local candidate identities",
                    )?;
                    if let Some(object) =
                        ctx.get_hash_map(&object_names, owner, "fcstd qualified candidate owner")?
                    {
                        let key = ctx.format_retained(
                            format_args!("{object}.{name}"),
                            "fcstd qualified candidate name",
                        )?;
                        ctx.push_btree_group(
                            &mut qualified_candidates,
                            key,
                            *id,
                            "fcstd qualified candidate keys",
                            "fcstd qualified candidate identities",
                        )?;
                    }
                }
            }
            let local = ctx.collect_hash_map(
                local_candidates.into_iter().map(|(key, mut ids)| {
                    let unique = (ids.len() == 1).then(|| ids.pop()).flatten();
                    (key, unique)
                }),
                "fcstd unique local candidates",
            )?;
            let qualified = ctx.collect_hash_map(
                qualified_candidates.into_iter().map(|(key, mut ids)| {
                    let unique = (ids.len() == 1).then(|| ids.pop()).flatten();
                    (key, unique)
                }),
                "fcstd unique qualified candidates",
            )?;
            dependency_storage.with_storage(|| {
                let mut dependencies =
                    ctx.vector_storage(parameters.len(), "fcstd parameter dependency results")?;
                for parameter in
                    ctx.admit_iter(parameters.as_slice(), "fcstd parameter dependency scan")?
                {
                    let owner_cycle = match parameter.owner.as_ref() {
                        Some(owner) => ctx.contains_btree_set(
                            cycle_affected_features,
                            owner,
                            "fcstd parameter dependency cycle owner",
                        )?,
                        None => false,
                    };
                    if owner_cycle {
                        ctx.push_vec(
                            &mut dependencies,
                            None,
                            "fcstd parameter dependency results",
                        )?;
                        continue;
                    }
                    let mut found = BTreeSet::new();
                    expression_identifiers_until(
                        ctx,
                        &parameter.expression,
                        "fcstd parameter expression identifiers",
                        |identifier| {
                            let qualified_dependency = ctx
                                .get_hash_map(
                                    &qualified,
                                    identifier,
                                    "fcstd qualified dependency lookup",
                                )?
                                .and_then(Option::as_ref)
                                .copied();
                            let dependency = if qualified_dependency.is_some() {
                                qualified_dependency
                            } else if let Some(owner) = parameter.owner.as_ref() {
                                let key = (owner, identifier);
                                ctx.get_hash_map(&local, &key, "fcstd local dependency lookup")?
                                    .and_then(Option::as_ref)
                                    .copied()
                            } else {
                                None
                            };
                            if let Some(dependency) = dependency {
                                if !ctx.equal(
                                    dependency,
                                    &parameter.id,
                                    "fcstd parameter self dependency check",
                                )? && !ctx.contains_btree_set(
                                    &found,
                                    dependency,
                                    "fcstd parameter dependency duplicate lookup",
                                )? {
                                    ctx.insert_btree_set(
                                        &mut found,
                                        dependency.try_clone_for_decode(
                                            ctx,
                                            "fcstd scratch dependency identity",
                                        )?,
                                        "fcstd parameter dependencies",
                                    )?;
                                }
                            }
                            Ok(true)
                        },
                    )?;
                    ctx.push_vec(
                        &mut dependencies,
                        Some(found),
                        "fcstd parameter dependency results",
                    )?;
                }
                Ok::<_, CodecError>(dependencies)
            })
        })?;
    drop(candidate_storage);
    for index in ctx.admit_iter(
        &(0..parameters.len()),
        "fcstd parameter dependency materialization",
    )? {
        let parameter = &mut parameters[index];
        parameter.dependencies = match &dependencies[index] {
            None => DistinctMembers::default(),
            Some(dependencies) => {
                let mut members =
                    ctx.vector_storage(dependencies.len(), "fcstd parameter dependency members")?;
                for dependency in
                    ctx.admit_iter(dependencies, "fcstd parameter dependency members")?
                {
                    ctx.push_vec(
                        &mut members,
                        dependency
                            .try_clone_for_decode(ctx, "fcstd parameter dependency identity")?,
                        "fcstd parameter dependency members",
                    )?;
                }
                DistinctMembers::try_from(members, ctx).map_err(CodecError::from)?
            }
        };
    }
    drop(dependencies);
    drop(dependency_storage);

    let mut owner_ordinal_storage =
        ctx.reserve_scoped(0, "fcstd owner ordinal grouping storage")?;
    let mut owner_ordinals = BTreeMap::<Option<FeatureId>, Vec<u32>>::new();
    for parameter in ctx.admit_iter(parameters.as_slice(), "fcstd ordinal owner groups")? {
        if let Some(ordinals) = ctx.get_mut_btree_map(
            &mut owner_ordinals,
            &parameter.owner,
            "fcstd ordinal owner groups",
        )? {
            owner_ordinal_storage.with_storage(|| {
                ctx.push_vec(ordinals, parameter.ordinal, "fcstd owner ordinals")
            })?;
        } else {
            owner_ordinal_storage.with_storage(|| {
                let owner = parameter
                    .owner
                    .as_ref()
                    .map(|owner| owner.try_clone_for_decode(ctx, "fcstd ordinal owner identity"))
                    .transpose()?;
                ctx.push_btree_group(
                    &mut owner_ordinals,
                    owner,
                    parameter.ordinal,
                    "fcstd ordinal owner groups",
                    "fcstd owner ordinals",
                )
            })?;
        }
    }
    for (_, ordinals) in ctx.admit_iter(&mut owner_ordinals, "fcstd owner ordinal sorting")? {
        ctx.stable_sort_by(
            ordinals,
            |value| value,
            Ord::cmp,
            "fcstd owner ordinals sort",
        )?;
    }
    let (parameter_cycle_features, parameter_cycle_storage) =
        order_parameters_by_dependencies(ctx, parameters)?;
    for index in ctx.admit_iter(
        &(0..parameters.len()),
        "fcstd parameter dependency cycle clearing",
    )? {
        let has_cycle = match parameters[index].owner.as_ref() {
            Some(owner) => ctx.contains_btree_set(
                &parameter_cycle_features,
                owner,
                "fcstd parameter dependency cycle check",
            )?,
            None => false,
        };
        if has_cycle {
            // The native property record retains the expression. A neutral
            // parameter edge would create an invented evaluation order for
            // a history that FreeCAD itself could not topologically sort.
            parameters[index].dependencies.clear();
        }
    }
    let mut next_ordinal_storage = ctx.reserve_scoped(0, "fcstd next ordinal storage")?;
    let mut next_ordinal = HashMap::<Option<&FeatureId>, usize>::new();
    for parameter in ctx.admit_iter(
        parameters.as_mut_slice(),
        "fcstd parameter ordinal assignment",
    )? {
        next_ordinal_storage.with_storage(|| {
            let owner = parameter.owner.as_ref();
            let index = ctx
                .get_hash_map(&next_ordinal, &owner, "fcstd next owner ordinal")?
                .copied()
                .unwrap_or(0);
            let owner_ordinals = ctx
                .get_btree_map(
                    &owner_ordinals,
                    &parameter.owner,
                    "fcstd owner ordinal values",
                )?
                .ok_or_else(|| {
                    CodecError::malformed("parameter owner lost its source ordinal list")
                })?;
            parameter.ordinal = owner_ordinals.get(index).copied().ok_or_else(|| {
                CodecError::malformed("parameter source ordinal list ended early")
            })?;
            let next_index = index.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("fcstd next owner ordinal", u64::MAX, u64::MAX)
            })?;
            ctx.insert_hash_map(
                &mut next_ordinal,
                owner,
                next_index,
                "fcstd next ordinal owners",
            )
        })?;
    }
    Ok((parameter_cycle_features, parameter_cycle_storage))
}

fn order_parameters_by_dependencies<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
) -> Result<(BTreeSet<FeatureId>, ScopedReservation<'ctx>), CodecError> {
    let mut known_storage = ctx.reserve_scoped(0, "fcstd known parameter storage")?;
    let mut known = BTreeSet::new();
    for parameter in ctx.admit_iter(parameters.as_slice(), "fcstd known parameter scan")? {
        if !ctx.contains_btree_set(&known, &parameter.id, "fcstd known parameter lookup")? {
            known_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut known,
                    parameter
                        .id
                        .try_clone_for_decode(ctx, "fcstd known parameter identity")?,
                    "fcstd known parameter identities",
                )
            })?;
        }
    }
    let mut remaining = std::mem::take(parameters);
    let mut emitted_storage = ctx.reserve_scoped(0, "fcstd emitted parameter storage")?;
    let mut emitted = BTreeSet::new();
    let mut cycle_storage = ctx.reserve_scoped(0, "fcstd parameter cycle feature storage")?;
    let mut cycle_features = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(index) = ctx.position_by(
            &remaining,
            |parameter| {
                ctx.all_by(
                    &parameter.dependencies,
                    |dependency| {
                        Ok(!ctx.contains_btree_set(
                            &known,
                            dependency,
                            "fcstd known dependency lookup",
                        )? || ctx.contains_btree_set(
                            &emitted,
                            dependency,
                            "fcstd emitted dependency lookup",
                        )?)
                    },
                    "fcstd parameter dependency members",
                )
            },
            "fcstd parameter dependency ordering",
        )?
        else {
            for parameter in ctx.admit_iter(&remaining, "fcstd parameter cycle owners")? {
                let Some(owner) = parameter.owner.as_ref() else {
                    continue;
                };
                if !ctx.contains_btree_set(
                    &cycle_features,
                    owner,
                    "fcstd parameter cycle owner lookup",
                )? {
                    cycle_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut cycle_features,
                            owner.try_clone_for_decode(
                                ctx,
                                "fcstd parameter cycle owner identity",
                            )?,
                            "fcstd parameter cycle owners",
                        )
                    })?;
                }
            }
            ctx.append_vec(parameters, &mut remaining, "fcstd reordered parameters")?;
            break;
        };
        ctx.rotate_left(
            &mut remaining[index..],
            1,
            "fcstd parameter dependency extraction",
        )?;
        let parameter = remaining
            .pop()
            .ok_or_else(|| CodecError::malformed("ready parameter disappeared"))?;
        emitted_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut emitted,
                parameter
                    .id
                    .try_clone_for_decode(ctx, "fcstd emitted parameter identity")?,
                "fcstd emitted parameter identities",
            )
        })?;
        ctx.push_vec(parameters, parameter, "fcstd reordered parameters")?;
    }
    Ok((cycle_features, cycle_storage))
}

fn expression_identifiers_until(
    ctx: &DecodeContext<'_>,
    expression: &str,
    operation: &'static str,
    mut visit: impl FnMut(&str) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let mut identifier_start = None;
    let mut offset = 0_usize;
    let mut characters = expression.chars();
    while let Some(character) = ctx.next_charged(&mut characters, operation)? {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '.') {
            identifier_start.get_or_insert(offset);
        } else if let Some(start) = identifier_start.take() {
            if !visit(&expression[start..offset])? {
                return Ok(false);
            }
        }
        offset += character.len_utf8();
    }
    if let Some(start) = identifier_start {
        return visit(&expression[start..]);
    }
    Ok(true)
}

fn neutral_constraint(
    ctx: &DecodeContext<'_>,
    kind: i64,
    loci: &[SketchLocus],
    parameter: Option<&ParameterId>,
    complete: bool,
) -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
    if !complete {
        return Ok(None);
    }
    let entity = |index| {
        loci.get(index)
            .map(|locus| {
                locus_entity(locus).try_clone_for_decode(ctx, "fcstd constraint entity identity")
            })
            .transpose()
    };
    let locus = |index| {
        loci.get(index)
            .map(|locus| copy_constraint_locus(ctx, locus, "fcstd constraint entity identity"))
            .transpose()
    };
    let pair = || -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
        let Some(first) = entity(0)? else {
            return Ok(None);
        };
        let Some(second) = entity(1)? else {
            return Ok(None);
        };
        Ok(Some((first, second)))
    };
    let parameter = || {
        parameter
            .map(|id| id.try_clone_for_decode(ctx, "fcstd constraint parameter identity copy"))
            .transpose()
    };
    Ok(Some(match kind {
        0 => SketchConstraintDefinitionInput::Disabled {},
        1 => SketchConstraintDefinitionInput::CoincidentLoci {
            loci: copy_constraint_loci(ctx, loci)?,
        },
        2 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Horizontal { entity }
        }
        3 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Vertical { entity }
        }
        4 => {
            let Some((first, second)) = pair()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Parallel { first, second }
        }
        5 => {
            let Some((first, second)) = pair()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Tangent { first, second }
        }
        10 => {
            let Some((first, second)) = pair()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Perpendicular { first, second }
        }
        12 => {
            let Some((first, second)) = pair()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Equal { first, second }
        }
        13 => {
            let Some(point) = locus(0)? else {
                return Ok(None);
            };
            let Some(entity) = entity(1)? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::PointOnObject { point, entity }
        }
        17 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Fixed { entity }
        }
        6 if loci.len() == 2 => {
            let Some(first) = locus(0)? else {
                return Ok(None);
            };
            let Some(second) = locus(1)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::DistanceLoci {
                first,
                second,
                parameter,
            }
        }
        6 => {
            let mut entities = ctx.vector_storage(loci.len(), "fcstd constraint entity copies")?;
            for locus in ctx.admit_iter(loci, "fcstd constraint entity copies")? {
                ctx.push_vec(
                    &mut entities,
                    locus_entity(locus)
                        .try_clone_for_decode(ctx, "fcstd constraint entity identity")?,
                    "fcstd constraint entity copies",
                )?;
            }
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Distance {
                entities,
                parameter,
            }
        }
        7 => {
            let Some(first) = locus(0)? else {
                return Ok(None);
            };
            let Some(second) = locus(1)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::HorizontalDistance {
                first,
                second,
                parameter,
            }
        }
        8 => {
            let Some(first) = locus(0)? else {
                return Ok(None);
            };
            let Some(second) = locus(1)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::VerticalDistance {
                first,
                second,
                parameter,
            }
        }
        9 if loci.len() == 2 && sketch_axis(&loci[0]).is_some() => {
            let Some(entity) = entity(1)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            let Some(axis) = sketch_axis(&loci[0]) else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::AngleToAxis {
                entity,
                axis,
                parameter,
            }
        }
        9 if loci.len() == 2 && sketch_axis(&loci[1]).is_some() => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            let Some(axis) = sketch_axis(&loci[1]) else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::AngleToAxis {
                entity,
                axis,
                parameter,
            }
        }
        9 if loci.len() == 1 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::AngleToAxis {
                entity,
                axis: SketchAxis::Horizontal,
                parameter,
            }
        }
        9 => {
            let Some(first) = entity(0)? else {
                return Ok(None);
            };
            let Some(second) = entity(1)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Angle {
                first,
                second,
                parameter,
            }
        }
        11 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Radius { entity, parameter }
        }
        18 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Diameter { entity, parameter }
        }
        16 => {
            let Some(incident) = locus(0)? else {
                return Ok(None);
            };
            let Some(refracted) = locus(1)? else {
                return Ok(None);
            };
            let Some(interface) = entity(2)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::SnellsLaw {
                incident,
                refracted,
                interface,
                parameter,
            }
        }
        19 => {
            let Some(entity) = entity(0)? else {
                return Ok(None);
            };
            let Some(parameter) = parameter()? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Weight { entity, parameter }
        }
        14 => {
            let Some(first) = locus(0)? else {
                return Ok(None);
            };
            let Some(second) = locus(1)? else {
                return Ok(None);
            };
            let Some(axis) = entity(2)? else {
                return Ok(None);
            };
            SketchConstraintDefinitionInput::Symmetric {
                first,
                second,
                axis,
            }
        }
        _ => return Ok(None),
    }))
}

fn sketch_axis(locus: &SketchLocus) -> Option<SketchAxis> {
    let id = locus_entity(locus);
    if id.as_str().ends_with(":reference-horizontal-axis") {
        Some(SketchAxis::Horizontal)
    } else if id.as_str().ends_with(":reference-vertical-axis") {
        Some(SketchAxis::Vertical)
    } else {
        None
    }
}

fn constraint_operands(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<Vec<(i64, i64)>, CodecError> {
    let ids_text = ctx.xml_attribute(node, "ElementIds", "FreeCAD design XML attribute")?;
    let positions_text =
        ctx.xml_attribute(node, "ElementPositions", "FreeCAD design XML attribute")?;
    match (ids_text, positions_text) {
        (Some(ids), Some(positions)) => {
            let ((ids_values, positions_values), integer_storage) = ctx
                .with_scoped_storage("fcstd constraint integer lane storage", || {
                    Ok::<_, CodecError>((split_ints(ctx, ids)?, split_ints(ctx, positions)?))
                })?;
            if ids_values.len() != positions_values.len() {
                return Err(malformed_design(
                    ctx,
                    format_args!("ElementIds and ElementPositions counts differ"),
                ));
            }
            let mut operands = Vec::new();
            let ids = ctx
                .admit_iter(&ids_values, "fcstd constraint entity values")?
                .copied();
            let mut positions = ctx
                .admit_iter(&positions_values, "fcstd constraint position values")?
                .copied();
            for entity in ids {
                let Some(position) = positions.next() else {
                    return Err(malformed_design(
                        ctx,
                        format_args!("ElementIds and ElementPositions counts differ"),
                    ));
                };
                if entity != -2000 {
                    ctx.push_vec(
                        &mut operands,
                        (entity, position),
                        "fcstd constraint operand pairs",
                    )?;
                }
            }
            drop(ids_values);
            drop(positions_values);
            drop(integer_storage);
            return Ok(operands);
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(malformed_design(
                ctx,
                format_args!("ElementIds and ElementPositions must both be present"),
            ));
        }
        (None, None) => {}
    }
    let mut operands = Vec::new();
    for (entity_name, position_name) in [
        ("First", "FirstPos"),
        ("Second", "SecondPos"),
        ("Third", "ThirdPos"),
    ] {
        match (
            ctx.xml_attribute(node, entity_name, "FreeCAD design XML attribute")?,
            ctx.xml_attribute(node, position_name, "FreeCAD design XML attribute")?,
        ) {
            (None, None) => {}
            (Some(entity), Some(position)) => {
                let entity = ctx
                    .parse_text::<i64>(entity, "fcstd constraint entity index")?
                    .map_err(|_| {
                        malformed_design(ctx, format_args!("constraint entity is not an integer"))
                    })?;
                let position = ctx
                    .parse_text::<i64>(position, "fcstd constraint position index")?
                    .map_err(|_| {
                        malformed_design(ctx, format_args!("constraint position is not an integer"))
                    })?;
                if entity != -2000 {
                    ctx.push_vec(
                        &mut operands,
                        (entity, position),
                        "fcstd constraint operand pairs",
                    )?;
                }
            }
            _ => {
                return Err(malformed_design(
                    ctx,
                    format_args!("constraint entity and position must both be present"),
                ));
            }
        }
    }
    Ok(operands)
}

fn direct_counted_records<'ctx, 'a, 'input>(
    ctx: &'ctx DecodeContext<'_>,
    xml: &'a roxmltree::Document<'input>,
    container_tag: &str,
    record_tag: &str,
    owner: &str,
) -> Result<(Vec<roxmltree::Node<'a, 'input>>, ScopedReservation<'ctx>), CodecError> {
    let root = ctx.xml_root_element(xml, "FreeCAD counted-list root")?;
    let mut container = None;
    let mut nodes = xml.descendants();
    while let Some(node) = ctx.next_charged(&mut nodes, "FreeCAD counted-list containers")? {
        if !ctx.xml_has_tag_name(node, container_tag, "FreeCAD counted-list container tag")? {
            continue;
        }
        if container.is_some() || node.parent() != Some(root) {
            return Err(malformed_design(
                ctx,
                format_args!("{owner} must contain exactly one direct {container_tag} value"),
            ));
        }
        container = Some(node);
    }
    let Some(container) = container else {
        return Err(malformed_design(
            ctx,
            format_args!("{owner} must contain exactly one direct {container_tag} value"),
        ));
    };
    let count = ctx
        .xml_attribute(container, "count", "FreeCAD counted-list count attribute")?
        .ok_or_else(|| {
            malformed_design(ctx, format_args!("{owner} has an invalid record count"))
        })?;
    let declared = ctx
        .parse_text::<usize>(count, "FreeCAD counted-list record count")?
        .map_err(|_| malformed_design(ctx, format_args!("{owner} has an invalid record count")))?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(malformed_design(
            ctx,
            format_args!("{owner} record count exceeds {MAX_SKETCH_RECORDS}"),
        ));
    }
    let mut storage = ctx.reserve_scoped(0, "FreeCAD counted-list record storage")?;
    let mut records = Vec::new();
    let mut children = container.children();
    while let Some(node) = ctx.next_charged(&mut children, "FreeCAD counted-list records")? {
        if !node.is_element() {
            continue;
        }
        if !ctx.xml_has_tag_name(node, record_tag, "FreeCAD counted-list record tag")? {
            return Err(malformed_design(
                ctx,
                format_args!("{owner} has a non-{record_tag} direct child"),
            ));
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut records,
            node,
            "fcstd counted sketch records",
        )?;
    }
    if ctx.any_by(
        xml.descendants(),
        |node| {
            Ok(
                ctx.xml_has_tag_name(node, record_tag, "FreeCAD counted-list nested record tag")?
                    && node.parent() != Some(container),
            )
        },
        "FreeCAD counted-list record descendants",
    )? {
        return Err(malformed_design(
            ctx,
            format_args!("{owner} has nested {record_tag} records"),
        ));
    }
    if declared != records.len() {
        return Err(malformed_design(
            ctx,
            format_args!(
                "{owner} declares {declared} records but contains {}",
                records.len()
            ),
        ));
    }
    Ok((records, storage))
}

fn split_ints(ctx: &DecodeContext<'_>, value: &str) -> Result<Vec<i64>, CodecError> {
    if ctx
        .trim_text(value, "fcstd constraint integer list")?
        .is_empty()
    {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    let mut remaining = value;
    loop {
        let (group, rest) =
            match ctx.split_once(remaining, ",", "fcstd constraint integer list groups")? {
                Some((group, rest)) => (group, Some(rest)),
                None => (remaining, None),
            };
        if ctx
            .trim_text(group, "fcstd constraint integer list group")?
            .is_empty()
        {
            return Err(malformed_design(
                ctx,
                format_args!("constraint integer list has an empty item"),
            ));
        }
        let mut bytes = ctx.admit_iter(group.as_bytes(), "fcstd constraint integer list tokens")?;
        let mut token_start = None;
        let mut byte_offset = 0_usize;
        loop {
            let byte = bytes.next();
            match byte {
                Some(byte) if byte.is_ascii_whitespace() => {
                    if let Some(start) = token_start.take() {
                        let token = &group[start..byte_offset];
                        let parsed = ctx
                            .parse_text::<i64>(token, "fcstd constraint integer item")?
                            .map_err(|_| {
                                malformed_design(
                                    ctx,
                                    format_args!("constraint integer list has an invalid integer"),
                                )
                            })?;
                        ctx.push_vec(&mut values, parsed, "fcstd constraint integer list")?;
                    }
                    byte_offset += 1;
                }
                Some(_) => {
                    token_start.get_or_insert(byte_offset);
                    byte_offset += 1;
                }
                None => {
                    if let Some(start) = token_start.take() {
                        let token = &group[start..byte_offset];
                        let parsed = ctx
                            .parse_text::<i64>(token, "fcstd constraint integer item")?
                            .map_err(|_| {
                                malformed_design(
                                    ctx,
                                    format_args!("constraint integer list has an invalid integer"),
                                )
                            })?;
                        ctx.push_vec(&mut values, parsed, "fcstd constraint integer list")?;
                    }
                    break;
                }
            }
        }
        match rest {
            Some(rest) => remaining = rest,
            None => break,
        }
    }
    Ok(values)
}

fn int_attr(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
) -> Result<Option<i64>, CodecError> {
    let Some(value) = ctx.xml_attribute(node, name, "FreeCAD design XML attribute")? else {
        return Ok(None);
    };
    Ok(ctx
        .parse_text::<i64>(value, "fcstd sketch integer attribute")?
        .ok())
}

fn resolve_operand(
    ctx: &DecodeContext<'_>,
    entity: i64,
    position: i64,
    entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, CodecError> {
    let reference = |suffix: &str| -> Result<Option<SketchLocus>, CodecError> {
        ctx.find_by(
            entities,
            |candidate| Ok(candidate.id().as_str().ends_with(suffix)),
            "fcstd sketch reference entity search",
        )?
        .map(|candidate| {
            candidate
                .id()
                .try_clone_for_decode(ctx, "fcstd resolved operand identity")
                .map(SketchLocus::Entity)
        })
        .transpose()
    };
    match (entity, position) {
        (-1, 0) => return reference(":reference-horizontal-axis"),
        (-1, 1) => return reference(":reference-root-point"),
        (-2, 0) => return reference(":reference-vertical-axis"),
        (-2, 1) => return reference(":reference-root-point"),
        _ => {}
    }
    if entity <= -3 {
        let Some(external_index) = entity
            .checked_neg()
            .and_then(|value| value.checked_sub(3))
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Ok(None);
        };
        let (suffix, _suffix_storage) = ctx.format_scoped(
            format_args!(":external:{external_index}"),
            "fcstd external geometry suffix",
        )?;
        let Some(entity) = ctx.find_by(
            entities,
            |candidate| {
                ctx.ends_with(
                    candidate.id().as_str(),
                    &suffix,
                    "fcstd external geometry identity suffix",
                )
            },
            "fcstd external geometry identity lookup",
        )?
        else {
            return Ok(None);
        };
        return sketch_locus(ctx, entity, position);
    }
    let Some(entity) = usize::try_from(entity)
        .ok()
        .and_then(|index| entities.get(index))
    else {
        return Ok(None);
    };
    sketch_locus(ctx, entity, position)
}

fn sketch_locus(
    ctx: &DecodeContext<'_>,
    entity: &SketchEntity,
    position: i64,
) -> Result<Option<SketchLocus>, CodecError> {
    if !matches!(position, 0..=3) {
        return Ok(None);
    }
    let id = entity
        .id()
        .try_clone_for_decode(ctx, "fcstd resolved operand identity")?;
    if matches!(
        *entity.geometry.definition(),
        SketchGeometryDefinition::Point { .. }
    ) {
        return Ok(Some(SketchLocus::Entity(id)));
    }
    Ok(Some(match position {
        0 => SketchLocus::Entity(id),
        1 => SketchLocus::Start(id),
        2 => SketchLocus::End(id),
        3 => SketchLocus::Center(id),
        _ => return Ok(None),
    }))
}

fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn copy_constraint_locus(
    ctx: &DecodeContext<'_>,
    locus: &SketchLocus,
    operation: &'static str,
) -> Result<SketchLocus, CodecError> {
    let entity = locus_entity(locus).try_clone_for_decode(ctx, operation)?;
    Ok(match locus {
        SketchLocus::Entity(_) => SketchLocus::Entity(entity),
        SketchLocus::Start(_) => SketchLocus::Start(entity),
        SketchLocus::End(_) => SketchLocus::End(entity),
        SketchLocus::Center(_) => SketchLocus::Center(entity),
    })
}

fn copy_constraint_loci(
    ctx: &DecodeContext<'_>,
    loci: &[SketchLocus],
) -> Result<Vec<SketchLocus>, CodecError> {
    let mut copies = ctx.vector_storage(loci.len(), "fcstd constraint locus copies")?;
    for locus in ctx.admit_iter(loci, "fcstd constraint locus copies")? {
        ctx.push_vec(
            &mut copies,
            copy_constraint_locus(ctx, locus, "fcstd constraint entity identity")?,
            "fcstd constraint locus copies",
        )?;
    }
    Ok(copies)
}

fn constraint_kind(kind: i64) -> &'static str {
    match kind {
        0 => "none",
        1 => "coincident",
        2 => "horizontal",
        3 => "vertical",
        4 => "parallel",
        5 => "tangent",
        6 => "distance",
        7 => "distance_x",
        8 => "distance_y",
        9 => "angle",
        10 => "perpendicular",
        11 => "radius",
        12 => "equal",
        13 => "point_on_object",
        14 => "symmetric",
        15 => "internal_alignment",
        16 => "snells_law",
        17 => "block",
        18 => "diameter",
        19 => "weight",
        20 => "group",
        21 => "text",
        _ => "unknown_future_constraint",
    }
}

fn sketch_geometry(
    ctx: &DecodeContext<'_>,
    kind: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<SketchGeometry, CodecError> {
    if !ctx.any_by(
        kind.chars(),
        |character| Ok(!character.is_whitespace()),
        "fcstd sketch geometry nonblank kind",
    )? {
        return Err(CodecError::malformed("native_kind must not be empty"));
    }
    let number = |name: &str| -> Result<Option<f64>, CodecError> {
        let Some(value) = ctx.get_btree_map(attributes, name, "fcstd sketch geometry attribute")?
        else {
            return Ok(None);
        };
        Ok(ctx
            .parse_text::<f64>(value, "fcstd sketch geometry number")?
            .ok())
    };
    let kind_is = |expected: &str| kind == expected;
    let native = || -> Result<SketchGeometry, CodecError> {
        let native_kind = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(kind, "fcstd native sketch geometry kind")?,
            "validate nonblank text",
        )?
        .ok_or_else(|| CodecError::malformed("native_kind must not be empty"))?;
        Ok(SketchGeometry::native(native_kind))
    };
    if kind_is("Part::GeomArcOfCircle") || kind_is("ArcOfCircle") {
        let frame_angle = number("AngleXU")?.unwrap_or(0.0);
        let center_x = number("CenterX")?;
        let center_y = number("CenterY")?;
        let radius = number("Radius")?;
        let start = match number("StartAngle")? {
            Some(value) => Some(value),
            None => number("FirstParameter")?,
        };
        let end = match number("EndAngle")? {
            Some(value) => Some(value),
            None => number("LastParameter")?,
        };
        let admitted = (|| -> Option<SketchGeometry> {
            let center = FinitePoint2::from_coordinates(
                FiniteReal::new(center_x?)?,
                FiniteReal::new(center_y?)?,
            );
            let radius = PositiveLength::new(radius?)?;
            let start = FiniteReal::new(start?)?;
            let end = FiniteReal::new(end?)?;
            let frame_angle = FiniteReal::new(frame_angle)?;
            SketchGeometry::from_parts(SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle: cadmpeg_ir::scalar::Angle::new(start.get() + frame_angle.get())?,
                end_angle: cadmpeg_ir::scalar::Angle::new(end.get() + frame_angle.get())?,
            })
            .ok()
        })();
        return match admitted {
            Some(geometry) => Ok(geometry),
            None => native(),
        };
    }
    let projected = if kind_is("Part::GeomLine")
        || kind_is("Part::GeomLineSegment")
        || kind_is("Line")
        || kind_is("LineSegment")
    {
        match (
            number("StartX")?,
            number("StartY")?,
            number("EndX")?,
            number("EndY")?,
        ) {
            (Some(start_x), Some(start_y), Some(end_x), Some(end_y)) => {
                Some(SketchGeometryDefinition::Line {
                    start: Point2::new(start_x, start_y),
                    end: Point2::new(end_x, end_y),
                })
            }
            _ => None,
        }
    } else if kind_is("Part::GeomEllipse")
        || kind_is("Part::GeomArcOfEllipse")
        || kind_is("Ellipse")
        || kind_is("ArcOfEllipse")
    {
        let major_angle = match number("MajorAngle")? {
            Some(angle) => Some(angle),
            None => match number("AngleXU")? {
                Some(angle) => Some(angle),
                None => match (number("MajorAxisY")?, number("MajorAxisX")?) {
                    (Some(y), Some(x)) => Some(y.atan2(x)),
                    _ => None,
                },
            },
        };
        let is_arc = kind_is("Part::GeomArcOfEllipse") || kind_is("ArcOfEllipse");
        let bounds = if is_arc {
            let start = match number("StartAngle")? {
                Some(value) => Some(value),
                None => number("FirstParameter")?,
            };
            let end = match number("EndAngle")? {
                Some(value) => Some(value),
                None => number("LastParameter")?,
            };
            start.zip(end).map(|(start, end)| Some([start, end]))
        } else {
            Some(None)
        };
        match (
            number("CenterX")?,
            number("CenterY")?,
            major_angle,
            number("MajorRadius")?,
            number("MinorRadius")?,
            bounds,
        ) {
            (Some(x), Some(y), Some(angle), Some(major), Some(minor), Some(bounds))
                if major > 0.0 && minor > 0.0 =>
            {
                (|| -> Option<SketchGeometryDefinition> {
                    Some(SketchGeometryDefinition::Ellipse {
                        center: Point2::new(x, y),
                        major_angle: cadmpeg_ir::scalar::Angle::new(angle)?,
                        radii: cadmpeg_ir::sketches::EllipseRadii {
                            major_radius: Length::new(major)?,
                            minor_radius: Length::new(minor)?,
                        },
                        bounds: match bounds {
                            Some([start, end]) => Some([
                                cadmpeg_ir::scalar::Angle::new(start)?,
                                cadmpeg_ir::scalar::Angle::new(end)?,
                            ]),
                            None => None,
                        },
                    })
                })()
            }
            _ => None,
        }
    } else if kind_is("Part::GeomHyperbola")
        || kind_is("Part::GeomArcOfHyperbola")
        || kind_is("Hyperbola")
        || kind_is("ArcOfHyperbola")
    {
        let is_arc = kind_is("Part::GeomArcOfHyperbola") || kind_is("ArcOfHyperbola");
        let bounds = if is_arc {
            let start = match number("StartAngle")? {
                Some(value) => Some(value),
                None => number("FirstParameter")?,
            };
            let end = match number("EndAngle")? {
                Some(value) => Some(value),
                None => number("LastParameter")?,
            };
            start.zip(end).map(|(start, end)| Some([start, end]))
        } else {
            Some(None)
        };
        let angle = match number("AngleXU")? {
            Some(angle) => Some(angle),
            None => number("MajorAngle")?,
        };
        match (
            number("CenterX")?,
            number("CenterY")?,
            angle,
            number("MajorRadius")?,
            number("MinorRadius")?,
            bounds,
        ) {
            (Some(x), Some(y), Some(angle), Some(major), Some(minor), Some(bounds))
                if major > 0.0 && minor > 0.0 =>
            {
                (|| -> Option<SketchGeometryDefinition> {
                    Some(SketchGeometryDefinition::Hyperbola {
                        center: Point2::new(x, y),
                        major_angle: cadmpeg_ir::scalar::Angle::new(angle)?,
                        major_radius: Length::new(major)?,
                        minor_radius: Length::new(minor)?,
                        bounds,
                    })
                })()
            }
            _ => None,
        }
    } else if kind_is("Part::GeomParabola")
        || kind_is("Part::GeomArcOfParabola")
        || kind_is("Parabola")
        || kind_is("ArcOfParabola")
    {
        let is_arc = kind_is("Part::GeomArcOfParabola") || kind_is("ArcOfParabola");
        let bounds = if is_arc {
            let start = match number("StartAngle")? {
                Some(value) => Some(value),
                None => number("FirstParameter")?,
            };
            let end = match number("EndAngle")? {
                Some(value) => Some(value),
                None => number("LastParameter")?,
            };
            start.zip(end).map(|(start, end)| Some([start, end]))
        } else {
            Some(None)
        };
        let angle = match number("AngleXU")? {
            Some(angle) => Some(angle),
            None => number("AxisAngle")?,
        };
        match (
            number("CenterX")?,
            number("CenterY")?,
            angle,
            number("Focal")?,
            bounds,
        ) {
            (Some(x), Some(y), Some(angle), Some(focal), Some(bounds)) if focal > 0.0 => {
                (|| -> Option<SketchGeometryDefinition> {
                    Some(SketchGeometryDefinition::Parabola {
                        vertex: Point2::new(x, y),
                        axis_angle: cadmpeg_ir::scalar::Angle::new(angle)?,
                        focal_length: Length::new(focal)?,
                        bounds,
                    })
                })()
            }
            _ => None,
        }
    } else if kind_is("Part::GeomCircle") || kind_is("Circle") {
        match (number("CenterX")?, number("CenterY")?, number("Radius")?) {
            (Some(x), Some(y), Some(radius)) if radius > 0.0 => {
                Length::new(radius).map(|radius| SketchGeometryDefinition::Circle {
                    center: Point2::new(x, y),
                    radius,
                })
            }
            (Some(x), Some(y), Some(0.0)) => Some(SketchGeometryDefinition::Point {
                position: Point2::new(x, y),
            }),
            _ => None,
        }
    } else if kind_is("Part::GeomPoint") {
        match (number("X")?, number("Y")?) {
            (Some(x), Some(y)) => Some(SketchGeometryDefinition::Point {
                position: Point2::new(x, y),
            }),
            _ => None,
        }
    } else {
        None
    };
    match projected {
        Some(definition) => SketchGeometry::try_from(definition).map_err(CodecError::malformed),
        None => native(),
    }
}

fn profile_ref(
    ctx: &DecodeContext<'_>,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Result<ProfileRef, CodecError> {
    let Some((property, target)) = profile_target(ctx, properties)? else {
        return Ok(ProfileRef::Planar(PlanarProfileRef::Unresolved(
            ctx.copy_retained_text(owner, "fcstd unresolved profile reference")?,
        )));
    };
    Ok(ProfileRef::Planar(
        match ctx.get_hash_map(sketches, target, "fcstd sketch profile lookup")? {
            Some(sketch) => PlanarProfileRef::Sketch(
                sketch.try_clone_for_decode(ctx, "fcstd sketch profile reference")?,
            ),
            None => PlanarProfileRef::Native(
                ctx.copy_retained_text(&property.id, "fcstd native profile reference")?,
            ),
        },
    ))
}

fn profile_target<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
) -> Result<Option<(&'a PropertyRecord, &'a str)>, CodecError> {
    let mut selected = None;
    for name in ["Profile", "Sketch", "Base", "Source"] {
        let Some(property) = property(ctx, properties, name)? else {
            continue;
        };
        let Some(link) = scalar_link(property) else {
            if (name == "Base") && !is_link_property_type(property.type_name.as_str()) {
                continue;
            }
            return Ok(None);
        };
        let Some(target) = link.object() else {
            return Ok(None);
        };
        if target.is_empty() || selected.is_some() {
            return Ok(None);
        }
        selected = Some((property, target));
    }
    Ok(selected)
}

fn revolution_axis(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<RevolutionAxis>, CodecError> {
    Ok(Some(RevolutionAxis {
        origin: vector_property(ctx, properties, "Base")?
            .map_or(cadmpeg_ir::features::FinitePoint3::ZERO, |vector| {
                vector.as_point()
            }),
        direction: required!(cadmpeg_ir::features::FeatureDirection3::new(
            required!(vector_property(ctx, properties, "Axis")?).get(),
        )),
        reference: None,
    }))
}

fn revolution_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let face_maker_class = if kind == "Part::Revolution" {
        match property(ctx, properties, "FaceMakerClass")? {
            Some(property) => string_property_value(ctx, property)?,
            None => None,
        }
    } else {
        None
    };
    let profile = match profile_ref(ctx, owner, properties, sketches)? {
        ProfileRef::Planar(PlanarProfileRef::Unresolved(_)) => None,
        profile => Some(profile),
    };
    let Some(mut axis) = revolution_axis(ctx, properties)? else {
        return Ok(None);
    };
    let Some(direction) = cadmpeg_ir::units::UnitVector3::normalized(*axis.direction) else {
        return Ok(None);
    };
    axis.direction = cadmpeg_ir::features::FeatureDirection3::from(direction);
    let angle = || -> Result<_, CodecError> {
        Ok(scalar_named(ctx, properties, "Angle")?
            .filter(|angle| angle.get() > 0.0)
            .and_then(|angle| cadmpeg_ir::scalar::PositiveAngle::new(angle.get().to_radians())))
    };
    let Some(mode) = enumeration_selector(ctx, properties, "Type", 0)? else {
        return Ok(None);
    };
    let extent = if kind == "Part::Revolution" {
        let Some(angle) = angle()? else {
            return Ok(None);
        };
        let Some(symmetric) = bool_selector(ctx, properties, "Symmetric", false)? else {
            return Ok(None);
        };
        if symmetric {
            RevolveExtent::Symmetric {
                termination: AngularTermination::Angle { angle },
            }
        } else {
            RevolveExtent::OneSided {
                termination: AngularTermination::Angle { angle },
            }
        }
    } else {
        match mode {
            0 => {
                let Some(angle) = angle()? else {
                    return Ok(None);
                };
                let Some(midplane) = bool_selector(ctx, properties, "Midplane", false)? else {
                    return Ok(None);
                };
                if midplane {
                    RevolveExtent::Symmetric {
                        termination: AngularTermination::Angle { angle },
                    }
                } else {
                    RevolveExtent::OneSided {
                        termination: AngularTermination::Angle { angle },
                    }
                }
            }
            1 => RevolveExtent::OneSided {
                termination: AngularTermination::ThroughAll {},
            },
            2 => RevolveExtent::OneSided {
                termination: AngularTermination::ToFirst {},
            },
            3 => {
                let Some(face) = singular_operand(ctx, properties, "UpToFace")? else {
                    return Ok(None);
                };
                RevolveExtent::OneSided {
                    termination: AngularTermination::ToFace {
                        face: cadmpeg_ir::features::FaceSelection::Native(
                            ctx.copy_retained_text(&face.id, "fcstd revolution terminal face")?,
                        ),
                        offset: None,
                    },
                }
            }
            4 => {
                let Some(first) = angle()? else {
                    return Ok(None);
                };
                let Some(second) = scalar_named(ctx, properties, "Angle2")?
                    .filter(|angle| angle.get() > 0.0)
                    .and_then(|angle| {
                        cadmpeg_ir::scalar::PositiveAngle::new(angle.get().to_radians())
                    })
                else {
                    return Ok(None);
                };
                RevolveExtent::TwoSided {
                    first: AngularTermination::Angle { angle: first },
                    second: AngularTermination::Angle { angle: second },
                }
            }
            _ => return Ok(None),
        }
    };
    let reversed = if kind.starts_with("PartDesign::") {
        let Some(reversed) = bool_selector(ctx, properties, "Reversed", false)? else {
            return Ok(None);
        };
        reversed
    } else {
        false
    };
    if reversed {
        axis.direction = axis.direction.reversed();
    }
    let axis_link = property(ctx, properties, "AxisLink")?;
    let reference_axis = property(ctx, properties, "ReferenceAxis")?;
    axis.reference = match (axis_link, reference_axis) {
        (None, None) => None,
        (Some(property), None) | (None, Some(property)) => {
            if ctx.any_by(
                property.links(),
                |link| Ok(nonempty_link(link.as_ref())),
                "fcstd revolution axis reference links",
            )? {
                if singular_reference_link(ctx, property)?.is_none() {
                    return Ok(None);
                }
                Some(PathRef::Native(ctx.copy_retained_text(
                    &property.id,
                    "fcstd revolution axis reference",
                )?))
            } else {
                None
            }
        }
        (Some(_), Some(_)) => return Ok(None),
    };
    let face_maker =
        if kind == "Part::Revolution" && property(ctx, properties, "FaceMakerClass")?.is_some() {
            let Some(face_maker_class) = face_maker_class else {
                return Ok(None);
            };
            let Some(face_maker) = FaceMaker::new(ctx, face_maker_class)? else {
                return Ok(None);
            };
            Some(face_maker)
        } else {
            None
        };
    let fuse_order =
        if kind.starts_with("PartDesign::") && property(ctx, properties, "FuseOrder")?.is_some() {
            let Some(value) = integer_property(ctx, properties, "FuseOrder")? else {
                return Ok(None);
            };
            Some(match value {
                0 => RevolutionFuseOrder::BaseFirst,
                1 => RevolutionFuseOrder::FeatureFirst,
                _ => return Ok(None),
            })
        } else {
            None
        };
    let profile = profile.and_then(|profile| match profile {
        ProfileRef::Planar(profile) => Some(profile),
        _ => None,
    });
    let solid = Some(if kind == "Part::Revolution" {
        let Some(solid) = bool_selector(ctx, properties, "Solid", false)? else {
            return Ok(None);
        };
        solid
    } else {
        true
    });
    let allow_multi_profile_faces = if kind.starts_with("PartDesign::") {
        let Some(allow) = bool_selector(ctx, properties, "AllowMultiFace", false)? else {
            return Ok(None);
        };
        Some(allow)
    } else {
        None
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Revolve {
            construction: match profile {
                Some(profile) => RevolveConstruction::Resolved {
                    profile,
                    axis,
                    extent,
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                },
                None => RevolveConstruction::Unresolved(
                    cadmpeg_ir::features::PartialRevolveConstruction::Profile {
                        axis: Some(axis),
                        extent: Some(extent),
                        solid,
                        face_maker,
                        fuse_order,
                        allow_multi_profile_faces,
                    },
                ),
            },
            op: if kind == "Part::Revolution" {
                BooleanOp::NewBody
            } else if kind.contains("Groove") {
                BooleanOp::Cut
            } else {
                BooleanOp::Join
            },
        },
    )))
}

fn vector_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<cadmpeg_ir::features::FiniteVector3>, CodecError> {
    let property = required!(property(ctx, properties, name)?);
    if !is_vector_property_type(&property.type_name) {
        return Ok(None);
    }
    Ok(required!(direct_root(
        ctx,
        property,
        "PropertyVector",
        |ctx, root| {
            let component = |name: &str| -> Result<Option<FiniteReal>, CodecError> {
                let Some(value) = ctx.xml_attribute(root, name, "FreeCAD design XML attribute")?
                else {
                    return Ok(None);
                };
                let Ok(value) = ctx.parse_text::<f64>(value, "fcstd vector property component")?
                else {
                    return Ok(None);
                };
                Ok(FiniteReal::new(value))
            };
            let Some(x) = component("valueX")? else {
                return Ok(None);
            };
            let Some(y) = component("valueY")? else {
                return Ok(None);
            };
            let Some(z) = component("valueZ")? else {
                return Ok(None);
            };
            Ok(Some(cadmpeg_ir::features::FiniteVector3::from_components(
                x, y, z,
            )))
        }
    )?))
}

fn vector_list_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    entries: &[EntryRecord],
) -> Result<Option<Vec<cadmpeg_ir::features::FinitePoint3>>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyVectorList" {
        return Ok(None);
    }
    Ok(direct_root(
        ctx,
        property,
        "VectorList",
        |ctx, root| -> Result<_, CodecError> {
            let Some(file) = ctx.xml_attribute(root, "file", "FreeCAD design XML attribute")?
            else {
                return Ok(None);
            };
            if file.is_empty() {
                return Ok(property.side_entries().is_empty().then(Vec::new));
            }
            let side_entries = property.side_entries();
            if side_entries.len() != 1
                || !ctx.equal(
                    side_entries[0].as_str(),
                    file,
                    "fcstd vector-list side entry",
                )?
            {
                return Ok(None);
            }
            let Some(entry) = ctx.find_by(
                entries,
                |entry| ctx.equal(entry.name(), file, "fcstd vector-list entry name"),
                "fcstd vector-list entry search",
            )?
            else {
                return Ok(None);
            };
            let data = entry.data();
            let mut view = View::over_retained(data);
            let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
                return Ok(None);
            };
            if count > MAX_SKETCH_RECORDS
                || view
                    .counted(cadmpeg_core::decode::u64_from_index(count), 24)
                    .is_none()
            {
                return Ok(None);
            }
            let mut points = ctx.vector_storage(count, "fcstd vector-list points")?;
            for _ in ctx.admit_iter(&(0..count), "fcstd vector-list points")? {
                let Some(point) = (|| {
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                        view.f64_le()?,
                        view.f64_le()?,
                        view.f64_le()?,
                    ))
                })() else {
                    return Ok(None);
                };
                ctx.push_vec(&mut points, point, "fcstd vector-list points")?;
            }
            Ok(view.is_empty().then_some(points))
        },
    )?
    .flatten())
}

fn part_construction_geometry_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let polygon_points = if kind == "Part::Polygon" {
        vector_list_property(ctx, properties, "Nodes", entries)?
    } else {
        None
    };
    let face_maker_class = if kind == "Part::Face" {
        match property(ctx, properties, "FaceMakerClass")? {
            Some(property) => string_property_value(ctx, property)?,
            None => None,
        }
    } else {
        None
    };
    if kind == "Part::Face" {
        let Some(sources) = property(ctx, properties, "Sources")? else {
            return Ok(None);
        };
        if sources.links().is_empty() {
            return Ok(None);
        }
        let Some(face_maker_class) = face_maker_class else {
            return Ok(None);
        };
        let Some(face_maker) = FaceMaker::new(ctx, face_maker_class)? else {
            return Ok(None);
        };
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::FaceFromShapes {
                sources: BodySelection::Native(
                    ctx.copy_retained_text(&sources.id, "fcstd face source selection")?,
                ),
                face_maker,
            },
        )));
    }
    let point = |x: &str, y: &str, z: &str| -> Result<_, CodecError> {
        Ok(Some(cadmpeg_ir::features::FinitePoint3::from_coordinates(
            required!(scalar_named(ctx, properties, x)?),
            required!(scalar_named(ctx, properties, y)?),
            required!(scalar_named(ctx, properties, z)?),
        )))
    };
    let angle = |name: &str| -> Result<_, CodecError> {
        Ok(scalar_named(ctx, properties, name)?
            .and_then(|value| cadmpeg_ir::scalar::Angle::new(value.get().to_radians())))
    };
    Ok(match kind {
        "Part::Vertex" => Some(FeatureDefinition::Operation(
            FeatureOperation::PointGeometry {
                position: required!(point("X", "Y", "Z")?),
            },
        )),
        "Part::Line" => Some(FeatureDefinition::Operation(
            FeatureOperation::LineSegment {
                segment: required!(cadmpeg_ir::features::FeatureLineSegment::from_parts(
                    required!(point("X1", "Y1", "Z1")?),
                    required!(point("X2", "Y2", "Z2")?),
                )),
            },
        )),
        "Part::Circle" => {
            let legacy_angles = property(ctx, properties, "Angle0")?.is_some();
            Some(FeatureDefinition::Operation(
                FeatureOperation::CircularArc {
                    arc: cadmpeg_ir::features::FeatureCircularArc::from_parts(
                        cadmpeg_ir::features::FinitePoint3::ZERO,
                        cadmpeg_ir::features::FeatureDirection3::Z_AXIS,
                        required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                            required!(scalar_named(ctx, properties, "Radius",)?)
                        )),
                        required!(
                            cadmpeg_ir::geometry::DirectedParameterRange::from_angle_endpoints([
                                required!(angle(if legacy_angles { "Angle0" } else { "Angle1" })?),
                                required!(angle(if legacy_angles { "Angle1" } else { "Angle2" })?),
                            ])
                            .ok()
                        ),
                    ),
                },
            ))
        }
        "Part::Ellipse" => Some(FeatureDefinition::Operation(
            FeatureOperation::EllipticArc {
                arc: required!(cadmpeg_ir::features::FeatureEllipticArc::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    [
                        required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                            required!(scalar_named(ctx, properties, "MajorRadius")?),
                        )),
                        required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                            required!(scalar_named(ctx, properties, "MinorRadius")?),
                        )),
                    ],
                    required!(
                        cadmpeg_ir::geometry::DirectedParameterRange::from_angle_endpoints([
                            required!(angle("Angle1")?),
                            required!(angle("Angle2")?),
                        ])
                        .ok()
                    ),
                )),
            },
        )),
        "Part::Polygon" => {
            let points = required!(polygon_points);
            let closed = bool_property(ctx, properties, "Close")?.unwrap_or(false);
            Some(FeatureDefinition::Operation(FeatureOperation::Polyline {
                chain: required!(cadmpeg_ir::features::FeaturePolyline::from_parts(
                    points, closed
                )),
            }))
        }
        "Part::RegularPolygon" => Some(FeatureDefinition::Operation(
            FeatureOperation::RegularPolygonCurve {
                sides: required!(cadmpeg_ir::features::PolygonSideCount::new(required!(
                    u32::try_from(required!(integer_property(ctx, properties, "Polygon")?)).ok()
                ),)),
                circumradius: required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                    required!(scalar_named(ctx, properties, "Circumradius")?),
                )),
            },
        )),
        "Part::Plane" => Some(FeatureDefinition::Operation(
            FeatureOperation::PlanarPatch {
                length: required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                    required!(scalar_named(ctx, properties, "Length",)?)
                )),
                width: required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                    required!(scalar_named(ctx, properties, "Width",)?)
                )),
            },
        )),
        _ => None,
    })
}

fn parametric_helix_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let radius = required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
        required!(scalar_named(ctx, properties, "Radius",)?)
    ));
    let segment_default = if kind == "Part::Spiral" {
        DEFAULT_PART_SPIRAL_SEGMENT_TURNS
    } else {
        0.0
    };
    let segment_value = required!(finite_float_selector(
        ctx,
        properties,
        "SegmentLength",
        "App::PropertyQuantityConstraint",
        required!(FiniteReal::new(segment_default)),
    )?);
    if segment_value.get() < 0.0 {
        return Ok(None);
    }
    let segment_turns = required!((segment_value.get() > 0.0)
        .then(|| cadmpeg_ir::scalar::PositiveReal::try_from(segment_value))
        .transpose()
        .ok());
    let (shape, revolutions, clockwise, construction_style) = if kind == "Part::Helix" {
        let pitch = required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
            required!(scalar_named(ctx, properties, "Pitch",)?)
        ));
        let height = required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
            required!(scalar_named(ctx, properties, "Height",)?)
        ));
        let angle = scalar_named(ctx, properties, "Angle")?.map_or(0.0, FiniteReal::get);
        if angle.abs() >= 90.0 {
            return Ok(None);
        }
        let clockwise = match required!(enumeration_selector(ctx, properties, "LocalCoord", 0)?) {
            0 => false,
            1 => true,
            _ => return Ok(None),
        };
        let construction_style = match required!(enumeration_selector(ctx, properties, "Style", 0)?)
        {
            0 => Some(HelixConstructionStyle::Legacy),
            1 => Some(HelixConstructionStyle::Corrected),
            _ => return Ok(None),
        };
        let shape = if angle == 0.0 {
            cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: pitch.into(),
            }
        } else {
            cadmpeg_ir::features::HelixShape::Conical {
                pitch: pitch.into(),
                cone_angle: required!(cadmpeg_ir::scalar::SlopeAngle::new(angle.to_radians())),
            }
        };
        (
            shape,
            required!(cadmpeg_ir::scalar::PositiveReal::new(
                height.get() / pitch.get()
            )),
            clockwise,
            construction_style,
        )
    } else {
        let growth = required!(
            cadmpeg_ir::scalar::NonNegativeLength::from_finite_assigned_real(required!(
                scalar_named(ctx, properties, "Growth")?
            ),)
        );
        let revolutions = required!(cadmpeg_ir::scalar::PositiveReal::from_finite(required!(
            scalar_named(ctx, properties, "Rotations")?
        )));
        (
            cadmpeg_ir::features::HelixShape::Spiral {
                radial_growth: growth.into(),
            },
            revolutions,
            false,
            None,
        )
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Helix {
            axis_origin: cadmpeg_ir::features::FinitePoint3::ZERO,
            axis_direction: cadmpeg_ir::features::FeatureDirection3::Z_AXIS,
            radius,
            shape,
            revolutions,
            start_angle: cadmpeg_ir::scalar::Angle::ZERO,
            clockwise,
            segment_turns,
            construction_style,
        },
    )))
}

/// Persisted extent selectors and draft angles shared by the extrude readers.
struct ExtrudeSource {
    extent: ExtrudeExtentSelector,
    taper: Option<cadmpeg_ir::scalar::SlopeAngle>,
    taper_reverse: Option<cadmpeg_ir::scalar::SlopeAngle>,
    taper_second: Option<cadmpeg_ir::scalar::SlopeAngle>,
}

struct ExtrudeExtentSelector {
    side_type: Option<u64>,
    legacy_two_lengths: bool,
    first_termination: FirstTermination,
}

#[derive(Clone, Copy)]
enum FirstTermination {
    Unread,
    Invalid,
    Value(u64),
}

/// The draft angle the extrude states under `key`, in canonical radians.
///
/// `FreeCAD` writes every taper property unconditionally and spells "no draft"
/// with the native sentinel `0`, so a stated zero decodes to absence. A stated
/// non-zero angle is degrees; `SlopeAngle` admits it after the conversion to
/// radians and refuses a value that is not finite or whose magnitude is not
/// strictly below half pi, which is the draft that folds the drafted face onto
/// the sweep direction. That refusal is reported, not swallowed: dropping it
/// would delete the whole feature over one property.
fn taper_angle(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    key: &str,
) -> Result<Option<cadmpeg_ir::scalar::SlopeAngle>, CodecError> {
    let Some(degrees) = scalar_named(ctx, properties, key)?.filter(|angle| angle.get() != 0.0)
    else {
        return Ok(None);
    };
    cadmpeg_ir::scalar::SlopeAngle::try_from(degrees.get().to_radians())
        .map(Some)
        .map_err(|error| malformed_design(ctx, format_args!("{key}: {error}")))
}

/// Read extent selectors once. Type also selects the first side's termination.
fn extrude_side_type(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<ExtrudeExtentSelector, CodecError> {
    let side_type_property = property(ctx, properties, "SideType")?;
    let first_termination = if side_type_property.is_none() {
        match enumeration_selector(ctx, properties, "Type", 0)? {
            Some(value) => FirstTermination::Value(value),
            None => FirstTermination::Invalid,
        }
    } else {
        FirstTermination::Unread
    };
    let legacy_two_lengths = matches!(first_termination, FirstTermination::Value(4));
    let side_type = if legacy_two_lengths {
        Some(1)
    } else {
        match bool_selector(ctx, properties, "Midplane", false)? {
            Some(true) => Some(2),
            Some(false) if side_type_property.is_some() => {
                enumeration_selector(ctx, properties, "SideType", 0)?
            }
            Some(false) => Some(0),
            None => None,
        }
    };
    Ok(ExtrudeExtentSelector {
        side_type,
        legacy_two_lengths,
        first_termination,
    })
}

fn extrusion_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    profile: ProfileRef,
    profile_normal: Option<Vector3>,
    sketches: &[Sketch],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let face_maker_class = if kind == "Part::Extrusion" {
        match property(ctx, properties, "FaceMakerClass")? {
            Some(property) => string_property_value(ctx, property)?,
            None => None,
        }
    } else {
        None
    };
    // `TaperAngle2` states the draft of a second, independent side. The
    // property set an extrude record means depends on its extent kind, so the
    // reader decides the kind first and reads the second side's draft only
    // for the kind that carries one. A one-sided or midplane extrude has no
    // second side: reading a property its kind does not carry is over-reach,
    // and refusing the whole feature over it deletes what the record states.
    let extent = extrude_side_type(ctx, properties)?;
    let taper_second = if extent.side_type == Some(1) {
        taper_angle(ctx, properties, "TaperAngle2")?
    } else {
        None
    };
    let source = ExtrudeSource {
        extent,
        taper: taper_angle(ctx, properties, "TaperAngle")?,
        taper_reverse: taper_angle(ctx, properties, "TaperAngleRev")?,
        taper_second,
    };
    extrusion_shape(
        ctx,
        kind,
        properties,
        (profile, profile_normal),
        sketches,
        &source,
        face_maker_class,
    )
}

fn extrusion_shape(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    profile_and_normal: (ProfileRef, Option<Vector3>),
    sketches: &[Sketch],
    source: &ExtrudeSource,
    face_maker_class: Option<String>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let (profile, profile_normal) = profile_and_normal;
    if kind == "Part::Extrusion" {
        let raw_direction = vector_property(ctx, properties, "Dir")?;
        let direction_magnitude = raw_direction.map(|direction| direction.get().norm());
        let direction_mode = required!(enumeration_selector(ctx, properties, "DirMode", 0)?);
        let (mut direction, direction_source) = match direction_mode {
            0 => (
                required!(cadmpeg_ir::units::UnitVector3::normalized(
                    required!(raw_direction).get()
                )),
                ExtrusionDirectionSource::Custom {},
            ),
            1 => {
                let reference = required!(property(ctx, properties, "DirLink")?);
                if reference.links().len() != 1 {
                    return Ok(None);
                }
                (
                    required!(cadmpeg_ir::units::UnitVector3::normalized(
                        required!(raw_direction).get()
                    )),
                    ExtrusionDirectionSource::Edge {
                        reference: PathRef::Native(
                            ctx.copy_retained_text(
                                &reference.id,
                                "fcstd extrusion direction link",
                            )?,
                        ),
                    },
                )
            }
            2 => {
                let normal = required!(match &profile {
                    ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) => ctx
                        .find_by(
                            sketches,
                            |sketch| {
                                ctx.equal(&sketch.id, sketch_id, "fcstd extrusion profile identity")
                            },
                            "fcstd extrusion profile lookup",
                        )?
                        .and_then(Sketch::resolved_placement)
                        .map(|(_, normal, _)| normal.get())
                        .or(profile_normal),
                    _ => profile_normal,
                });
                (
                    required!(cadmpeg_ir::units::UnitVector3::normalized(normal)),
                    ExtrusionDirectionSource::ProfileNormal {},
                )
            }
            _ => return Ok(None),
        };
        let signed_length = |name| -> Result<_, CodecError> {
            Ok(match scalar_named(ctx, properties, name)? {
                Some(value) => Some(value),
                None => Some(FiniteReal::ZERO),
            })
        };
        let mut forward = required!(signed_length("LengthFwd")?).get();
        let reverse = required!(signed_length("LengthRev")?).get();
        if forward == 0.0 && reverse == 0.0 {
            forward =
                required!(direction_magnitude.filter(|value| value.is_finite() && *value > 0.0));
        }
        let symmetric = required!(bool_selector(ctx, properties, "Symmetric", false)?);
        let (extent, reverse_direction) = if symmetric {
            // A symmetric extent mirrors one side across the profile plane, so
            // its single side carries the taper once (from `TaperAngle`).
            (
                ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: required!(cadmpeg_ir::scalar::NonZeroLength::new(required!(
                                (forward != 0.0).then_some(forward.abs())
                            ),)),
                        },
                        draft: source.taper,
                    },
                },
                false,
            )
        } else {
            let forward_travel = (forward != 0.0).then_some((forward, source.taper));
            let reverse_travel = (reverse != 0.0).then_some((-reverse, source.taper_reverse));
            let same_side = forward_travel
                .zip(reverse_travel)
                .is_some_and(|((first, _), (second, _))| first.signum() == second.signum());
            if same_side && source.taper != source.taper_reverse {
                return Ok(None);
            }
            let farthest = |positive: bool| {
                [forward_travel, reverse_travel]
                    .into_iter()
                    .flatten()
                    .filter(|(travel, _)| (*travel > 0.0) == positive)
                    .max_by(|left, right| left.0.abs().total_cmp(&right.0.abs()))
            };
            let positive = farthest(true);
            let negative = farthest(false);
            match (positive, negative) {
                (Some((length, draft)), None) => (
                    ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: required!(cadmpeg_ir::scalar::NonZeroLength::new(length)),
                            },
                            draft,
                        },
                    },
                    false,
                ),
                (None, Some((length, draft))) => (
                    ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: required!(cadmpeg_ir::scalar::NonZeroLength::new(-length)),
                            },
                            draft,
                        },
                    },
                    true,
                ),
                (Some((first, first_draft)), Some((second, second_draft))) => (
                    ExtrudeExtent::TwoSided {
                        first: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: required!(cadmpeg_ir::scalar::NonZeroLength::new(first)),
                            },
                            draft: first_draft,
                        },
                        second: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: required!(cadmpeg_ir::scalar::NonZeroLength::new(-second)),
                            },
                            draft: second_draft,
                        },
                    },
                    false,
                ),
                (None, None) => return Ok(None),
            }
        };
        if reverse_direction ^ required!(bool_selector(ctx, properties, "Reversed", false)?) {
            direction = direction.reversed();
        }
        let face_maker = if property(ctx, properties, "FaceMakerClass")?.is_some() {
            let maker = required!(FaceMaker::new(ctx, required!(face_maker_class))?);
            if property(ctx, properties, "FaceMakerMode")?.is_some()
                && required!(u32::try_from(required!(integer_property(
                    ctx,
                    properties,
                    "FaceMakerMode"
                )?))
                .ok())
                    != maker.mode()
            {
                return Ok(None);
            }
            Some(maker)
        } else {
            None
        };
        let inner_wire_taper = if property(ctx, properties, "InnerWireTaper")?.is_some() {
            Some(
                match required!(integer_property(ctx, properties, "InnerWireTaper")?) {
                    0 => InnerWireTaper::Inverted,
                    1 => InnerWireTaper::SameAsOuter,
                    _ => return Ok(None),
                },
            )
        } else {
            None
        };
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile,
                direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                    vector: cadmpeg_ir::features::FeatureDirection3::from(direction),
                    source: Some(direction_source),
                },
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
                extent,
                op: BooleanOp::NewBody,
                solid: Some(required!(bool_selector(ctx, properties, "Solid", false)?)),
                face_maker,
                inner_wire_taper,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )));
    }
    let legacy_two_lengths = source.extent.legacy_two_lengths;
    let termination = |side: u8| -> Result<Option<LinearTermination>, CodecError> {
        let (type_name, length_name, offset_name, face_name, shape_name) = if side == 1 {
            ("Type", "Length", "Offset", "UpToFace", "UpToShape")
        } else {
            ("Type2", "Length2", "Offset2", "UpToFace2", "UpToShape2")
        };
        let termination_type = if legacy_two_lengths {
            0
        } else if side == 1 {
            match source.extent.first_termination {
                FirstTermination::Value(value) => value,
                FirstTermination::Invalid => return Ok(None),
                FirstTermination::Unread => {
                    required!(enumeration_selector(ctx, properties, type_name, 0)?)
                }
            }
        } else {
            required!(enumeration_selector(ctx, properties, type_name, 0)?)
        };
        let offset = if property(ctx, properties, offset_name)?.is_some() {
            Some(Length::from_assigned_real(required!(scalar_named(
                ctx,
                properties,
                offset_name,
            )?)))
        } else {
            None
        };
        Ok(match termination_type {
            0 => Some(LinearTermination::Blind {
                length: required!(cadmpeg_ir::scalar::NonZeroLength::from_assigned_real(
                    required!(scalar_named(ctx, properties, length_name,)?)
                )),
            }),
            1 if kind.contains("Pocket") => Some(LinearTermination::ThroughAll {}),
            1 => Some(LinearTermination::ToLast {}),
            2 => Some(LinearTermination::ToFirst {}),
            3 => Some(LinearTermination::ToFace {
                face: cadmpeg_ir::features::FaceSelection::Native(ctx.copy_retained_text(
                    &required!(singular_operand(ctx, properties, face_name)?).id,
                    "fcstd extrusion face termination",
                )?),
                offset,
            }),
            5 => Some(LinearTermination::ToShape {
                target: cadmpeg_ir::features::FaceSelection::Native(ctx.copy_retained_text(
                    &required!(singular_operand(ctx, properties, shape_name)?).id,
                    "fcstd extrusion shape termination",
                )?),
            }),
            _ => None,
        })
    };
    let side_type = required!(source.extent.side_type);
    // `TaperAngle2` describes a second, independent side and reaches the IR
    // only when the extent actually carries one (`SideType` 1 / two-sided). A
    // symmetric (Midplane) pad mirrors side one, so it has no second side to
    // receive it; the native property remains retained but maps nowhere.
    let extent = match side_type {
        0 => ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: required!(termination(1)?),
                draft: source.taper,
            },
        },
        1 => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: required!(termination(1)?),
                draft: source.taper,
            },
            second: ExtrudeSide {
                termination: required!(termination(2)?),
                draft: source.taper_second,
            },
        },
        2 => ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: required!(termination(1)?),
                draft: source.taper,
            },
        },
        _ => return Ok(None),
    };
    let use_custom = required!(bool_selector(ctx, properties, "UseCustomVector", false)?);
    let reference_axis = match property(ctx, properties, "ReferenceAxis")? {
        Some(property) => {
            let nonempty = |link: &Option<crate::native::LinkTarget>| {
                Ok(link
                    .as_ref()
                    .is_some_and(|link| link.document().is_some() || link.object().is_some()))
            };
            match ctx.position_by(
                property.links(),
                nonempty,
                "fcstd extrusion reference axis lookup",
            )? {
                None => None,
                Some(index) => {
                    if ctx.any_by(
                        &property.links()[index + 1..],
                        nonempty,
                        "fcstd extrusion reference axis count",
                    )? {
                        return Ok(None);
                    }
                    Some(property)
                }
            }
        }
        None => None,
    };
    let mut direction = if use_custom {
        cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: cadmpeg_ir::features::FeatureDirection3::from(required!(
                cadmpeg_ir::units::UnitVector3::normalized(
                    required!(vector_property(ctx, properties, "Direction")?).get(),
                )
            )),
            source: Some(ExtrusionDirectionSource::Custom {}),
        }
    } else if let Some(reference_axis) = reference_axis {
        cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: cadmpeg_ir::features::FeatureDirection3::from(required!(
                cadmpeg_ir::units::UnitVector3::normalized(
                    required!(vector_property(ctx, properties, "Direction")?).get(),
                )
            )),
            source: Some(ExtrusionDirectionSource::Edge {
                reference: PathRef::Native(
                    ctx.copy_retained_text(&reference_axis.id, "fcstd extrusion reference axis")?,
                ),
            }),
        }
    } else {
        let normal = match &profile {
            ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) => ctx
                .find_by(
                    sketches,
                    |sketch| ctx.equal(&sketch.id, sketch_id, "fcstd extrusion profile identity"),
                    "fcstd extrusion profile lookup",
                )?
                .and_then(Sketch::resolved_placement)
                .map(|(_, normal, _)| normal.get())
                .or(profile_normal),
            ProfileRef::Planar(PlanarProfileRef::Native(_)) => profile_normal,
            _ => return Ok(None),
        }
        .and_then(cadmpeg_ir::units::UnitVector3::normalized);
        match normal {
            Some(vector) => cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::FeatureDirection3::from(vector),
                source: Some(ExtrusionDirectionSource::ProfileNormal {}),
            },
            None => cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
        }
    };
    if required!(bool_selector(ctx, properties, "Reversed", false)?) {
        let cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } = &mut direction else {
            return Ok(None);
        };
        *vector = vector.reversed();
    }
    let length_along_profile_normal = Some(required!(bool_selector(
        ctx,
        properties,
        "AlongSketchNormal",
        true
    )?));
    let allow_multi_profile_faces = Some(required!(bool_selector(
        ctx,
        properties,
        "AllowMultiFace",
        false
    )?));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Extrude {
            profile,
            direction,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            extent,
            op: if kind.contains("Pocket") {
                BooleanOp::Cut
            } else {
                BooleanOp::Join
            },
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal,
            allow_multi_profile_faces,
        },
    )))
}

fn dress_up_edge_selection(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<EdgeSelection>, CodecError> {
    let use_all_edges = if matches!(kind, "PartDesign::Fillet" | "PartDesign::Chamfer") {
        let Some(value) = bool_selector(ctx, properties, "UseAllEdges", false)? else {
            return Ok(None);
        };
        value
    } else {
        false
    };
    if use_all_edges {
        return Ok(Some(EdgeSelection::All));
    }
    Ok(Some(match property(ctx, properties, "Base")? {
        Some(property) => EdgeSelection::Native(
            ctx.copy_retained_text(&property.id, "fcstd dress-up edge selection")?,
        ),
        None => EdgeSelection::Unresolved,
    }))
}

fn scale_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(base) = singular_operand(ctx, properties, "Base")? else {
        return Ok(None);
    };
    let factor = |name| -> Result<_, CodecError> {
        Ok(scalar_named(ctx, properties, name)?
            .and_then(cadmpeg_ir::scalar::NonZeroReal::from_finite))
    };
    let Some(uniform) = bool_selector(ctx, properties, "Uniform", true)? else {
        return Ok(None);
    };
    let factors = if uniform {
        let Some(factor) = factor("UniformScale")? else {
            return Ok(None);
        };
        ScaleFactors::Uniform { factor }
    } else {
        let Some(x) = factor("XScale")? else {
            return Ok(None);
        };
        let Some(y) = factor("YScale")? else {
            return Ok(None);
        };
        let Some(z) = factor("ZScale")? else {
            return Ok(None);
        };
        ScaleFactors::PerAxis { factors: [x, y, z] }
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Scale {
            bodies: BodySelection::Native(
                ctx.copy_retained_text(&base.id, "fcstd scale base selection")?,
            ),
            center: Some(ScaleCenter::ModelOrigin),
            factors,
        },
    )))
}

fn fillet_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(edges) = dress_up_edge_selection(ctx, kind, properties)? else {
        return Ok(None);
    };
    if matches!(edges, EdgeSelection::Unresolved) {
        return Ok(None);
    }
    let (part_values, _part_value_storage) =
        ctx.with_scoped_storage("fcstd edge-treatment source values", || {
            Ok::<_, CodecError>(if kind == "Part::Fillet" {
                part_fillet_edge_values(ctx, properties, entries)?
            } else {
                None
            })
        })?;
    let radius = if kind == "Part::Fillet" {
        let values = required!(part_values);
        let radius = required!(cadmpeg_ir::scalar::PositiveLength::new(
            required!(values.first())[0]
        ));
        required!(ctx
            .all_by(
                &values,
                |[first, second]| Ok(*first == radius.get() && *second == radius.get()),
                "fcstd part fillet uniform radius check",
            )?
            .then_some(()));
        radius
    } else {
        required!(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
            required!(scalar_named(ctx, properties, "Radius",)?)
        ))
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: ctx
                .collect_vec(
                    [cadmpeg_ir::features::edge_treatments::FilletGroup {
                        edges,
                        radius: RadiusSpec::Constant { radius },
                        tangency_weight: None,
                    }],
                    "fcstd fillet groups",
                )?
                .try_into()
                .map_err(CodecError::malformed)?,
        },
    )))
}

fn chamfer_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
    program_version: Option<&str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(edges) = dress_up_edge_selection(ctx, kind, properties)? else {
        return Ok(None);
    };
    if matches!(edges, EdgeSelection::Unresolved) {
        return Ok(None);
    }
    let (part_values, _part_value_storage) =
        ctx.with_scoped_storage("fcstd edge-treatment source values", || {
            Ok::<_, CodecError>(if kind == "Part::Chamfer" {
                part_fillet_edge_values(ctx, properties, entries)?
            } else {
                None
            })
        })?;
    let (spec, legacy_flip_mode) = if kind == "Part::Chamfer" {
        let values = required!(part_values);
        let [first_raw, second_raw] = *required!(values.first());
        let first = required!(cadmpeg_ir::scalar::PositiveLength::new(first_raw));
        let second = required!(cadmpeg_ir::scalar::PositiveLength::new(second_raw));
        if !ctx.all_by(
            &values,
            |[candidate_first, candidate_second]| {
                Ok(*candidate_first == first.get() && *candidate_second == second.get())
            },
            "fcstd part chamfer uniform distance check",
        )? {
            return Ok(None);
        }
        let spec = if first == second {
            ChamferSpec::Distance { distance: first }
        } else {
            ChamferSpec::TwoDistances { first, second }
        };
        (spec, false)
    } else {
        required!(chamfer_spec(ctx, properties)?)
    };
    let flip_direction = if kind == "PartDesign::Chamfer" {
        required!(bool_selector(ctx, properties, "FlipDirection", false)?)
    } else {
        false
    };
    let legacy_flip = kind == "PartDesign::Chamfer"
        && program_version.is_some_and(|version| version.starts_with('0'))
        && legacy_flip_mode;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: ctx
                .collect_vec(
                    [cadmpeg_ir::features::edge_treatments::ChamferGroup { edges, spec }],
                    "fcstd chamfer groups",
                )?
                .try_into()
                .map_err(CodecError::malformed)?,
            flip_direction: if legacy_flip {
                !flip_direction
            } else {
                flip_direction
            },
        },
    )))
}

fn part_fillet_edge_values(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Option<Vec<[f64; 2]>>, CodecError> {
    let Some(property) = property(ctx, properties, "Edges")? else {
        return Ok(None);
    };
    let Some(entry_name) = property.side_entries().first() else {
        return Ok(None);
    };
    let Some(data) = ctx
        .find_by(
            entries,
            |entry| ctx.equal(entry.name(), entry_name.as_str(), "fcstd fillet entry name"),
            "fcstd fillet entry lookup",
        )?
        .map(crate::native::EntryRecord::data)
    else {
        return Ok(None);
    };
    let mut view = View::over_retained(data);
    let Some(count) = view.u32_le() else {
        return Ok(None);
    };
    if usize::try_from(count).map_err(|_| {
        ctx.refuse_codec_limit(
            "FreeCAD count",
            cadmpeg_core::decode::u64_from_index(usize::MAX),
            u64::from(count),
        )
    })? > MAX_SKETCH_RECORDS
    {
        return Ok(None);
    }
    let Some(bounded) = view.counted(u64::from(count), 20) else {
        return Ok(None);
    };
    let mut values = ctx.vector_storage(bounded.get(), "fcstd fillet edge values")?;
    for _ in ctx.admit_iter(&(0..count), "fcstd fillet edge values")? {
        let Some((_, first, second)) =
            (|| Some((view.u32_le()?, view.f64_le()?, view.f64_le()?)))()
        else {
            return Ok(None);
        };
        ctx.push_vec(&mut values, [first, second], "fcstd fillet edge values")?;
    }
    Ok(view.is_empty().then_some(values))
}

fn shell_mode(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<ShellMode>, CodecError> {
    let absent_default = u64::from(kind == "Part::Offset2D");
    Ok(
        match required!(enumeration_selector(
            ctx,
            properties,
            "Mode",
            absent_default
        )?) {
            0 => Some(ShellMode::Skin),
            1 => Some(ShellMode::Pipe),
            2 if kind != "Part::Offset2D" => Some(ShellMode::BothSides),
            _ => None,
        },
    )
}

fn shell_join(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<ShellJoin>, CodecError> {
    Ok(
        match required!(enumeration_selector(ctx, properties, "Join", 0)?) {
            0 => Some(ShellJoin::Arc),
            1 if kind == "PartDesign::Thickness" => Some(ShellJoin::Intersection),
            1 => Some(ShellJoin::Tangent),
            2 => Some(ShellJoin::Intersection),
            _ => None,
        },
    )
}

fn thickness_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(thickness) = scalar_named(ctx, properties, "Value")? else {
        return Ok(None);
    };
    if thickness.get() == 0.0 {
        return Ok(None);
    }
    let source_name = if kind == "Part::Thickness" {
        "Faces"
    } else {
        "Base"
    };
    let Some(selection) = property(ctx, properties, source_name)? else {
        return Ok(None);
    };
    if selection.links().is_empty() {
        return Ok(None);
    }
    let outward = thickness.get() > 0.0;
    let Some(thickness) = cadmpeg_ir::scalar::PositiveLength::from_assigned_real(thickness.abs())
    else {
        return Ok(None);
    };
    let Some(mode) = shell_mode(ctx, kind, properties)? else {
        return Ok(None);
    };
    let Some(join) = shell_join(ctx, kind, properties)? else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Shell {
            bodies: None,
            removed_faces: cadmpeg_ir::features::FaceSelection::Native(
                ctx.copy_retained_text(&selection.id, "fcstd thickness faces identity")?,
            ),
            thickness: Some(thickness),
            outward: Some(if kind == "Part::Thickness" {
                outward
            } else {
                !bool_property(ctx, properties, "Reversed")?.unwrap_or(false)
            }),
            mode: Some(mode),
            join: Some(join),
            resolve_intersections: Some(
                bool_property(ctx, properties, "Intersection")?.unwrap_or(false),
            ),
            allow_self_intersections: Some(
                bool_property(ctx, properties, "SelfIntersection")?.unwrap_or(false),
            ),
        },
    )))
}

fn offset_shape_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(source) = singular_operand(ctx, properties, "Source")? else {
        return Ok(None);
    };
    let Some(distance) = scalar_named(ctx, properties, "Value")?
        .and_then(cadmpeg_ir::scalar::NonZeroLength::from_assigned_real)
    else {
        return Ok(None);
    };
    let Some(mode) = shell_mode(ctx, kind, properties)? else {
        return Ok(None);
    };
    if kind == "Part::Offset2D" && mode == ShellMode::BothSides {
        return Ok(None);
    }
    let Some(join) = shell_join(ctx, kind, properties)? else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::OffsetShape {
            source: BodySelection::Native(
                ctx.copy_retained_text(&source.id, "fcstd offset source identity")?,
            ),
            distance,
            mode,
            join,
            resolve_intersections: bool_property(ctx, properties, "Intersection")?.unwrap_or(false),
            allow_self_intersections: bool_property(ctx, properties, "SelfIntersection")?
                .unwrap_or(false),
            fill: bool_property(ctx, properties, "Fill")?.unwrap_or(false),
            planar: kind == "Part::Offset2D",
        },
    )))
}

fn derived_shape_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    match kind {
        "Part::Compound" | "Part::Compound2" => {
            let Some(links) = property(ctx, properties, "Links")? else {
                return Ok(property(ctx, properties, "Shape")?
                    .map(|_| FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})));
            };
            if links.links().is_empty() {
                return Ok(None);
            }
            Ok(Some(FeatureDefinition::Operation(
                FeatureOperation::Compound {
                    members: BodySelection::Native(
                        ctx.copy_retained_text(&links.id, "fcstd compound members identity")?,
                    ),
                },
            )))
        }
        "Part::Refine" | "Part::Reverse" => {
            let Some(source) = property(ctx, properties, "Source")? else {
                return Ok(None);
            };
            if source.links().len() != 1 {
                return Ok(None);
            }
            let source = BodySelection::Native(
                ctx.copy_retained_text(&source.id, "fcstd derived source identity")?,
            );
            Ok(Some(if kind == "Part::Refine" {
                FeatureDefinition::Operation(FeatureOperation::RefineShape { source })
            } else {
                FeatureDefinition::Operation(FeatureOperation::ReverseShape { source })
            }))
        }
        _ => Ok(None),
    }
}

fn cached_shape_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    Ok(property(ctx, properties, "Shape")?
        .filter(|shape| !shape.side_entries().is_empty())
        .map(|_| FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})))
}

fn ruled_surface_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(first) =
        property(ctx, properties, "Curve1")?.filter(|property| property.links().len() == 1)
    else {
        return Ok(None);
    };
    let Some(second) =
        property(ctx, properties, "Curve2")?.filter(|property| property.links().len() == 1)
    else {
        return Ok(None);
    };
    let orientation = match integer_property(ctx, properties, "Orientation")?.unwrap_or(0) {
        0 => RuledCurveOrientation::Automatic,
        1 => RuledCurveOrientation::Forward,
        2 => RuledCurveOrientation::Reversed,
        _ => return Ok(None),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::RuledBetweenCurves {
            first: PathRef::Native(
                ctx.copy_retained_text(&first.id, "fcstd ruled first curve identity")?,
            ),
            second: PathRef::Native(
                ctx.copy_retained_text(&second.id, "fcstd ruled second curve identity")?,
            ),
            orientation,
        },
    )))
}

fn section_shape_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(base) =
        property(ctx, properties, "Base")?.filter(|property| property.links().len() == 1)
    else {
        return Ok(None);
    };
    let Some(tool) =
        property(ctx, properties, "Tool")?.filter(|property| property.links().len() == 1)
    else {
        return Ok(None);
    };
    let base =
        BodySelection::Native(ctx.copy_retained_text(&base.id, "fcstd section base identity")?);
    let tool =
        BodySelection::Native(ctx.copy_retained_text(&tool.id, "fcstd section tool identity")?);
    cadmpeg_ir::features::SectionOperands::new(base, tool, ctx)?
        .ok()
        .map(|operands| -> Result<_, CodecError> {
            Ok(FeatureDefinition::Operation(
                FeatureOperation::SectionShape {
                    operands,
                    approximate: Some(
                        bool_property(ctx, properties, "Approximation")?.unwrap_or(false),
                    ),
                },
            ))
        })
        .transpose()
}

fn mirror_shape_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(source) = property(ctx, properties, "Source")? else {
        return Ok(None);
    };
    if source.links().len() != 1 {
        return Ok(None);
    }
    let Some(origin) = vector_property(ctx, properties, "Base")? else {
        return Ok(None);
    };
    let plane_reference = match property(ctx, properties, "MirrorPlane")? {
        Some(property)
            if ctx.any_by(
                property.links(),
                |link| Ok(nonempty_link(link.as_ref())),
                "fcstd mirror plane links",
            )? =>
        {
            Some(property)
        }
        _ => None,
    };
    let Some(plane_normal) = vector_property(ctx, properties, "Normal")?
        .and_then(|normal| cadmpeg_ir::units::UnitVector3::normalized(normal.get()))
    else {
        return Ok(None);
    };
    let plane_reference = plane_reference
        .map(|property| {
            ctx.copy_retained_text(&property.id, "fcstd mirror plane identity")
                .map(cadmpeg_ir::features::FaceSelection::Native)
        })
        .transpose()?;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MirrorShape {
            source: BodySelection::Native(
                ctx.copy_retained_text(&source.id, "fcstd mirror source identity")?,
            ),
            plane_origin: origin.as_point(),
            plane_normal,
            plane_reference,
        },
    )))
}

fn project_on_surface_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(sources) = property(ctx, properties, "Projection")? else {
        return Ok(None);
    };
    if sources.links().is_empty() {
        return Ok(None);
    }
    let Some(support) = property(ctx, properties, "SupportFace")? else {
        return Ok(None);
    };
    if support.links().len() != 1 {
        return Ok(None);
    }
    let Some(mode_selector) = enumeration_selector(ctx, properties, "Mode", 0)? else {
        return Ok(None);
    };
    let mode = match mode_selector {
        0 => SurfaceProjectionMode::All,
        1 => SurfaceProjectionMode::Faces,
        2 => SurfaceProjectionMode::Edges,
        _ => return Ok(None),
    };
    let height = if property(ctx, properties, "Height")?.is_some() {
        let Some(value) = scalar_named(ctx, properties, "Height")?
            .and_then(cadmpeg_ir::scalar::NonNegativeLength::from_finite_assigned_real)
        else {
            return Ok(None);
        };
        value
    } else {
        cadmpeg_ir::scalar::NonNegativeLength::ZERO
    };
    let offset = if property(ctx, properties, "Offset")?.is_some() {
        let Some(value) = scalar_named(ctx, properties, "Offset")? else {
            return Ok(None);
        };
        Length::from_assigned_real(value)
    } else {
        Length::ZERO
    };
    let Some(direction) = vector_property(ctx, properties, "Direction")?
        .and_then(|value| cadmpeg_ir::units::UnitVector3::normalized(value.get()))
    else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::ProjectOnSurface {
            sources: PathRef::Native(
                ctx.copy_retained_text(&sources.id, "fcstd projection sources identity")?,
            ),
            support_face: cadmpeg_ir::features::FaceSelection::Native(
                ctx.copy_retained_text(&support.id, "fcstd projection support identity")?,
            ),
            direction,
            mode,
            height,
            offset,
        },
    )))
}

fn draft_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    objects: &BTreeMap<&str, &ObjectRecord>,
    properties_by_owner: &BTreeMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(faces) = property(ctx, properties, "Base")? else {
        return Ok(None);
    };
    let Some(neutral_plane) = property(ctx, properties, "NeutralPlane")? else {
        return Ok(None);
    };
    let plane_normal = plane_reference(
        ctx,
        properties,
        "NeutralPlane",
        objects,
        properties_by_owner,
    )?
    .map(|(_, normal)| normal);
    let has_pull_direction = match property(ctx, properties, "PullDirection")? {
        Some(property) => ctx.any_by(
            property.links(),
            |link| Ok(nonempty_link(link.as_ref())),
            "fcstd draft pull direction links",
        )?,
        None => false,
    };
    let pull_direction = if has_pull_direction {
        axis_reference(
            ctx,
            properties,
            "PullDirection",
            objects,
            properties_by_owner,
        )?
        .map(|(_, direction)| direction)
    } else {
        plane_normal
    };
    let reversed = bool_property(ctx, properties, "Reversed")?.unwrap_or(false);
    let Some(angle) = scalar_named(ctx, properties, "Angle")? else {
        return Ok(None);
    };
    let Some(angle) = cadmpeg_ir::scalar::SlopeAngle::new(
        if reversed { -angle.get() } else { angle.get() }.to_radians(),
    ) else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Native(
                ctx.copy_retained_text(&faces.id, "fcstd draft faces identity")?,
            ),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: cadmpeg_ir::features::FaceSelection::Native(
                    ctx.copy_retained_text(
                        &neutral_plane.id,
                        "fcstd draft neutral plane identity",
                    )?,
                ),
                pull: pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
                    direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
                    plane: None,
                }),
            },
            angle: Some(angle),
            outward: Some(reversed),
        },
    )))
}

fn chamfer_spec(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<(ChamferSpec, bool)>, CodecError> {
    let value = required!(property(ctx, properties, "ChamferType")?
        .map(|property| scalar_value(ctx, property))
        .transpose()?
        .unwrap_or(Some(FiniteReal::ZERO)));
    let mode = match value.get() {
        value if value > -1.0 && value < 1.0 => 0,
        value if (1.0..2.0).contains(&value) => 1,
        value if (2.0..3.0).contains(&value) => 2,
        _ => return Ok(None),
    };
    let legacy_flip_mode = value.get() == 1.0 || value.get() == 2.0;
    let first = property(ctx, properties, "Size")?
        .map(|property| scalar_value(ctx, property))
        .transpose()?
        .flatten()
        .and_then(cadmpeg_ir::scalar::PositiveLength::from_assigned_real);
    let spec = match (mode, first) {
        (0, Some(distance)) => Some(ChamferSpec::Distance { distance }),
        (1, Some(first)) => property(ctx, properties, "Size2")?
            .map(|property| scalar_value(ctx, property))
            .transpose()?
            .flatten()
            .and_then(cadmpeg_ir::scalar::PositiveLength::from_assigned_real)
            .map(|second| ChamferSpec::TwoDistances { first, second }),
        (2, Some(distance)) => property(ctx, properties, "Angle")?
            .map(|property| scalar_value(ctx, property))
            .transpose()?
            .flatten()
            .filter(|angle| angle.get() > 0.0 && angle.get() < 180.0)
            .and_then(|angle| {
                Some(ChamferSpec::DistanceAngle {
                    distance,
                    angle: cadmpeg_ir::scalar::InteriorAngle::new(angle.get().to_radians())?,
                })
            }),
        _ => None,
    };
    Ok(spec.map(|spec| (spec, legacy_flip_mode)))
}

fn property<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    ctx.find_by(
        properties,
        |property| Ok(property.name == name),
        "fcstd design property lookup",
    )
    .map(Option::<&&PropertyRecord>::copied)
}

fn nonempty_link(link: Option<&crate::native::LinkTarget>) -> bool {
    link.is_some_and(|link| {
        link.document().is_some() || link.object().is_some_and(|object| !object.is_empty())
    })
}

fn singular_operand<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(None);
    };
    let [Some(link)] = property.links() else {
        return Ok(None);
    };
    Ok(link.object().map(|_| property))
}

fn is_vector_property_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyVector"
            | "App::PropertyVectorDistance"
            | "App::PropertyPosition"
            | "App::PropertyDirection"
    )
}

fn scalar_value_tag(type_name: &str) -> Option<&'static str> {
    match type_name {
        "App::PropertyBool" => Some("Bool"),
        "App::PropertyEnumeration"
        | "App::PropertyInteger"
        | "App::PropertyIntegerConstraint"
        | "App::PropertyPercent" => Some("Integer"),
        "App::PropertyFloat"
        | "App::PropertyFloatConstraint"
        | "App::PropertyPrecision"
        | "App::PropertyAcceleration"
        | "App::PropertyAmountOfSubstance"
        | "App::PropertyAngle"
        | "App::PropertyArea"
        | "App::PropertyCompressiveStrength"
        | "App::PropertyCurrentDensity"
        | "App::PropertyDensity"
        | "App::PropertyDissipationRate"
        | "App::PropertyDistance"
        | "App::PropertyDynamicViscosity"
        | "App::PropertyElectricalCapacitance"
        | "App::PropertyElectricalConductance"
        | "App::PropertyElectricalConductivity"
        | "App::PropertyElectricalInductance"
        | "App::PropertyElectricalResistance"
        | "App::PropertyElectricCharge"
        | "App::PropertySurfaceChargeDensity"
        | "App::PropertyVolumeChargeDensity"
        | "App::PropertyElectricCurrent"
        | "App::PropertyElectricPotential"
        | "App::PropertyElectromagneticPotential"
        | "App::PropertyFrequency"
        | "App::PropertyForce"
        | "App::PropertyHeatFlux"
        | "App::PropertyInverseArea"
        | "App::PropertyInverseLength"
        | "App::PropertyInverseVolume"
        | "App::PropertyKinematicViscosity"
        | "App::PropertyLength"
        | "App::PropertyLuminousIntensity"
        | "App::PropertyMagneticFieldStrength"
        | "App::PropertyMagneticFlux"
        | "App::PropertyMagneticFluxDensity"
        | "App::PropertyMagnetization"
        | "App::PropertyMass"
        | "App::PropertyMoment"
        | "App::PropertyPressure"
        | "App::PropertyPower"
        | "App::PropertyQuantity"
        | "App::PropertyQuantityConstraint"
        | "App::PropertyShearModulus"
        | "App::PropertySpecificEnergy"
        | "App::PropertySpecificHeat"
        | "App::PropertySpeed"
        | "App::PropertyStiffness"
        | "App::PropertyStiffnessDensity"
        | "App::PropertyStress"
        | "App::PropertyTemperature"
        | "App::PropertyThermalConductivity"
        | "App::PropertyThermalExpansionCoefficient"
        | "App::PropertyThermalTransferCoefficient"
        | "App::PropertyTime"
        | "App::PropertyUltimateTensileStrength"
        | "App::PropertyVacuumPermittivity"
        | "App::PropertyVelocity"
        | "App::PropertyVolume"
        | "App::PropertyVolumeFlowRate"
        | "App::PropertyVolumetricThermalExpansionCoefficient"
        | "App::PropertyWork"
        | "App::PropertyYieldStrength"
        | "App::PropertyYoungsModulus" => Some("Float"),
        _ => None,
    }
}

fn text_value_tag(type_name: &str) -> Option<&'static str> {
    scalar_value_tag(type_name).or(match type_name {
        "App::PropertyBoolList" => Some("BoolList"),
        "App::PropertyFile"
        | "App::PropertyFont"
        | "App::PropertyPersistentObject"
        | "App::PropertyString" => Some("String"),
        "App::PropertyFileIncluded" => Some("FileIncluded"),
        "App::PropertyPath" => Some("Path"),
        "App::PropertyUUID" => Some("Uuid"),
        _ => None,
    })
}

fn scalar_value(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<FiniteReal>, CodecError> {
    let tag = required!(scalar_value_tag(&property.type_name));
    if tag == "Bool" {
        return Ok(None);
    }
    let value = required!(direct_root_value(
        ctx,
        property,
        tag,
        "value",
        |ctx, value| {
            let Ok(value) = ctx.parse_text::<f64>(value, "fcstd scalar property value")? else {
                return Ok(None);
            };
            Ok(FiniteReal::new(value))
        }
    )?);
    Ok(Some(value))
}

fn scalar_text<T>(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    use_value: impl FnOnce(&DecodeContext<'_>, &str) -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    let tag = required!(text_value_tag(&property.type_name));
    direct_root_value(ctx, property, tag, "value", use_value)
}

fn direct_root_value<T>(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_tag: &str,
    attribute: &str,
    use_value: impl FnOnce(&DecodeContext<'_>, &str) -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    direct_root(ctx, property, expected_tag, |ctx, root| {
        let Some(value) = ctx.xml_attribute(root, attribute, "FreeCAD design XML attribute")?
        else {
            return Ok(None);
        };
        use_value(ctx, value)
    })
    .map(Option::flatten)
}

fn direct_root<T>(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_tag: &str,
    use_root: impl FnOnce(&DecodeContext<'_>, roxmltree::Node<'_, '_>) -> Result<T, CodecError>,
) -> Result<Option<T>, CodecError> {
    let admitted_document =
        match ctx.parse_xml(property.xml.text(), "FreeCAD direct property XML tree") {
            Ok(tree) => tree,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(None),
        };
    let document = admitted_document.document();
    let wrapper = ctx.xml_root_element(document, "FreeCAD direct property root")?;
    let mut root = None;
    let mut nodes = document.descendants();
    while let Some(node) = ctx.next_charged(&mut nodes, "FreeCAD direct property values")? {
        if !ctx.xml_has_tag_name(node, expected_tag, "FreeCAD direct property tag")? {
            continue;
        }
        if root.is_some() || node.parent() != Some(wrapper) {
            return Ok(None);
        }
        root = Some(node);
    }
    match root {
        Some(root) => Ok(Some(use_root(ctx, root)?)),
        None => Ok(None),
    }
}

fn native_parameters(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let mut parameters = BTreeMap::new();
    for property in ctx.admit_iter(properties, "fcstd native parameter properties")? {
        let Some(value) = scalar_text(ctx, property, |ctx, text| {
            Ok(Some(ctx.copy_retained_text(
                text,
                "fcstd native parameter value",
            )?))
        })?
        else {
            continue;
        };
        let Some(name) = NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(&property.name, "fcstd native parameter name")?,
            "validate nonblank text",
        )?
        else {
            continue;
        };
        ctx.insert_btree_map(&mut parameters, name, value, "fcstd native parameters")?;
    }
    Ok(parameters)
}

fn native_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: ctx
            .copy_retained_text(kind, "fcstd native feature kind")?
            .into(),
        parameters: native_parameters(ctx, properties)?,
    }))
}

fn primitive_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let length = |name: &str| -> Result<_, CodecError> {
        Ok(property(ctx, properties, name)?
            .map(|property| scalar_value(ctx, property))
            .transpose()?
            .flatten()
            .map(Length::from_assigned_real))
    };
    let angle = |name: &str| -> Result<_, CodecError> {
        Ok(property(ctx, properties, name)?
            .map(|property| scalar_value(ctx, property))
            .transpose()?
            .flatten()
            .and_then(|value| cadmpeg_ir::scalar::Angle::new(value.get().to_radians())))
    };
    let solid = if kind.ends_with("Box") {
        PrimitiveSolidKind::Box {
            length: required!(length("Length")?),
            width: required!(length("Width")?),
            height: required!(length("Height")?),
        }
    } else if kind.ends_with("Cylinder") {
        PrimitiveSolidKind::Cylinder {
            radius: required!(length("Radius")?),
            height: required!(length("Height")?),
            angle: required!(angle("Angle")?),
        }
    } else if kind.ends_with("Cone") {
        let radius1 = required!(length("Radius1")?);
        let radius2 = required!(length("Radius2")?);
        PrimitiveSolidKind::Cone {
            radius1,
            radius2,
            height: required!(length("Height")?),
            angle: required!(angle("Angle")?),
        }
    } else if kind.ends_with("Sphere") {
        PrimitiveSolidKind::Sphere {
            radius: required!(length("Radius")?),
            latitude1: required!(angle("Angle1")?),
            latitude2: required!(angle("Angle2")?),
            longitude: required!(angle("Angle3")?),
        }
    } else if kind.ends_with("Ellipsoid") {
        let x_radius = required!(length("Radius2")?);
        let y_radius = required!(length("Radius3")?);
        PrimitiveSolidKind::Ellipsoid {
            x_radius,
            y_radius: if y_radius.get() == 0.0 {
                x_radius
            } else {
                y_radius
            },
            z_radius: required!(length("Radius1")?),
            latitude1: required!(angle("Angle1")?),
            latitude2: required!(angle("Angle2")?),
            longitude: required!(angle("Angle3")?),
        }
    } else if kind.ends_with("Torus") {
        PrimitiveSolidKind::Torus {
            major_radius: required!(length("Radius1")?),
            minor_radius: required!(length("Radius2")?),
            latitude1: required!(angle("Angle1")?),
            latitude2: required!(angle("Angle2")?),
            longitude: required!(angle("Angle3")?),
        }
    } else if kind.ends_with("Prism") {
        PrimitiveSolidKind::Prism {
            sides: required!(u32::try_from(required!(integer_property(
                ctx, properties, "Polygon"
            )?))
            .ok()),
            circumradius: required!(length("Circumradius")?),
            height: required!(length("Height")?),
        }
    } else if kind.ends_with("Wedge") {
        PrimitiveSolidKind::Wedge {
            xmin: required!(length("Xmin")?),
            ymin: required!(length("Ymin")?),
            zmin: required!(length("Zmin")?),
            x2min: required!(length("X2min")?),
            z2min: required!(length("Z2min")?),
            xmax: required!(length("Xmax")?),
            ymax: required!(length("Ymax")?),
            zmax: required!(length("Zmax")?),
            x2max: required!(length("X2max")?),
            z2max: required!(length("Z2max")?),
        }
    } else {
        return Ok(None);
    };
    let op = operation_boolean(kind);
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Primitive {
            solid: required!(PrimitiveSolid::new(solid).ok()),
            op,
        },
    )))
}

fn datum_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((origin, z_axis, x_axis, y_axis)) = placement_frame(ctx, properties)? else {
        return Ok(None);
    };
    let definition = match kind {
        "PartDesign::Plane" => {
            let Some(frame) =
                cadmpeg_ir::features::FeatureDatumPlaneFrame::new(origin, z_axis, x_axis)
            else {
                return Ok(None);
            };
            FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame })
        }
        "PartDesign::Line" => {
            let Some(origin) = cadmpeg_ir::features::FinitePoint3::new(origin) else {
                return Ok(None);
            };
            let Some(direction) = cadmpeg_ir::features::FeatureDirection3::new(z_axis) else {
                return Ok(None);
            };
            FeatureDefinition::Operation(FeatureOperation::DatumAxis { origin, direction })
        }
        "PartDesign::Point" => {
            let Some(position) = cadmpeg_ir::features::FinitePoint3::new(origin) else {
                return Ok(None);
            };
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position,
                construction: None,
            })
        }
        "PartDesign::CoordinateSystem" => {
            let Some(frame) =
                cadmpeg_ir::features::FeatureCoordinateFrame::new(origin, x_axis, y_axis, z_axis)
            else {
                return Ok(None);
            };
            FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { frame })
        }
        _ => return Ok(None),
    };
    Ok(Some(definition))
}

fn boolean_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let op = if kind == "PartDesign::Boolean" {
        let Some(selector) = enumeration_selector(ctx, properties, "Type", 0)? else {
            return Ok(None);
        };
        match selector {
            0 => cadmpeg_ir::features::BooleanKind::Join,
            1 => cadmpeg_ir::features::BooleanKind::Cut,
            2 => cadmpeg_ir::features::BooleanKind::Intersect,
            _ => return Ok(None),
        }
    } else if kind.ends_with("Cut") {
        cadmpeg_ir::features::BooleanKind::Cut
    } else if kind.ends_with("Common") || kind.ends_with("MultiCommon") {
        cadmpeg_ir::features::BooleanKind::Intersect
    } else if kind.ends_with("Fuse") || kind.ends_with("MultiFuse") {
        cadmpeg_ir::features::BooleanKind::Join
    } else {
        return Ok(None);
    };
    let (target, tools) = if kind == "PartDesign::Boolean" {
        let Some(group) = property(ctx, properties, "Group")? else {
            return Ok(None);
        };
        if group.links().is_empty() {
            return Ok(None);
        }
        let has_base_feature = match property(ctx, properties, "BaseFeature")? {
            Some(property) => ctx.any_by(
                property.links(),
                |link| Ok(nonempty_link(link.as_ref())),
                "fcstd boolean base feature links",
            )?,
            None => false,
        };
        if has_base_feature {
            let Some(base) = singular_operand(ctx, properties, "BaseFeature")? else {
                return Ok(None);
            };
            (
                BodySelection::Native(
                    ctx.copy_retained_text(&base.id, "fcstd boolean base feature identity")?,
                ),
                BodySelection::Native(
                    ctx.copy_retained_text(&group.id, "fcstd boolean group identity")?,
                ),
            )
        } else {
            let last = group.links().len() - 1;
            (
                BodySelection::Native(ctx.format_retained(
                    format_args!("{}:link:{last}", group.id),
                    "fcstd boolean final group link",
                )?),
                BodySelection::Native(ctx.format_retained(
                    format_args!("{}:links:0..{last}", group.id),
                    "fcstd boolean preceding group links",
                )?),
            )
        }
    } else if property(ctx, properties, "Base")?.is_some()
        || property(ctx, properties, "Tool")?.is_some()
    {
        let Some(base) = singular_operand(ctx, properties, "Base")? else {
            return Ok(None);
        };
        let Some(tool) = singular_operand(ctx, properties, "Tool")? else {
            return Ok(None);
        };
        (
            BodySelection::Native(ctx.copy_retained_text(&base.id, "fcstd boolean base identity")?),
            BodySelection::Native(ctx.copy_retained_text(&tool.id, "fcstd boolean tool identity")?),
        )
    } else {
        let Some(shapes) = property(ctx, properties, "Shapes")? else {
            return Ok(None);
        };
        if shapes.links().len() < 2 {
            return Ok(None);
        }
        (
            BodySelection::Native(ctx.format_retained(
                format_args!("{}:link:0", shapes.id),
                "fcstd boolean first shape link",
            )?),
            BodySelection::Native(ctx.format_retained(
                format_args!("{}:links:1..{}", shapes.id, shapes.links().len()),
                "fcstd boolean remaining shape links",
            )?),
        )
    };
    let Some(operands) = cadmpeg_ir::features::CombineOperands::new(target, tools, ctx)?.ok()
    else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Combine {
            operands,
            op,
            keep_tools: false,
        },
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanarProfileSource<'a> {
    Sketch(&'a SketchId),
    Native(&'a str),
}

impl cadmpeg_core::decode::cost::DecodeCost for PlanarProfileSource<'_> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Sketch(id) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(*id, ctx, operation)
            }
            Self::Native(id) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(*id, ctx, operation)
            }
        }
    }
}

impl PlanarProfileSource<'_> {
    fn into_planar(
        self,
        ctx: &DecodeContext<'_>,
        sketch_operation: &'static str,
        native_operation: &'static str,
    ) -> Result<PlanarProfileRef, CodecError> {
        Ok(match self {
            Self::Sketch(sketch) => {
                PlanarProfileRef::Sketch(sketch.try_clone_for_decode(ctx, sketch_operation)?)
            }
            Self::Native(native) => {
                PlanarProfileRef::Native(ctx.copy_retained_text(native, native_operation)?)
            }
        })
    }
}

fn loft_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let mut profiles = Vec::new();
    let mut profile_storage = ctx.reserve_scoped(0, "fcstd loft profile source storage")?;
    let profile_sources = property(ctx, properties, "Profile")?;
    let section_sources = property(ctx, properties, "Sections")?;
    for source in [profile_sources, section_sources].into_iter().flatten() {
        for link in ctx.admit_iter(source.links(), "fcstd loft profiles")? {
            let Some(object) = link.as_ref().and_then(|link| link.object()) else {
                continue;
            };
            let profile = match ctx.get_hash_map(sketches, object, "fcstd loft sketch lookup")? {
                Some(sketch) => PlanarProfileSource::Sketch(sketch),
                None => PlanarProfileSource::Native(object),
            };
            ctx.push_scoped_vec(
                &mut profile_storage,
                &mut profiles,
                profile,
                "fcstd loft profiles",
            )?;
        }
    }
    if profiles.len() < 2 {
        return Ok(None);
    }
    let max_degree = if property(ctx, properties, "MaxDegree")?.is_some() {
        let Some(value) = integer_property(ctx, properties, "MaxDegree")?
            .and_then(|value| u32::try_from(value).ok())
            .and_then(std::num::NonZeroU32::new)
        else {
            return Ok(None);
        };
        Some(value)
    } else {
        None
    };
    let part_design = kind.starts_with("PartDesign::");
    let Some(closed) = bool_selector(ctx, properties, "Closed", false)? else {
        return Ok(None);
    };
    let solid = if part_design {
        true
    } else {
        let Some(value) = bool_selector(ctx, properties, "Solid", true)? else {
            return Ok(None);
        };
        value
    };
    let Some(ruled) = bool_selector(ctx, properties, "Ruled", false)? else {
        return Ok(None);
    };
    let linearize = if part_design {
        false
    } else {
        let Some(value) = bool_selector(ctx, properties, "Linearize", false)? else {
            return Ok(None);
        };
        value
    };
    let allow_multi_profile_faces = if part_design {
        let Some(value) = bool_selector(ctx, properties, "AllowMultiFace", false)? else {
            return Ok(None);
        };
        Some(value)
    } else {
        None
    };
    let mut sections = ctx.vector_storage(profiles.len(), "fcstd loft sections")?;
    for profile in ctx.admit_iter(profiles, "fcstd loft section sources")? {
        let profile = ProfileRef::Planar(profile.into_planar(
            ctx,
            "fcstd loft sketch identity",
            "fcstd loft native profile identity",
        )?);
        ctx.push_vec(
            &mut sections,
            cadmpeg_ir::features::LoftSection::Profile(profile),
            "fcstd loft sections",
        )?;
    }
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
        op: operation_boolean(kind),
        closed,
        solid,
        ruled,
        linearize,
        max_degree,
        allow_multi_profile_faces,
    })))
}

fn sweep_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let mut profiles = Vec::new();
    let mut profile_storage = ctx.reserve_scoped(0, "fcstd sweep profile source storage")?;
    let profile_sources = property(ctx, properties, "Profile")?;
    let section_sources = property(ctx, properties, "Sections")?;
    for source in [profile_sources, section_sources].into_iter().flatten() {
        for link in ctx.admit_iter(source.links(), "fcstd sweep profiles")? {
            let Some(object) = link.as_ref().and_then(|link| link.object()) else {
                continue;
            };
            let profile = match ctx.get_hash_map(sketches, object, "fcstd sweep sketch lookup")? {
                Some(sketch) => PlanarProfileSource::Sketch(sketch),
                None => PlanarProfileSource::Native(object),
            };
            ctx.push_scoped_vec(
                &mut profile_storage,
                &mut profiles,
                profile,
                "fcstd sweep profiles",
            )?;
        }
    }
    ctx.dedup_vec(&mut profiles, "fcstd sweep profile deduplication")?;
    if profiles.is_empty() {
        return Ok(None);
    }
    ctx.rotate_left(&mut profiles, 1, "fcstd sweep primary profile removal")?;
    let profile = profiles
        .pop()
        .ok_or_else(|| CodecError::malformed("sweep primary profile disappeared"))?;
    let path_candidate = match property(ctx, properties, "Spine")? {
        Some(property) => Some(property),
        None => property(ctx, properties, "Path")?,
    };
    let Some(path_candidate) = path_candidate else {
        return Ok(None);
    };
    let Some(path_property) = singular_operand(ctx, properties, &path_candidate.name)? else {
        return Ok(None);
    };
    let part_design = kind.starts_with("PartDesign::");
    let solid = if part_design {
        true
    } else {
        let Some(value) = bool_selector(ctx, properties, "Solid", true)? else {
            return Ok(None);
        };
        value
    };
    let path_tangent = if part_design {
        let Some(value) = bool_selector(ctx, properties, "SpineTangent", false)? else {
            return Ok(None);
        };
        value
    } else {
        false
    };
    let auxiliary_spine_tangent = if part_design {
        let Some(value) = bool_selector(ctx, properties, "AuxiliarySpineTangent", false)? else {
            return Ok(None);
        };
        value
    } else {
        false
    };
    let auxiliary_curvilinear = if part_design {
        let Some(value) = bool_selector(ctx, properties, "AuxiliaryCurvilinear", true)? else {
            return Ok(None);
        };
        value
    } else {
        true
    };
    let transition = match integer_property(ctx, properties, "Transition")?
        .unwrap_or(u64::from(kind == "Part::Sweep"))
    {
        0 => SweepTransition::Transformed,
        1 => SweepTransition::RightCorner,
        2 => SweepTransition::RoundCorner,
        _ => return Ok(None),
    };
    let orientation = if kind == "Part::Sweep" {
        let Some(frenet) = bool_selector(ctx, properties, "Frenet", true)? else {
            return Ok(None);
        };
        if frenet {
            SweepOrientation::Frenet {}
        } else {
            SweepOrientation::CorrectedFrenet {}
        }
    } else {
        match integer_property(ctx, properties, "Mode")?.unwrap_or(0) {
            0 => SweepOrientation::CorrectedFrenet {},
            1 => SweepOrientation::Fixed {},
            2 => SweepOrientation::Frenet {},
            3 => {
                let Some(auxiliary) = singular_operand(ctx, properties, "AuxiliarySpine")? else {
                    return Ok(None);
                };
                SweepOrientation::Auxiliary {
                    path: PathRef::Native(ctx.copy_retained_text(
                        &auxiliary.id,
                        "fcstd sweep auxiliary spine identity",
                    )?),
                    tangent: auxiliary_spine_tangent,
                    curvilinear: auxiliary_curvilinear,
                }
            }
            4 => SweepOrientation::Binormal {
                direction: match vector_property(ctx, properties, "Binormal")?
                    .and_then(|value| cadmpeg_ir::units::UnitVector3::normalized(value.get()))
                {
                    Some(direction) => direction,
                    None => return Ok(None),
                },
            },
            _ => return Ok(None),
        }
    };
    let transformation = if kind == "Part::Sweep" {
        SweepTransformation::Constant
    } else {
        match integer_property(ctx, properties, "Transformation")?.unwrap_or(0) {
            0 => SweepTransformation::Constant,
            1 => SweepTransformation::MultiSection,
            2 => SweepTransformation::Linear,
            3 => SweepTransformation::SShape,
            4 => SweepTransformation::Interpolation,
            _ => return Ok(None),
        }
    };
    let linearize = if kind == "Part::Sweep" {
        let Some(value) = bool_selector(ctx, properties, "Linearize", false)? else {
            return Ok(None);
        };
        value
    } else {
        false
    };
    let allow_multi_profile_faces = if part_design {
        let Some(value) = bool_selector(ctx, properties, "AllowMultiFace", false)? else {
            return Ok(None);
        };
        Some(value)
    } else {
        None
    };
    let primary = profile.into_planar(
        ctx,
        "fcstd sweep sketch identity",
        "fcstd sweep native profile identity",
    )?;
    let shape = if solid {
        let Some(op) = operation_boolean(kind).try_into().ok() else {
            return Ok(None);
        };
        let mut sections = ctx.vector_storage(profiles.len(), "fcstd solid sweep sections")?;
        for profile in ctx.admit_iter(profiles, "fcstd solid sweep section sources")? {
            let planar = profile.into_planar(
                ctx,
                "fcstd sweep sketch identity",
                "fcstd sweep native profile identity",
            )?;
            ctx.push_vec(
                &mut sections,
                cadmpeg_ir::features::SweepSection::Profile(planar),
                "fcstd solid sweep sections",
            )?;
        }
        cadmpeg_ir::features::SweepShape::Solid {
            op,
            section: cadmpeg_ir::features::SweepSection::Profile(primary),
            sections,
        }
    } else {
        let mut sections = ctx.vector_storage(profiles.len(), "fcstd sheet sweep sections")?;
        for profile in ctx.admit_iter(profiles, "fcstd sheet sweep section sources")? {
            let planar = profile.into_planar(
                ctx,
                "fcstd sweep sketch identity",
                "fcstd sweep native profile identity",
            )?;
            ctx.push_vec(
                &mut sections,
                cadmpeg_ir::features::SweepSection::Profile(planar),
                "fcstd sheet sweep sections",
            )?;
        }
        cadmpeg_ir::features::SweepShape::Surface {
            section: cadmpeg_ir::features::SweepSection::Profile(primary),
            sections,
        }
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Sweep {
            shape,
            path: Some(PathRef::Native(ctx.copy_retained_text(
                &path_property.id,
                "fcstd sweep path identity",
            )?)),
            orientation: Some(orientation),
            transition: Some(transition),
            transformation: Some(transformation),
            path_tangent,
            linearize,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces,
        },
    )))
}

fn hole_definition(
    ctx: &DecodeContext<'_>,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
    objects: &BTreeMap<&str, &ObjectRecord>,
    properties_by_owner: &BTreeMap<&str, Vec<&PropertyRecord>>,
    program_version: Option<&str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let profile = profile_ref(ctx, owner, properties, sketches)?;
    let thread_type = enumeration_selector(ctx, properties, "ThreadType", 0)?;
    let (designation_label, class_label, fit_label, threaded) =
        if thread_type.is_some_and(|value| value != 0) {
            let designation = enumeration_label(ctx, properties, "ThreadSize")?;
            match bool_selector(ctx, properties, "Threaded", false)? {
                Some(true) => (
                    designation,
                    enumeration_label(ctx, properties, "ThreadClass")?,
                    None,
                    Some(true),
                ),
                Some(false) => (
                    designation,
                    None,
                    enumeration_label(ctx, properties, "ThreadFit")?,
                    Some(false),
                ),
                None => (designation, None, None, None),
            }
        } else {
            (None, None, None, None)
        };
    let ProfileRef::Planar(planar_profile) = profile else {
        return Ok(None);
    };
    if matches!(&planar_profile, PlanarProfileRef::Unresolved(_)) {
        return Ok(None);
    }
    let filter_bits = required!(integer_selector(ctx, properties, "BaseProfileType", 6)?);
    let profile_filter = match filter_bits & 7 {
        1 => HoleProfileFilter::Points,
        2 => HoleProfileFilter::Circles,
        3 => HoleProfileFilter::PointsAndCircles,
        4 => HoleProfileFilter::Arcs,
        5 => HoleProfileFilter::PointsAndArcs,
        6 => HoleProfileFilter::CirclesAndArcs,
        7 => HoleProfileFilter::All,
        _ => return Ok(None),
    };
    let positive = |name| -> Result<_, CodecError> {
        Ok(scalar_named(ctx, properties, name)?.and_then(PositiveReal::from_finite))
    };
    let diameter = required!(positive("Diameter")?);
    let cut_angle = || -> Result<_, CodecError> {
        Ok(positive("HoleCutCountersinkAngle")?
            .filter(|value| value.get() < 180.0)
            .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::new(value.get().to_radians())))
    };
    let legacy_cut_types = match program_version {
        Some(version) => {
            freecad_program_version(ctx, version)?.is_some_and(|version| version < (0, 21))
        }
        None => false,
    };
    let kind = match required!(enumeration_selector(ctx, properties, "HoleCutType", 0)?) {
        0 => HoleKind::Simple,
        1 => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDiameter")?
            )),
            depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDepth",)?
            )),
        },
        2 => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDiameter")?
            )),
            angle: required!(cut_angle()?),
        },
        3 if !legacy_cut_types => HoleKind::Counterdrill {
            diameters: required!(cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                    positive("HoleCutDiameter",)?
                )),
                None,
            )
            .ok()),

            depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDepth",)?
            )),
            angle: required!(cut_angle()?),
        },
        3 | 5 if legacy_cut_types => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDiameter")?
            )),
            depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDepth",)?
            )),
        },
        4 if legacy_cut_types => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("HoleCutDiameter")?
            )),
            angle: required!(cut_angle()?),
        },
        _ => return Ok(None),
    };
    let extent = match required!(enumeration_selector(ctx, properties, "DepthType", 0)?) {
        0 => LinearTermination::Blind {
            length: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                positive("Depth",)?
            ))
            .into(),
        },
        1 => LinearTermination::ThroughAll {},
        _ => return Ok(None),
    };
    let bottom = match required!(enumeration_selector(ctx, properties, "DrillPoint", 1)?) {
        0 => HoleBottom::Flat,
        1 => HoleBottom::Angled {
            included_angle: required!(cadmpeg_ir::scalar::InteriorAngle::new(
                required!(positive("DrillPointAngle")?).get().to_radians(),
            )),
            depth_to_tip: required!(bool_selector(ctx, properties, "DrillForDepth", false)?),
        },
        _ => return Ok(None),
    };
    let tapered = required!(bool_selector(ctx, properties, "Tapered", false)?);
    let taper_angle = tapered
        .then(|| -> Result<_, CodecError> {
            Ok(positive("TaperedAngle")?
                .filter(|value| value.get() < 180.0)
                .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::new(value.get().to_radians())))
        })
        .transpose()?
        .flatten();
    if tapered && taper_angle.is_none() {
        return Ok(None);
    }
    let thread_type = required!(thread_type);
    let specification = if thread_type == 0 {
        None
    } else {
        let threaded = required!(threaded);
        let standard = required!(cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            required!(thread_standard(thread_type)),
            "validate nonblank text"
        )?);
        let designation = designation_label;
        let modeled = if property(ctx, properties, "ModelThread")?.is_some() {
            required!(bool_selector(ctx, properties, "ModelThread", false)?)
        } else {
            required!(bool_selector(ctx, properties, "ModelActualThread", false)?)
        };
        let cosmetic = required!(bool_selector(ctx, properties, "CosmeticThread", false)?);
        let hand = match required!(enumeration_selector(ctx, properties, "ThreadDirection", 0)?) {
            0 => ThreadHand::Right,
            1 => ThreadHand::Left,
            _ => return Ok(None),
        };
        let depth = match required!(enumeration_selector(ctx, properties, "ThreadDepthType", 0)?) {
            0 => HoleThreadDepth::HoleDepth,
            1 => HoleThreadDepth::Blind {
                depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(required!(
                    positive("ThreadDepth")?
                )),
            },
            2 => HoleThreadDepth::TappedStandard,
            _ => return Ok(None),
        };
        let clearance = if required!(bool_selector(
            ctx,
            properties,
            "UseCustomThreadClearance",
            false
        )?) {
            Some(Length::from_assigned_real(required!(scalar_named(
                ctx,
                properties,
                "CustomThreadClearance",
            )?)))
        } else {
            None
        };
        Some(Box::new(if threaded {
            HoleSpecification::Threaded {
                standard,
                designation,
                class: class_label,
                modeled,
                cosmetic,
                pitch: positive("ThreadPitch")?
                    .map(cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real),
                major_diameter: positive("ThreadDiameter")?
                    .map(cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real),
                hand,
                depth,
                clearance,
            }
        } else {
            HoleSpecification::Clearance {
                standard,
                designation,
                fit: fit_label,
                modeled,
                cosmetic,
                hand,
                depth,
                clearance,
            }
        }))
    };
    let direction = axis_reference(ctx, properties, "Profile", objects, properties_by_owner)?
        .map(|(_, direction)| cadmpeg_ir::features::FeatureDirection3::from(direction));
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: Some(planar_profile),
        profile_filter: Some(profile_filter),
        face: None,
        direction,
        placements: None,
        shape: required!(cadmpeg_ir::features::holes::HoleShape::new(
            HoleConstruction::Form {
                kind,
                specification,
            },
            None,
            Some(cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(diameter)),
        )
        .ok()),

        extent: Some(extent),
        bottom: Some(bottom),
        taper_angle,
        allow_multi_profile_faces: Some(required!(bool_selector(
            ctx,
            properties,
            "AllowMultiFace",
            false
        )?)),
    })))
}

fn freecad_program_version(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<Option<(u64, u64)>, CodecError> {
    let mut part_start = None;
    let mut first_dot = None;
    let mut second_dot = None;
    let mut offset = 0_usize;
    let mut characters = value.chars();
    while let Some(character) = ctx.next_charged(&mut characters, "fcstd program version scan")? {
        if character.is_ascii_digit() || character == '.' {
            part_start.get_or_insert(offset);
            if character == '.' {
                if first_dot.is_none() {
                    first_dot = Some(offset);
                } else if second_dot.is_none() {
                    second_dot = Some(offset);
                }
            }
        } else if let (Some(start), Some(dot)) = (part_start, first_dot) {
            if let Ok(major) =
                ctx.parse_text::<u64>(&value[start..dot], "fcstd program version major")?
            {
                let minor_end = second_dot.unwrap_or(offset);
                if let Ok(minor) = ctx
                    .parse_text::<u64>(&value[dot + 1..minor_end], "fcstd program version minor")?
                {
                    return Ok(Some((major, minor)));
                }
            }
            part_start = None;
            first_dot = None;
            second_dot = None;
        } else {
            part_start = None;
            first_dot = None;
            second_dot = None;
        }
        offset += character.len_utf8();
    }
    if let (Some(start), Some(dot)) = (part_start, first_dot) {
        if let Ok(major) =
            ctx.parse_text::<u64>(&value[start..dot], "fcstd program version major")?
        {
            let minor_end = second_dot.unwrap_or(offset);
            if let Ok(minor) =
                ctx.parse_text::<u64>(&value[dot + 1..minor_end], "fcstd program version minor")?
            {
                return Ok(Some((major, minor)));
            }
        }
    }
    Ok(None)
}

fn thread_standard(value: u64) -> Option<&'static str> {
    [
        "None",
        "ISO metric",
        "ISO metric fine",
        "UNC",
        "UNF",
        "UNEF",
        "NPT",
        "BSP",
        "BSW",
        "BSF",
        "ISO tyre",
    ]
    .get(usize::try_from(value).ok()?)
    .copied()
}

fn helical_sweep_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
    objects: &BTreeMap<&str, &ObjectRecord>,
    properties_by_owner: &BTreeMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((law, axis_origin, axis_direction)) = (|| -> Result<_, CodecError> {
        let law = match required!(enumeration_selector(ctx, properties, "Mode", 0)?) {
            0 => HelicalSweepLaw::PitchHeightAngle,
            1 => HelicalSweepLaw::PitchTurnsAngle,
            2 => HelicalSweepLaw::HeightTurnsAngle,
            3 => HelicalSweepLaw::HeightTurnsGrowth,
            _ => return Ok(None),
        };
        let (axis_origin, axis_direction) = match vector_property(ctx, properties, "Base")?
            .zip(vector_property(ctx, properties, "Axis")?)
        {
            Some((origin, direction)) => (
                origin.as_point().get(),
                required!(cadmpeg_ir::units::UnitVector3::normalized(direction.get())),
            ),
            None => required!(axis_reference(
                ctx,
                properties,
                "ReferenceAxis",
                objects,
                properties_by_owner
            )?),
        };
        Ok(Some((law, axis_origin, axis_direction)))
    })()?
    else {
        return Ok(None);
    };
    let profile = profile_ref(ctx, owner, properties, sketches)?;
    let ProfileRef::Planar(planar_profile) = profile else {
        return Ok(None);
    };
    if matches!(&planar_profile, PlanarProfileRef::Unresolved(_)) {
        return Ok(None);
    }
    let construction = HelicalSweepConstruction {
        profile: planar_profile,
        axis_origin: required!(cadmpeg_ir::features::FinitePoint3::new(axis_origin)),
        axis_direction,
        law,
        pitch: required!(
            cadmpeg_ir::scalar::NonNegativeLength::from_finite_assigned_real(required!(
                scalar_named(ctx, properties, "Pitch",)?
            ))
        ),
        travel: required!(cadmpeg_ir::features::HelicalSweepTravel::new(
            Length::from_assigned_real(required!(scalar_named(ctx, properties, "Height")?)),
            Length::from_assigned_real(required!(scalar_named(ctx, properties, "Growth")?)),
        )),
        turns: required!(cadmpeg_ir::scalar::PositiveReal::from_finite(required!(
            scalar_named(ctx, properties, "Turns",)?
        ))),
        cone_angle: required!(cadmpeg_ir::scalar::Angle::new(
            required!(scalar_named(ctx, properties, "Angle")?)
                .get()
                .to_radians(),
        )),
        left_handed: required!(bool_selector(ctx, properties, "LeftHanded", false)?),
        reversed: required!(bool_selector(ctx, properties, "Reversed", false)?),
        tolerance: Some(required!(cadmpeg_ir::scalar::PositiveReal::from_finite(
            required!(finite_float_selector(
                ctx,
                properties,
                "Tolerance",
                "App::PropertyFloatConstraint",
                required!(FiniteReal::new(DEFAULT_HELICAL_SWEEP_TOLERANCE)),
            )?),
        ))),
        allow_multi_profile_faces: Some(required!(bool_selector(
            ctx,
            properties,
            "AllowMultiFace",
            false
        )?)),
    };
    let op = if kind.ends_with("SubtractiveHelix") {
        if required!(bool_selector(ctx, properties, "Outside", false)?) {
            BooleanOp::Intersect
        } else {
            BooleanOp::Cut
        }
    } else {
        BooleanOp::Join
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::HelicalSweep { construction, op },
    )))
}

fn binder_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(support) = property(ctx, properties, "Support")? else {
        return Ok(None);
    };
    let mut sources = Vec::new();
    for link in ctx.admit_iter(support.links(), "fcstd binder sources")? {
        let Some(link) = link.as_ref().filter(|link| link.object().is_some()) else {
            continue;
        };
        let Some(target) = binder_target(ctx, link, features)? else {
            return Ok(None);
        };
        let mut subelements = Vec::new();
        let complete = visit_link_selectors(ctx, link, "fcstd binder subelements", |selector| {
            let Some(selector) = cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                ctx.copy_retained_text(selector, "fcstd binder subelement selector")?,
                "validate nonblank text",
            )?
            else {
                return Ok(false);
            };
            ctx.push_vec(&mut subelements, selector, "fcstd binder subelements")?;
            Ok(true)
        })?;
        if !complete {
            return Ok(None);
        }
        ctx.push_vec(
            &mut sources,
            BinderSource {
                target,
                subelements,
            },
            "fcstd binder sources",
        )?;
    }
    let construction = if kind == "PartDesign::ShapeBinder" {
        let Some(trace_support) = bool_selector(ctx, properties, "TraceSupport", false)? else {
            return Ok(None);
        };
        BinderConstruction::Shape { trace_support }
    } else {
        let Some(distance) = finite_float_selector(
            ctx,
            properties,
            "Offset",
            "App::PropertyFloat",
            FiniteReal::ZERO,
        )?
        else {
            return Ok(None);
        };
        let Some(offset_join) = enumeration_selector(ctx, properties, "OffsetJoinType", 0)? else {
            return Ok(None);
        };
        let Some(offset_fill) = bool_selector(ctx, properties, "OffsetFill", false)? else {
            return Ok(None);
        };
        let Some(offset_open_result) = bool_selector(ctx, properties, "OffsetOpenResult", false)?
        else {
            return Ok(None);
        };
        let Some(offset_intersection) =
            bool_selector(ctx, properties, "OffsetIntersection", false)?
        else {
            return Ok(None);
        };
        let offset = if distance.get() == 0.0 {
            None
        } else {
            let Some(distance) = cadmpeg_ir::scalar::NonZeroLength::from_assigned_real(distance)
            else {
                return Ok(None);
            };
            Some(BinderOffset {
                distance,
                join: match offset_join {
                    0 => BinderOffsetJoin::Arcs,
                    1 => BinderOffsetJoin::Tangent,
                    2 => BinderOffsetJoin::Intersection,
                    _ => return Ok(None),
                },
                fill: offset_fill,
                open_result: offset_open_result,
                intersection: offset_intersection,
            })
        };
        let context = if let Some(property) =
            match crate::native::sole_property_matching(ctx, properties, |property| {
                property.name == "Context"
            })? {
                Ok(property) => property,
                Err(_) => return Ok(None),
            } {
            if property.type_name != "App::PropertyXLink"
                || property.links().len() != 1
                || !property.links()[0]
                    .as_ref()
                    .is_none_or(|link| link.subelements().is_empty())
            {
                return Ok(None);
            }
            match property
                .links()
                .first()
                .and_then(Option::as_ref)
                .filter(|link| link.object().is_some_and(|object| !object.is_empty()))
            {
                Some(link) => binder_target(ctx, link, features)?,
                None => None,
            }
        } else {
            None
        };
        let Some(lifecycle) = enumeration_selector(ctx, properties, "BindMode", 0)? else {
            return Ok(None);
        };
        let Some(relative) = bool_selector(ctx, properties, "Relative", true)? else {
            return Ok(None);
        };
        let Some(copy_on_change) = enumeration_selector(ctx, properties, "BindCopyOnChange", 0)?
        else {
            return Ok(None);
        };
        let Some(claim_children) = bool_selector(ctx, properties, "ClaimChildren", false)? else {
            return Ok(None);
        };
        let Some(fuse) = bool_selector(ctx, properties, "Fuse", false)? else {
            return Ok(None);
        };
        let Some(make_face) = bool_selector(ctx, properties, "MakeFace", true)? else {
            return Ok(None);
        };
        let Some(partial_load) = bool_selector(ctx, properties, "PartialLoad", false)? else {
            return Ok(None);
        };
        let Some(refine) = bool_selector(ctx, properties, "Refine", true)? else {
            return Ok(None);
        };
        BinderConstruction::SubShape {
            lifecycle: match lifecycle {
                0 => BinderLifecycle::Synchronized,
                1 => BinderLifecycle::Frozen,
                2 => BinderLifecycle::Detached,
                _ => return Ok(None),
            },
            placement: if relative {
                BinderPlacement::Relative
            } else {
                BinderPlacement::Global
            },
            copy_on_change: match copy_on_change {
                0 => BinderCopyOnChange::Disabled,
                1 => BinderCopyOnChange::Enabled,
                2 => BinderCopyOnChange::Mutated,
                _ => return Ok(None),
            },
            claim_children,
            fuse,
            make_face,
            partial_load,
            refine,
            offset,
            context,
        }
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Binder {
            sources,
            construction,
        },
    )))
}

fn binder_target(
    ctx: &DecodeContext<'_>,
    link: &crate::native::LinkTarget,
    features: &HashMap<&str, FeatureId>,
) -> Result<Option<BinderTarget>, CodecError> {
    let Some(object) = link.object() else {
        return Ok(None);
    };
    if let Some(document) = link.document() {
        let Some(document) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(document.as_str(), "fcstd external binder document")?,
            "validate nonblank text",
        )?
        else {
            return Ok(None);
        };
        let Some(object) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(object, "fcstd external binder object")?,
            "validate nonblank text",
        )?
        else {
            return Ok(None);
        };
        return Ok(Some(BinderTarget::External { document, object }));
    }
    Ok(Some(
        match ctx.get_hash_map(features, object, "fcstd binder feature lookup")? {
            Some(feature) => BinderTarget::Feature {
                feature: feature.try_clone_for_decode(ctx, "fcstd binder feature target")?,
            },
            None => BinderTarget::Native {
                reference: match cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    ctx.copy_retained_text(object, "fcstd binder native target")?,
                    "validate nonblank text",
                )? {
                    Some(reference) => reference,
                    None => return Ok(None),
                },
            },
        },
    ))
}

fn enumeration_label(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyEnumeration" {
        return Ok(None);
    }
    let admitted_document = match ctx.parse_xml(property.xml.text(), "FreeCAD XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let document = admitted_document.document();
    let root = ctx.xml_root_element(document, "FreeCAD design XML root_element")?;
    if !ctx.xml_has_tag_name(root, "Property", "FreeCAD design XML tag")? {
        return Ok(None);
    }
    let mut element_values = root.children();
    let Some(integer) = ctx.find_by(
        element_values.by_ref(),
        |node| Ok(node.is_element()),
        "FreeCAD enumeration value children",
    )?
    else {
        return Ok(None);
    };
    if !ctx.xml_has_tag_name(integer, "Integer", "FreeCAD design XML tag")? {
        return Ok(None);
    }
    let custom_list = ctx.find_by(
        element_values.by_ref(),
        |node| Ok(node.is_element()),
        "FreeCAD enumeration value children",
    )?;
    if ctx
        .find_by(
            element_values.by_ref(),
            |node| Ok(node.is_element()),
            "FreeCAD enumeration value children",
        )?
        .is_some()
    {
        return Ok(None);
    }
    if let Some(custom_list) = custom_list {
        if !ctx.xml_has_tag_name(custom_list, "CustomEnumList", "FreeCAD design XML tag")? {
            return Ok(None);
        }
    }
    if ctx.any_by(
        integer.children(),
        |node| Ok(node.is_element()),
        "FreeCAD enumeration integer children",
    )? {
        return Ok(None);
    }
    let custom = match ctx.xml_attribute(integer, "CustomEnum", "FreeCAD design XML attribute")? {
        None => false,
        Some("true") => true,
        Some(_) => return Ok(None),
    };
    if custom != custom_list.is_some() {
        return Ok(None);
    }
    let Some(index_text) = ctx.xml_attribute(integer, "value", "FreeCAD design XML attribute")?
    else {
        return Ok(None);
    };
    let Ok(index) = ctx.parse_text::<usize>(index_text, "fcstd enumeration index")? else {
        return Ok(None);
    };
    let Some(custom_list) = custom_list else {
        return Ok(None);
    };
    let Some(count_text) =
        ctx.xml_attribute(custom_list, "count", "FreeCAD design XML attribute")?
    else {
        return Ok(None);
    };
    let Ok(count) = ctx.parse_text::<usize>(count_text, "fcstd custom enumeration count")? else {
        return Ok(None);
    };
    let mut selected = None;
    let mut actual = 0usize;
    let mut xml_nodes_22 = custom_list.children();
    while let Some(value) = ctx.next_charged(&mut xml_nodes_22, "FreeCAD design XML traversal")? {
        if !value.is_element() {
            continue;
        }
        if !ctx.xml_has_tag_name(value, "Enum", "FreeCAD design XML tag")? {
            return Ok(None);
        }
        if ctx.any_by(
            value.children(),
            |node| Ok(node.is_element()),
            "FreeCAD enumeration label children",
        )? {
            return Ok(None);
        }
        let Some(label) = ctx.xml_attribute(value, "value", "FreeCAD design XML attribute")? else {
            return Ok(None);
        };
        if actual == index {
            selected = Some(label);
        }
        actual = actual.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("FreeCAD custom enumeration value count", u64::MAX, u64::MAX)
        })?;
    }
    if actual != count {
        return Ok(None);
    }
    selected
        .map(|label| ctx.copy_retained_text(label, "fcstd hole enumeration label"))
        .transpose()
}

#[derive(Clone, Copy)]
struct PatternSources<'a, 'b> {
    objects: &'a [ObjectRecord],
    object_by_id: &'a BTreeMap<&'b str, &'b ObjectRecord>,
    predecessors: &'a BTreeMap<&'b str, &'b FeatureId>,
    properties_by_owner: &'a BTreeMap<&'b str, Vec<&'b PropertyRecord>>,
    entries: &'a [EntryRecord],
}

fn pattern_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
    sources: PatternSources<'_, '_>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let PatternSources {
        objects,
        object_by_id,
        predecessors,
        properties_by_owner,
        ..
    } = sources;
    let originals = match property(ctx, properties, "Originals")? {
        Some(originals) if !originals.links().is_empty() => Some(originals),
        _ => match property(ctx, properties, "BaseFeature")? {
            Some(base_feature)
                if ctx.any_by(
                    base_feature.links(),
                    |link| {
                        Ok(link
                            .as_ref()
                            .and_then(|link| link.object())
                            .is_some_and(|object| !object.is_empty()))
                    },
                    "fcstd pattern base feature links",
                )? =>
            {
                Some(base_feature)
            }
            _ => None,
        },
    };
    let (seeds, _seed_storage) = if let Some(originals) = originals {
        let mut seed_storage = ctx.reserve_scoped(0, "fcstd pattern seed storage")?;
        let mut seeds = Vec::new();
        let mut links = originals.links().iter();
        while let Some(link) = ctx.next_charged(&mut links, "fcstd pattern original links")? {
            let Some(target) = link.as_ref().and_then(crate::native::LinkTarget::object) else {
                continue;
            };
            if let Some(feature) =
                ctx.get_hash_map(features, target, "fcstd pattern feature lookup")?
            {
                let feature = feature.try_clone_for_decode(ctx, "fcstd pattern seed identity")?;
                ctx.push_scoped_vec(
                    &mut seed_storage,
                    &mut seeds,
                    feature,
                    "fcstd pattern source seeds",
                )?;
            } else if !ctx
                .get_btree_map(object_by_id, target, "fcstd pattern object lookup")?
                .is_some_and(|object| {
                    matches!(
                        object.type_name.as_str(),
                        "App::Line" | "App::Plane" | "App::Point" | "App::CoordinateSystem"
                    )
                })
            {
                return Ok(None);
            }
        }
        if seeds.is_empty() {
            return Ok(None);
        }
        (seeds, seed_storage)
    } else if let Some(seeds) =
        multi_transform_stage_seeds(ctx, owner, features, objects, properties_by_owner)?
    {
        seeds
    } else {
        let Some(feature) = ctx
            .get_btree_map(predecessors, owner, "fcstd implicit body predecessor")?
            .copied()
        else {
            return Ok(None);
        };
        let mut storage = ctx.reserve_scoped(0, "fcstd implicit pattern seed storage")?;
        let mut seeds = Vec::new();
        let feature = feature.try_clone_for_decode(ctx, "fcstd implicit pattern seed identity")?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut seeds,
            feature,
            "fcstd implicit pattern seed",
        )?;
        (seeds, storage)
    };

    let pattern =
        if kind.ends_with("MultiTransform") {
            let Some(transformations) = property(ctx, properties, "Transformations")? else {
                return Ok(None);
            };
            if transformations.links().is_empty() {
                return Ok(None);
            }
            let mut stages =
                ctx.vector_storage(transformations.links().len(), "freecad pattern stages")?;
            for link in ctx.admit_iter(
                transformations.links(),
                "fcstd pattern transformation links",
            )? {
                let Some(target) = link.as_ref().and_then(|link| link.object()) else {
                    return Ok(None);
                };
                let Some(object) =
                    ctx.get_btree_map(object_by_id, target, "fcstd pattern stage object lookup")?
                else {
                    return Ok(None);
                };
                let Some(owned) = ctx.get_btree_map(
                    properties_by_owner,
                    target,
                    "fcstd pattern transformation properties",
                )?
                else {
                    return Ok(None);
                };
                let Some(pattern) = pattern_kind::<
                    cadmpeg_ir::features::patterns::NoNestedComposite,
                >(ctx, &object.type_name, owned, sources)?
                else {
                    return Ok(None);
                };
                ctx.push_vec(
                    &mut stages,
                    PatternStage {
                        pattern: Box::new(pattern),
                    },
                    "freecad pattern stages",
                )?;
            }
            let Some(pattern) = cadmpeg_ir::features::patterns::CompositePattern::new(stages).ok()
            else {
                return Ok(None);
            };
            let Some(pattern) =
                PatternKind::new(PatternTransform::Composite { stages: pattern }).ok()
            else {
                return Ok(None);
            };
            pattern
        } else {
            let Some(pattern) = pattern_kind(ctx, kind, properties, sources)? else {
                return Ok(None);
            };
            pattern
        };
    let pattern_seeds = ctx.collect_vec(
        seeds.into_iter().map(PatternSeed::Feature),
        "fcstd pattern seed variants",
    )?;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds: pattern_seeds,
            pattern,
        },
    )))
}

fn multi_transform_stage_seeds<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    stage: &str,
    features: &HashMap<&str, FeatureId>,
    objects: &[ObjectRecord],
    properties_by_owner: &BTreeMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<(Vec<FeatureId>, ScopedReservation<'ctx>)>, CodecError> {
    let mut consumers = objects.iter();
    while let Some(consumer) =
        ctx.next_charged(&mut consumers, "fcstd multi-transform consumers")?
    {
        let Some(owned) = ctx.get_btree_map(
            properties_by_owner,
            consumer.id().as_str(),
            "fcstd multi-transform consumer properties",
        )?
        else {
            continue;
        };
        let Some(transformations) = property(ctx, owned, "Transformations")? else {
            continue;
        };
        if !ctx.any_by(
            transformations.links(),
            |link| match link.as_ref().and_then(crate::native::LinkTarget::object) {
                Some(object) => ctx.equal(object, stage, "fcstd multi-transform stage identity"),
                None => Ok(false),
            },
            "fcstd multi-transform stage links",
        )? {
            continue;
        }
        let originals = match property(ctx, owned, "Originals")?
            .filter(|property| !property.links().is_empty())
        {
            Some(originals) => Some(originals),
            None => property(ctx, owned, "BaseFeature")?,
        };
        let Some(originals) = originals else {
            continue;
        };
        let (selected, _selected_storage) =
            ctx.with_scoped_storage("fcstd multi-transform seed selection storage", || {
                let mut selected = Vec::new();
                let complete = ctx.all_by(
                    originals.links(),
                    |link| {
                        let Some(object) =
                            link.as_ref().and_then(crate::native::LinkTarget::object)
                        else {
                            return Ok(true);
                        };
                        let Some(feature) = ctx.get_hash_map(
                            features,
                            object,
                            "fcstd multi-transform feature lookup",
                        )?
                        else {
                            return Ok(false);
                        };
                        ctx.push_vec(
                            &mut selected,
                            feature,
                            "fcstd multi-transform selected seeds",
                        )?;
                        Ok(true)
                    },
                    "fcstd multi-transform originals",
                )?;
                Ok::<_, CodecError>((complete, selected))
            })?;
        let (complete, selected) = selected;
        if !complete || selected.is_empty() {
            continue;
        }
        let mut seed_storage = ctx.reserve_scoped(0, "fcstd multi-transform seed storage")?;
        let mut seeds = Vec::new();
        for feature in ctx.admit_iter(&selected, "fcstd multi-transform source seeds")? {
            let feature =
                feature.try_clone_for_decode(ctx, "fcstd multi-transform seed identity")?;
            ctx.push_scoped_vec(
                &mut seed_storage,
                &mut seeds,
                feature,
                "fcstd multi-transform source seeds",
            )?;
        }
        return Ok(Some((seeds, seed_storage)));
    }
    Ok(None)
}

fn body_predecessors<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    objects: &'a [ObjectRecord],
    features: &'a HashMap<&str, FeatureId>,
    properties_by_owner: &BTreeMap<&str, Vec<&'a PropertyRecord>>,
) -> Result<(BTreeMap<&'a str, &'a FeatureId>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("fcstd body predecessor storage", || {
        let mut predecessors = BTreeMap::new();
        for object in ctx.admit_iter(objects, "fcstd body predecessor objects")? {
            let Some(owned) = ctx.get_btree_map(
                properties_by_owner,
                object.id().as_str(),
                "fcstd body predecessor properties",
            )?
            else {
                continue;
            };
            let Some(members) = body_membership_property(ctx, owned)? else {
                continue;
            };
            let mut previous = None;
            let (mut seen, mut seen_storage) = ctx
                .with_scoped_storage("fcstd body predecessor seen storage", || {
                    Ok::<_, CodecError>(BTreeSet::new())
                })?;
            for link in ctx.admit_iter(members.links(), "fcstd body predecessor members")? {
                let Some(member) = link.as_ref().and_then(crate::native::LinkTarget::object) else {
                    continue;
                };
                if seen_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut seen, member, "fcstd body predecessor first member")
                })? {
                    if let Some(previous) = previous {
                        if !ctx.contains_key_btree_map(
                            &predecessors,
                            member,
                            "fcstd body predecessor first body",
                        )? {
                            ctx.insert_btree_map(
                                &mut predecessors,
                                member,
                                previous,
                                "fcstd body predecessor index",
                            )?;
                        }
                    }
                }
                if let Some(feature) =
                    ctx.get_hash_map(features, member, "fcstd body predecessor feature")?
                {
                    previous = Some(feature);
                }
            }
        }
        Ok::<_, CodecError>(predecessors)
    })
}

fn pattern_kind<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    sources: PatternSources<'_, '_>,
) -> Result<Option<PatternKind<C>>, CodecError> {
    let PatternSources {
        object_by_id,
        properties_by_owner,
        entries,
        ..
    } = sources;
    if kind.ends_with("Mirrored") {
        if let Some((plane_origin, plane_normal)) = plane_reference(
            ctx,
            properties,
            "MirrorPlane",
            object_by_id,
            properties_by_owner,
        )? {
            let Some(plane_origin) = cadmpeg_ir::features::FinitePoint3::new(plane_origin) else {
                return Ok(None);
            };
            return Ok(PatternKind::new(PatternTransform::Mirror {
                plane_origin,
                plane_normal: cadmpeg_ir::features::FeatureDirection3::from(plane_normal),
            })
            .ok());
        }
        let Some(plane) = property(ctx, properties, "MirrorPlane")? else {
            return Ok(None);
        };
        return Ok(PatternKind::new(PatternTransform::MirrorReference {
            plane: cadmpeg_ir::features::FaceSelection::Native(
                ctx.copy_retained_text(&plane.id, "fcstd mirrored pattern plane identity")?,
            ),
        })
        .ok());
    }

    let Some(count) = (if kind.ends_with("Scaled") {
        integer_selector(ctx, properties, "Occurrences", 2)?
    } else {
        let absent_default = if kind.ends_with("PolarPattern") { 3 } else { 2 };
        integer_constraint_selector(ctx, properties, "Occurrences", absent_default, true)?
    }) else {
        return Ok(None);
    };
    if count == 0 || count > cadmpeg_core::decode::u64_from_index(MAX_SKETCH_RECORDS) {
        return Ok(None);
    }
    let count = u32::try_from(count)
        .map_err(|_| CodecError::Malformed("pattern count exceeds u32".into()))?;
    let Some(mode) = enumeration_selector(ctx, properties, "Mode", 0)? else {
        return Ok(None);
    };

    if kind.ends_with("Scaled") {
        let final_factor = required!(cadmpeg_ir::scalar::PositiveReal::from_finite(required!(
            scalar_named(ctx, properties, "Factor")?
        )));
        return Ok(
            (count >= 2).then_some(required!(PatternKind::new(PatternTransform::Scale {
                center: PatternScaleCenter::FirstSeedCentroid,
                final_factor,
                count,
            })
            .ok())),
        );
    }

    let pattern = if kind.ends_with("LinearPattern") {
        let Some(first) = linear_pattern_axis(ctx, properties, "", count, mode, sources)? else {
            return Ok(None);
        };
        let Some(count2) = integer_constraint_selector(ctx, properties, "Occurrences2", 1, false)?
        else {
            return Ok(None);
        };
        if count2 == 0 || count2 > cadmpeg_core::decode::u64_from_index(MAX_SKETCH_RECORDS) {
            return Ok(None);
        }
        if count2 > 1 {
            let Some(mode2) = enumeration_selector(ctx, properties, "Mode2", 0)? else {
                return Ok(None);
            };
            let Some(second) = linear_pattern_axis(
                ctx,
                properties,
                "2",
                u32::try_from(count2)
                    .map_err(|_| CodecError::Malformed("pattern count exceeds u32".into()))?,
                mode2,
                sources,
            )?
            else {
                return Ok(None);
            };
            let mut stages = ctx.vector_storage(2, "freecad linear pattern stages")?;
            for pattern in [first, second] {
                ctx.push_vec(
                    &mut stages,
                    PatternStage {
                        pattern: Box::new(pattern),
                    },
                    "freecad linear pattern stages",
                )?;
            }
            let Some(stages) = C::rebuild(stages).ok() else {
                return Ok(None);
            };
            let Some(pattern) = PatternKind::new(PatternTransform::Composite { stages }).ok()
            else {
                return Ok(None);
            };
            pattern
        } else {
            first.widen()
        }
    } else if kind.ends_with("PolarPattern") {
        let Some((axis_origin, mut axis_dir)) =
            axis_reference(ctx, properties, "Axis", object_by_id, properties_by_owner)?
        else {
            return Ok(None);
        };
        let Some(reversed) = bool_selector(ctx, properties, "Reversed", false)? else {
            return Ok(None);
        };
        if reversed {
            axis_dir = axis_dir.reversed();
        }
        let Some(axis_origin) = cadmpeg_ir::features::FinitePoint3::new(axis_origin) else {
            return Ok(None);
        };
        let (angles, _location_storage) =
            ctx.with_scoped_storage("fcstd pattern location storage", || {
                pattern_locations(
                    ctx,
                    properties,
                    "",
                    count,
                    mode,
                    ("Angle", "Offset"),
                    entries,
                )
            })?;
        let Some(angles) = angles else {
            return Ok(None);
        };
        if let Some(step) = uniform_step(ctx, &angles, "fcstd circular pattern angle spacing")? {
            let Some(angle) = cadmpeg_ir::scalar::PositiveAngle::new(
                (step.get() * f64::from(count - 1)).to_radians(),
            ) else {
                return Ok(None);
            };
            let Some(pattern) = PatternKind::new(PatternTransform::Circular {
                axis_origin,
                axis_dir: cadmpeg_ir::features::FeatureDirection3::from(axis_dir),
                angle,
                count,
            })
            .ok() else {
                return Ok(None);
            };
            pattern
        } else {
            let mut converted =
                ctx.vector_storage(angles.len(), "fcstd circular pattern angles")?;
            for angle in ctx.admit_iter(&angles, "fcstd circular pattern angles")? {
                let Some(angle) = cadmpeg_ir::scalar::Angle::new(angle.get().to_radians()) else {
                    return Ok(None);
                };
                ctx.push_vec(&mut converted, angle, "fcstd circular pattern angles")?;
            }
            let Some(pattern) = PatternKind::new(PatternTransform::CircularAngles {
                axis_origin,
                axis_dir,
                angles: converted,
            })
            .ok() else {
                return Ok(None);
            };
            pattern
        }
    } else {
        return Ok(None);
    };
    Ok(Some(pattern))
}

fn linear_pattern_axis(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    suffix: &str,
    count: u32,
    mode: u64,
    sources: PatternSources<'_, '_>,
) -> Result<Option<cadmpeg_ir::features::patterns::StagePatternKind>, CodecError> {
    let PatternSources {
        object_by_id,
        properties_by_owner,
        entries,
        ..
    } = sources;
    let name = |base: &str| {
        ctx.format_scoped(
            format_args!("{base}{suffix}"),
            "fcstd linear pattern property name",
        )
    };
    let (direction_name, _direction_name_storage) = name("Direction")?;
    let mut direction = axis_reference(
        ctx,
        properties,
        &direction_name,
        object_by_id,
        properties_by_owner,
    )?
    .map(|(_, direction)| direction);
    let (reversed_name, _reversed_name_storage) = name("Reversed")?;
    let Some(reversed) = bool_selector(ctx, properties, &reversed_name, false)? else {
        return Ok(None);
    };
    if reversed {
        direction = direction.map(cadmpeg_ir::units::UnitVector3::reversed);
    }
    let direction = direction.map(cadmpeg_ir::features::FeatureDirection3::from);
    let (offsets, _location_storage) =
        ctx.with_scoped_storage("fcstd pattern location storage", || {
            pattern_locations(
                ctx,
                properties,
                suffix,
                count,
                mode,
                ("Length", "Offset"),
                entries,
            )
        })?;
    let Some(offsets) = offsets else {
        return Ok(None);
    };
    if let Some(spacing) = uniform_step(ctx, &offsets, "fcstd linear pattern spacing")? {
        let Some(spacing) = cadmpeg_ir::scalar::PositiveLength::from_assigned_real(spacing) else {
            return Ok(None);
        };
        Ok(PatternKind::new(PatternTransform::Linear {
            direction,
            spacing,
            count,
            second: None,
        })
        .ok())
    } else {
        let converted = ctx.collect_vec(
            offsets.into_iter().map(Length::from_assigned_real),
            "fcstd linear pattern offsets",
        )?;
        Ok(PatternKind::new(PatternTransform::LinearOffsets {
            direction,
            offsets: converted,
        })
        .ok())
    }
}

fn pattern_locations(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    suffix: &str,
    count: u32,
    mode: u64,
    value_fields: (&str, &str),
    entries: &[EntryRecord],
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    let (extent_base, offset_base) = value_fields;
    if count == 0 {
        return Ok(None);
    }
    if count == 1 {
        return Ok(Some(ctx.alloc_filled(
            1,
            FiniteReal::ZERO,
            "freecad pattern locations",
        )?));
    }
    let name = |base: &str| {
        ctx.format_scoped(
            format_args!("{base}{suffix}"),
            "fcstd pattern property name",
        )
    };
    let location_count = usize::try_from(count).map_err(|_| {
        ctx.refuse_codec_limit(
            "FreeCAD count",
            cadmpeg_core::decode::u64_from_index(usize::MAX),
            u64::from(count),
        )
    })?;
    let interval_count = location_count - 1;
    let (intervals, _interval_storage) =
        ctx.with_scoped_storage("fcstd pattern interval storage", || {
            let intervals = match mode {
                0 => {
                    let (extent_name, _extent_name_storage) = name(extent_base)?;
                    let Some(extent) = scalar_named(ctx, properties, &extent_name)? else {
                        return Ok(None);
                    };
                    let Some(interval) = FiniteReal::new(extent.get() / f64::from(count - 1))
                    else {
                        return Ok(None);
                    };
                    ctx.alloc_filled(interval_count, interval, "freecad pattern intervals")?
                }
                1 => {
                    let (offset_name, _offset_name_storage) = name(offset_base)?;
                    let Some(fallback) = scalar_named(ctx, properties, &offset_name)? else {
                        return Ok(None);
                    };
                    let (spacings_name, _spacings_name_storage) = name("Spacings")?;
                    let spacings = match property(ctx, properties, &spacings_name)? {
                        Some(property) => numeric_list(ctx, property, entries)?,
                        None => Some(Vec::new()),
                    };
                    let Some(spacings) = spacings else {
                        return Ok(None);
                    };
                    let (pattern_name, _pattern_name_storage) = name("SpacingPattern")?;
                    let pattern = match property(ctx, properties, &pattern_name)? {
                        Some(property) => numeric_list(ctx, property, entries)?,
                        None => Some(Vec::new()),
                    };
                    let Some(pattern) = pattern else {
                        return Ok(None);
                    };
                    if !spacings.is_empty()
                        && spacings.len()
                            != usize::try_from(count).map_err(|_| {
                                ctx.refuse_codec_limit(
                                    "FreeCAD count",
                                    cadmpeg_core::decode::u64_from_index(usize::MAX),
                                    u64::from(count),
                                )
                            })? - 1
                    {
                        return Ok(None);
                    }
                    let mut intervals =
                        ctx.vector_storage(interval_count, "freecad pattern intervals")?;
                    for index in
                        ctx.admit_iter(&(0..interval_count), "freecad pattern intervals")?
                    {
                        let explicit = spacings
                            .get(index)
                            .copied()
                            .filter(|value| value.get() != -1.0);
                        ctx.push_vec(
                            &mut intervals,
                            if let Some(explicit) = explicit {
                                explicit
                            } else if pattern.len() > 1 {
                                pattern[index % pattern.len()]
                            } else {
                                fallback
                            },
                            "freecad pattern intervals",
                        )?;
                    }
                    intervals
                }
                _ => return Ok(None),
            };
            Ok::<_, CodecError>(Some(intervals))
        })?;
    let Some(intervals) = intervals else {
        return Ok(None);
    };
    let mut locations = ctx.vector_storage(location_count, "freecad pattern locations")?;
    ctx.push_vec(
        &mut locations,
        FiniteReal::ZERO,
        "freecad pattern locations",
    )?;
    let mut location = FiniteReal::ZERO;
    for interval in ctx.admit_iter(&intervals, "freecad pattern location integration")? {
        let Some(interval) = cadmpeg_ir::scalar::PositiveReal::from_finite(*interval) else {
            return Ok(None);
        };
        let Some(next) = FiniteReal::new(location.get() + interval.get()) else {
            return Ok(None);
        };
        location = next;
        ctx.push_vec(&mut locations, location, "freecad pattern locations")?;
    }
    Ok(Some(locations))
}

fn uniform_step(
    ctx: &DecodeContext<'_>,
    locations: &[FiniteReal],
    operation: &'static str,
) -> Result<Option<FiniteReal>, CodecError> {
    let step = required!(locations.get(1).copied());
    Ok(ctx
        .all_by(
            locations.windows(2),
            |pair| {
                Ok((pair[1].get() - pair[0].get() - step.get()).abs()
                    <= f64::EPSILON * step.get().abs())
            },
            operation,
        )?
        .then_some(step))
}

fn axis_reference(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    objects: &BTreeMap<&str, &ObjectRecord>,
    properties_by_owner: &BTreeMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<(Point3, cadmpeg_ir::units::UnitVector3)>, CodecError> {
    if let Some(direction) = vector_property(ctx, properties, name)? {
        return Ok(Some((
            Point3::new(0.0, 0.0, 0.0),
            required!(cadmpeg_ir::units::UnitVector3::normalized(direction.get())),
        )));
    }
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(None);
    };
    let Some((link, selector)) = singular_reference_link(ctx, property)? else {
        return Ok(None);
    };
    let target = required!(link.object());
    let object = required!(ctx.get_btree_map(objects, target, "fcstd reference object lookup")?);
    let owned = required!(ctx.get_btree_map(
        properties_by_owner,
        target,
        "fcstd axis reference properties",
    )?);
    let (origin, z_axis, x_axis, y_axis) = required!(placement_frame(ctx, owned)?);
    let direction = match object.type_name.as_str() {
        "PartDesign::Line" | "App::Line" => z_axis,
        "PartDesign::Plane" | "App::Plane" => z_axis,
        "PartDesign::CoordinateSystem" => match selector {
            Some("X_Axis" | "XAxis" | "X") => x_axis,
            Some("Y_Axis" | "YAxis" | "Y") => y_axis,
            Some("Z_Axis" | "ZAxis" | "Z") | None => z_axis,
            _ => return Ok(None),
        },
        kind if is_sketch(kind) => match selector {
            Some("H_Axis") => x_axis,
            Some("V_Axis") => y_axis,
            Some("N_Axis") | None => z_axis,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some((
        origin,
        required!(cadmpeg_ir::units::UnitVector3::normalized(direction)),
    )))
}

fn plane_reference(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    objects: &BTreeMap<&str, &ObjectRecord>,
    properties_by_owner: &BTreeMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<(Point3, cadmpeg_ir::units::UnitVector3)>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(None);
    };
    let Some((link, selector)) = singular_reference_link(ctx, property)? else {
        return Ok(None);
    };
    let Some(target) = link.object() else {
        return Ok(None);
    };
    let Some(object) = ctx.get_btree_map(objects, target, "fcstd reference object lookup")? else {
        return Ok(None);
    };
    let Some(owned) = ctx.get_btree_map(
        properties_by_owner,
        target,
        "fcstd plane reference properties",
    )?
    else {
        return Ok(None);
    };
    let Some((origin, z_axis, x_axis, y_axis)) = placement_frame(ctx, owned)? else {
        return Ok(None);
    };
    let normal = match object.type_name.as_str() {
        "PartDesign::Plane" | "App::Plane" => z_axis,
        "PartDesign::CoordinateSystem" => match selector {
            Some("XY_Plane" | "XYPlane" | "XY") | None => z_axis,
            Some("XZ_Plane" | "XZPlane" | "XZ") => y_axis,
            Some("YZ_Plane" | "YZPlane" | "YZ") => x_axis,
            _ => return Ok(None),
        },
        kind if is_sketch(kind) => match selector {
            None | Some("N_Axis") => z_axis,
            Some("H_Axis") => y_axis,
            Some("V_Axis") => x_axis,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    let Some(normal) = cadmpeg_ir::units::UnitVector3::normalized(normal) else {
        return Ok(None);
    };
    Ok(Some((origin, normal)))
}

fn visit_link_selectors<'a>(
    ctx: &DecodeContext<'_>,
    link: &'a crate::native::LinkTarget,
    operation: &'static str,
    mut visit: impl FnMut(&'a str) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let mut selectors = link.subelements().iter();
    while let Some(selector) = ctx.next_charged(&mut selectors, operation)? {
        let mut token_start = None;
        let mut offset = 0usize;
        let mut characters = selector.chars();
        while let Some(character) = ctx.next_charged(&mut characters, operation)? {
            if character.is_ascii_whitespace() {
                if let Some(start) = token_start.take() {
                    if !visit(&selector[start..offset])? {
                        return Ok(false);
                    }
                }
            } else {
                token_start.get_or_insert(offset);
            }
            offset += character.len_utf8();
        }
        if let Some(start) = token_start {
            if !visit(&selector[start..])? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn singular_reference_link<'a>(
    ctx: &DecodeContext<'_>,
    property: &'a PropertyRecord,
) -> Result<Option<(&'a crate::native::LinkTarget, Option<&'a str>)>, CodecError> {
    let Some(link) = scalar_link(property) else {
        return Ok(None);
    };
    let Some(object) = link.object() else {
        return Ok(None);
    };
    if object.is_empty() {
        return Ok(None);
    }
    let mut selector = None;
    let mut multiple = false;
    let complete = visit_link_selectors(ctx, link, "fcstd singular link selectors", |value| {
        if selector.is_some() {
            multiple = true;
            return Ok(false);
        }
        selector = Some(value);
        Ok(true)
    })?;
    if !complete || multiple {
        return Ok(None);
    }
    Ok(Some((link, selector)))
}

fn scalar_link(property: &PropertyRecord) -> Option<&crate::native::LinkTarget> {
    if !matches!(
        property.type_name.as_str(),
        "App::PropertyLink"
            | "App::PropertyLinkChild"
            | "App::PropertyLinkGlobal"
            | "App::PropertyLinkHidden"
            | "App::PropertyLinkSub"
            | "App::PropertyLinkSubChild"
            | "App::PropertyLinkSubGlobal"
            | "App::PropertyLinkSubHidden"
    ) {
        return None;
    }
    let [link] = property.links() else {
        return None;
    };
    link.as_ref()
}

fn is_link_property_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyLink"
            | "App::PropertyLinkChild"
            | "App::PropertyLinkGlobal"
            | "App::PropertyLinkHidden"
            | "App::PropertyLinkList"
            | "App::PropertyLinkListChild"
            | "App::PropertyLinkListGlobal"
            | "App::PropertyLinkListHidden"
            | "App::PropertyLinkSub"
            | "App::PropertyLinkSubChild"
            | "App::PropertyLinkSubGlobal"
            | "App::PropertyLinkSubHidden"
            | "App::PropertyLinkSubList"
            | "App::PropertyLinkSubListChild"
            | "App::PropertyLinkSubListGlobal"
            | "App::PropertyLinkSubListHidden"
            | "App::PropertyXLink"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSub"
            | "App::PropertyXLinkSubHidden"
            | "App::PropertyXLinkSubList"
    )
}

fn scalar_named(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<FiniteReal>, CodecError> {
    Ok(property(ctx, properties, name)?
        .map(|property| scalar_value(ctx, property))
        .transpose()?
        .flatten())
}

fn string_property_value(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<String>, CodecError> {
    if property.type_name != "App::PropertyString" {
        return Ok(None);
    }
    direct_root_value(ctx, property, "String", "value", |ctx, value| {
        Ok(Some(ctx.copy_retained_text(
            value,
            "fcstd string property value",
        )?))
    })
}

fn integer_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<u64>, CodecError> {
    let value = required!(scalar_named(ctx, properties, name)?);
    let value = value.get();
    if value < 0.0 || value.fract() != 0.0 {
        return Ok(None);
    }
    Ok(Some(if value >= U64_UPPER_EXCLUSIVE {
        u64::MAX
    } else {
        required!(cadmpeg_core::convert::truncate_f64_to_u64(value))
    }))
}

fn integer_selector(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: u64,
) -> Result<Option<u64>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(Some(absent_default));
    };
    if property.type_name != "App::PropertyInteger" {
        return Ok(None);
    }
    let value = required!(direct_root_value(
        ctx,
        property,
        "Integer",
        "value",
        |ctx, value| Ok(ctx
            .parse_text::<i64>(value, "fcstd integer property value")?
            .ok())
    )?);
    Ok(u64::try_from(value).ok())
}

fn integer_constraint_selector(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: u64,
    accept_legacy_integer: bool,
) -> Result<Option<u64>, CodecError> {
    let Some(property) = property(ctx, properties, name)? else {
        return Ok(Some(absent_default));
    };
    if property.type_name != "App::PropertyIntegerConstraint"
        && !(accept_legacy_integer && property.type_name == "App::PropertyInteger")
    {
        return Ok(None);
    }
    let value = required!(direct_root_value(
        ctx,
        property,
        "Integer",
        "value",
        |ctx, value| Ok(ctx
            .parse_text::<i64>(value, "fcstd integer constraint value")?
            .ok())
    )?);
    Ok(u64::try_from(value).ok())
}

fn numeric_list(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    entries: &[EntryRecord],
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    if property.type_name != "App::PropertyFloatList" {
        return Ok(None);
    }
    Ok(direct_root(
        ctx,
        property,
        "FloatList",
        |ctx, root| -> Result<_, CodecError> {
            let Some(file) = ctx.xml_attribute(root, "file", "FreeCAD design XML attribute")?
            else {
                return Ok(None);
            };
            if file.is_empty() {
                return Ok(property.side_entries().is_empty().then(Vec::new));
            }
            let side_entries = property.side_entries();
            if side_entries.len() != 1
                || !ctx.equal(
                    side_entries[0].as_str(),
                    file,
                    "fcstd numeric-list side entry",
                )?
            {
                return Ok(None);
            }
            let Some(entry) = ctx.find_by(
                entries,
                |entry| ctx.equal(entry.name(), file, "fcstd numeric-list entry name"),
                "fcstd numeric-list entry search",
            )?
            else {
                return Ok(None);
            };
            let data = entry.data();
            let mut view = View::over_retained(data);
            let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
                return Ok(None);
            };
            if count > MAX_SKETCH_RECORDS
                || view
                    .counted(cadmpeg_core::decode::u64_from_index(count), 8)
                    .is_none()
            {
                return Ok(None);
            }
            let mut values = ctx.vector_storage(count, "fcstd numeric-list values")?;
            for _ in ctx.admit_iter(&(0..count), "fcstd numeric-list values")? {
                let Some(value) = view.f64_le().and_then(FiniteReal::new) else {
                    return Ok(None);
                };
                ctx.push_vec(&mut values, value, "fcstd numeric-list values")?;
            }
            Ok(view.is_empty().then_some(values))
        },
    )?
    .flatten())
}

fn operation_boolean(kind: &str) -> BooleanOp {
    if kind.contains("Subtractive") {
        BooleanOp::Cut
    } else if kind.contains("Additive") {
        BooleanOp::Join
    } else {
        BooleanOp::NewBody
    }
}

fn feature_id(ctx: &DecodeContext<'_>, object: &ObjectRecord) -> Result<FeatureId, CodecError> {
    FeatureId::mint(design_identity_text(
        ctx,
        "feature",
        object,
        format_args!(""),
        "fcstd design feature identity",
    )?)
    .map_err(CodecError::malformed)
}

fn design_identity_text(
    ctx: &DecodeContext<'_>,
    kind: &str,
    object: &ObjectRecord,
    tail: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!(
            "fcstd:design:{kind}#{}{tail}",
            ctx.split_once(object.id(), "#", "fcstd design object identity key")?
                .map_or(object.id().as_str(), |(_, key)| key)
        ),
        operation,
    )
}

fn feature_base_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(index) = ctx.position_by(
        properties,
        |property| Ok::<_, CodecError>(property.name.as_str() == "BaseFeature"),
        "fcstd base feature property search",
    )?
    else {
        return Ok(None);
    };
    if ctx
        .position_by(
            &properties[index + 1..],
            |property| Ok::<_, CodecError>(property.name.as_str() == "BaseFeature"),
            "fcstd base feature duplicate search",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let property = properties[index];
    if property.type_name.as_str() != "App::PropertyLink" || property.links().len() != 1 {
        return Ok(None);
    }
    let Some(source) = property.links()[0]
        .as_ref()
        .and_then(crate::native::LinkTarget::object)
    else {
        return Ok(None);
    };
    let Some(feature) =
        ctx.get_hash_map(feature_ids, source, "fcstd feature base source lookup")?
    else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DerivedGeometry {
            source: feature.try_clone_for_decode(ctx, "fcstd feature base source identity")?,
        },
    )))
}

fn imported_geometry_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let path = match property(ctx, properties, "FileName")? {
        Some(property) => string_property_value(ctx, property)?,
        None => None,
    };
    let Some(path) = path else { return Ok(None) };
    if path.is_empty() {
        return Ok(None);
    }
    let format = match kind {
        "Part::ImportStep" => GeometryImportFormat::Step,
        "Part::ImportIges" => GeometryImportFormat::Iges,
        "Part::ImportBrep" | "Part::CurveNet" => GeometryImportFormat::Brep,
        _ => return Ok(None),
    };
    let Some(path) = path.try_into().ok() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::ImportedGeometry { path, format },
    )))
}

fn is_sketch(kind: &str) -> bool {
    kind == "Sketcher::SketchObject"
}
fn is_datum(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::Plane"
            | "PartDesign::Line"
            | "PartDesign::Point"
            | "PartDesign::CoordinateSystem"
    )
}
fn is_extrusion(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::Pad" | "PartDesign::Pocket" | "Part::Extrusion"
    )
}
fn is_hole(kind: &str) -> bool {
    kind == "PartDesign::Hole"
}
fn is_revolution(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::Revolution" | "PartDesign::Groove" | "Part::Revolution"
    )
}
fn is_primitive(kind: &str) -> bool {
    matches!(
        kind,
        "Part::Box"
            | "Part::Cylinder"
            | "Part::Cone"
            | "Part::Sphere"
            | "Part::Ellipsoid"
            | "Part::Torus"
            | "Part::Prism"
            | "Part::Wedge"
            | "PartDesign::Box"
            | "PartDesign::AdditiveBox"
            | "PartDesign::SubtractiveBox"
            | "PartDesign::Cylinder"
            | "PartDesign::AdditiveCylinder"
            | "PartDesign::SubtractiveCylinder"
            | "PartDesign::Cone"
            | "PartDesign::AdditiveCone"
            | "PartDesign::SubtractiveCone"
            | "PartDesign::Sphere"
            | "PartDesign::AdditiveSphere"
            | "PartDesign::SubtractiveSphere"
            | "PartDesign::Ellipsoid"
            | "PartDesign::AdditiveEllipsoid"
            | "PartDesign::SubtractiveEllipsoid"
            | "PartDesign::Torus"
            | "PartDesign::AdditiveTorus"
            | "PartDesign::SubtractiveTorus"
            | "PartDesign::Prism"
            | "PartDesign::AdditivePrism"
            | "PartDesign::SubtractivePrism"
            | "PartDesign::Wedge"
            | "PartDesign::AdditiveWedge"
            | "PartDesign::SubtractiveWedge"
    )
}
fn is_part_construction_geometry(kind: &str) -> bool {
    matches!(
        kind,
        "Part::Vertex"
            | "Part::Line"
            | "Part::Circle"
            | "Part::Ellipse"
            | "Part::Polygon"
            | "Part::RegularPolygon"
            | "Part::Plane"
            | "Part::Face"
    )
}
fn is_stored_geometry_feature(kind: &str) -> bool {
    matches!(
        kind,
        "Part::Feature"
            | "Part::FeatureExt"
            | "Part::FeatureGeometrySet"
            | "Part::Spline"
            | "Part::Part2DObject"
            | "PartDesign::Feature"
    )
}
fn is_imported_geometry(kind: &str) -> bool {
    matches!(
        kind,
        "Part::ImportStep" | "Part::ImportIges" | "Part::ImportBrep" | "Part::CurveNet"
    )
}
fn is_boolean(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::Boolean"
            | "Part::Cut"
            | "Part::Fuse"
            | "Part::MultiFuse"
            | "Part::Common"
            | "Part::MultiCommon"
    )
}
fn is_loft(kind: &str) -> bool {
    kind == "Part::Loft"
        || matches!(
            kind,
            "PartDesign::AdditiveLoft" | "PartDesign::SubtractiveLoft"
        )
}
fn is_sweep(kind: &str) -> bool {
    kind == "Part::Sweep"
        || matches!(
            kind,
            "PartDesign::AdditivePipe" | "PartDesign::SubtractivePipe"
        )
}
fn is_helical_sweep(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::AdditiveHelix" | "PartDesign::SubtractiveHelix"
    )
}
fn is_parametric_helix(kind: &str) -> bool {
    matches!(kind, "Part::Helix" | "Part::Spiral")
}
fn is_binder(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::ShapeBinder" | "PartDesign::SubShapeBinder"
    )
}
fn is_pattern(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::LinearPattern"
            | "PartDesign::PolarPattern"
            | "PartDesign::Mirrored"
            | "PartDesign::Scaled"
            | "PartDesign::MultiTransform"
    )
}
fn is_dress_up(kind: &str) -> bool {
    is_fillet(kind)
        || is_chamfer(kind)
        || matches!(
            kind,
            "PartDesign::Thickness" | "PartDesign::Draft" | "Part::Thickness"
        )
}
fn is_fillet(kind: &str) -> bool {
    matches!(kind, "Part::Fillet" | "PartDesign::Fillet")
}
fn is_chamfer(kind: &str) -> bool {
    matches!(kind, "Part::Chamfer" | "PartDesign::Chamfer")
}
fn is_body(kind: &str) -> bool {
    kind == "PartDesign::Body"
}
fn is_spreadsheet(kind: &str) -> bool {
    kind == "Spreadsheet::Sheet"
}
fn is_design_object(kind: &str) -> bool {
    is_spreadsheet(kind)
        || is_body(kind)
        || is_datum(kind)
        || is_sketch(kind)
        || is_primitive(kind)
        || is_part_construction_geometry(kind)
        || is_stored_geometry_feature(kind)
        || is_imported_geometry(kind)
        || is_boolean(kind)
        || is_loft(kind)
        || is_sweep(kind)
        || is_helical_sweep(kind)
        || is_parametric_helix(kind)
        || is_binder(kind)
        || is_pattern(kind)
        || kind == "Part::Scale"
        || is_hole(kind)
        || is_extrusion(kind)
        || is_revolution(kind)
        || is_dress_up(kind)
        || matches!(kind, "Part::Offset" | "Part::Offset2D")
        || matches!(
            kind,
            "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
        )
        || matches!(
            kind,
            "Part::RuledSurface" | "Part::Section" | "Part::Mirroring" | "Part::ProjectOnSurface"
        )
        || kind == "PartDesign::FeatureBase"
}

pub(crate) fn census(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    features: &[Feature],
) -> Result<Vec<crate::native::DesignCensusRecord>, CodecError> {
    let (features_by_native, _features_storage) =
        ctx.with_scoped_storage("FreeCAD design census index storage", || {
            let mut features_by_native = HashMap::new();
            for feature in ctx.admit_iter(features, "FreeCAD design census feature records")? {
                if let Some(native_ref) = feature.native_ref.as_deref() {
                    ctx.insert_hash_map(
                        &mut features_by_native,
                        native_ref,
                        feature,
                        "FreeCAD design census feature index",
                    )?;
                }
            }
            Ok::<_, CodecError>(features_by_native)
        })?;
    let mut census = Vec::new();
    for object in ctx.admit_iter(objects, "FreeCAD design census objects")? {
        if !is_design_object(&object.type_name) {
            continue;
        }
        let feature = ctx
            .get_hash_map(
                &features_by_native,
                object.id().as_str(),
                "FreeCAD design census feature index",
            )?
            .ok_or_else(|| {
                malformed_design(
                    ctx,
                    format_args!(
                        "design object {} has no neutral history projection",
                        object.id()
                    ),
                )
            })?;
        let (definition, post_processed) = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. } => (operation, true),
            FeatureDefinition::Operation(operation) => (operation, false),
        };
        let semantic_kind = match definition {
            FeatureOperation::TreeNode { .. } => "tree_node",
            FeatureOperation::BaseFeature { .. } => "base_feature",
            FeatureOperation::MeshImport { .. } => "mesh_import",
            FeatureOperation::InsertBodies { .. } => "insert_bodies",
            FeatureOperation::InsertComponent { .. } => "insert_component",
            FeatureOperation::AssemblyJoint { .. } => "assembly_joint",
            FeatureOperation::Form { .. } => "form",
            FeatureOperation::CosmeticThread { .. } => "cosmetic_thread",
            FeatureOperation::ReferenceImage { .. } => "reference_image",
            FeatureOperation::Decal { .. } => "decal",
            FeatureOperation::DatumPrincipalPlane { .. } => "datum_principal_plane",
            FeatureOperation::DatumPlane { .. } => "datum_plane",
            FeatureOperation::DatumThreePointPlane { .. } => "datum_three_point_plane",
            FeatureOperation::DatumOffsetPlane { .. } => "datum_offset_plane",
            FeatureOperation::DatumAxis { .. } => "datum_axis",
            FeatureOperation::DatumPoint { .. } => "datum_point",
            FeatureOperation::PointGeometry { .. } => "point_geometry",
            FeatureOperation::LineSegment { .. } => "line_segment",
            FeatureOperation::CircularArc { .. } => "circular_arc",
            FeatureOperation::EllipticArc { .. } => "elliptic_arc",
            FeatureOperation::Polyline { .. } => "polyline",
            FeatureOperation::RegularPolygonCurve { .. } => "regular_polygon_curve",
            FeatureOperation::PlanarPatch { .. } => "planar_patch",
            FeatureOperation::FaceFromShapes { .. } => "face_from_shapes",
            FeatureOperation::DatumCoordinateSystem { .. } => "datum_coordinate_system",
            FeatureOperation::Block { .. } => "block",
            FeatureOperation::EquationCurve { .. } => "equation_curve",
            FeatureOperation::ProjectedCurve { .. } => "projected_curve",
            FeatureOperation::ProjectOnSurface { .. } => "project_on_surface",
            FeatureOperation::CompositeCurve { .. } => "composite_curve",
            FeatureOperation::Helix { .. } => "helix",
            FeatureOperation::HelixNativeAxis { .. } => "helix_native_axis",
            FeatureOperation::Coil { .. } => "coil",
            FeatureOperation::Sphere { .. } => "sphere",
            FeatureOperation::Torus { .. } => "torus",
            FeatureOperation::Wrap { .. } => "wrap",
            FeatureOperation::Sketch { .. } => "sketch",
            FeatureOperation::SpatialSketch { .. } => "spatial_sketch",
            FeatureOperation::SketchBlockDefinition { .. } => "sketch_block_definition",
            FeatureOperation::SketchBlockInstance { .. } => "sketch_block_instance",
            FeatureOperation::StoredGeometry { .. } => "stored_geometry",
            FeatureOperation::ExtractBody { .. } => "extract_body",
            FeatureOperation::DerivedGeometry { .. } => "derived_geometry",
            FeatureOperation::ImportedGeometry { .. } => "imported_geometry",
            FeatureOperation::Primitive { .. } => "primitive",
            FeatureOperation::Revolve { .. } => "revolve",
            FeatureOperation::Sweep { .. } => "sweep",
            FeatureOperation::HelicalSweep { .. } => "helical_sweep",
            FeatureOperation::Binder { .. } => "binder",
            FeatureOperation::Rib { .. } => "rib",
            FeatureOperation::SheetMetalBaseFlange { .. } => "sheet_metal_base_flange",
            FeatureOperation::SheetMetalEdgeFlange { .. } => "sheet_metal_edge_flange",
            FeatureOperation::SheetMetalHem { .. } => "sheet_metal_hem",
            FeatureOperation::Fillet { .. } => "fillet",
            FeatureOperation::FullRoundFillet { .. } => "full_round_fillet",
            FeatureOperation::FaceBlend { .. } => "face_blend",
            FeatureOperation::Chamfer { .. } => "chamfer",
            FeatureOperation::Shell { .. } => "shell",
            FeatureOperation::OffsetShape { .. } => "offset_shape",
            FeatureOperation::Compound { .. } => "compound",
            FeatureOperation::RefineShape { .. } => "refine_shape",
            FeatureOperation::ReverseShape { .. } => "reverse_shape",
            FeatureOperation::RuledBetweenCurves { .. } => "ruled_between_curves",
            FeatureOperation::SectionShape { .. } => "section_shape",
            FeatureOperation::MirrorShape { .. } => "mirror_shape",
            FeatureOperation::Thicken { .. } => "thicken",
            FeatureOperation::OffsetSurface { .. } => "offset_surface",
            FeatureOperation::KnitSurface { .. } => "knit_surface",
            FeatureOperation::SewBodies { .. } => "sew_bodies",
            FeatureOperation::FilledSurface { .. } => "filled_surface",
            FeatureOperation::TrimSurface { .. } => "trim_surface",
            FeatureOperation::ExtendSurface { .. } => "extend_surface",
            FeatureOperation::RuledSurface { .. } => "ruled_surface",
            FeatureOperation::Draft { .. } => "draft",
            FeatureOperation::Combine { .. } => "combine",
            FeatureOperation::BoundaryFill { .. } => "boundary_fill",
            FeatureOperation::CutWithSurface { .. } => "cut_with_surface",
            FeatureOperation::TrimBodies { .. } => "trim_bodies",
            FeatureOperation::SplitBody { .. } => "split_body",
            FeatureOperation::SplitFace { .. } => "split_face",
            FeatureOperation::DeleteBody { .. } => "delete_body",
            FeatureOperation::DeleteFace { .. } => "delete_face",
            FeatureOperation::ReplaceFace { .. } => "replace_face",
            FeatureOperation::MoveFace { .. } => "move_face",
            FeatureOperation::MoveBody { .. } => "move_body",
            FeatureOperation::Dome { .. } => "dome",
            FeatureOperation::Flex { .. } => "flex",
            FeatureOperation::Scale { .. } => "scale",
            FeatureOperation::Hole { .. } => "hole",
            FeatureOperation::Pattern { .. } => "pattern",
            FeatureOperation::Unresolved { .. } => "unresolved",
            FeatureOperation::Native { .. } => "native",
            FeatureOperation::Extrude { .. } => "extrude",
            FeatureOperation::Loft { .. } => "loft",
        };
        let semantic_kind =
            ctx.copy_retained_text(semantic_kind, "FreeCAD design census semantic kind")?;
        ctx.push_vec(
            &mut census,
            crate::native::DesignCensusRecord {
                id: crate::native::native_child_id_charged(
                    ctx,
                    "design-census",
                    object.id(),
                    "projection",
                )?,
                object: ctx.copy_retained_text(object.id(), "FreeCAD design census object")?,
                type_name: ctx
                    .copy_retained_text(&object.type_name, "FreeCAD design census type")?,
                feature: ctx
                    .copy_retained_text(feature.id.as_str(), "FreeCAD design census feature")?,
                semantic_kind,
                post_processed,
            },
            "FreeCAD design census records",
        )?;
    }
    ctx.stable_sort_by(
        &mut census,
        |value| &value.id,
        Ord::cmp,
        "FreeCAD design census sort",
    )?;
    Ok(census)
}

#[cfg(test)]
pub(crate) mod tests;
