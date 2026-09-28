// SPDX-License-Identifier: Apache-2.0
//! Transfer of `FCStd` construction history into neutral design entities.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::KnotVector;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchAxis, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
    SketchId, SketchLocus, SketchNativeOperand,
};
use cadmpeg_ir::spreadsheets::{
    CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetDimension, SpreadsheetId,
    SpreadsheetRange,
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
        SurfaceProjectionMode, SweepMode, SweepOrientation, SweepTransformation, SweepTransition,
        TreeChildren,
    },
    scalar::{FiniteReal, Length, NonZeroReal, PositiveLength, PositiveReal},
};

use crate::brep::ShapePayloadRecord;
use crate::native::{malformed, EntryRecord, ObjectRecord, PropertyRecord};
use crate::resource::{collection_allocation_failed, collection_vec, insert_hash_map, reserve_vec_items, reserved_vec, retained_format, retained_string};

const MAX_SKETCH_RECORDS: usize = 1_000_000;
const EXTERNAL_GEO_AXIS_COUNT: usize = 2;
const EXTERNAL_GEOMETRY_MISSING_FLAG: u64 = 1 << 3;
const DEFAULT_HELICAL_SWEEP_TOLERANCE: f64 = 0.1;
const DEFAULT_PART_SPIRAL_SEGMENT_TURNS: f64 = 1.0;
const U64_UPPER_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

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
    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !properties_by_owner.contains_key(property.owner.as_str()) {
            insert_hash_map(ctx, &mut properties_by_owner, property.owner.as_str(), Vec::new(), "fcstd design owner index")?;
        }
        if let Some(owned) = properties_by_owner.get_mut(property.owner.as_str()) {
            crate::resource::reserve_vec_items(ctx, owned, 1, "fcstd design owner properties")?;
            owned.push(property);
        }
    }
    let mut feature_ids = HashMap::new();
    for object in objects.iter().filter(|object| is_design_object(&object.type_name)) {
        insert_hash_map(ctx, &mut feature_ids, object.id.as_str(), feature_id(ctx, object)?, "fcstd design feature ids")?;
    }
    let mut parent_by_member = HashMap::new();
    for body in objects.iter().filter(|object| is_body(&object.type_name)) {
        let Some(property) = properties_by_owner.get(body.id.as_str())
            .and_then(|properties| body_membership_property(properties)) else { continue; };
        for member in property.links().iter().flatten().filter_map(crate::native::LinkTarget::object) {
            insert_hash_map(ctx, &mut parent_by_member, member, feature_id(ctx, body)?, "fcstd design body membership")?;
        }
    }
    let mut sketch_ids = HashMap::new();
    for object in objects.iter().filter(|object| is_sketch(&object.type_name)) {
        let id = SketchId::mint(design_identity_text(
            ctx, "sketch", object, format_args!(""), "fcstd design sketch identity",
        )?).map_err(CodecError::malformed)?;
        insert_hash_map(ctx, &mut sketch_ids, object.id.as_str(), id, "fcstd design sketch ids")?;
    }
    let mut body_ids = collection_vec(ctx, ir.model.bodies.len(), "fcstd design body ids")?;
    for body in &ir.model.bodies {
        body_ids.push(cadmpeg_ir::ids::BodyId::mint(retained_string(ctx, body.id.as_str(), "fcstd design body id")?)
            .map_err(CodecError::malformed)?);
    }
    let (feature_ordinals, mut cycle_affected) = feature_ordinals(
        ctx,
        objects,
        &properties_by_owner,
        &parent_by_member,
    )?;
    let mut ordinal_by_feature = HashMap::new();
    for object in objects.iter().filter(|object| is_design_object(&object.type_name)) {
        insert_hash_map(ctx, &mut ordinal_by_feature, feature_id(ctx, object)?, feature_ordinals[object.id.as_str()], "fcstd design feature ordinals")?;
    }

    for object in objects {
        if !is_design_object(&object.type_name) {
            continue;
        }
        let source = properties_by_owner.get(object.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let mut owned = collection_vec(ctx, source.len(), "fcstd design selected properties")?;
        owned.extend_from_slice(source);
        let id = feature_id(ctx, object)?;
        let mut definition = if is_spreadsheet(&object.type_name) {
            reserve_vec_items(ctx, &mut ir.model.spreadsheets, 1, "fcstd design spreadsheets")?;
            ir.model.spreadsheets.push(append_spreadsheet(
                ctx,
                &mut ir.model.parameters,
                object,
                &owned,
            )?);
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: TreeChildren::default(),
            })
        } else if is_body(&object.type_name) {
            body_definition(ctx, &owned, &feature_ids)?.map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_datum(&object.type_name) {
            datum_definition(&object.type_name, &owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_sketch(&object.type_name) {
            let decoded = parse_sketch(ctx, object, &owned)?;
            let sketch = decoded.sketch;
            let sketch_id = SketchId::mint(retained_string(ctx, sketch.id.as_str(), "fcstd design sketch identity")?)
                .map_err(CodecError::malformed)?;
            insert_hash_map(ctx, &mut sketch_ids, object.id.as_str(),
                SketchId::mint(retained_string(ctx, sketch_id.as_str(), "fcstd design sketch index identity")?)
                    .map_err(CodecError::malformed)?, "fcstd design sketch ids")?;
            reserve_vec_items(ctx, &mut ir.model.sketches, 1, "fcstd neutral sketches")?;
            ir.model.sketches.push(sketch);
            reserve_vec_items(ctx, &mut ir.model.sketch_entities, decoded.entities.len(), "fcstd neutral sketch entities")?;
            ir.model.sketch_entities.extend(decoded.entities);
            reserve_vec_items(ctx, &mut ir.model.sketch_constraints, decoded.constraints.len(), "fcstd neutral sketch constraints")?;
            ir.model.sketch_constraints.extend(decoded.constraints);
            reserve_vec_items(ctx, &mut ir.model.parameters, decoded.parameters.len(), "fcstd sketch parameters")?;
            ir.model.parameters.extend(decoded.parameters);
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
            })
        } else if is_stored_geometry_feature(&object.type_name) {
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        } else if object.type_name == "PartDesign::FeatureBase" {
            feature_base_definition(&owned, &feature_ids).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_imported_geometry(&object.type_name) {
            imported_geometry_definition(ctx, &object.type_name, &owned)?.map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_part_construction_geometry(&object.type_name) {
            part_construction_geometry_definition(ctx, &object.type_name, &owned, entries)?
                .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_primitive(&object.type_name) {
            primitive_definition(&object.type_name, &owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_boolean(&object.type_name) {
            boolean_definition(&object.type_name, &owned)
                .or_else(|| {
                    (object.type_name != "PartDesign::Boolean")
                        .then(|| cached_shape_definition(&owned))
                        .flatten()
                })
                .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_loft(&object.type_name) {
            loft_definition(&object.type_name, &owned, &sketch_ids)
                .or_else(|| cached_shape_definition(&owned))
                .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_sweep(&object.type_name) {
            sweep_definition(&object.type_name, &owned, &sketch_ids).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_helical_sweep(&object.type_name) {
            helical_sweep_definition(
                ctx,
                &object.type_name,
                &object.id,
                &owned,
                &sketch_ids,
                objects,
                &properties_by_owner,
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if matches!(object.type_name.as_str(), "Part::Helix" | "Part::Spiral") {
            parametric_helix_definition(&object.type_name, &owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_binder(&object.type_name) {
            binder_definition(&object.type_name, &owned, &feature_ids).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_pattern(&object.type_name) {
            pattern_definition(
                ctx,
                &object.type_name,
                &object.id,
                &owned,
                &feature_ids,
                PatternSources {
                    objects,
                    properties_by_owner: &properties_by_owner,
                    entries,
                },
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if object.type_name == "Part::Scale" {
            scale_definition(&owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_hole(&object.type_name) {
            hole_definition(
                ctx,
                &object.id,
                &owned,
                &sketch_ids,
                objects,
                &properties_by_owner,
                program_version,
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_extrusion(&object.type_name) {
            let profile = match profile_ref(ctx, &object.id, &owned, &sketch_ids)? {
                ProfileRef::Planar(PlanarProfileRef::Unresolved(_)) => {
                    ["Profile", "Sketch", "Base", "Source"]
                        .iter()
                        .find_map(|name| property(&owned, name))
                        .map_or_else(
                            || retained_string(ctx, &object.id, "fcstd unresolved profile identity")
                                .map(|id| ProfileRef::Planar(PlanarProfileRef::Unresolved(id))),
                            |property| retained_string(ctx, &property.id, "fcstd native profile identity")
                                .map(|id| ProfileRef::Planar(PlanarProfileRef::Native(id))),
                        )?
                }
                profile => profile,
            };
            let profile_normal = profile_target(&owned)
                .and_then(|(_, target)| objects.iter().find(|object| object.id == target))
                .map(|profile_object| {
                    let profile_properties = properties_by_owner
                        .get(profile_object.id.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    sketch_frame(ctx, profile_properties).map(|frame| frame.1)
                })
                .transpose()?;
            extrusion_definition(
                ctx,
                &object.type_name,
                &owned,
                profile,
                profile_normal,
                &ir.model.sketches,
            )?
            .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_revolution(&object.type_name) {
            revolution_definition(ctx, &object.type_name, &object.id, &owned, &sketch_ids)?
                .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if matches!(
            object.type_name.as_str(),
            "PartDesign::Thickness" | "Part::Thickness"
        ) {
            thickness_definition(&object.type_name, &owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if matches!(object.type_name.as_str(), "Part::Offset" | "Part::Offset2D") {
            offset_shape_definition(&object.type_name, &owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if matches!(
            object.type_name.as_str(),
            "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
        ) {
            derived_shape_definition(&object.type_name, &owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if object.type_name == "Part::RuledSurface" {
            ruled_surface_definition(&owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if object.type_name == "Part::Section" {
            section_shape_definition(&owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if object.type_name == "Part::Mirroring" {
            mirror_shape_definition(&owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if object.type_name == "Part::ProjectOnSurface" {
            project_on_surface_definition(&owned).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if object.type_name == "PartDesign::Draft" {
            draft_definition(&owned, objects, &properties_by_owner).map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_fillet(&object.type_name) {
            fillet_definition(&object.type_name, &owned, entries)
                .or_else(|| cached_shape_definition(&owned))
                .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else if is_chamfer(&object.type_name) {
            chamfer_definition(&object.type_name, &owned, entries, program_version)
                .or_else(|| cached_shape_definition(&owned))
                .map_or_else(|| native_definition(ctx, &object.type_name, &owned), Ok)?
        } else {
            native_definition(ctx, &object.type_name, &owned)?
        };
        if cycle_affected.contains(object.id.as_str()) {
            definition = native_definition(ctx, &object.type_name, &owned)?;
        }
        let mut semantic_dependencies = Vec::new();
        if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = &definition {
            for seed in seeds {
                if let PatternSeed::Feature(feature) = seed {
                    reserve_vec_items(ctx, &mut semantic_dependencies, 1, "fcstd design pattern dependencies")?;
                    semantic_dependencies.push(FeatureId::mint(retained_string(ctx, feature.as_str(), "fcstd design pattern dependency")?)
                        .map_err(CodecError::malformed)?);
                }
            }
        }
        let definition = post_processed_definition(ctx, definition, &object.type_name, &owned)?;
        append_operation_parameters(ctx, &mut ir.model.parameters, object, &owned)?;
        let mut outputs = Vec::new();
        for payload in payloads.iter().filter(|payload| owned.iter().any(|property| property.id == payload.property)) {
            let prefix = crate::native::model_id_charged_at(
                ctx, "body", &payload.id, "", "fcstd design body output prefix",
            )?;
            for body in body_ids.iter().filter(|body| body.as_str().starts_with(&prefix)) {
                reserve_vec_items(ctx, &mut outputs, 1, "fcstd design feature outputs")?;
                outputs.push(cadmpeg_ir::ids::BodyId::mint(retained_string(ctx, body.as_str(), "fcstd design output body")?)
                    .map_err(CodecError::malformed)?);
            }
        }
        let cycle_affected = cycle_affected.contains(object.id.as_str());
        let dependencies = if cycle_affected {
            // The native object and property arenas retain the exact cycle.
            // A neutral edge would require a decoder-owned cycle break and
            // would change when persisted declaration order changes.
            Vec::new()
        } else {
            let mut dependency_objects = Vec::new();
            if !is_body(&object.type_name) {
                for dependency in &object.dependencies {
                    reserve_vec_items(ctx, &mut dependency_objects, 1, "fcstd design dependency candidates")?;
                    dependency_objects.push((dependency.as_str(), true));
                }
            }
            for dependency in owned.iter().flat_map(|property| property.links())
                .filter_map(|link| link.as_ref()?.object()) {
                reserve_vec_items(ctx, &mut dependency_objects, 1, "fcstd design dependency candidates")?;
                dependency_objects.push((dependency, false));
            }
            let mut seen_dependencies = BTreeSet::new();
            let mut dependencies = Vec::new();
            for (dependency, declared) in dependency_objects {
                if seen_dependencies.contains(dependency) {
                    continue;
                }
                ctx.charge_collection_items(1, "fcstd design unique dependencies")?;
                seen_dependencies.insert(dependency);
                if let Some(feature) = feature_ids.get(dependency) {
                    if declared || ordinal_by_feature.get(feature)
                        .is_some_and(|ordinal| *ordinal < feature_ordinals[object.id.as_str()]) {
                        reserve_vec_items(ctx, &mut dependencies, 1, "fcstd design feature dependencies")?;
                        dependencies.push(FeatureId::mint(retained_string(ctx, feature.as_str(), "fcstd design feature dependency")?)
                            .map_err(CodecError::malformed)?);
                    }
                }
            }
            for dependency in semantic_dependencies {
                if !dependencies.contains(&dependency)
                    && ordinal_by_feature
                        .get(&dependency)
                        .is_some_and(|ordinal| *ordinal < feature_ordinals[object.id.as_str()])
                {
                    reserve_vec_items(ctx, &mut dependencies, 1, "fcstd design feature dependencies")?;
                    dependencies.push(dependency);
                }
            }
            dependencies
        };
        ctx.charge_collection_items(dependencies.len() as u64, "fcstd distinct feature dependencies")?;
        ctx.charge_collection_items(outputs.len() as u64, "fcstd distinct feature outputs")?;
        reserve_vec_items(ctx, &mut ir.model.features, 1, "fcstd neutral features")?;
        ir.model.features.push(Feature {
            id,
            ordinal: feature_ordinals[object.id.as_str()],
            name: Some(retained_string(ctx, &object.name, "fcstd feature name")?),
            suppressed: bool_property(&owned, "Suppressed"),
            dependencies: (dependencies).into_iter().collect(),
            source_properties: feature_state(ctx, &object.id, &owned)?,
            source_tag: Some(retained_string(ctx, &object.type_name, "fcstd feature source type")?),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                outputs
                    .try_into()
                    .map_err(cadmpeg_core::CodecError::malformed)?,
            ),
            native_ref: Some(retained_string(ctx, &object.id, "fcstd feature native reference")?),
        });
    }
    let mut initial_cycle_affected_features = BTreeSet::new();
    for object in objects.iter().filter(|object| cycle_affected.contains(object.id.as_str())) {
        ctx.charge_collection_items(1, "fcstd design cycle feature identities")?;
        initial_cycle_affected_features.insert(feature_id(ctx, object)?);
    }
    let parameter_cycle_features = bind_parameter_dependencies(
        ctx,
        &mut ir.model.parameters,
        objects,
        &initial_cycle_affected_features,
    )?;
    for object in objects {
        if !parameter_cycle_features.contains(&feature_id(ctx, object)?) {
            continue;
        }
        ctx.charge_collection_items(1, "fcstd design parameter cycle objects")?;
        cycle_affected.insert(retained_string(ctx, &object.id, "fcstd design parameter cycle identity")?);
        if let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(object.id.as_str()))
        {
            feature.evaluation.set_definition(native_definition(
                ctx,
                &object.type_name,
                properties_by_owner
                    .get(object.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            )?);
            feature.dependencies.clear();
        }
    }
    Ok(cycle_affected)
}

fn body_membership_property<'a>(properties: &'a [&PropertyRecord]) -> Option<&'a PropertyRecord> {
    match (property(properties, "Group"), property(properties, "Model")) {
        (Some(group), None) | (None, Some(group)) if group.type_name == "App::PropertyLinkList" => {
            Some(group)
        }
        _ => None,
    }
}

fn body_membership_carrier_is_valid(properties: &[&PropertyRecord]) -> bool {
    match (property(properties, "Group"), property(properties, "Model")) {
        (None, None) => true,
        (Some(group), None) | (None, Some(group)) => group.type_name == "App::PropertyLinkList",
        (Some(_), Some(_)) => false,
    }
}

fn body_definition(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    if !body_membership_carrier_is_valid(properties) {
        return Ok(None);
    }
    let mut children = Vec::new();
    if let Some(property) = body_membership_property(properties) {
        for target in property.links().iter().filter_map(|link| link.as_ref()?.object()) {
            if let Some(feature) = feature_ids.get(target) {
                reserve_vec_items(ctx, &mut children, 1, "fcstd body member features")?;
                children.push(FeatureId::mint(retained_string(
                    ctx, feature.as_str(), "fcstd body member feature identity",
                )?).map_err(CodecError::malformed)?);
            }
        }
    }
    let active_child = match body_tip(ctx, properties, feature_ids)? {
        BodyTipResolution::Valid(active_child) => active_child,
        BodyTipResolution::Invalid => return Ok(None),
    };
    ctx.charge_collection_items(children.len() as u64, "fcstd distinct body children")?;
    Ok(cadmpeg_ir::features::TreeChildren::new(children, active_child).ok().map(|children| FeatureDefinition::Operation(FeatureOperation::TreeNode {
        role: FeatureTreeNodeRole::SolidBodies,
        children,
    })))
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
    let Some(property) = property(properties, "Tip") else {
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
            match feature_ids.get(target) {
                Some(feature) => BodyTipResolution::Valid(Some(FeatureId::mint(
                    retained_string(ctx, feature.as_str(), "fcstd body tip feature identity")?,
                ).map_err(CodecError::malformed)?)),
                None => BodyTipResolution::Invalid,
            }
        }
        _ => BodyTipResolution::Invalid,
    })
}

fn feature_ordinals<'a>(
    ctx: &DecodeContext<'_>,
    objects: &'a [ObjectRecord],
    properties_by_owner: &HashMap<&'a str, Vec<&'a PropertyRecord>>,
    parent_by_member: &HashMap<&'a str, FeatureId>,
) -> Result<(HashMap<&'a str, u64>, BTreeSet<String>), CodecError> {
    let count = objects.iter().filter(|object| is_design_object(&object.type_name)).count();
    let mut design_objects = collection_vec(ctx, count, "fcstd design ordered objects")?;
    design_objects.extend(objects.iter().filter(|object| is_design_object(&object.type_name)));
    let mut object_by_id = HashMap::new();
    let mut object_by_name = HashMap::new();
    let mut object_by_feature = HashMap::new();
    for (map, operation) in [
        (&mut object_by_id, "fcstd design id index"),
        (&mut object_by_name, "fcstd design name index"),
    ] {
        ctx.charge_collection_items(count as u64, operation)?;
        map.try_reserve(count).map_err(|_| collection_allocation_failed(ctx, count as u64, operation))?;
    }
    ctx.charge_collection_items(count as u64, "fcstd design feature index")?;
    object_by_feature.try_reserve(count).map_err(|_| collection_allocation_failed(ctx, count as u64, "fcstd design feature index"))?;
    let mut source_ordinals = collection_vec(ctx, count, "fcstd design source ordinals")?;
    for object in &design_objects {
        object_by_id.insert(object.id.as_str(), *object);
        object_by_name.insert(object.name.as_str(), *object);
        object_by_feature.insert(feature_id(ctx, object)?, object.id.as_str());
        source_ordinals.push(object.order as u64);
    }
    source_ordinals.sort_unstable();
    let mut emitted = BTreeSet::new();
    let mut ordinals = HashMap::new();
    let mut cycle_affected = BTreeSet::new();

    while emitted.len() < design_objects.len() {
        ctx.charge_work(design_objects.len() as u64, "fcstd design dependency ordering")?;
        let next = design_objects
            .iter()
            .copied()
            .filter(|object| !emitted.contains(object.id.as_str()))
            .filter(|object| {
                let parent_ready = parent_by_member
                    .get(object.id.as_str())
                    .and_then(|parent| object_by_feature.get(parent))
                    .is_none_or(|parent| emitted.contains(parent));
                if !parent_ready {
                    return false;
                }

                let declared_ready = is_body(&object.type_name)
                    || object.dependencies.iter().all(|dependency| {
                        !object_by_id.contains_key(dependency.as_str())
                            || emitted.contains(dependency.as_str())
                    });
                if !declared_ready {
                    return false;
                }
                let expressions_ready = properties_by_owner
                    .get(object.id.as_str())
                    .into_iter()
                    .flatten()
                    .flat_map(|property| property.values())
                    .filter_map(|value| value.attributes.get("expression"))
                    .flat_map(|expression| expression_identifiers(expression))
                    .filter_map(|identifier| identifier.split_once('.').map(|(owner, _)| owner))
                    .filter_map(|owner| object_by_name.get(owner))
                    .all(|dependency| {
                        dependency.id == object.id || emitted.contains(dependency.id.as_str())
                    });
                if !expressions_ready {
                    return false;
                }
                if is_body(&object.type_name) {
                    return true;
                }

                properties_by_owner
                    .get(object.id.as_str())
                    .into_iter()
                    .flatten()
                    .flat_map(|property| {
                        property
                            .links()
                            .iter()
                            .map(move |link| (property.name.as_str(), link))
                    })
                    .filter_map(|(property_name, link)| {
                        Some((property_name, link.as_ref()?.object()?))
                    })
                    .filter(|(property_name, dependency)| {
                        object_by_id.get(dependency).is_some_and(|dependency| {
                            matches!(
                                *property_name,
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
                        })
                    })
                    .all(|(_, dependency)| emitted.contains(dependency))
            })
            .min_by_key(|object| object.order);
        let next = if let Some(next) = next {
            next
        } else {
            for object in design_objects.iter().copied().filter(|object| !emitted.contains(object.id.as_str())) {
                ctx.charge_collection_items(1, "fcstd design cycle affected objects")?;
                cycle_affected.insert(retained_string(ctx, &object.id, "fcstd design cycle object")?);
            }
            design_objects.iter().copied()
                .filter(|object| !emitted.contains(object.id.as_str()))
                .min_by_key(|object| object.order)
                .ok_or_else(|| {
                    CodecError::malformed(
                        "design object ordering lost an un-emitted object while resolving a cycle",
                    )
                })?
        };
        let ordinal = source_ordinals[ordinals.len()];
        ctx.charge_collection_items(1, "fcstd design emitted objects")?;
        emitted.insert(next.id.as_str());
        ctx.charge_collection_items(1, "fcstd design ordinals")?;
        ordinals.try_reserve(1).map_err(|_| collection_allocation_failed(ctx, 1, "fcstd design ordinals"))?;
        ordinals.insert(next.id.as_str(), ordinal);
    }

    Ok((ordinals, cycle_affected))
}

/// Apply an operation's shape-refinement and boolean-tolerance controls.
fn post_processed_definition(
    ctx: &DecodeContext<'_>,
    definition: FeatureDefinition,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<FeatureDefinition, CodecError> {
    Ok(match post_process_controls(properties) {
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
fn post_process_controls(properties: &[&PropertyRecord]) -> PostProcessControlState {
    let refine = match unique_named_property(properties, "Refine") {
        NamedProperty::Present(property) => match direct_bool_value(property) {
            Some(value) => Some(value),
            None => return PostProcessControlState::Malformed,
        },
        NamedProperty::Absent => None,
        NamedProperty::Duplicate => return PostProcessControlState::Malformed,
    };
    let fuzzy_tolerance = match unique_named_property(properties, "FuzzyTolerance") {
        NamedProperty::Present(property) => match direct_fuzzy_tolerance(property) {
            Some(value) => Some(value),
            None => return PostProcessControlState::Malformed,
        },
        NamedProperty::Absent => None,
        NamedProperty::Duplicate => return PostProcessControlState::Malformed,
    };
    if refine.is_none() && fuzzy_tolerance.is_none() {
        return PostProcessControlState::Absent;
    }
    PostProcessControlState::Valid {
        refine: refine.unwrap_or(false),
        fuzzy_tolerance: fuzzy_tolerance.unwrap_or(FuzzyTolerance::KernelDefault),
    }
}

enum NamedProperty<'a> {
    Absent,
    Present(&'a PropertyRecord),
    Duplicate,
}

fn unique_named_property<'a>(properties: &[&'a PropertyRecord], name: &str) -> NamedProperty<'a> {
    match crate::native::unique_property(properties.iter().copied(), |property| {
        property.name == name
    }) {
        Ok(Some(property)) => NamedProperty::Present(property),
        Ok(None) => NamedProperty::Absent,
        Err(_) => NamedProperty::Duplicate,
    }
}

fn direct_spreadsheet_value<'a, 'input: 'a>(
    ctx: &DecodeContext<'_>,
    xml: &'a roxmltree::Document<'input>,
    tag: &str,
    property_id: &str,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let total = xml
        .descendants()
        .filter(|node| node.has_tag_name(tag))
        .count();
    if total == 0 {
        return Err(malformed_design(ctx, format_args!("{property_id} has no {tag} value")));
    }
    if total > 1 {
        return Err(malformed_design(ctx, format_args!(
            "{property_id} has multiple {tag} values"
        )));
    }
    xml.root_element()
        .children()
        .find(|node| node.has_tag_name(tag))
        .ok_or_else(|| malformed_design(ctx, format_args!("{property_id} has no direct {tag} value")))
}

fn append_spreadsheet(
    ctx: &DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<Spreadsheet, CodecError> {
    let property = crate::native::unique_property(properties.iter().copied(), |property| {
        property.name == "cells" && property.type_name == "Spreadsheet::PropertySheet"
    })
    .map_err(|_| malformed("spreadsheet has multiple cells properties"))?
    .ok_or_else(|| {
        malformed_design(ctx, format_args!(
            "spreadsheet {} has no cells property",
            object.id
        ))
    })?;
    let xml = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
        malformed_design(ctx, format_args!("invalid spreadsheet {}: {error}", property.id))
    })?;
    let cells = direct_spreadsheet_value(ctx, &xml, "Cells", &property.id)?;
    let declared = cells
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            malformed_design(ctx, format_args!("{} has invalid Cells Count", property.id))
        })?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(malformed_design(ctx, format_args!(
            "{} cell count exceeds {MAX_SKETCH_RECORDS}",
            property.id
        )));
    }
    let found = cells.children().filter(|node| node.has_tag_name("Cell")).count();
    if declared != found {
        return Err(malformed_design(ctx, format_args!(
            "{} declares {declared} cells but contains {}",
            property.id,
            found
        )));
    }
    let mut cell_ids = collection_vec(ctx, found, "FreeCAD spreadsheet cells")?;
    let mut merged_ranges: Vec<SpreadsheetRange> = Vec::new();
    for (index, cell) in cells.children().filter(|node| node.has_tag_name("Cell")).enumerate() {
        let address = cell.attribute("address").ok_or_else(|| {
            malformed_design(ctx, format_args!("{} cell has no address", property.id))
        })?;
        let content = cell.attribute("content").unwrap_or_default();
        let name = cell.attribute("alias").unwrap_or(address);
        let mut retained = BTreeMap::new();
        ctx.charge_collection_items(1, "fcstd spreadsheet cell properties")?;
        retained.insert(
            cadmpeg_core::nonblank_literal!("address"),
            retained_string(ctx, address, "fcstd spreadsheet address")?,
        );
        for attribute in [
            cadmpeg_core::nonblank_literal!("alias"),
            cadmpeg_core::nonblank_literal!("alignment"),
            cadmpeg_core::nonblank_literal!("style"),
            cadmpeg_core::nonblank_literal!("foregroundColor"),
            cadmpeg_core::nonblank_literal!("backgroundColor"),
            cadmpeg_core::nonblank_literal!("displayUnit"),
            cadmpeg_core::nonblank_literal!("rowSpan"),
            cadmpeg_core::nonblank_literal!("colSpan"),
        ] {
            if let Some(value) = cell.attribute(attribute.as_str()) {
                ctx.charge_collection_items(1, "fcstd spreadsheet cell properties")?;
                retained.insert(attribute, retained_string(ctx, value, "fcstd spreadsheet cell attribute")?);
            }
        }
        let cell_address = CellAddress::parse(address).ok_or_else(|| {
            malformed_design(ctx, format_args!("{} cell has invalid address", property.id))
        })?;
        let address_key = crate::native::encoded_segment_charged(
            ctx, address, "fcstd spreadsheet cell address key",
        )?;
        let id = ParameterId::mint(design_identity_text(
            ctx, "parameter", object, format_args!(":cell:{address_key}"),
            "fcstd spreadsheet cell identity",
        )?).map_err(CodecError::malformed)?;
        cell_ids.push(SpreadsheetCell {
            address: cell_address,
            parameter: ParameterId::mint(retained_string(ctx, id.as_str(), "fcstd spreadsheet cell parameter")?)
                .map_err(CodecError::malformed)?,
        });
        if let Some(range) = merged_range(cell)? {
            if !merged_ranges
                .iter()
                .any(|existing| existing.contains(range.start()))
            {
                reserve_vec_items(ctx, &mut merged_ranges, 1, "fcstd spreadsheet merged ranges")?;
                merged_ranges.push(range);
            }
        }
        reserve_vec_items(ctx, parameters, 1, "fcstd spreadsheet parameters")?;
        parameters.push(DesignParameter {
            id,
            owner: Some(feature_id(ctx, object)?),
            ordinal: index as u32,
            name: retained_string(ctx, name, "fcstd spreadsheet cell name")?,
            expression: retained_string(ctx, content, "fcstd spreadsheet cell expression")?,
            display: None,
            value: (!content.starts_with('='))
                .then(|| {
                    content
                        .parse::<f64>()
                        .ok()
                        .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                        .map(ParameterValue::Real)
                })
                .flatten(),
            dependencies: DistinctMembers::default(),
            properties: retained,
            pmi: None,
            native_ref: Some(retained_string(ctx, &property.id, "fcstd spreadsheet parameter native reference")?),
        });
    }
    let column_widths = spreadsheet_dimensions(
        ctx,
        properties,
        "Spreadsheet::PropertyColumnWidths",
        "columnWidths",
        "ColumnInfo",
        "Column",
        "width",
    )?;
    let row_heights = spreadsheet_dimensions(
        ctx,
        properties,
        "Spreadsheet::PropertyRowHeights",
        "rowHeights",
        "RowInfo",
        "Row",
        "height",
    )?;
    ctx.charge_collection_items(cell_ids.len() as u64, "fcstd spreadsheet distinct parameter IDs")?;
    ctx.charge_collection_items(cell_ids.len() as u64, "fcstd spreadsheet distinct addresses")?;
    ctx.charge_collection_items(column_widths.len() as u64, "fcstd spreadsheet distinct column widths")?;
    ctx.charge_collection_items(row_heights.len() as u64, "fcstd spreadsheet distinct row heights")?;
    Spreadsheet::new(
        SpreadsheetId::mint(design_identity_text(
            ctx, "spreadsheet", object, format_args!(""), "fcstd spreadsheet identity",
        )?).map_err(CodecError::malformed)?,
        feature_id(ctx, object)?,
        cell_ids,
        column_widths,
        row_heights,
        merged_ranges,
        Some(retained_string(ctx, &object.id, "fcstd spreadsheet native reference")?),
    )
    .map_err(CodecError::malformed)
}

fn spreadsheet_dimensions(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    type_name: &str,
    property_name: &str,
    container: &str,
    element: &str,
    value_name: &str,
) -> Result<Vec<SpreadsheetDimension>, CodecError> {
    let Some(property) = crate::native::unique_property(properties.iter().copied(), |property| {
        property.name == property_name && property.type_name == type_name
    })
    .map_err(|_| {
        malformed_design(ctx, format_args!(
            "spreadsheet has multiple {property_name} properties"
        ))
    })?
    else {
        return Ok(Vec::new());
    };
    let xml = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
        malformed_design(ctx, format_args!(
            "invalid spreadsheet dimension {}: {error}",
            property.id
        ))
    })?;
    let root = direct_spreadsheet_value(ctx, &xml, container, &property.id)?;
    let found = root.children().filter(|node| node.has_tag_name(element)).count();
    let declared = root
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            malformed_design(ctx, format_args!("{} has invalid dimension count", property.id))
        })?;
    if declared != found || declared > MAX_SKETCH_RECORDS {
        return Err(malformed_design(ctx, format_args!(
            "{} dimension count does not match its records",
            property.id
        )));
    }
    let mut dimensions = collection_vec(ctx, found, "fcstd spreadsheet dimensions")?;
    for record in root.children().filter(|node| node.has_tag_name(element)) {
            let name = record.attribute("name").ok_or_else(|| {
                malformed_design(ctx, format_args!("{} dimension has no name", property.id))
            })?;
            let pixels = record
                .attribute(value_name)
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or_else(|| {
                    malformed_design(ctx, format_args!(
                        "{} dimension has invalid size",
                        property.id
                    ))
                })?;
            let index = if element == "Column" {
                CellAddress::parse(&crate::resource::retained_suffix(ctx, name, "1", "fcstd spreadsheet column address")?)
                    .map(cadmpeg_ir::CellAddress::col)
                    .ok_or_else(|| {
                        malformed_design(ctx, format_args!(
                            "{} dimension has invalid column {name}",
                            property.id
                        ))
                    })?
            } else {
                name.parse::<u32>()
                    .ok()
                    .filter(|row| *row > 0)
                    .ok_or_else(|| {
                        malformed_design(ctx, format_args!(
                            "{} dimension has invalid row {name}",
                            property.id
                        ))
                    })?
            };
            let index = std::num::NonZeroU32::new(index).ok_or_else(|| {
                malformed_design(ctx, format_args!(
                    "{} dimension index must be nonzero",
                    property.id
                ))
            })?;
            dimensions.push(SpreadsheetDimension { index, pixels });
    }
    Ok(dimensions)
}

fn merged_range(cell: roxmltree::Node<'_, '_>) -> Result<Option<SpreadsheetRange>, CodecError> {
    let rows = cell
        .attribute("rowSpan")
        .map_or(Ok(1_i32), str::parse::<i32>)
        .map_err(|_| CodecError::Malformed("spreadsheet cell has invalid row span".into()))?;
    let columns = cell
        .attribute("colSpan")
        .map_or(Ok(1_i32), str::parse::<i32>)
        .map_err(|_| CodecError::Malformed("spreadsheet cell has invalid column span".into()))?;
    if rows < 1 || columns < 1 {
        return Ok(None);
    }
    if rows == 1 && columns == 1 {
        return Ok(None);
    }
    let start = cell
        .attribute("address")
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell has no address".into()))?;
    let end = offset_cell_address(start, (rows - 1) as u32, (columns - 1) as u32)
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell span is out of range".into()))?;
    let start = CellAddress::parse(start)
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell has invalid address".into()))?;
    let end = CellAddress::parse(&end)
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell span is out of range".into()))?;
    SpreadsheetRange::new(start, end)
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell span is out of range".into()))
        .map(Some)
}

fn offset_cell_address(address: &str, rows: u32, columns: u32) -> Option<String> {
    let (row, mut column) = cell_address(address)?;
    let row = row.checked_add(rows)?;
    column = column.checked_add(columns)?;
    let mut label = Vec::new();
    while column > 0 {
        column -= 1;
        label.push(b'A' + (column % 26) as u8);
        column /= 26;
    }
    label.reverse();
    Some(format!("{}{row}", String::from_utf8(label).ok()?))
}

fn cell_address(address: &str) -> Option<(u32, u32)> {
    let split = address.find(|character: char| character.is_ascii_digit())?;
    let column = address[..split].bytes().try_fold(0_u32, |value, byte| {
        byte.is_ascii_uppercase().then(|| {
            value
                .checked_mul(26)?
                .checked_add(u32::from(byte - b'A' + 1))
        })?
    })?;
    let row = address[split..].parse::<u32>().ok()?;
    if row == 0 || column == 0 {
        return None;
    }
    Some((row, column))
}

#[cfg(test)]
fn range_contains_address(range: &SpreadsheetRange, address: &str) -> bool {
    CellAddress::parse(address).is_some_and(|address| range.contains(address))
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
    for property in properties
        .iter()
        .copied()
        .filter(|property| NAMES.contains(&property.name.as_str()))
    {
        if parameters.iter().any(|parameter| {
            parameter.owner.as_ref() == Some(&owner) && parameter.name == property.name
        }) {
            continue;
        }
        let Some(value) = scalar_value(property) else {
            continue;
        };
        let expression = expression_binding(ctx, properties, &property.name)?;
        let is_angle = property.type_name.contains("Angle");
        let mut retained = BTreeMap::new();
        if let Some((native_ref, _)) = &expression {
            ctx.charge_collection_items(1, "fcstd operation expression properties")?;
            retained.insert(
                cadmpeg_core::nonblank_literal!("expression_native_ref"),
                retained_string(ctx, native_ref, "fcstd operation expression reference")?,
            );
        }
        let expression = match expression {
            Some((_, expression)) => expression,
            None => match scalar_text(property, |text| retained_string(ctx, text, "fcstd operation scalar expression")) {
                Some(text) => text?,
                None => retained_format(ctx, format_args!("{}", value.get()), "fcstd operation numeric expression")?,
            },
        };
        reserve_vec_items(ctx, parameters, 1, "fcstd operation parameters")?;
        parameters.push(DesignParameter {
            id: ParameterId::mint(design_identity_text(
                ctx, "parameter", object,
                format_args!(":{}", crate::native::encoded_segment_charged(
                    ctx, &property.name, "fcstd operation parameter name key",
                )?),
                "fcstd operation parameter identity",
            )?).map_err(CodecError::malformed)?,
            owner: Some(FeatureId::mint(retained_string(ctx, owner.as_str(), "fcstd operation parameter owner")?)
                .map_err(CodecError::malformed)?),
            ordinal: property.order as u32,
            name: retained_string(ctx, &property.name, "fcstd operation parameter name")?,
            expression,
            display: None,
            value: if is_angle {
                cadmpeg_ir::scalar::Angle::new(value.get().to_radians()).map(ParameterValue::Angle)
            } else {
                Some(ParameterValue::Length(Length::from_assigned_real(value)))
            },
            dependencies: DistinctMembers::default(),
            properties: retained,
            pmi: None,
            native_ref: Some(retained_string(ctx, &property.id, "fcstd operation parameter native reference")?),
        });
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
    node: roxmltree::Node<'a, 'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    let mut carriers = node.children().filter(|child| {
        child.is_element()
            && !matches!(
                child.tag_name().name(),
                "Construction" | "GeoExtensions" | "UID"
            )
    });
    let carrier = carriers.next()?;
    carriers.next().is_none().then_some(carrier)
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
    Err(malformed_design(ctx, format_args!(
        "sketch Geometry record {ordinal} declares {kind} but carries <{}>, expected <{expected}>",
        carrier.tag_name().name()
    )))
}

fn external_geometry_metadata(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    ordinal: usize,
) -> Result<(Option<String>, bool), CodecError> {
    let mut extensions = node
        .children()
        .filter(|child| child.has_tag_name("GeoExtensions"))
        .flat_map(|container| container.children())
        .filter(|child| {
            child.has_tag_name("GeoExtension")
                && child.attribute("type") == Some("Sketcher::ExternalGeometryExtension")
        });
    let extension = extensions.next();
    if extensions.next().is_some() {
        return Err(malformed_design(ctx, format_args!(
            "sketch ExternalGeo Geometry record {ordinal} has multiple ExternalGeometryExtension values"
        )));
    }
    let extension_ref = extension.and_then(|extension| extension.attribute("Ref"));
    let geometry_ref = node.attribute("ref");
    if let (Some(extension_ref), Some(geometry_ref)) = (extension_ref, geometry_ref) {
        if extension_ref != geometry_ref {
            return Err(malformed_design(ctx, format_args!(
                "sketch ExternalGeo Geometry record {ordinal} has conflicting Ref and ref values"
            )));
        }
    }
    let reference = extension_ref.or(geometry_ref)
        .filter(|value| !value.is_empty())
        .map(|value| retained_string(ctx, value, "fcstd external geometry reference"))
        .transpose()?;

    let extension_flags = extension
        .and_then(|extension| extension.attribute("Flags"))
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                malformed_design(ctx, format_args!(
                    "sketch ExternalGeo Geometry record {ordinal} has invalid Flags"
                ))
            })
        })
        .transpose()?;
    let geometry_flags = node
        .attribute("flags")
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                malformed_design(ctx, format_args!(
                    "sketch ExternalGeo Geometry record {ordinal} has invalid flags"
                ))
            })
        })
        .transpose()?;
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
        return Err(malformed_design(ctx, format_args!(
            "{owner} must contain the two reserved ExternalGeo axis records"
        )));
    }
    for (index, (expected_value, expected_label)) in
        [(-1_i64, "-1"), (-2_i64, "-2")].into_iter().enumerate()
    {
        let node = records[index];
        let id = node.attribute("id").ok_or_else(|| {
            malformed_design(ctx, format_args!(
                "{owner} reserved ExternalGeo record {} has no id",
                index + 1
            ))
        })?;
        if id.parse::<i64>().ok() != Some(expected_value) {
            return Err(malformed_design(ctx, format_args!(
                "{owner} reserved ExternalGeo record {} has id {id}, expected {expected_label}",
                index + 1
            )));
        }
        let (reference, _) = external_geometry_metadata(ctx, node, index + 1)?;
        if reference.is_some() {
            return Err(malformed_design(ctx, format_args!(
                "{owner} reserved ExternalGeo record {} has an external reference",
                index + 1
            )));
        }
    }
    Ok(())
}

fn external_link_key(ctx: &DecodeContext<'_>, reference: &crate::native::LinkTarget) -> Result<Option<String>, CodecError> {
    let Some(object) = reference.object() else { return Ok(None); };
    let Some(subelement) = reference.subelements().first() else { return Ok(None); };
    Ok(Some(crate::resource::retained_join(ctx, &[crate::native::id_key(object), subelement.as_str()], ".", "fcstd external link key")?))
}

fn external_link_indices(
    ctx: &DecodeContext<'_>,
    references: Option<&PropertyRecord>,
) -> Result<HashMap<String, usize>, CodecError> {
    let mut indices = HashMap::new();
    if let Some(references) = references {
        for (index, reference) in references.links().iter().enumerate() {
            let Some(key) = reference.as_ref().map(|reference| external_link_key(ctx, reference)).transpose()?.flatten() else {
                continue;
            };
            if indices.contains_key(&key) {
                return Err(malformed_design(ctx, format_args!(
                    "sketch ExternalGeometry links contain duplicate key {key}"
                )));
            }
            insert_hash_map(ctx, &mut indices, key, index, "fcstd external link index")?;
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
        for attribute in carrier.attributes() {
            ctx.charge_collection_items(1, "fcstd sketch carrier attributes")?;
            attributes.insert(
                retained_string(ctx, attribute.name(), "fcstd sketch attribute name")?,
                retained_string(ctx, attribute.value(), "fcstd sketch attribute value")?,
            );
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
        ctx, "sketch", object, format_args!(""), "fcstd design sketch identity",
    )?).map_err(CodecError::malformed)?;
    let mut entities = Vec::new();
    let mut matched_references = BTreeSet::new();
    if let Some(geometry) = property(properties, "Geometry") {
        if geometry.type_name != "Part::PropertyGeometryList" {
            return Err(malformed_design(ctx, format_args!(
                "{} has runtime type {}, expected Part::PropertyGeometryList",
                geometry.id, geometry.type_name
            )));
        }
        let xml = roxmltree::Document::parse(geometry.xml.text()).map_err(|error| {
            malformed_design(ctx, format_args!(
                "invalid sketch geometry {}: {error}",
                geometry.id
            ))
        })?;
        let records = direct_counted_records(ctx, &xml, "GeometryList", "Geometry", &geometry.id)?;
        for (index, node) in records.into_iter().enumerate() {
            let carrier = sketch_carrier(node);
            if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
                validate_sketch_carrier(ctx, kind, carrier, index + 1)?;
            }
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown");
            let native_kind = retained_string(ctx, native_kind, "fcstd sketch geometry kind")?;
            let attributes = sketch_attributes(ctx, carrier)?;
            let geometry_value = match carrier
                .map(|carrier| sketch_nurbs(ctx, &native_kind, carrier))
                .transpose()?
                .flatten()
            {
                Some(nurbs) => nurbs,
                None => sketch_geometry(ctx, &native_kind, &attributes)?,
            };
            reserve_vec_items(ctx, &mut entities, 1, "fcstd sketch entities")?;
            entities.push(
                SketchEntity::new(
                    SketchEntityId::mint(design_identity_text(
                        ctx, "sketch-entity", object, format_args!(":{}", index + 1),
                        "fcstd sketch geometry identity",
                    )?).map_err(CodecError::malformed)?,
                    SketchId::mint(retained_string(ctx, id.as_str(), "fcstd sketch entity parent")?)
                        .map_err(CodecError::malformed)?,
                    geometry_value,
                )
                .with_construction(node.descendants().any(|child| {
                    child.has_tag_name("Construction")
                        && child.attribute("value").is_some_and(|value| value != "0")
                }))
                .with_native_ref(Some(retained_string(ctx, &geometry.id, "fcstd sketch geometry native reference")?)),
            );
        }
    }
    if let Some(external_geometry) = property(properties, "ExternalGeo") {
        if external_geometry.type_name != "Part::PropertyGeometryList" {
            return Err(malformed_design(ctx, format_args!(
                "{} has runtime type {}, expected Part::PropertyGeometryList",
                external_geometry.id, external_geometry.type_name
            )));
        }
        let xml = roxmltree::Document::parse(external_geometry.xml.text()).map_err(|error| {
            malformed_design(ctx, format_args!(
                "invalid external sketch geometry {}: {error}",
                external_geometry.id
            ))
        })?;
        let records =
            direct_counted_records(ctx, &xml, "GeometryList", "Geometry", &external_geometry.id)?;
        validate_external_geo_prefix(ctx, &records, &external_geometry.id)?;
        let references = property(properties, "ExternalGeometry");
        if let Some(references) = references {
            if references.type_name != "App::PropertyLinkSubList" {
                return Err(malformed_design(ctx, format_args!(
                    "{} has runtime type {}, expected App::PropertyLinkSubList",
                    references.id, references.type_name
                )));
            }
        }
        let link_indices = external_link_indices(ctx, references)?;
        for (external_index, node) in records
            .into_iter()
            .skip(EXTERNAL_GEO_AXIS_COUNT)
            .enumerate()
        {
            let (cache_reference, missing) = external_geometry_metadata(ctx, node, external_index + 3)?;
            let reference_index = cache_reference
                .as_deref()
                .and_then(|cache_reference| link_indices.get(cache_reference).copied());
            if let (Some(cache_reference), None) = (cache_reference.as_deref(), reference_index) {
                if !missing {
                    return Err(malformed_design(ctx, format_args!(
                        "sketch ExternalGeo Geometry record {} reference {cache_reference} has no matching ExternalGeometry link",
                        external_index + 3
                    )));
                }
            }
            if let Some(reference_index) = reference_index {
                ctx.charge_collection_items(1, "fcstd sketch matched references")?;
                matched_references.insert(reference_index);
            }
            let carrier = sketch_carrier(node);
            if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
                validate_sketch_carrier(ctx, kind, carrier, external_index + 3)?;
            }
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown");
            let native_kind = retained_string(ctx, native_kind, "fcstd external sketch geometry kind")?;
            let attributes = sketch_attributes(ctx, carrier)?;
            let geometry = match carrier
                .map(|carrier| sketch_nurbs(ctx, &native_kind, carrier))
                .transpose()?
                .flatten()
            {
                Some(nurbs) => nurbs,
                None => sketch_geometry(ctx, &native_kind, &attributes)?,
            };
            reserve_vec_items(ctx, &mut entities, 1, "fcstd sketch entities")?;
            entities.push(
                SketchEntity::new(
                    SketchEntityId::mint(design_identity_text(
                        ctx, "sketch-entity", object, format_args!(":external:{external_index}"),
                        "fcstd sketch external geometry identity",
                    )?).map_err(CodecError::malformed)?,
                    SketchId::mint(retained_string(ctx, id.as_str(), "fcstd sketch entity parent")?)
                        .map_err(CodecError::malformed)?,
                    geometry,
                )
                .with_construction(true)
                .with_native_ref(Some(retained_string(ctx, &external_geometry.id, "fcstd external geometry native reference")?))
                .with_geometry_ref(references.map(|property| retained_string(ctx, &property.id, "fcstd external geometry reference property")).transpose()?)
                .with_endpoint_refs(
                    reference_index
                        .and_then(|index| {
                            references.and_then(|property| property.links().get(index))
                        })
                        .and_then(Option::as_ref)
                        .map(|reference| crate::resource::retained_strings(ctx, reference.subelements(), "fcstd sketch external endpoint refs"))
                        .transpose()?.unwrap_or_default(),
                ),
            );
        }
    }
    if let Some(references) = property(properties, "ExternalGeometry") {
        for (external_index, reference) in references.links().iter().enumerate() {
            if matched_references.contains(&external_index) {
                continue;
            }
            let Some(reference) = reference.as_ref() else {
                continue;
            };
            let Some(target_object) = reference.object() else {
                continue;
            };
            let numeric_suffix = format!(":external:{external_index}");
            let entity_kind = if entities
                .iter()
                .any(|entity| entity.id().as_str().ends_with(&numeric_suffix))
            {
                "external-link"
            } else {
                "external"
            };
            reserve_vec_items(ctx, &mut entities, 1, "fcstd sketch entities")?;
            entities.push(
                SketchEntity::new(
                    SketchEntityId::mint(design_identity_text(
                        ctx, "sketch-entity", object,
                        format_args!(":{entity_kind}:{external_index}"),
                        "fcstd sketch external link identity",
                    )?).map_err(CodecError::malformed)?,
                    SketchId::mint(retained_string(ctx, id.as_str(), "fcstd sketch entity parent")?)
                        .map_err(CodecError::malformed)?,
                    SketchGeometry::try_from(SketchGeometryDefinition::ExternalReference {
                        document: reference.document_name().map(|name| retained_string(ctx, name, "fcstd sketch external document")).transpose()?,
                        object: cadmpeg_core::text::NonBlankString::new(retained_string(ctx, target_object, "fcstd sketch external object")?).ok_or_else(
                            || cadmpeg_core::CodecError::malformed("object must not be empty"),
                        )?,
                        subelements: crate::resource::retained_strings(ctx, reference.subelements(), "fcstd sketch external subelements")?,
                    })
                    .map_err(CodecError::malformed)?,
                )
                .with_construction(true)
                .with_native_ref(Some(retained_string(ctx, &references.id, "fcstd sketch external native reference")?))
                .with_geometry_ref(Some(retained_string(ctx, &references.id, "fcstd sketch external geometry reference")?))
                .with_endpoint_refs(crate::resource::retained_strings(ctx, reference.subelements(), "fcstd sketch external endpoint refs")?),
            );
        }
    }
    let (horizontal_axis, vertical_axis, root_point) = builtin_reference_usage(ctx, properties)?;
    if horizontal_axis {
        reserve_vec_items(ctx, &mut entities, 1, "fcstd sketch entities")?;
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx, "sketch-entity", object, format_args!(":reference-horizontal-axis"),
                    "fcstd sketch horizontal axis identity",
                )?).map_err(CodecError::malformed)?,
                SketchId::mint(retained_string(ctx, id.as_str(), "fcstd sketch entity parent")?)
                    .map_err(CodecError::malformed)?,
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                })
                .map_err(CodecError::malformed)?,
            )
            .with_construction(true)
            .with_native_ref(Some(retained_string(ctx, &object.id, "fcstd sketch axis native reference")?)),
        );
    }
    if vertical_axis {
        reserve_vec_items(ctx, &mut entities, 1, "fcstd sketch entities")?;
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx, "sketch-entity", object, format_args!(":reference-vertical-axis"),
                    "fcstd sketch vertical axis identity",
                )?).map_err(CodecError::malformed)?,
                SketchId::mint(retained_string(ctx, id.as_str(), "fcstd sketch entity parent")?)
                    .map_err(CodecError::malformed)?,
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(0.0, 1.0),
                })
                .map_err(CodecError::malformed)?,
            )
            .with_construction(true)
            .with_native_ref(Some(retained_string(ctx, &object.id, "fcstd sketch axis native reference")?)),
        );
    }
    if root_point {
        reserve_vec_items(ctx, &mut entities, 1, "fcstd sketch entities")?;
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx, "sketch-entity", object, format_args!(":reference-root-point"),
                    "fcstd sketch root point identity",
                )?).map_err(CodecError::malformed)?,
                SketchId::mint(retained_string(ctx, id.as_str(), "fcstd sketch entity parent")?)
                    .map_err(CodecError::malformed)?,
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(0.0, 0.0),
                })
                .map_err(CodecError::malformed)?,
            )
            .with_construction(true)
            .with_native_ref(Some(retained_string(ctx, &object.id, "fcstd sketch axis native reference")?)),
        );
    }
    let (constraints, parameters) = parse_constraints(ctx, object, properties, &id, &entities)?;
    let profiles = build_profiles(ctx, &entities, &constraints)?;
    let (origin, normal, u_axis) = sketch_frame(ctx, properties)?;
    Ok(SketchTransfer {
        sketch: Sketch {
            id,
            name: Some(retained_string(ctx, &object.name, "fcstd sketch name")?),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, normal, u_axis)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            native_ref: Some(retained_string(ctx, &object.id, "fcstd sketch native reference")?),
        },
        entities,
        constraints,
        parameters,
    })
}

fn builtin_reference_usage(ctx: &DecodeContext<'_>, properties: &[&PropertyRecord]) -> Result<(bool, bool, bool), CodecError> {
    let Some(property) = property(properties, "Constraints") else {
        return Ok((false, false, false));
    };
    let Ok(xml) = roxmltree::Document::parse(property.xml.text()) else {
        return Ok((false, false, false));
    };
    let mut horizontal = false;
    let mut vertical = false;
    let mut root = false;
    for node in xml
        .descendants()
        .filter(|node| node.has_tag_name("Constrain"))
    {
        let type_code = int_attr(node, "Type");
        let operands = match constraint_operands(ctx, node) {
            Ok(operands) => operands,
            Err(CodecError::ResourceLimit(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(_) => continue,
        };
        root |= matches!(type_code, Some(7 | 8)) && operands.len() == 1;
        for (entity, position) in operands {
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
    if lanes.weights.is_some() {
        ctx.charge_collection_items(
            lanes.control_points.len() as u64,
            "fcstd sketch NURBS weighted pole pairs",
        )?;
    }
    Ok(Some(SketchGeometry::nurbs(
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
            lanes.degree,
            lanes.knots,
            lanes.control_points,
            lanes.weights,
            lanes.periodic,
        )?,
    )))
}

fn sketch_nurbs_lanes(
    ctx: &DecodeContext<'_>,
    kind: &str,
    node: roxmltree::Node<'_, '_>,
) -> Result<Option<SketchNurbsLanes>, CodecError> {
    if !matches!(kind, "Part::GeomBSplineCurve" | "BSplineCurve")
        && !node.has_tag_name("BSplineCurve")
    {
        return Ok(None);
    }
    let Some((degree, periodic, pole_count, knot_count)) = (|| {
        Some((
            node.attribute("Degree")?.parse::<u32>().ok()?,
            matches!(node.attribute("IsPeriodic")?, "1" | "true" | "True"),
            node.attribute("PolesCount")?.parse::<usize>().ok()?,
            node.attribute("KnotsCount")?.parse::<usize>().ok()?,
        ))
    })() else { return Ok(None); };
    if pole_count == 0
        || knot_count == 0
        || pole_count > MAX_SKETCH_RECORDS
        || knot_count > MAX_SKETCH_RECORDS
    {
        return Ok(None);
    }
    if node.children().filter(|child| child.has_tag_name("Pole")).count() != pole_count {
        return Ok(None);
    }
    let mut poles = collection_vec(ctx, pole_count, "fcstd sketch NURBS poles")?;
    for pole in node.children().filter(|child| child.has_tag_name("Pole")) {
        let Some(value) = (|| {
            let point = FinitePoint2::new(Point2::new(
                pole.attribute("X")?.parse().ok()?,
                pole.attribute("Y")?.parse().ok()?,
            ))?;
            let z = FiniteReal::new(pole.attribute("Z")?.parse::<f64>().ok()?)?;
            if z.get().abs() > f64::EPSILON {
                return None;
            }
            let weight = PositiveReal::new(pole.attribute("Weight")?.parse::<f64>().ok()?)?;
            Some((point, weight))
        })() else { return Ok(None); };
        poles.push(value);
    }
    if node.children().filter(|child| child.has_tag_name("Knot")).count() != knot_count {
        return Ok(None);
    }
    let mut knots = collection_vec(ctx, knot_count, "fcstd sketch NURBS knots")?;
    for knot in node.children().filter(|child| child.has_tag_name("Knot")) {
        let Some(value) = (|| {
            Some((
                FiniteReal::new(knot.attribute("Value")?.parse::<f64>().ok()?)?,
                knot.attribute("Mult")?.parse::<usize>().ok()?,
            ))
        })() else { return Ok(None); };
        knots.push(value);
    }
    if poles.len() != pole_count
        || knots.len() != knot_count
        || degree == 0
        || usize::try_from(degree)
            .ok()
            .is_none_or(|degree| degree >= pole_count)
        || knots
            .iter()
            .any(|(_, multiplicity)| *multiplicity == 0 || *multiplicity > MAX_SKETCH_RECORDS)
        || knots
            .windows(2)
            .any(|pair| pair[0].0.get() >= pair[1].0.get())
    {
        return Ok(None);
    }
    let Some(expanded_count) = knots.iter().try_fold(0_usize, |count, (_, multiplicity)| {
        count.checked_add(*multiplicity)
    }) else { return Ok(None); };
    if expanded_count > MAX_SKETCH_RECORDS {
        return Ok(None);
    }
    if !periodic {
        let Some(expected) = usize::try_from(degree).ok()
            .and_then(|degree| pole_count.checked_add(degree))
            .and_then(|count| count.checked_add(1)) else { return Ok(None); };
        if expanded_count != expected { return Ok(None); }
    }
    let mut full_knots = collection_vec(ctx, expanded_count, "fcstd sketch NURBS expanded knots")?;
    full_knots.extend(knots.iter().flat_map(|(value, multiplicity)| std::iter::repeat_n(*value, *multiplicity)));
    let mut control_points = collection_vec(ctx, pole_count, "fcstd sketch NURBS control points")?;
    control_points.extend(poles.iter().map(|(point, _)| *point));
    let mut weights = collection_vec(ctx, pole_count, "fcstd sketch NURBS weights")?;
    weights.extend(poles.iter().map(|(_, weight)| *weight));
    let weights = if weights.iter().any(|weight| (weight.get() - 1.0).abs() > f64::EPSILON) {
        let mut converted = collection_vec(ctx, pole_count, "fcstd sketch NURBS nonzero weights")?;
        converted.extend(weights.into_iter().map(NonZeroReal::from));
        Some(converted)
    } else { None };
    ctx.charge_collection_items(expanded_count as u64, "fcstd sketch NURBS knot conversion")?;
    let Some(knots) = KnotVector::from_finite_lanes(full_knots).ok() else {
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

fn sketch_frame(ctx: &DecodeContext<'_>, properties: &[&PropertyRecord]) -> Result<(Point3, Vector3, Vector3), CodecError> {
    validate_sketch_placement(ctx, properties)?;
    Ok(placement_frame(properties).map_or_else(
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

fn placement_frame(properties: &[&PropertyRecord]) -> Option<(Point3, Vector3, Vector3, Vector3)> {
    let property =
        property(properties, "Placement").or_else(|| property(properties, "AttachmentOffset"))?;
    let matrix = crate::placement::placement_matrix_unreported(property)?.rows();
    let column = |index| Vector3::new(matrix[0][index], matrix[1][index], matrix[2][index]);
    Some((
        Point3::new(matrix[0][3], matrix[1][3], matrix[2][3]),
        column(2),
        column(0),
        column(1),
    ))
}

fn validate_sketch_placement(ctx: &DecodeContext<'_>, properties: &[&PropertyRecord]) -> Result<(), CodecError> {
    let Some(property) =
        property(properties, "Placement").or_else(|| property(properties, "AttachmentOffset"))
    else {
        return Ok(());
    };
    let error = if property.type_name != "App::PropertyPlacement" {
        Some(retained_format(ctx, format_args!(
            "sketch {} placement carrier has runtime type {}",
            property.name, property.type_name
        ), "FreeCAD sketch placement error")?)
    } else if property.values().len() != 1 || property.values()[0].tag != "PropertyPlacement" {
        Some(retained_format(ctx, format_args!(
            "sketch {} placement carrier requires one PropertyPlacement value",
            property.name
        ), "FreeCAD sketch placement error")?)
    } else if placement_frame(properties).is_none() {
        Some(retained_format(ctx, format_args!(
            "sketch {} placement carrier has incomplete or invalid components",
            property.name
        ), "FreeCAD sketch placement error")?)
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
    for property in properties.iter().filter(|property| STATE_NAMES.contains(&property.name.as_str())) {
        let value = if let Some(link) = property.links().first()
            .and_then(|link| link.as_ref()?.object()) {
            retained_string(ctx, link, "fcstd feature state value")?
        } else if let Some(value) = scalar_text(property, |text| {
            retained_string(ctx, text, "fcstd feature state value")
        }) {
            value?
        } else {
            retained_string(ctx, property.xml.text(), "fcstd feature state value")?
        };
        let name = retained_string(ctx, &property.name, "fcstd feature state name")?;
        let Some(name) = NonBlankString::new(name) else {
            return Err(crate::resource::malformed_charged(
                ctx, format_args!("{object} states a property with a blank key"),
                "fcstd feature state blank key error",
            ));
        };
        if state.contains_key(&name) {
            return Err(crate::resource::malformed_charged(
                ctx, format_args!("{object} states the property {name} a second time"),
                "fcstd feature state duplicate key error",
            ));
        }
        ctx.charge_collection_items(1, "fcstd feature state properties")?;
        state.insert(name, value);
    }
    Ok(state)
}

fn bool_property(properties: &[&PropertyRecord], name: &str) -> Option<bool> {
    scalar_text(property(properties, name)?, |value| {
        if value == "1" || value.eq_ignore_ascii_case("true") {
            Some(true)
        } else if value == "0" || value.eq_ignore_ascii_case("false") {
            Some(false)
        } else {
            None
        }
    })?
}

/// Read an operation enumeration while keeping absence distinct from malformed persistence.
/// `FreeCAD` constructors provide the legacy default for an absent property; a present property
/// must use the exact enumeration carrier before its value can select neutral semantics.
fn enumeration_selector(
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: u64,
) -> Option<u64> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != "App::PropertyEnumeration" {
        return None;
    }
    let value = direct_root_value(property, "Integer", "value", str::parse::<i64>)?.ok()?;
    u64::try_from(value).ok()
}

/// Read a persisted boolean while keeping absence distinct from malformed persistence.
/// `FreeCAD` constructors provide the legacy default for an absent property; a present property
/// must use the exact boolean carrier before its value can select neutral semantics.
fn bool_selector(properties: &[&PropertyRecord], name: &str, absent_default: bool) -> Option<bool> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    direct_bool_value(property)
}

fn finite_float_selector(
    properties: &[&PropertyRecord],
    name: &str,
    runtime_type: &str,
    absent_default: FiniteReal,
) -> Option<FiniteReal> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != runtime_type {
        return None;
    }
    let value = direct_root_value(property, "Float", "value", str::parse::<f64>)?.ok()?;
    FiniteReal::new(value)
}

fn direct_bool_value(property: &PropertyRecord) -> Option<bool> {
    if property.type_name != "App::PropertyBool" {
        return None;
    }
    direct_root_value(property, "Bool", "value", |value| match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })?
}

fn direct_fuzzy_tolerance(property: &PropertyRecord) -> Option<FuzzyTolerance> {
    if property.type_name != "App::PropertyFloatConstraint" {
        return None;
    }
    let value = direct_root_value(property, "Float", "value", str::parse::<f64>)?
        .ok()
        .and_then(FiniteReal::new)?;
    Some(
        match cadmpeg_ir::scalar::PositiveLength::from_assigned_real(value) {
            Some(explicit) => FuzzyTolerance::Explicit(explicit),
            None if value.get() < 0.0 => FuzzyTolerance::Automatic,
            None => FuzzyTolerance::KernelDefault,
        },
    )
}

fn parse_constraints(
    ctx: &DecodeContext<'_>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Result<(Vec<SketchConstraint>, Vec<DesignParameter>), CodecError> {
    let Some(property) = property(properties, "Constraints") else {
        return Ok((Vec::new(), Vec::new()));
    };
    if property.type_name != "Sketcher::PropertyConstraintList" {
        return Err(malformed_design(ctx, format_args!(
            "{} has runtime type {}, expected Sketcher::PropertyConstraintList",
            property.id, property.type_name
        )));
    }
    let xml = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
        malformed_design(ctx, format_args!(
            "invalid sketch constraints {}: {error}",
            property.id
        ))
    })?;
    let records = direct_counted_records(ctx, &xml, "ConstraintList", "Constrain", &property.id)?;
    let mut constraints = Vec::new();
    let mut parameters = Vec::new();
    for (index, node) in records.into_iter().enumerate() {
        let (type_code, native_kind) = match node.attribute("Type") {
            None => (None, "missing_type".to_owned()),
            Some(value) => match value.parse::<i64>() {
                Ok(type_code) => (Some(type_code), constraint_kind(type_code).to_owned()),
                Err(_) => (None, "malformed_type".to_owned()),
            },
        };
        let operands = constraint_operands(ctx, node).map_err(|error| match error {
            CodecError::Malformed(message) => malformed_design(ctx, format_args!(
                "{} constraint {}: {message}", property.id, index + 1
            )),
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
        let mut resolved = collection_vec(ctx, operands.len(), "fcstd resolved constraint operands")?;
        for (entity, position) in &operands {
            if let Some(locus) = resolve(*entity, *position)? {
                resolved.push(locus);
            }
        }
        let all_resolved = resolved.len() == operands.len();
        if matches!(type_code, Some(7 | 8)) && operands.len() == 1 && resolved.len() == 1 {
            if let Some(root) = entities
                .iter()
                .find(|entity| entity.id().as_str().ends_with(":reference-root-point"))
            {
                reserve_vec_items(ctx, &mut resolved, 1, "fcstd resolved constraint operands")?;
                resolved.insert(0, SketchLocus::Entity(SketchEntityId::mint(retained_string(
                    ctx, root.id().as_str(), "fcstd constraint root entity",
                )?).map_err(CodecError::malformed)?));
            }
        }
        let parameter = if matches!(type_code, Some(6..=9 | 11 | 16 | 18 | 19)) {
            node.attribute("Value")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| {
                    let id = ParameterId::mint(design_identity_text(
                        ctx, "parameter", object, format_args!(":constraint:{}", index + 1),
                        "fcstd constraint parameter identity",
                    )?).map_err(CodecError::malformed)?;
                    let value = match type_code {
                        Some(9) => ParameterValue::Angle(
                            cadmpeg_ir::scalar::Angle::new(value).ok_or_else(|| {
                                CodecError::malformed("constraint angle must be finite")
                            })?,
                        ),
                        Some(16 | 19) => ParameterValue::Real(
                            cadmpeg_ir::scalar::FiniteReal::new(value).ok_or_else(|| {
                                CodecError::malformed("constraint real must be finite")
                            })?,
                        ),
                        _ => ParameterValue::Length(Length::new(value).ok_or_else(|| {
                            CodecError::malformed("constraint length must be finite")
                        })?),
                    };
                    let path = retained_format(
                        ctx, format_args!("Constraints[{index}]"),
                        "fcstd constraint expression path",
                    )?;
                    let expression = expression_binding(ctx, properties, &path)?;
                    let mut parameter_properties = BTreeMap::new();
                    ctx.charge_collection_items(1, "fcstd constraint parameter properties")?;
                    parameter_properties.insert(
                        cadmpeg_core::nonblank_literal!("is_driving"),
                        retained_string(ctx, node.attribute("IsDriving").unwrap_or("1"), "fcstd constraint driving flag")?,
                    );
                    if let Some(name) = node.attribute("Name").filter(|name| !name.is_empty()) {
                        ctx.charge_collection_items(1, "fcstd constraint parameter properties")?;
                        parameter_properties.insert(
                            cadmpeg_core::nonblank_literal!("source_name"),
                            retained_string(ctx, name, "fcstd constraint source name")?,
                        );
                    }
                    if let Some((native_ref, _)) = &expression {
                        ctx.charge_collection_items(1, "fcstd constraint parameter properties")?;
                        parameter_properties.insert(
                            cadmpeg_core::nonblank_literal!("expression_native_ref"),
                            retained_string(ctx, native_ref, "fcstd constraint expression reference")?,
                        );
                    }
                    reserve_vec_items(ctx, &mut parameters, 1, "fcstd constraint parameters")?;
                    let expression = match expression {
                        Some((_, expression)) => expression,
                        None => retained_string(ctx, node.attribute("Value").unwrap_or_default(), "fcstd constraint expression")?,
                    };
                    parameters.push(DesignParameter {
                        id: ParameterId::mint(retained_string(ctx, id.as_str(), "fcstd constraint parameter identity")?)
                            .map_err(CodecError::malformed)?,
                        owner: Some(feature_id(ctx, object)?),
                        ordinal: index as u32,
                        name: retained_format(
                            ctx, format_args!("Constraint{}", index + 1),
                            "fcstd constraint parameter name",
                        )?,
                        expression,
                        display: None,
                        value: Some(value),
                        dependencies: DistinctMembers::default(),
                        properties: parameter_properties,
                        pmi: None,
                        native_ref: Some(retained_string(ctx, &property.id, "fcstd constraint parameter native reference")?),
                    });
                    Ok::<_, CodecError>(id)
                })
                .transpose()?
        } else {
            None
        };
        let internal_alignment = || -> Result<Option<SketchConstraintDefinitionInput>, CodecError> {
            use cadmpeg_ir::sketches::SketchInternalAlignment as Alignment;
            let alignment = (|| {
                let index = || node.attribute("InternalAlignmentIndex")
                    .and_then(|value| value.parse::<u32>().ok());
                Some(match int_attr(node, "InternalAlignmentType")? {
                1 => Alignment::EllipseMajorDiameter,
                2 => Alignment::EllipseMinorDiameter,
                3 => Alignment::EllipseFocus1,
                4 => Alignment::EllipseFocus2,
                5 => Alignment::HyperbolaMajor,
                6 => Alignment::HyperbolaMinor,
                7 => Alignment::HyperbolaFocus,
                8 => Alignment::ParabolaFocus,
                9 => Alignment::BsplineControlPoint(index()?),
                10 => Alignment::BsplineKnotPoint(index()?),
                11 => Alignment::ParabolaFocalAxis,
                _ => return None,
                })
            })();
            let Some(alignment) = alignment else { return Ok(None); };
            let Some(helper) = resolved.first() else { return Ok(None); };
            let Some(parent) = resolved.get(1) else { return Ok(None); };
            Ok(Some(SketchConstraintDefinitionInput::InternalAlignment {
                helper: copy_constraint_entity(ctx, locus_entity(helper))?,
                parent: copy_constraint_entity(ctx, locus_entity(parent))?,
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
                    let Some(metadata) = node.attribute("MetaData") else { return Ok(None); };
                    let _reservation = ctx.reserve_scoped(
                        metadata.len() as u64, "fcstd constraint text metadata parse",
                    )?;
                    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata) else {
                        return Ok(None);
                    };
                    let Some(text) = metadata.get("text").and_then(serde_json::Value::as_str) else {
                        return Ok(None);
                    };
                    Ok(Some(SketchConstraintDefinitionInput::Text {
                        elements: copy_constraint_loci(ctx, &resolved)?,
                        text: retained_string(ctx, text, "fcstd constraint text")?,
                        font: metadata
                            .get("font")
                            .and_then(serde_json::Value::as_str)
                            .map(|font| retained_string(ctx, font, "fcstd constraint font"))
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
        let native_kind = cadmpeg_core::text::NonBlankString::new(native_kind)
            .ok_or_else(|| CodecError::malformed("empty native constraint kind"))?;
        let mut native_operands = Vec::new();
        for (entity, position) in &operands {
            if *entity >= 0 && resolve(*entity, *position)?.is_some() {
                continue;
            }
            let native_kind = cadmpeg_core::text::NonBlankString::new(retained_format(
                ctx, format_args!("position:{position}"), "fcstd native operand position kind",
            )?)
                .ok_or_else(|| malformed_design(ctx, format_args!(
                    "{} constraint {} has an empty source operand kind", property.id, index + 1
                )))?;
            reserve_vec_items(ctx, &mut native_operands, 1, "fcstd native constraint operands")?;
            native_operands.push(SketchNativeOperand {
                native_kind,
                field: None,
                object_index: u32::try_from(*entity).ok(),
                native_ref: None,
            });
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
            definition = type_code.map(|kind| midpoint_constraint(ctx, kind, &operands, entities))
                .transpose()?.flatten();
        }
        if definition.is_none() {
            if let Some(type_code) = type_code {
                definition = neutral_constraint(ctx, type_code, &resolved, parameter.as_ref(), all_resolved)?;
            }
        }
        let definition = if let Some(definition) = definition {
            definition
        } else {
            let mut entities = collection_vec(ctx, resolved.len(), "fcstd native constraint entities")?;
            for locus in &resolved {
                entities.push(copy_constraint_entity(ctx, locus_entity(locus))?);
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
        reserve_vec_items(ctx, &mut constraints, 1, "fcstd sketch constraints")?;
        constraints.push(SketchConstraint {
            id: SketchConstraintId::mint(design_identity_text(
                ctx, "sketch-constraint", object, format_args!(":{}", index + 1),
                "fcstd sketch constraint identity",
            )?).map_err(CodecError::malformed)?,
            sketch: SketchId::mint(retained_string(ctx, sketch.as_str(), "fcstd constraint sketch identity")?)
                .map_err(CodecError::malformed)?,
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            name: nonempty_attr(ctx, node, "Name")?,
            driving: bool_attr(node, "IsDriving"),
            active: bool_attr(node, "IsActive"),
            virtual_space: bool_attr(node, "IsInVirtualSpace"),
            visible: bool_attr(node, "IsVisible"),
            orientation: node
                .attribute("Orientation")
                .and_then(|value| value.parse().ok()),
            label_distance: label_attr(node, "LabelDistance"),
            label_position: label_attr(node, "LabelPosition"),
            metadata: nonempty_attr(ctx, node, "MetaData")?,
            native_ref: Some(retained_string(ctx, &property.id, "fcstd constraint native reference")?),
        });
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
        let Some(bounded) = entities
            .iter()
            .find(|candidate| candidate.id() == locus_entity(&midpoint)) else {
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
        let Some(point_entity) = entities
            .iter()
            .find(|candidate| candidate.id() == locus_entity(&point)) else {
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
            entity: cadmpeg_ir::sketches::SketchEntityId::mint(retained_string(
                ctx, bounded.id().as_str(), "fcstd midpoint line identity",
            )?).map_err(CodecError::malformed)?,
        }));
    }
    Ok(None)
}

fn bool_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<bool> {
    let value = node.attribute(name)?;
    if value == "1" || value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value == "0" || value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn label_attr(
    node: roxmltree::Node<'_, '_>,
    name: &str,
) -> Option<cadmpeg_ir::sketches::SketchLabelValue> {
    node.attribute(name)
        .and_then(|value| value.parse::<f64>().ok())
        .and_then(|value| cadmpeg_ir::sketches::SketchLabelValue::try_from(value).ok())
}

fn nonempty_attr(ctx: &DecodeContext<'_>, node: roxmltree::Node<'_, '_>, name: &str) -> Result<Option<String>, CodecError> {
    node.attribute(name).filter(|value| !value.is_empty())
        .map(|value| retained_string(ctx, value, "fcstd constraint attribute"))
        .transpose()
}

fn expression_binding(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    path: &str,
) -> Result<Option<(String, String)>, CodecError> {
    let Some(engine) = property(properties, "ExpressionEngine") else { return Ok(None); };
    let value = engine
        .values()
        .iter()
        .find(|value| {
            value.tag == "Expression"
                && value
                    .attributes
                    .get("path")
                    .is_some_and(|value| value == path)
        });
    let Some(expression) = value.and_then(|value| value.attributes.get("expression")) else {
        return Ok(None);
    };
    Ok(Some((
        retained_string(ctx, &engine.id, "fcstd expression engine reference")?,
        retained_string(ctx, expression, "fcstd expression text")?,
    )))
}

fn bind_parameter_dependencies(
    ctx: &DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    objects: &[ObjectRecord],
    cycle_affected_features: &BTreeSet<FeatureId>,
) -> Result<BTreeSet<FeatureId>, CodecError> {
    let mut object_names = HashMap::new();
    for object in objects {
        insert_hash_map(
            ctx, &mut object_names, feature_id(ctx, object)?, object.name.as_str(),
            "fcstd parameter dependency object names",
        )?;
    }
    let mut candidates = collection_vec(ctx, parameters.len(), "fcstd parameter dependency candidates")?;
    for parameter in parameters.iter() {
        let source_name = parameter.properties.get("source_name")
            .filter(|source_name| *source_name != &parameter.name);
        let mut names = collection_vec(
            ctx, 1 + usize::from(source_name.is_some()), "fcstd parameter candidate names",
        )?;
        names.push(retained_string(ctx, &parameter.name, "fcstd parameter candidate name")?);
        if let Some(source_name) = source_name {
            names.push(retained_string(ctx, source_name, "fcstd parameter source name")?);
        }
        candidates.push((
            ParameterId::mint(retained_string(ctx, parameter.id.as_str(), "fcstd parameter candidate identity")?)
                .map_err(CodecError::malformed)?,
            parameter.owner.as_ref().map(|owner| FeatureId::mint(retained_string(
                ctx, owner.as_str(), "fcstd parameter candidate owner",
            )?).map_err(CodecError::malformed)).transpose()?,
            names,
        ));
    }
    let mut local_candidates = HashMap::<(FeatureId, String), Vec<ParameterId>>::new();
    let mut qualified_candidates = HashMap::<String, Vec<ParameterId>>::new();
    for (id, owner, names) in &candidates {
        let Some(owner) = owner else { continue };
        for name in names {
            let key = (
                FeatureId::mint(retained_string(ctx, owner.as_str(), "fcstd local candidate owner")?)
                    .map_err(CodecError::malformed)?,
                retained_string(ctx, name, "fcstd local candidate name")?,
            );
            if !local_candidates.contains_key(&key) {
                ctx.charge_collection_items(1, "fcstd local candidate keys")?;
                local_candidates.try_reserve(1).map_err(|_| collection_allocation_failed(
                    ctx, 1, "fcstd local candidate keys",
                ))?;
            }
            let bucket = local_candidates.entry(key).or_default();
            reserve_vec_items(ctx, bucket, 1, "fcstd local candidate identities")?;
            bucket.push(ParameterId::mint(retained_string(
                ctx, id.as_str(), "fcstd local candidate identity",
            )?).map_err(CodecError::malformed)?);
            if let Some(object) = object_names.get(owner) {
                let key = retained_format(
                    ctx, format_args!("{object}.{name}"), "fcstd qualified candidate name",
                )?;
                if !qualified_candidates.contains_key(&key) {
                    ctx.charge_collection_items(1, "fcstd qualified candidate keys")?;
                    qualified_candidates.try_reserve(1).map_err(|_| collection_allocation_failed(
                        ctx, 1, "fcstd qualified candidate keys",
                    ))?;
                }
                let bucket = qualified_candidates.entry(key).or_default();
                reserve_vec_items(ctx, bucket, 1, "fcstd qualified candidate identities")?;
                bucket.push(ParameterId::mint(retained_string(
                    ctx, id.as_str(), "fcstd qualified candidate identity",
                )?).map_err(CodecError::malformed)?);
            }
        }
    }
    let mut local = HashMap::new();
    for (key, ids) in local_candidates {
        if ids.len() == 1 {
            insert_hash_map(
                ctx, &mut local, key, ids.into_iter().next().ok_or_else(|| CodecError::malformed(
                    "singleton local candidate lost its identity",
                ))?, "fcstd unique local candidates",
            )?;
        }
    }
    let mut qualified = HashMap::new();
    for (key, ids) in qualified_candidates {
        if ids.len() == 1 {
            insert_hash_map(
                ctx, &mut qualified, key, ids.into_iter().next().ok_or_else(|| CodecError::malformed(
                    "singleton qualified candidate lost its identity",
                ))?, "fcstd unique qualified candidates",
            )?;
        }
    }
    for parameter in parameters.iter_mut() {
        let mut dependencies = BTreeSet::new();
        for identifier in expression_identifiers(&parameter.expression) {
            let dependency = if let Some(qualified) = qualified.get(identifier) {
                Some(qualified)
            } else if let Some(owner) = parameter.owner.as_ref() {
                local.get(&(
                    FeatureId::mint(retained_string(
                        ctx, owner.as_str(), "fcstd dependency lookup owner",
                    )?).map_err(CodecError::malformed)?,
                    retained_string(ctx, identifier, "fcstd dependency lookup name")?,
                ))
            } else {
                None
            };
            if let Some(dependency) = dependency.filter(|id| **id != parameter.id) {
                if !dependencies.contains(dependency) {
                    ctx.charge_collection_items(1, "fcstd parameter dependencies")?;
                    dependencies.insert(ParameterId::mint(retained_string(
                        ctx, dependency.as_str(), "fcstd parameter dependency identity",
                    )?).map_err(CodecError::malformed)?);
                }
            }
        }
        parameter.dependencies = if parameter
            .owner
            .as_ref()
            .is_some_and(|owner| cycle_affected_features.contains(owner))
        {
            // The native property record retains the expression. A neutral
            // parameter edge would create an invented evaluation order for
            // a history that FreeCAD itself could not topologically sort.
            DistinctMembers::default()
        } else {
            let mut members = collection_vec(
                ctx, dependencies.len(), "fcstd parameter dependency members",
            )?;
            members.extend(dependencies);
            ctx.charge_collection_items(members.len() as u64, "fcstd parameter distinct check")?;
            members.try_into().map_err(CodecError::malformed)?
        };
    }
    let mut owner_ordinals = HashMap::<Option<FeatureId>, Vec<u32>>::new();
    for parameter in parameters.iter() {
        let owner = parameter.owner.as_ref().map(|owner| FeatureId::mint(retained_string(
            ctx, owner.as_str(), "fcstd ordinal owner identity",
        )?).map_err(CodecError::malformed)).transpose()?;
        if !owner_ordinals.contains_key(&owner) {
            ctx.charge_collection_items(1, "fcstd ordinal owner groups")?;
            owner_ordinals.try_reserve(1).map_err(|_| collection_allocation_failed(
                ctx, 1, "fcstd ordinal owner groups",
            ))?;
        }
        let ordinals = owner_ordinals.entry(owner).or_default();
        reserve_vec_items(ctx, ordinals, 1, "fcstd owner ordinals")?;
        ordinals.push(parameter.ordinal);
    }
    for ordinals in owner_ordinals.values_mut() {
        ordinals.sort_unstable();
    }
    let parameter_cycle_features = order_parameters_by_dependencies(ctx, parameters)?;
    for parameter in parameters.iter_mut() {
        if parameter
            .owner
            .as_ref()
            .is_some_and(|owner| parameter_cycle_features.contains(owner))
        {
            // The native property record retains the expression. A neutral
            // parameter edge would create an invented evaluation order for
            // a history that FreeCAD itself could not topologically sort.
            parameter.dependencies.clear();
        }
    }
    let mut next_ordinal = HashMap::<Option<FeatureId>, usize>::new();
    for parameter in parameters {
        let owner = parameter.owner.as_ref().map(|owner| FeatureId::mint(retained_string(
            ctx, owner.as_str(), "fcstd next ordinal owner",
        )?).map_err(CodecError::malformed)).transpose()?;
        if !next_ordinal.contains_key(&owner) {
            ctx.charge_collection_items(1, "fcstd next ordinal owners")?;
            next_ordinal.try_reserve(1).map_err(|_| collection_allocation_failed(
                ctx, 1, "fcstd next ordinal owners",
            ))?;
        }
        let index = next_ordinal.entry(owner).or_default();
        parameter.ordinal = owner_ordinals[&parameter.owner][*index];
        *index += 1;
    }
    Ok(parameter_cycle_features)
}

fn order_parameters_by_dependencies(
    ctx: &DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
) -> Result<BTreeSet<FeatureId>, CodecError> {
    let mut known = BTreeSet::new();
    for parameter in parameters.iter() {
        if !known.contains(&parameter.id) {
            ctx.charge_collection_items(1, "fcstd known parameter identities")?;
            known.insert(ParameterId::mint(retained_string(
                ctx, parameter.id.as_str(), "fcstd known parameter identity",
            )?).map_err(CodecError::malformed)?);
        }
    }
    let mut remaining = std::mem::take(parameters);
    let mut emitted = BTreeSet::new();
    let mut cycle_features = BTreeSet::new();
    while !remaining.is_empty() {
        ctx.charge_work(remaining.len() as u64, "fcstd parameter dependency ordering")?;
        let Some(index) = remaining.iter().position(|parameter| {
            parameter
                .dependencies
                .iter()
                .all(|dependency| !known.contains(dependency) || emitted.contains(dependency))
        }) else {
            for owner in remaining.iter().filter_map(|parameter| parameter.owner.as_ref()) {
                if !cycle_features.contains(owner) {
                    ctx.charge_collection_items(1, "fcstd parameter cycle owners")?;
                    cycle_features.insert(FeatureId::mint(retained_string(
                        ctx, owner.as_str(), "fcstd parameter cycle owner identity",
                    )?).map_err(CodecError::malformed)?);
                }
            }
            reserve_vec_items(ctx, parameters, remaining.len(), "fcstd reordered parameters")?;
            parameters.append(&mut remaining);
            break;
        };
        let parameter = remaining.remove(index);
        ctx.charge_collection_items(1, "fcstd emitted parameter identities")?;
        emitted.insert(ParameterId::mint(retained_string(
            ctx, parameter.id.as_str(), "fcstd emitted parameter identity",
        )?).map_err(CodecError::malformed)?);
        reserve_vec_items(ctx, parameters, 1, "fcstd reordered parameters")?;
        parameters.push(parameter);
    }
    Ok(cycle_features)
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = &str> {
    expression
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '.'
        })
        .filter(|identifier| !identifier.is_empty())
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
    let entity = |index| loci.get(index)
        .map(|locus| copy_constraint_entity(ctx, locus_entity(locus))).transpose();
    let locus = |index| loci.get(index)
        .map(|locus| copy_constraint_locus(ctx, locus)).transpose();
    let pair = || -> Result<Option<(SketchEntityId, SketchEntityId)>, CodecError> {
        let Some(first) = entity(0)? else { return Ok(None); };
        let Some(second) = entity(1)? else { return Ok(None); };
        Ok(Some((first, second)))
    };
    let parameter = || parameter.map(|id| ParameterId::mint(retained_string(
        ctx, id.as_str(), "fcstd constraint parameter identity copy",
    )?).map_err(CodecError::malformed)).transpose();
    Ok(Some(match kind {
        0 => SketchConstraintDefinitionInput::Disabled {},
        1 => SketchConstraintDefinitionInput::CoincidentLoci {
            loci: copy_constraint_loci(ctx, loci)?,
        },
        2 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            SketchConstraintDefinitionInput::Horizontal { entity }
        }
        3 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            SketchConstraintDefinitionInput::Vertical { entity }
        }
        4 => {
            let Some((first, second)) = pair()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Parallel { first, second }
        }
        5 => {
            let Some((first, second)) = pair()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Tangent { first, second }
        }
        10 => {
            let Some((first, second)) = pair()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Perpendicular { first, second }
        }
        12 => {
            let Some((first, second)) = pair()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Equal { first, second }
        }
        13 => {
            let Some(point) = locus(0)? else { return Ok(None); };
            let Some(entity) = entity(1)? else { return Ok(None); };
            SketchConstraintDefinitionInput::PointOnObject { point, entity }
        }
        17 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            SketchConstraintDefinitionInput::Fixed { entity }
        }
        6 if loci.len() == 2 => {
            let Some(first) = locus(0)? else { return Ok(None); };
            let Some(second) = locus(1)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::DistanceLoci { first, second, parameter }
        }
        6 => {
            let mut entities = collection_vec(ctx, loci.len(), "fcstd constraint entity copies")?;
            for locus in loci {
                entities.push(copy_constraint_entity(ctx, locus_entity(locus))?);
            }
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Distance { entities, parameter }
        }
        7 => {
            let Some(first) = locus(0)? else { return Ok(None); };
            let Some(second) = locus(1)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::HorizontalDistance { first, second, parameter }
        }
        8 => {
            let Some(first) = locus(0)? else { return Ok(None); };
            let Some(second) = locus(1)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::VerticalDistance { first, second, parameter }
        }
        9 if loci.len() == 2 && sketch_axis(&loci[0]).is_some() => {
            let Some(entity) = entity(1)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            let Some(axis) = sketch_axis(&loci[0]) else { return Ok(None); };
            SketchConstraintDefinitionInput::AngleToAxis {
                entity, axis, parameter,
            }
        }
        9 if loci.len() == 2 && sketch_axis(&loci[1]).is_some() => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            let Some(axis) = sketch_axis(&loci[1]) else { return Ok(None); };
            SketchConstraintDefinitionInput::AngleToAxis {
                entity, axis, parameter,
            }
        }
        9 if loci.len() == 1 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::AngleToAxis {
                entity, axis: SketchAxis::Horizontal, parameter,
            }
        }
        9 => {
            let Some(first) = entity(0)? else { return Ok(None); };
            let Some(second) = entity(1)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Angle { first, second, parameter }
        }
        11 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Radius { entity, parameter }
        }
        18 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Diameter { entity, parameter }
        }
        16 => {
            let Some(incident) = locus(0)? else { return Ok(None); };
            let Some(refracted) = locus(1)? else { return Ok(None); };
            let Some(interface) = entity(2)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::SnellsLaw {
                incident, refracted, interface, parameter,
            }
        }
        19 => {
            let Some(entity) = entity(0)? else { return Ok(None); };
            let Some(parameter) = parameter()? else { return Ok(None); };
            SketchConstraintDefinitionInput::Weight { entity, parameter }
        }
        14 => {
            let Some(first) = locus(0)? else { return Ok(None); };
            let Some(second) = locus(1)? else { return Ok(None); };
            let Some(axis) = entity(2)? else { return Ok(None); };
            SketchConstraintDefinitionInput::Symmetric { first, second, axis }
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

fn constraint_operands(ctx: &DecodeContext<'_>, node: roxmltree::Node<'_, '_>) -> Result<Vec<(i64, i64)>, CodecError> {
    match (
        node.attribute("ElementIds"),
        node.attribute("ElementPositions"),
    ) {
        (Some(ids), Some(positions)) => {
            let ids = split_ints(ctx, ids)?;
            let positions = split_ints(ctx, positions)?;
            if ids.len() != positions.len() {
                return Err(CodecError::malformed("ElementIds and ElementPositions counts differ"));
            }
            let mut operands = collection_vec(ctx, ids.len(), "fcstd constraint operand pairs")?;
            operands.extend(ids.into_iter().zip(positions).filter(|(entity, _)| *entity != -2000));
            return Ok(operands);
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(CodecError::malformed("ElementIds and ElementPositions must both be present"));
        }
        (None, None) => {}
    }
    let mut operands = Vec::new();
    for (entity_name, position_name) in [
        ("First", "FirstPos"),
        ("Second", "SecondPos"),
        ("Third", "ThirdPos"),
    ] {
        match (node.attribute(entity_name), node.attribute(position_name)) {
            (None, None) => {}
            (Some(entity), Some(position)) => {
                let entity = entity
                    .parse::<i64>()
                    .map_err(|_| CodecError::malformed("constraint entity is not an integer"))?;
                let position = position
                    .parse::<i64>()
                    .map_err(|_| CodecError::malformed("constraint position is not an integer"))?;
                if entity != -2000 {
                    operands.push((entity, position));
                }
            }
            _ => return Err(CodecError::malformed("constraint entity and position must both be present")),
        }
    }
    Ok(operands)
}

fn direct_counted_records<'a, 'input>(
    ctx: &DecodeContext<'_>,
    xml: &'a roxmltree::Document<'input>,
    container_tag: &str,
    record_tag: &str,
    owner: &str,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut containers = xml.root_element().children()
        .filter(|node| node.is_element() && node.has_tag_name(container_tag));
    let Some(container) = containers.next() else {
        return Err(malformed_design(ctx, format_args!(
            "{owner} must contain exactly one direct {container_tag} value"
        )));
    };
    if containers.next().is_some()
        || xml
            .descendants()
            .filter(|node| node.has_tag_name(container_tag))
            .count()
            != 1
    {
        return Err(malformed_design(ctx, format_args!(
            "{owner} must contain exactly one direct {container_tag} value"
        )));
    }
    let declared = container
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            malformed_design(ctx, format_args!("{owner} has an invalid record count"))
        })?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(malformed_design(ctx, format_args!(
            "{owner} record count exceeds {MAX_SKETCH_RECORDS}"
        )));
    }
    let found = container.children().filter(roxmltree::Node::is_element).count();
    if container.children().filter(roxmltree::Node::is_element).any(|node| !node.has_tag_name(record_tag)) {
        return Err(malformed_design(ctx, format_args!(
            "{owner} has a non-{record_tag} direct child"
        )));
    }
    if xml
        .descendants()
        .filter(|node| node.has_tag_name(record_tag))
        .count()
        != found
    {
        return Err(malformed_design(ctx, format_args!(
            "{owner} has nested {record_tag} records"
        )));
    }
    if declared != found {
        return Err(malformed_design(ctx, format_args!(
            "{owner} declares {declared} records but contains {}",
            found
        )));
    }
    let mut records = collection_vec(ctx, found, "fcstd counted sketch records")?;
    records.extend(container.children().filter(roxmltree::Node::is_element));
    Ok(records)
}

fn split_ints(ctx: &DecodeContext<'_>, value: &str) -> Result<Vec<i64>, CodecError> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for group in value.split(',') {
        if group.trim().is_empty() {
            return Err(CodecError::malformed("constraint integer list has an empty item"));
        }
        for part in group.split_ascii_whitespace() {
            reserve_vec_items(ctx, &mut values, 1, "fcstd constraint integer list")?;
            values.push(
                part.parse::<i64>()
                    .map_err(|_| CodecError::malformed("constraint integer list has an invalid integer"))?,
            );
        }
    }
    Ok(values)
}

fn int_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<i64> {
    node.attribute(name)?.parse().ok()
}

fn resolve_operand(
    ctx: &DecodeContext<'_>,
    entity: i64,
    position: i64,
    entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, CodecError> {
    let reference = |suffix: &str| {
        entities
            .iter()
            .find(|candidate| candidate.id().as_str().ends_with(suffix))
            .map(|candidate| retained_string(ctx, candidate.id().as_str(), "fcstd resolved operand identity")
                .and_then(|id| SketchEntityId::mint(id).map_err(CodecError::malformed))
                .map(SketchLocus::Entity))
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
        let Some(external_index) = entity.checked_neg().and_then(|value| value.checked_sub(3))
            .and_then(|value| usize::try_from(value).ok()) else { return Ok(None); };
        let suffix = format!(":external:{external_index}");
        let Some(entity) = entities
            .iter()
            .find(|candidate| candidate.id().as_str().ends_with(&suffix)) else {
            return Ok(None);
        };
        return sketch_locus(ctx, entity, position);
    }
    let Some(entity) = usize::try_from(entity).ok().and_then(|index| entities.get(index)) else {
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
    let id = SketchEntityId::mint(retained_string(
        ctx, entity.id().as_str(), "fcstd resolved operand identity",
    )?).map_err(CodecError::malformed)?;
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

fn copy_constraint_entity(
    ctx: &DecodeContext<'_>,
    entity: &SketchEntityId,
) -> Result<SketchEntityId, CodecError> {
    SketchEntityId::mint(retained_string(
        ctx, entity.as_str(), "fcstd constraint entity identity",
    )?).map_err(CodecError::malformed)
}

fn copy_constraint_locus(
    ctx: &DecodeContext<'_>,
    locus: &SketchLocus,
) -> Result<SketchLocus, CodecError> {
    let entity = copy_constraint_entity(ctx, locus_entity(locus))?;
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
    let mut copies = collection_vec(ctx, loci.len(), "fcstd constraint locus copies")?;
    for locus in loci {
        copies.push(copy_constraint_locus(ctx, locus)?);
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
    if !kind.chars().any(|character| !character.is_whitespace()) {
        return Err(CodecError::malformed("native_kind must not be empty"));
    }
    let number = |name: &str| attributes.get(name).and_then(|value| value.parse().ok());
    let native = || -> Result<SketchGeometry, CodecError> {
        let native_kind = cadmpeg_core::text::NonBlankString::new(retained_string(
            ctx, kind, "fcstd native sketch geometry kind",
        )?).ok_or_else(|| CodecError::malformed("native_kind must not be empty"))?;
        Ok(SketchGeometry::native(native_kind))
    };
    if matches!(kind, "Part::GeomArcOfCircle" | "ArcOfCircle") {
        let frame_angle = number("AngleXU").unwrap_or(0.0);
        let admitted = (|| {
            let center = FinitePoint2::from_coordinates(
                FiniteReal::new(number("CenterX")?)?,
                FiniteReal::new(number("CenterY")?)?,
            );
            let radius = PositiveLength::new(number("Radius")?)?;
            let start =
                FiniteReal::new(number("StartAngle").or_else(|| number("FirstParameter"))?)?;
            let end = FiniteReal::new(number("EndAngle").or_else(|| number("LastParameter"))?)?;
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
    let project = || {
        Some(
            if matches!(
                kind,
                "Part::GeomLine" | "Part::GeomLineSegment" | "Line" | "LineSegment"
            ) {
                match (
                    number("StartX"),
                    number("StartY"),
                    number("EndX"),
                    number("EndY"),
                ) {
                    (Some(start_x), Some(start_y), Some(end_x), Some(end_y)) => {
                        SketchGeometryDefinition::Line {
                            start: Point2::new(start_x, start_y),
                            end: Point2::new(end_x, end_y),
                        }
                    }
                    _ => return None,
                }
            } else if matches!(
                kind,
                "Part::GeomEllipse" | "Part::GeomArcOfEllipse" | "Ellipse" | "ArcOfEllipse"
            ) {
                let major_angle = number("MajorAngle")
                    .or_else(|| number("AngleXU"))
                    .or_else(|| Some(number("MajorAxisY")?.atan2(number("MajorAxisX")?)));
                let bounds = if matches!(kind, "Part::GeomArcOfEllipse" | "ArcOfEllipse") {
                    number("StartAngle")
                        .or_else(|| number("FirstParameter"))
                        .zip(number("EndAngle").or_else(|| number("LastParameter")))
                        .map(|(start, end)| Some([start, end]))
                } else {
                    Some(None)
                };
                match (
                    number("CenterX"),
                    number("CenterY"),
                    major_angle,
                    number("MajorRadius"),
                    number("MinorRadius"),
                    bounds,
                ) {
                    (Some(x), Some(y), Some(angle), Some(major), Some(minor), Some(bounds))
                        if major > 0.0 && minor > 0.0 =>
                    {
                        SketchGeometryDefinition::Ellipse {
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
                        }
                    }
                    _ => return None,
                }
            } else if matches!(
                kind,
                "Part::GeomHyperbola" | "Part::GeomArcOfHyperbola" | "Hyperbola" | "ArcOfHyperbola"
            ) {
                let bounds = if matches!(kind, "Part::GeomArcOfHyperbola" | "ArcOfHyperbola") {
                    number("StartAngle")
                        .or_else(|| number("FirstParameter"))
                        .zip(number("EndAngle").or_else(|| number("LastParameter")))
                        .map(|(start, end)| Some([start, end]))
                } else {
                    Some(None)
                };
                match (
                    number("CenterX"),
                    number("CenterY"),
                    number("AngleXU").or_else(|| number("MajorAngle")),
                    number("MajorRadius"),
                    number("MinorRadius"),
                    bounds,
                ) {
                    (Some(x), Some(y), Some(angle), Some(major), Some(minor), Some(bounds))
                        if major > 0.0 && minor > 0.0 =>
                    {
                        SketchGeometryDefinition::Hyperbola {
                            center: Point2::new(x, y),
                            major_angle: cadmpeg_ir::scalar::Angle::new(angle)?,
                            major_radius: Length::new(major)?,
                            minor_radius: Length::new(minor)?,
                            bounds,
                        }
                    }
                    _ => return None,
                }
            } else if matches!(
                kind,
                "Part::GeomParabola" | "Part::GeomArcOfParabola" | "Parabola" | "ArcOfParabola"
            ) {
                let bounds = if matches!(kind, "Part::GeomArcOfParabola" | "ArcOfParabola") {
                    number("StartAngle")
                        .or_else(|| number("FirstParameter"))
                        .zip(number("EndAngle").or_else(|| number("LastParameter")))
                        .map(|(start, end)| Some([start, end]))
                } else {
                    Some(None)
                };
                match (
                    number("CenterX"),
                    number("CenterY"),
                    number("AngleXU").or_else(|| number("AxisAngle")),
                    number("Focal"),
                    bounds,
                ) {
                    (Some(x), Some(y), Some(angle), Some(focal), Some(bounds)) if focal > 0.0 => {
                        SketchGeometryDefinition::Parabola {
                            vertex: Point2::new(x, y),
                            axis_angle: cadmpeg_ir::scalar::Angle::new(angle)?,
                            focal_length: Length::new(focal)?,
                            bounds,
                        }
                    }
                    _ => return None,
                }
            } else if matches!(kind, "Part::GeomCircle" | "Circle") {
                match (number("CenterX"), number("CenterY"), number("Radius")) {
                    (Some(x), Some(y), Some(radius)) if radius > 0.0 => {
                        SketchGeometryDefinition::Circle {
                            center: Point2::new(x, y),
                            radius: Length::new(radius)?,
                        }
                    }
                    (Some(x), Some(y), Some(0.0)) => SketchGeometryDefinition::Point {
                        position: Point2::new(x, y),
                    },
                    _ => return None,
                }
            } else if kind == "Part::GeomPoint" {
                match (number("X"), number("Y")) {
                    (Some(x), Some(y)) => SketchGeometryDefinition::Point {
                        position: Point2::new(x, y),
                    },
                    _ => return None,
                }
            } else {
                return None
            },
        )
    };
    match project() {
        Some(definition) => SketchGeometry::try_from(definition).map_err(CodecError::malformed),
        None => native(),
    }
}

fn build_profiles(
    ctx: &DecodeContext<'_>,
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    // Internal entities arrive in GeometryList order; appended external and built-in reference
    // entities are construction entries. Indices therefore preserve the persisted ordinal for
    // every eligible profile entity.
    ctx.charge_work(entities.len() as u64, "FCStd profile entity scan")?;
    let eligible_count = entities
        .iter()
        .filter(|entity| !entity.construction)
        .count();
    let ordinal_work = eligible_count as u64 * (u64::from(eligible_count.max(2).ilog2()) + 1);
    ctx.charge_work(ordinal_work, "FCStd profile ordinal index")?;
    ctx.charge_collection_items(eligible_count as u64, "FCStd profile ordinals")?;
    let profile_entities = entities
        .iter()
        .enumerate()
        .filter(|(_, entity)| !entity.construction)
        .map(|(index, _)| index)
        .collect::<BTreeSet<_>>();
    ctx.charge_work(eligible_count as u64, "FCStd remaining profile ordinals")?;
    ctx.charge_collection_items(eligible_count as u64, "FCStd remaining profile ordinals")?;
    let mut unused = profile_entities.clone();
    let explicit_relations =
        explicit_endpoint_relations(ctx, &profile_entities, entities, constraints)?;
    let index = EndpointIndex::new(ctx, &profile_entities, entities)?;
    let mut ambiguous = BTreeSet::new();
    ctx.charge_work(eligible_count as u64 * 2, "FCStd profile ambiguity scan")?;
    for &entity in &unused {
        for start in [true, false] {
            let matches = endpoint_candidates(
                ctx,
                EndpointLocus { entity, start },
                &unused,
                &explicit_relations,
                entities,
                &index,
            )?;
            if matches.len() > 1 {
                if !ambiguous.contains(&entity) {
                    ctx.charge_collection_items(1, "FCStd ambiguous profile ordinals")?;
                    ambiguous.insert(entity);
                }
                for candidate in matches {
                    if !ambiguous.contains(&candidate.entity) {
                        ctx.charge_collection_items(1, "FCStd ambiguous profile ordinals")?;
                        ambiguous.insert(candidate.entity);
                    }
                }
            }
        }
    }
    let mut profiles = Vec::new();
    // FreeCAD persists no profile seed. CADIR selects the first remaining persisted ordinal.
    while let Some(first) = unused.pop_first() {
        ctx.charge_work(1, "FCStd profile chain construction")?;
        ctx.charge_collection_items(1, "FCStd profile uses")?;
        let mut chain = VecDeque::new();
        chain.try_reserve(1).map_err(|_| collection_allocation_failed(
            ctx, 1, "FCStd profile uses",
        ))?;
        chain.push_back(SketchEntityUse {
            entity: SketchEntityId::mint(retained_string(
                ctx, entities[first].id().as_str(), "FCStd profile use identity",
            )?).map_err(CodecError::malformed)?,
            reversed: false,
        });
        if ambiguous.contains(&first) {
            reserve_vec_items(ctx, &mut profiles, 1, "FCStd profile chains")?;
            profiles.push(chain.into());
            continue;
        }
        if endpoints(&entities[first]).is_none() {
            reserve_vec_items(ctx, &mut profiles, 1, "FCStd profile chains")?;
            profiles.push(chain.into());
            continue;
        }
        let mut head = EndpointLocus {
            entity: first,
            start: true,
        };
        let mut tail = EndpointLocus {
            entity: first,
            start: false,
        };
        loop {
            let mut candidates =
                endpoint_candidates(ctx, tail, &unused, &explicit_relations, entities, &index)?;
            candidates.retain(|candidate| !ambiguous.contains(&candidate.entity));
            let Some(candidate) = (candidates.len() == 1).then(|| candidates[0]) else {
                break;
            };
            let (reversed, next_tail) = if candidate.start {
                (
                    false,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: false,
                    },
                )
            } else {
                (
                    true,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: true,
                    },
                )
            };
            unused.remove(&candidate.entity);
            ctx.charge_collection_items(1, "FCStd profile uses")?;
            chain.try_reserve(1).map_err(|_| collection_allocation_failed(
                ctx, 1, "FCStd profile uses",
            ))?;
            chain.push_back(SketchEntityUse {
                entity: SketchEntityId::mint(retained_string(
                    ctx, entities[candidate.entity].id().as_str(), "FCStd profile use identity",
                )?).map_err(CodecError::malformed)?,
                reversed,
            });
            tail = next_tail;
        }
        loop {
            let mut candidates =
                endpoint_candidates(ctx, head, &unused, &explicit_relations, entities, &index)?;
            candidates.retain(|candidate| !ambiguous.contains(&candidate.entity));
            let Some(candidate) = (candidates.len() == 1).then(|| candidates[0]) else {
                break;
            };
            let (reversed, next_head) = if candidate.start {
                (
                    true,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: false,
                    },
                )
            } else {
                (
                    false,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: true,
                    },
                )
            };
            unused.remove(&candidate.entity);
            ctx.charge_collection_items(1, "FCStd profile uses")?;
            chain.try_reserve(1).map_err(|_| collection_allocation_failed(
                ctx, 1, "FCStd profile uses",
            ))?;
            chain.push_front(SketchEntityUse {
                entity: SketchEntityId::mint(retained_string(
                    ctx, entities[candidate.entity].id().as_str(), "FCStd profile use identity",
                )?).map_err(CodecError::malformed)?,
                reversed,
            });
            head = next_head;
        }
        reserve_vec_items(ctx, &mut profiles, 1, "FCStd profile chains")?;
        profiles.push(chain.into());
    }
    Ok(profiles)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EndpointLocus {
    entity: usize,
    start: bool,
}

struct IndexedEndpoint {
    locus: EndpointLocus,
    point: Point2,
}

struct EndpointIndex {
    by_scale: BTreeMap<u64, Vec<IndexedEndpoint>>,
}

impl EndpointIndex {
    fn new(
        ctx: &DecodeContext<'_>,
        profile_entities: &BTreeSet<usize>,
        entities: &[SketchEntity],
    ) -> Result<Self, CodecError> {
        let mut by_scale = BTreeMap::<u64, Vec<IndexedEndpoint>>::new();
        for &index in profile_entities {
            ctx.charge_work(1, "FCStd profile endpoint extraction")?;
            if let Some((start, end)) = endpoints(&entities[index]) {
                ctx.charge_collection_items(2, "FCStd profile endpoint index")?;
                for (at_start, point) in [(true, start), (false, end)] {
                    by_scale
                        .entry(endpoint_scale_bucket(point))
                        .or_default()
                        .push(IndexedEndpoint {
                            locus: EndpointLocus {
                                entity: index,
                                start: at_start,
                            },
                            point,
                        });
                }
            }
        }
        for bucket in by_scale.values_mut() {
            let sorting_work = bucket.len() as u64 * (u64::from(bucket.len().max(2).ilog2()) + 1);
            ctx.charge_work(sorting_work, "FCStd profile index sort")?;
            bucket.sort_by(|left, right| left.point.u.total_cmp(&right.point.u));
        }
        Ok(Self { by_scale })
    }
}

fn endpoint_scale_bucket(point: Point2) -> u64 {
    point.u.abs().max(point.v.abs()).max(1.0).to_bits() >> 52
}

fn endpoint_candidates(
    ctx: &DecodeContext<'_>,
    endpoint: EndpointLocus,
    available: &BTreeSet<usize>,
    explicit_relations: &BTreeMap<EndpointLocus, BTreeSet<EndpointLocus>>,
    entities: &[SketchEntity],
    index: &EndpointIndex,
) -> Result<Vec<EndpointLocus>, CodecError> {
    // Active explicit coincident loci override coordinates. Coordinate matching below is the
    // decoder-owned CADIR boundary, not a producer tolerance.
    if let Some(explicit) = explicit_relations.get(&endpoint) {
        ctx.charge_work(explicit.len() as u64, "FCStd explicit profile candidates")?;
        let match_count = explicit
            .iter()
            .filter(|candidate| available.contains(&candidate.entity))
            .count();
        ctx.charge_collection_items(match_count as u64, "FCStd profile candidates")?;
        let matches = explicit
            .iter()
            .copied()
            .filter(|candidate| available.contains(&candidate.entity))
            .collect::<Vec<_>>();
        return Ok(matches);
    }
    let Some(point) = endpoint_point(endpoint, entities) else {
        return Ok(Vec::new());
    };
    let mut matches = Vec::new();
    let scale = endpoint_scale_bucket(point);
    for bucket_number in (scale - 1)..=(scale + 1) {
        let Some(bucket) = index.by_scale.get(&bucket_number) else {
            continue;
        };
        let bucket_scale = f64::from_bits((bucket_number.max(scale) + 1) << 52).min(f64::MAX);
        let tolerance = SKETCH_ENDPOINT_ROUNDING_ULPS * f64::EPSILON * bucket_scale;
        ctx.charge_work(
            bucket.len().max(2).ilog2() as u64 + 1,
            "FCStd profile index search",
        )?;
        let first = bucket.partition_point(|candidate| candidate.point.u < point.u - tolerance);
        for candidate in bucket[first..]
            .iter()
            .take_while(|candidate| candidate.point.u <= point.u + tolerance)
        {
            ctx.charge_work(1, "FCStd profile candidate comparison")?;
            if candidate.locus.entity != endpoint.entity
                && available.contains(&candidate.locus.entity)
                && !explicit_relations.contains_key(&candidate.locus)
                && endpoints_match_by_roundoff(point, candidate.point)
            {
                ctx.charge_collection_items(1, "FCStd profile candidates")?;
                matches.push(candidate.locus);
            }
        }
    }
    ctx.charge_work(
        matches.len().max(2).ilog2() as u64 * matches.len() as u64,
        "FCStd profile candidate order",
    )?;
    matches.sort_unstable();
    Ok(matches)
}

fn explicit_endpoint_relations(
    ctx: &DecodeContext<'_>,
    profile_entities: &BTreeSet<usize>,
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> Result<BTreeMap<EndpointLocus, BTreeSet<EndpointLocus>>, CodecError> {
    ctx.charge_work(entities.len() as u64, "FCStd profile entity lookup")?;
    ctx.charge_collection_items(entities.len() as u64, "FCStd profile entity lookup")?;
    let entity_indices = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.id().as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut relations = BTreeMap::new();
    ctx.charge_work(constraints.len() as u64, "FCStd profile constraint scan")?;
    for constraint in constraints {
        if constraint.active == Some(false) {
            continue;
        }
        let SketchConstraintDefinitionInput::CoincidentLoci { loci } = constraint.definition.kind()
        else {
            continue;
        };
        let endpoint_count = loci
            .iter()
            .filter(|locus| matches!(locus, SketchLocus::Start(_) | SketchLocus::End(_)))
            .count();
        ctx.charge_work(endpoint_count as u64, "FCStd explicit profile loci")?;
        ctx.charge_collection_items(endpoint_count as u64, "FCStd explicit profile loci")?;
        let endpoints = loci
            .iter()
            .filter_map(|locus| match locus {
                SketchLocus::Start(entity) => Some(EndpointLocus {
                    entity: entity_indices.get(entity.as_str()).copied()?,
                    start: true,
                }),
                SketchLocus::End(entity) => Some(EndpointLocus {
                    entity: entity_indices.get(entity.as_str()).copied()?,
                    start: false,
                }),
                _ => None,
            })
            .filter(|locus| profile_entities.contains(&locus.entity))
            .collect::<BTreeSet<_>>();
        for first in endpoints.iter().copied() {
            for candidate in endpoints
                .iter()
                .copied()
                .filter(|candidate| *candidate != first)
            {
                ctx.charge_work(1, "FCStd explicit profile relations")?;
                let related = relations.entry(first).or_insert_with(BTreeSet::new);
                if !related.contains(&candidate) {
                    ctx.charge_collection_items(1, "FCStd explicit profile relations")?;
                    related.insert(candidate);
                }
            }
        }
    }
    Ok(relations)
}

fn endpoint_point(endpoint: EndpointLocus, entities: &[SketchEntity]) -> Option<Point2> {
    endpoints(&entities[endpoint.entity])
        .map(|points| if endpoint.start { points.0 } else { points.1 })
}

fn endpoints(entity: &SketchEntity) -> Option<(Point2, Point2)> {
    match *entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some((start.get(), end.get())),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            Point2::new(
                center.u + radius.get() * start_angle.get().cos(),
                center.v + radius.get() * start_angle.get().sin(),
            ),
            Point2::new(
                center.u + radius.get() * end_angle.get().cos(),
                center.v + radius.get() * end_angle.get().sin(),
            ),
        )),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds: Some([start, end]),
        } => {
            let major = Point2::new(major_angle.get().cos(), major_angle.get().sin());
            let minor = Point2::new(-major.v, major.u);
            let point = |parameter: f64| {
                let (along_major, along_minor) = (parameter.cos(), parameter.sin());
                Point2::new(
                    center.u
                        + radii.major().get() * along_major * major.u
                        + radii.minor().get() * along_minor * minor.u,
                    center.v
                        + radii.major().get() * along_major * major.v
                        + radii.minor().get() * along_minor * minor.v,
                )
            };
            Some((point(start.get()), point(end.get())))
        }
        _ => None,
    }
}

const SKETCH_ENDPOINT_ROUNDING_ULPS: f64 = 64.0;

fn endpoints_match_by_roundoff(a: Point2, b: Point2) -> bool {
    let scale =
        a.u.abs()
            .max(a.v.abs())
            .max(b.u.abs())
            .max(b.v.abs())
            .max(1.0);
    (a.u - b.u).hypot(a.v - b.v) <= SKETCH_ENDPOINT_ROUNDING_ULPS * f64::EPSILON * scale
}

fn profile_ref(
    ctx: &DecodeContext<'_>,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Result<ProfileRef, CodecError> {
    let Some((property, target)) = profile_target(properties) else {
        return Ok(ProfileRef::Planar(PlanarProfileRef::Unresolved(
            retained_string(ctx, owner, "fcstd unresolved profile reference")?,
        )));
    };
    Ok(ProfileRef::Planar(match sketches.get(target) {
        Some(sketch) => PlanarProfileRef::Sketch(cadmpeg_ir::sketches::SketchId::mint(
            retained_string(ctx, sketch.as_str(), "fcstd sketch profile reference")?,
        ).map_err(CodecError::malformed)?),
        None => PlanarProfileRef::Native(retained_string(
            ctx, &property.id, "fcstd native profile reference",
        )?),
    }))
}

fn profile_target<'a>(properties: &'a [&PropertyRecord]) -> Option<(&'a PropertyRecord, &'a str)> {
    let mut selected = None;
    for name in ["Profile", "Sketch", "Base", "Source"] {
        let Some(property) = property(properties, name) else {
            continue;
        };
        let Some(link) = scalar_link(property) else {
            if name == "Base" && !is_link_property_type(property.type_name.as_str()) {
                continue;
            }
            return None;
        };
        let target = link.object()?;
        if target.is_empty() || selected.is_some() {
            return None;
        }
        selected = Some((property, target));
    }
    selected
}

fn revolution_axis(properties: &[&PropertyRecord]) -> Option<RevolutionAxis> {
    Some(RevolutionAxis {
        origin: vector_property(properties, "Base")
            .map_or(cadmpeg_ir::features::FinitePoint3::ZERO, |vector| {
                vector.as_point()
            }),
        direction: cadmpeg_ir::features::FeatureDirection3::new(
            vector_property(properties, "Axis")?.get(),
        )?,
        reference: None,
    })
}

fn revolution_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let face_maker_class = if kind == "Part::Revolution" {
        match property(properties, "FaceMakerClass") {
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
    let Some(mut axis) = revolution_axis(properties) else { return Ok(None); };
    let Some(direction) = cadmpeg_ir::units::UnitVector3::normalized(*axis.direction) else {
        return Ok(None);
    };
    axis.direction = cadmpeg_ir::features::FeatureDirection3::from(direction);
    let angle = || {
        scalar_named(properties, "Angle")
            .filter(|angle| angle.get() > 0.0)
            .and_then(|angle| cadmpeg_ir::scalar::PositiveAngle::new(angle.get().to_radians()))
    };
    let Some(mode) = enumeration_selector(properties, "Type", 0) else { return Ok(None); };
    let extent = if kind == "Part::Revolution" {
        let Some(angle) = angle() else { return Ok(None); };
        let Some(symmetric) = bool_selector(properties, "Symmetric", false) else {
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
                let Some(angle) = angle() else { return Ok(None); };
                let Some(midplane) = bool_selector(properties, "Midplane", false) else {
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
                let Some(face) = singular_operand(properties, "UpToFace") else {
                    return Ok(None);
                };
                RevolveExtent::OneSided {
                    termination: AngularTermination::ToFace {
                        face: cadmpeg_ir::features::FaceSelection::Native(retained_string(
                            ctx, &face.id, "fcstd revolution terminal face",
                        )?),
                        offset: None,
                    },
                }
            }
            4 => {
                let Some(first) = angle() else { return Ok(None); };
                let Some(second) = scalar_named(properties, "Angle2")
                    .filter(|angle| angle.get() > 0.0)
                    .and_then(|angle| cadmpeg_ir::scalar::PositiveAngle::new(angle.get().to_radians()))
                else { return Ok(None); };
                RevolveExtent::TwoSided {
                    first: AngularTermination::Angle { angle: first },
                    second: AngularTermination::Angle { angle: second },
                }
            }
            _ => return Ok(None),
        }
    };
    let reversed = if kind.starts_with("PartDesign::") {
        let Some(reversed) = bool_selector(properties, "Reversed", false) else {
            return Ok(None);
        };
        reversed
    } else {
        false
    };
    if reversed {
        axis.direction = axis.direction.reversed();
    }
    let axis_reference_properties = ["AxisLink", "ReferenceAxis"]
        .iter()
        .filter_map(|name| property(properties, name))
        .collect::<Vec<_>>();
    axis.reference = match axis_reference_properties.as_slice() {
        [] => None,
        [property] => {
            if property.links().iter().any(|link| nonempty_link(link.as_ref())) {
                if singular_reference_link(property).is_none() {
                    return Ok(None);
                }
                Some(PathRef::Native(retained_string(
                    ctx, &property.id, "fcstd revolution axis reference",
                )?))
            } else {
                None
            }
        }
        _ => return Ok(None),
    };
    let face_maker = if kind == "Part::Revolution"
        && property(properties, "FaceMakerClass").is_some()
    {
        let Some(face_maker_class) = face_maker_class else { return Ok(None); };
        let Some(face_maker) = FaceMaker::new(face_maker_class) else { return Ok(None); };
        Some(face_maker)
    } else {
        None
    };
    let fuse_order = if kind.starts_with("PartDesign::")
        && property(properties, "FuseOrder").is_some()
    {
        let Some(value) = integer_property(properties, "FuseOrder") else { return Ok(None); };
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
        let Some(solid) = bool_selector(properties, "Solid", false) else { return Ok(None); };
        solid
    } else {
        true
    });
    let allow_multi_profile_faces = if kind.starts_with("PartDesign::") {
        let Some(allow) = bool_selector(properties, "AllowMultiFace", false) else {
            return Ok(None);
        };
        Some(allow)
    } else {
        None
    };
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Revolve {
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
    })))
}

fn vector_property(
    properties: &[&PropertyRecord],
    name: &str,
) -> Option<cadmpeg_ir::features::FiniteVector3> {
    let property = property(properties, name)?;
    if !is_vector_property_type(&property.type_name) {
        return None;
    }
    direct_root(property, "PropertyVector", |root| {
        let component = |name: &str| root.attribute(name)?.parse::<f64>().ok().and_then(FiniteReal::new);
        Some(cadmpeg_ir::features::FiniteVector3::from_components(
            component("valueX")?,
            component("valueY")?,
            component("valueZ")?,
        ))
    })?
}

fn vector_list_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    entries: &[EntryRecord],
) -> Result<Option<Vec<cadmpeg_ir::features::FinitePoint3>>, CodecError> {
    let Some(property) = property(properties, name) else { return Ok(None) };
    if property.type_name != "App::PropertyVectorList" {
        return Ok(None);
    }
    direct_root(property, "VectorList", |root| {
        let Some(file) = root.attribute("file") else { return Ok(None) };
        if file.is_empty() {
            return Ok(property.side_entries().is_empty().then(Vec::new));
        }
        if property.side_entries() != [file] {
            return Ok(None);
        }
        let Some(data) = entries.iter().find(|entry| entry.name == file).map(|entry| entry.data.as_slice()) else {
            return Ok(None);
        };
        let mut view = View::over_retained(data);
        let Some(count) = view.u32_le().map(|count| count as usize) else { return Ok(None) };
        if count > MAX_SKETCH_RECORDS || view.counted(count as u64, 24).is_none() {
            return Ok(None);
        }
        let mut points = collection_vec(ctx, count, "fcstd vector-list points")?;
        for _ in 0..count {
            let Some(point) = (|| {
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                    view.f64_le()?, view.f64_le()?, view.f64_le()?,
                ))
            })() else { return Ok(None) };
            points.push(point);
        }
        Ok(view.is_empty().then_some(points))
    }).unwrap_or(Ok(None))
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
        match property(properties, "FaceMakerClass") {
            Some(property) => string_property_value(ctx, property)?,
            None => None,
        }
    } else {
        None
    };
    if kind == "Part::Face" {
        let Some(sources) = property(properties, "Sources") else { return Ok(None); };
        if sources.links().is_empty() {
            return Ok(None);
        }
        let Some(face_maker_class) = face_maker_class else { return Ok(None); };
        let Some(face_maker) = FaceMaker::new(face_maker_class) else { return Ok(None); };
        return Ok(Some(FeatureDefinition::Operation(FeatureOperation::FaceFromShapes {
            sources: BodySelection::Native(retained_string(
                ctx, &sources.id, "fcstd face source selection",
            )?),
            face_maker,
        })));
    }
    Ok((|| {
    let point = |x: &str, y: &str, z: &str| {
        Some(cadmpeg_ir::features::FinitePoint3::from_coordinates(
            scalar_named(properties, x)?,
            scalar_named(properties, y)?,
            scalar_named(properties, z)?,
        ))
    };
    let angle = |name: &str| {
        scalar_named(properties, name)
            .and_then(|value| cadmpeg_ir::scalar::Angle::new(value.get().to_radians()))
    };
    match kind {
        "Part::Vertex" => Some(FeatureDefinition::Operation(
            FeatureOperation::PointGeometry {
                position: point("X", "Y", "Z")?,
            },
        )),
        "Part::Line" => Some(FeatureDefinition::Operation(
            FeatureOperation::LineSegment {
                segment: cadmpeg_ir::features::FeatureLineSegment::from_parts(
                    point("X1", "Y1", "Z1")?,
                    point("X2", "Y2", "Z2")?,
                )?,
            },
        )),
        "Part::Circle" => {
            let legacy_angles = property(properties, "Angle0").is_some();
            Some(FeatureDefinition::Operation(
                FeatureOperation::CircularArc {
                    arc: cadmpeg_ir::features::FeatureCircularArc::from_parts(
                        cadmpeg_ir::features::FinitePoint3::ZERO,
                        cadmpeg_ir::features::FeatureDirection3::Z_AXIS,
                        cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
                            properties, "Radius",
                        )?)?,
                        cadmpeg_ir::geometry::DirectedParameterRange::from_angle_endpoints([
                            angle(if legacy_angles { "Angle0" } else { "Angle1" })?,
                            angle(if legacy_angles { "Angle1" } else { "Angle2" })?,
                        ])
                        .ok()?,
                    ),
                },
            ))
        }
        "Part::Ellipse" => Some(FeatureDefinition::Operation(
            FeatureOperation::EllipticArc {
                arc: cadmpeg_ir::features::FeatureEllipticArc::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    [
                        cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
                            properties,
                            "MajorRadius",
                        )?)?,
                        cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
                            properties,
                            "MinorRadius",
                        )?)?,
                    ],
                    cadmpeg_ir::geometry::DirectedParameterRange::from_angle_endpoints([
                        angle("Angle1")?,
                        angle("Angle2")?,
                    ])
                    .ok()?,
                )?,
            },
        )),
        "Part::Polygon" => {
            let points = polygon_points?;
            let closed = bool_property(properties, "Close").unwrap_or(false);
            Some(FeatureDefinition::Operation(FeatureOperation::Polyline {
                chain: cadmpeg_ir::features::FeaturePolyline::from_parts(points, closed)?,
            }))
        }
        "Part::RegularPolygon" => Some(FeatureDefinition::Operation(
            FeatureOperation::RegularPolygonCurve {
                sides: cadmpeg_ir::features::PolygonSideCount::new(
                    u32::try_from(integer_property(properties, "Polygon")?).ok()?,
                )?,
                circumradius: cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
                    scalar_named(properties, "Circumradius")?,
                )?,
            },
        )),
        "Part::Plane" => Some(FeatureDefinition::Operation(
            FeatureOperation::PlanarPatch {
                length: cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
                    properties, "Length",
                )?)?,
                width: cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
                    properties, "Width",
                )?)?,
            },
        )),
        _ => None,
    }
    })())
}

fn parametric_helix_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let radius = cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
        properties, "Radius",
    )?)?;
    let segment_default = if kind == "Part::Spiral" {
        DEFAULT_PART_SPIRAL_SEGMENT_TURNS
    } else {
        0.0
    };
    let segment_value = finite_float_selector(
        properties,
        "SegmentLength",
        "App::PropertyQuantityConstraint",
        FiniteReal::new(segment_default)?,
    )?;
    if segment_value.get() < 0.0 {
        return None;
    }
    let segment_turns = (segment_value.get() > 0.0)
        .then(|| cadmpeg_ir::scalar::PositiveReal::try_from(segment_value))
        .transpose()
        .ok()?;
    let (shape, revolutions, clockwise, construction_style) = if kind == "Part::Helix" {
        let pitch = cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
            properties, "Pitch",
        )?)?;
        let height = cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(
            properties, "Height",
        )?)?;
        let angle = scalar_named(properties, "Angle").map_or(0.0, FiniteReal::get);
        if angle.abs() >= 90.0 {
            return None;
        }
        let clockwise = match enumeration_selector(properties, "LocalCoord", 0)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let construction_style = match enumeration_selector(properties, "Style", 0)? {
            0 => Some(HelixConstructionStyle::Legacy),
            1 => Some(HelixConstructionStyle::Corrected),
            _ => return None,
        };
        let shape = if angle == 0.0 {
            cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: pitch.into(),
            }
        } else {
            cadmpeg_ir::features::HelixShape::Conical {
                pitch: pitch.into(),
                cone_angle: cadmpeg_ir::scalar::SlopeAngle::new(angle.to_radians())?,
            }
        };
        (
            shape,
            cadmpeg_ir::scalar::PositiveReal::new(height.get() / pitch.get())?,
            clockwise,
            construction_style,
        )
    } else {
        let growth = cadmpeg_ir::scalar::NonNegativeLength::from_finite_assigned_real(
            scalar_named(properties, "Growth")?,
        )?;
        let revolutions =
            cadmpeg_ir::scalar::PositiveReal::from_finite(scalar_named(properties, "Rotations")?)?;
        (
            cadmpeg_ir::features::HelixShape::Spiral {
                radial_growth: growth.into(),
            },
            revolutions,
            false,
            None,
        )
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Helix {
        axis_origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        axis_direction: cadmpeg_ir::features::FeatureDirection3::Z_AXIS,
        radius,
        shape,
        revolutions,
        start_angle: cadmpeg_ir::scalar::Angle::ZERO,
        clockwise,
        segment_turns,
        construction_style,
    }))
}

/// The draft angles an extrude states, one per native taper property.
///
/// `taper` and `taper_reverse` are read for every extrude. `taper_second` is
/// read only when the extent is two-sided, because `TaperAngle2` belongs to the
/// second side and a one-sided or midplane extent states no second side; on
/// those the field is `None` without the property being parsed.
struct ExtrudeDrafts {
    /// `TaperAngle`: the draft of the first (or only, or symmetric) side.
    taper: Option<cadmpeg_ir::scalar::SlopeAngle>,
    /// `TaperAngleRev`: the draft of the reverse side of a `Part::Extrusion`.
    taper_reverse: Option<cadmpeg_ir::scalar::SlopeAngle>,
    /// `TaperAngle2`: the draft of the second side of a two-sided extent.
    taper_second: Option<cadmpeg_ir::scalar::SlopeAngle>,
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
    let Some(degrees) = scalar_named(properties, key).filter(|angle| angle.get() != 0.0) else {
        return Ok(None);
    };
    cadmpeg_ir::scalar::SlopeAngle::try_from(degrees.get().to_radians())
        .map(Some)
        .map_err(|error| malformed_design(ctx, format_args!("{key}: {error}")))
}

/// Whether the record states the legacy two-length extent.
fn legacy_two_length_extent(properties: &[&PropertyRecord]) -> bool {
    property(properties, "SideType").is_none()
        && enumeration_selector(properties, "Type", 0) == Some(4)
}

/// The extent kind the extrude record states: 0 one-sided, 1 two-sided,
/// 2 midplane.
fn extrude_side_type(properties: &[&PropertyRecord]) -> Option<u64> {
    if legacy_two_length_extent(properties) {
        Some(1)
    } else if bool_selector(properties, "Midplane", false)? {
        Some(2)
    } else if property(properties, "SideType").is_some() {
        enumeration_selector(properties, "SideType", 0)
    } else {
        Some(0)
    }
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
        match property(properties, "FaceMakerClass") {
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
    let taper_second = if extrude_side_type(properties) == Some(1) {
        taper_angle(ctx, properties, "TaperAngle2")?
    } else {
        None
    };
    let drafts = ExtrudeDrafts {
        taper: taper_angle(ctx, properties, "TaperAngle")?,
        taper_reverse: taper_angle(ctx, properties, "TaperAngleRev")?,
        taper_second,
    };
    Ok(extrusion_shape(
        kind,
        properties,
        profile,
        profile_normal,
        sketches,
        &drafts,
        face_maker_class,
    ))
}

fn extrusion_shape(
    kind: &str,
    properties: &[&PropertyRecord],
    profile: ProfileRef,
    profile_normal: Option<Vector3>,
    sketches: &[Sketch],
    drafts: &ExtrudeDrafts,
    face_maker_class: Option<String>,
) -> Option<FeatureDefinition> {
    if kind == "Part::Extrusion" {
        let raw_direction = vector_property(properties, "Dir");
        let direction_magnitude = raw_direction.map(|direction| direction.get().norm());
        let direction_mode = enumeration_selector(properties, "DirMode", 0)?;
        let (mut direction, direction_source) = match direction_mode {
            0 => (
                cadmpeg_ir::units::UnitVector3::normalized(raw_direction?.get())?,
                ExtrusionDirectionSource::Custom {},
            ),
            1 => {
                let reference = property(properties, "DirLink")?;
                if reference.links().len() != 1 {
                    return None;
                }
                (
                    cadmpeg_ir::units::UnitVector3::normalized(raw_direction?.get())?,
                    ExtrusionDirectionSource::Edge {
                        reference: PathRef::Native(reference.id.clone()),
                    },
                )
            }
            2 => {
                let normal = match &profile {
                    ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) => sketches
                        .iter()
                        .find(|sketch| sketch.id == *sketch_id)
                        .and_then(Sketch::resolved_placement)
                        .map(|(_, normal, _)| normal.get())
                        .or(profile_normal),
                    _ => profile_normal,
                }?;
                (
                    cadmpeg_ir::units::UnitVector3::normalized(normal)?,
                    ExtrusionDirectionSource::ProfileNormal {},
                )
            }
            _ => return None,
        };
        let signed_length = |name| match scalar_named(properties, name) {
            Some(value) => Some(value),
            None => Some(FiniteReal::ZERO),
        };
        let mut forward = signed_length("LengthFwd")?.get();
        let reverse = signed_length("LengthRev")?.get();
        if forward == 0.0 && reverse == 0.0 {
            forward = direction_magnitude.filter(|value| value.is_finite() && *value > 0.0)?;
        }
        let symmetric = bool_selector(properties, "Symmetric", false)?;
        let (extent, reverse_direction) = if symmetric {
            // A symmetric extent mirrors one side across the profile plane, so
            // its single side carries the taper once (from `TaperAngle`).
            (
                ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(
                                (forward != 0.0).then_some(forward.abs())?,
                            )?,
                        },
                        draft: drafts.taper,
                    },
                },
                false,
            )
        } else {
            let forward_travel = (forward != 0.0).then_some((forward, drafts.taper));
            let reverse_travel = (reverse != 0.0).then_some((-reverse, drafts.taper_reverse));
            let same_side = forward_travel
                .zip(reverse_travel)
                .is_some_and(|((first, _), (second, _))| first.signum() == second.signum());
            if same_side && drafts.taper != drafts.taper_reverse {
                return None;
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
                                length: cadmpeg_ir::scalar::NonZeroLength::new(length)?,
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
                                length: cadmpeg_ir::scalar::NonZeroLength::new(-length)?,
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
                                length: cadmpeg_ir::scalar::NonZeroLength::new(first)?,
                            },
                            draft: first_draft,
                        },
                        second: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: cadmpeg_ir::scalar::NonZeroLength::new(-second)?,
                            },
                            draft: second_draft,
                        },
                    },
                    false,
                ),
                (None, None) => return None,
            }
        };
        if reverse_direction ^ bool_selector(properties, "Reversed", false)? {
            direction = direction.reversed();
        }
        let face_maker = if property(properties, "FaceMakerClass").is_some() {
            let maker = FaceMaker::new(face_maker_class?)?;
            if property(properties, "FaceMakerMode").is_some()
                && u32::try_from(integer_property(properties, "FaceMakerMode")?).ok()?
                    != maker.mode()
            {
                return None;
            }
            Some(maker)
        } else {
            None
        };
        let inner_wire_taper = if property(properties, "InnerWireTaper").is_some() {
            Some(match integer_property(properties, "InnerWireTaper")? {
                0 => InnerWireTaper::Inverted,
                1 => InnerWireTaper::SameAsOuter,
                _ => return None,
            })
        } else {
            None
        };
        return Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile,
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::FeatureDirection3::from(direction),
                source: Some(direction_source),
            },
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            extent,
            op: BooleanOp::NewBody,
            solid: Some(bool_selector(properties, "Solid", false)?),
            face_maker,
            inner_wire_taper,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }));
    }
    let legacy_two_lengths = legacy_two_length_extent(properties);
    let termination = |side: u8| {
        let suffix = if side == 1 { "" } else { "2" };
        let type_name = format!("Type{suffix}");
        let length_name = format!("Length{suffix}");
        let offset_name = format!("Offset{suffix}");
        let face_name = format!("UpToFace{suffix}");
        let shape_name = format!("UpToShape{suffix}");
        let termination_type = if legacy_two_lengths {
            0
        } else {
            enumeration_selector(properties, &type_name, 0)?
        };
        let offset = if property(properties, &offset_name).is_some() {
            Some(Length::from_assigned_real(scalar_named(
                properties,
                &offset_name,
            )?))
        } else {
            None
        };
        match termination_type {
            0 => Some(LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::from_assigned_real(scalar_named(
                    properties,
                    &length_name,
                )?)?,
            }),
            1 if kind.contains("Pocket") => Some(LinearTermination::ThroughAll {}),
            1 => Some(LinearTermination::ToLast {}),
            2 => Some(LinearTermination::ToFirst {}),
            3 => Some(LinearTermination::ToFace {
                face: cadmpeg_ir::features::FaceSelection::Native(
                    singular_operand(properties, &face_name)?.id.clone(),
                ),
                offset,
            }),
            5 => Some(LinearTermination::ToShape {
                target: cadmpeg_ir::features::FaceSelection::Native(
                    singular_operand(properties, &shape_name)?.id.clone(),
                ),
            }),
            _ => None,
        }
    };
    let side_type = extrude_side_type(properties)?;
    // `TaperAngle2` describes a second, independent side and reaches the IR
    // only when the extent actually carries one (`SideType` 1 / two-sided). A
    // symmetric (Midplane) pad mirrors side one, so it has no second side to
    // receive it; the native property remains retained but maps nowhere.
    let extent = match side_type {
        0 => ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: termination(1)?,
                draft: drafts.taper,
            },
        },
        1 => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: termination(1)?,
                draft: drafts.taper,
            },
            second: ExtrudeSide {
                termination: termination(2)?,
                draft: drafts.taper_second,
            },
        },
        2 => ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: termination(1)?,
                draft: drafts.taper,
            },
        },
        _ => return None,
    };
    let use_custom = bool_selector(properties, "UseCustomVector", false)?;
    let is_nonempty_link = |link: Option<&crate::native::LinkTarget>| {
        link.is_some_and(|link| link.document().is_some() || link.object().is_some())
    };
    let reference_axis = property(properties, "ReferenceAxis").filter(|property| {
        property
            .links()
            .iter()
            .any(|link| is_nonempty_link(link.as_ref()))
    });
    if reference_axis.is_some_and(|property| {
        property
            .links()
            .iter()
            .filter(|link| is_nonempty_link(link.as_ref()))
            .count()
            != 1
    }) {
        return None;
    }
    let mut direction = if use_custom {
        cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: cadmpeg_ir::features::FeatureDirection3::from(
                cadmpeg_ir::units::UnitVector3::normalized(
                    vector_property(properties, "Direction")?.get(),
                )?,
            ),
            source: Some(ExtrusionDirectionSource::Custom {}),
        }
    } else if let Some(reference_axis) = reference_axis {
        cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: cadmpeg_ir::features::FeatureDirection3::from(
                cadmpeg_ir::units::UnitVector3::normalized(
                    vector_property(properties, "Direction")?.get(),
                )?,
            ),
            source: Some(ExtrusionDirectionSource::Edge {
                reference: PathRef::Native(reference_axis.id.clone()),
            }),
        }
    } else {
        let normal = match &profile {
            ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) => sketches
                .iter()
                .find(|sketch| sketch.id == *sketch_id)
                .and_then(Sketch::resolved_placement)
                .map(|(_, normal, _)| normal.get())
                .or(profile_normal),
            ProfileRef::Planar(PlanarProfileRef::Native(_)) => profile_normal,
            _ => return None,
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
    if bool_selector(properties, "Reversed", false)? {
        let cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } = &mut direction else {
            return None;
        };
        *vector = vector.reversed();
    }
    let length_along_profile_normal = Some(bool_selector(properties, "AlongSketchNormal", true)?);
    let allow_multi_profile_faces = Some(bool_selector(properties, "AllowMultiFace", false)?);
    Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
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
    }))
}

fn dress_up_edge_selection(kind: &str, properties: &[&PropertyRecord]) -> Option<EdgeSelection> {
    let use_all_edges = if matches!(kind, "PartDesign::Fillet" | "PartDesign::Chamfer") {
        bool_selector(properties, "UseAllEdges", false)?
    } else {
        false
    };
    Some(if use_all_edges {
        EdgeSelection::All
    } else {
        property(properties, "Base").map_or(EdgeSelection::Unresolved, |property| {
            EdgeSelection::Native(property.id.clone())
        })
    })
}

fn scale_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let base = singular_operand(properties, "Base")?;
    let factor = |name| {
        scalar_named(properties, name).and_then(cadmpeg_ir::scalar::NonZeroReal::from_finite)
    };
    let factors = if bool_selector(properties, "Uniform", true)? {
        ScaleFactors::Uniform {
            factor: factor("UniformScale")?,
        }
    } else {
        ScaleFactors::PerAxis {
            factors: [factor("XScale")?, factor("YScale")?, factor("ZScale")?],
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Scale {
        bodies: BodySelection::Native(base.id.clone()),
        center: Some(ScaleCenter::ModelOrigin),
        factors,
    }))
}

fn fillet_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<FeatureDefinition> {
    let edges = dress_up_edge_selection(kind, properties)?;
    if matches!(edges, EdgeSelection::Unresolved) {
        return None;
    }
    let radius = if kind == "Part::Fillet" {
        let values = part_fillet_edge_values(properties, entries)?;
        let radius = cadmpeg_ir::scalar::PositiveLength::new(values.first()?.1)?;
        values
            .iter()
            .all(|(_, first, second)| *first == radius.get() && *second == radius.get())
            .then_some(())?;
        radius
    } else {
        cadmpeg_ir::scalar::PositiveLength::from_assigned_real(scalar_named(properties, "Radius")?)?
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::FilletGroup {
                edges,
                radius: RadiusSpec::Constant { radius },
                tangency_weight: None,
            },
        ),
    }))
}

fn chamfer_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
    program_version: Option<&str>,
) -> Option<FeatureDefinition> {
    let edges = dress_up_edge_selection(kind, properties)?;
    if matches!(edges, EdgeSelection::Unresolved) {
        return None;
    }
    let spec = if kind == "Part::Chamfer" {
        let values = part_fillet_edge_values(properties, entries)?;
        let (_, first_raw, second_raw) = *values.first()?;
        let first = cadmpeg_ir::scalar::PositiveLength::new(first_raw)?;
        let second = cadmpeg_ir::scalar::PositiveLength::new(second_raw)?;
        if !values.iter().all(|(_, candidate_first, candidate_second)| {
            *candidate_first == first.get() && *candidate_second == second.get()
        }) {
            return None;
        }
        if first == second {
            ChamferSpec::Distance { distance: first }
        } else {
            ChamferSpec::TwoDistances { first, second }
        }
    } else {
        chamfer_spec(properties)?
    };
    let flip_direction = if kind == "PartDesign::Chamfer" {
        bool_selector(properties, "FlipDirection", false)?
    } else {
        false
    };
    let legacy_flip = kind == "PartDesign::Chamfer"
        && program_version.is_some_and(|version| version.starts_with('0'))
        && property(properties, "ChamferType")
            .and_then(scalar_value)
            .is_some_and(|value| value.get() == 1.0 || value.get() == 2.0);
    Some(FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::ChamferGroup { edges, spec },
        ),
        flip_direction: if legacy_flip {
            !flip_direction
        } else {
            flip_direction
        },
    }))
}

fn part_fillet_edge_values(
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<Vec<(u32, f64, f64)>> {
    let property = property(properties, "Edges")?;
    let entry_name = property.side_entries().first()?;
    let data = &entries.iter().find(|entry| entry.name == *entry_name)?.data;
    let mut view = View::over_retained(data);
    let count = view.u32_le()?;
    if count as usize > MAX_SKETCH_RECORDS {
        return None;
    }
    let values = view.read_counted(u64::from(count), 20, |view| {
        Some((view.u32_le()?, view.f64_le()?, view.f64_le()?))
    })?;
    view.is_empty().then_some(values)
}

fn shell_mode(kind: &str, properties: &[&PropertyRecord]) -> Option<ShellMode> {
    let absent_default = u64::from(kind == "Part::Offset2D");
    match enumeration_selector(properties, "Mode", absent_default)? {
        0 => Some(ShellMode::Skin),
        1 => Some(ShellMode::Pipe),
        2 if kind != "Part::Offset2D" => Some(ShellMode::BothSides),
        _ => None,
    }
}

fn shell_join(kind: &str, properties: &[&PropertyRecord]) -> Option<ShellJoin> {
    match enumeration_selector(properties, "Join", 0)? {
        0 => Some(ShellJoin::Arc),
        1 if kind == "PartDesign::Thickness" => Some(ShellJoin::Intersection),
        1 => Some(ShellJoin::Tangent),
        2 => Some(ShellJoin::Intersection),
        _ => None,
    }
}

fn thickness_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let thickness = scalar_named(properties, "Value")?;
    if thickness.get() == 0.0 {
        return None;
    }
    let source_name = if kind == "Part::Thickness" {
        "Faces"
    } else {
        "Base"
    };
    let selection = property(properties, source_name)?;
    if selection.links().is_empty() {
        return None;
    }
    Some(FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: cadmpeg_ir::features::FaceSelection::Native(selection.id.clone()),
        thickness: Some(cadmpeg_ir::scalar::PositiveLength::from_assigned_real(
            thickness.abs(),
        )?),
        outward: Some(if kind == "Part::Thickness" {
            thickness.get() > 0.0
        } else {
            !bool_property(properties, "Reversed").unwrap_or(false)
        }),
        mode: Some(shell_mode(kind, properties)?),
        join: Some(shell_join(kind, properties)?),
        resolve_intersections: Some(bool_property(properties, "Intersection").unwrap_or(false)),
        allow_self_intersections: Some(
            bool_property(properties, "SelfIntersection").unwrap_or(false),
        ),
    }))
}

fn offset_shape_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let source = singular_operand(properties, "Source")?;
    let distance =
        cadmpeg_ir::scalar::NonZeroLength::from_assigned_real(scalar_named(properties, "Value")?)?;
    let mode = shell_mode(kind, properties)?;
    if kind == "Part::Offset2D" && mode == ShellMode::BothSides {
        return None;
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::OffsetShape {
            source: BodySelection::Native(source.id.clone()),
            distance,
            mode,
            join: shell_join(kind, properties)?,
            resolve_intersections: bool_property(properties, "Intersection").unwrap_or(false),
            allow_self_intersections: bool_property(properties, "SelfIntersection")
                .unwrap_or(false),
            fill: bool_property(properties, "Fill").unwrap_or(false),
            planar: kind == "Part::Offset2D",
        },
    ))
}

fn derived_shape_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    match kind {
        "Part::Compound" | "Part::Compound2" => {
            let Some(links) = property(properties, "Links") else {
                return property(properties, "Shape")
                    .map(|_| FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}));
            };
            if links.links().is_empty() {
                return None;
            }
            Some(FeatureDefinition::Operation(FeatureOperation::Compound {
                members: BodySelection::Native(links.id.clone()),
            }))
        }
        "Part::Refine" | "Part::Reverse" => {
            let source = property(properties, "Source")?;
            if source.links().len() != 1 {
                return None;
            }
            let source = BodySelection::Native(source.id.clone());
            Some(if kind == "Part::Refine" {
                FeatureDefinition::Operation(FeatureOperation::RefineShape { source })
            } else {
                FeatureDefinition::Operation(FeatureOperation::ReverseShape { source })
            })
        }
        _ => None,
    }
}

fn cached_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    property(properties, "Shape")
        .filter(|shape| !shape.side_entries().is_empty())
        .map(|_| FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}))
}

fn ruled_surface_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let curve = |name| {
        let property = property(properties, name)?;
        (property.links().len() == 1).then(|| PathRef::Native(property.id.clone()))
    };
    let orientation = match integer_property(properties, "Orientation").unwrap_or(0) {
        0 => RuledCurveOrientation::Automatic,
        1 => RuledCurveOrientation::Forward,
        2 => RuledCurveOrientation::Reversed,
        _ => return None,
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::RuledBetweenCurves {
            first: curve("Curve1")?,
            second: curve("Curve2")?,
            orientation,
        },
    ))
}

fn section_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let operand = |name| {
        let property = property(properties, name)?;
        (property.links().len() == 1).then(|| BodySelection::Native(property.id.clone()))
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::SectionShape {
            operands: cadmpeg_ir::features::SectionOperands::new(
                operand("Base")?,
                operand("Tool")?,
            )
            .ok()?,

            approximate: Some(bool_property(properties, "Approximation").unwrap_or(false)),
        },
    ))
}

fn mirror_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let source = property(properties, "Source")?;
    if source.links().len() != 1 {
        return None;
    }
    let origin = vector_property(properties, "Base")?;
    let plane_reference = property(properties, "MirrorPlane")
        .filter(|property| {
            property
                .links()
                .iter()
                .any(|link| nonempty_link(link.as_ref()))
        })
        .map(|property| cadmpeg_ir::features::FaceSelection::Native(property.id.clone()));
    Some(FeatureDefinition::Operation(
        FeatureOperation::MirrorShape {
            source: BodySelection::Native(source.id.clone()),
            plane_origin: origin.as_point(),
            plane_normal: cadmpeg_ir::units::UnitVector3::normalized(
                vector_property(properties, "Normal")?.get(),
            )?,
            plane_reference,
        },
    ))
}

fn project_on_surface_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let sources = property(properties, "Projection")?;
    if sources.links().is_empty() {
        return None;
    }
    let support = property(properties, "SupportFace")?;
    if support.links().len() != 1 {
        return None;
    }
    let mode = match enumeration_selector(properties, "Mode", 0)? {
        0 => SurfaceProjectionMode::All,
        1 => SurfaceProjectionMode::Faces,
        2 => SurfaceProjectionMode::Edges,
        _ => return None,
    };
    let height = if property(properties, "Height").is_some() {
        cadmpeg_ir::scalar::NonNegativeLength::from_finite_assigned_real(scalar_named(
            properties, "Height",
        )?)?
    } else {
        cadmpeg_ir::scalar::NonNegativeLength::ZERO
    };
    let offset = if property(properties, "Offset").is_some() {
        Length::from_assigned_real(scalar_named(properties, "Offset")?)
    } else {
        Length::ZERO
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::ProjectOnSurface {
            sources: PathRef::Native(sources.id.clone()),
            support_face: cadmpeg_ir::features::FaceSelection::Native(support.id.clone()),
            direction: cadmpeg_ir::units::UnitVector3::normalized(
                vector_property(properties, "Direction")?.get(),
            )?,
            mode,
            height,
            offset,
        },
    ))
}

fn draft_definition(
    properties: &[&PropertyRecord],
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<FeatureDefinition> {
    let faces = property(properties, "Base")?;
    let neutral_plane = property(properties, "NeutralPlane")?;
    let plane_normal = plane_reference(properties, "NeutralPlane", objects, properties_by_owner)
        .map(|(_, normal)| normal);
    let pull_direction = if property(properties, "PullDirection").is_some_and(|property| {
        property
            .links()
            .iter()
            .any(|link| nonempty_link(link.as_ref()))
    }) {
        axis_reference(properties, "PullDirection", objects, properties_by_owner)
            .map(|(_, direction)| direction)
    } else {
        plane_normal
    };
    let reversed = bool_property(properties, "Reversed").unwrap_or(false);
    let angle = scalar_named(properties, "Angle")?;
    Some(FeatureDefinition::Operation(FeatureOperation::Draft {
        faces: cadmpeg_ir::features::FaceSelection::Native(faces.id.clone()),
        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
            plane: cadmpeg_ir::features::FaceSelection::Native(neutral_plane.id.clone()),
            pull: pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
                direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
                plane: None,
            }),
        },
        angle: Some(cadmpeg_ir::scalar::SlopeAngle::new(
            if reversed { -angle.get() } else { angle.get() }.to_radians(),
        )?),
        outward: Some(reversed),
    }))
}

fn chamfer_spec(properties: &[&PropertyRecord]) -> Option<ChamferSpec> {
    let mode = property(properties, "ChamferType").map_or(Some(0), |property| {
        scalar_value(property).and_then(|value| match value.get() {
            value if value > -1.0 && value < 1.0 => Some(0),
            value if (1.0..2.0).contains(&value) => Some(1),
            value if (2.0..3.0).contains(&value) => Some(2),
            _ => None,
        })
    })?;
    let first = property(properties, "Size")
        .and_then(scalar_value)
        .and_then(cadmpeg_ir::scalar::PositiveLength::from_assigned_real);
    match (mode, first) {
        (0, Some(distance)) => Some(ChamferSpec::Distance { distance }),
        (1, Some(first)) => property(properties, "Size2")
            .and_then(scalar_value)
            .and_then(cadmpeg_ir::scalar::PositiveLength::from_assigned_real)
            .map(|second| ChamferSpec::TwoDistances { first, second }),
        (2, Some(distance)) => property(properties, "Angle")
            .and_then(scalar_value)
            .filter(|angle| angle.get() > 0.0 && angle.get() < 180.0)
            .and_then(|angle| {
                Some(ChamferSpec::DistanceAngle {
                    distance,
                    angle: cadmpeg_ir::scalar::InteriorAngle::new(angle.get().to_radians())?,
                })
            }),
        _ => None,
    }
}

fn property<'a>(properties: &'a [&PropertyRecord], name: &str) -> Option<&'a PropertyRecord> {
    properties
        .iter()
        .copied()
        .find(|property| property.name == name)
}

fn nonempty_link(link: Option<&crate::native::LinkTarget>) -> bool {
    link.is_some_and(|link| {
        link.document().is_some() || link.object().is_some_and(|object| !object.is_empty())
    })
}

fn singular_operand<'a>(
    properties: &'a [&PropertyRecord],
    name: &str,
) -> Option<&'a PropertyRecord> {
    let property = property(properties, name)?;
    let [Some(link)] = property.links() else {
        return None;
    };
    link.object().map(|_| property)
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

fn scalar_value(property: &PropertyRecord) -> Option<FiniteReal> {
    let tag = scalar_value_tag(&property.type_name)?;
    if tag == "Bool" {
        return None;
    }
    let value = direct_root_value(property, tag, "value", str::parse::<f64>)?.ok()?;
    FiniteReal::new(value)
}

fn scalar_text<T>(property: &PropertyRecord, use_value: impl FnOnce(&str) -> T) -> Option<T> {
    let tag = text_value_tag(&property.type_name)?;
    direct_root_value(property, tag, "value", use_value)
}

fn direct_root_value<T>(
    property: &PropertyRecord,
    expected_tag: &str,
    attribute: &str,
    use_value: impl FnOnce(&str) -> T,
) -> Option<T> {
    direct_root(property, expected_tag, |root| {
        root.attribute(attribute).map(use_value)
    })?
}

fn direct_root<T>(
    property: &PropertyRecord,
    expected_tag: &str,
    use_root: impl FnOnce(roxmltree::Node<'_, '_>) -> T,
) -> Option<T> {
    let document = roxmltree::Document::parse(property.xml.text()).ok()?;
    let mut roots = document.root_element().children()
        .filter(|node| node.is_element() && node.has_tag_name(expected_tag));
    let root = roots.next()?;
    if roots.next().is_some()
        || document.descendants().filter(|node| node.has_tag_name(expected_tag)).count() != 1 {
        return None;
    }
    Some(use_root(root))
}

fn native_parameters(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let mut parameters = BTreeMap::new();
    for property in properties {
        let Some(value) = scalar_text(property, |text| {
            retained_string(ctx, text, "fcstd native parameter value")
        }) else { continue };
        let Some(name) = NonBlankString::new(retained_string(
            ctx, &property.name, "fcstd native parameter name",
        )?) else { continue };
        let value = value?;
        if !parameters.contains_key(&name) {
            ctx.charge_collection_items(1, "fcstd native parameters")?;
        }
        parameters.insert(name, value);
    }
    Ok(parameters)
}

fn native_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: retained_string(ctx, kind, "fcstd native feature kind")?.into(),
        parameters: native_parameters(ctx, properties)?,
    }))
}

fn primitive_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let length = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .map(Length::from_assigned_real)
    };
    let angle = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .and_then(|value| cadmpeg_ir::scalar::Angle::new(value.get().to_radians()))
    };
    let solid = if kind.ends_with("Box") {
        PrimitiveSolidKind::Box {
            length: length("Length")?,
            width: length("Width")?,
            height: length("Height")?,
        }
    } else if kind.ends_with("Cylinder") {
        PrimitiveSolidKind::Cylinder {
            radius: length("Radius")?,
            height: length("Height")?,
            angle: angle("Angle")?,
        }
    } else if kind.ends_with("Cone") {
        let radius1 = length("Radius1")?;
        let radius2 = length("Radius2")?;
        PrimitiveSolidKind::Cone {
            radius1,
            radius2,
            height: length("Height")?,
            angle: angle("Angle")?,
        }
    } else if kind.ends_with("Sphere") {
        PrimitiveSolidKind::Sphere {
            radius: length("Radius")?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Ellipsoid") {
        let x_radius = length("Radius2")?;
        let y_radius = length("Radius3")?;
        PrimitiveSolidKind::Ellipsoid {
            x_radius,
            y_radius: if y_radius.get() == 0.0 {
                x_radius
            } else {
                y_radius
            },
            z_radius: length("Radius1")?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Torus") {
        PrimitiveSolidKind::Torus {
            major_radius: length("Radius1")?,
            minor_radius: length("Radius2")?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Prism") {
        PrimitiveSolidKind::Prism {
            sides: u32::try_from(integer_property(properties, "Polygon")?).ok()?,
            circumradius: length("Circumradius")?,
            height: length("Height")?,
        }
    } else if kind.ends_with("Wedge") {
        PrimitiveSolidKind::Wedge {
            xmin: length("Xmin")?,
            ymin: length("Ymin")?,
            zmin: length("Zmin")?,
            x2min: length("X2min")?,
            z2min: length("Z2min")?,
            xmax: length("Xmax")?,
            ymax: length("Ymax")?,
            zmax: length("Zmax")?,
            x2max: length("X2max")?,
            z2max: length("Z2max")?,
        }
    } else {
        return None;
    };
    let op = if kind.contains("Subtractive") {
        BooleanOp::Cut
    } else if kind.contains("Additive") {
        BooleanOp::Join
    } else {
        BooleanOp::NewBody
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Primitive {
        solid: PrimitiveSolid::new(solid).ok()?,
        op,
    }))
}

fn datum_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let (origin, z_axis, x_axis, y_axis) = placement_frame(properties)?;
    Some(match kind {
        "PartDesign::Plane" => FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(origin, z_axis, x_axis)?,
        }),
        "PartDesign::Line" => FeatureDefinition::Operation(FeatureOperation::DatumAxis {
            origin: cadmpeg_ir::features::FinitePoint3::new(origin)?,
            direction: cadmpeg_ir::features::FeatureDirection3::new(z_axis)?,
        }),
        "PartDesign::Point" => FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position: cadmpeg_ir::features::FinitePoint3::new(origin)?,
            construction: None,
        }),
        "PartDesign::CoordinateSystem" => {
            FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem {
                frame: cadmpeg_ir::features::FeatureCoordinateFrame::new(
                    origin, x_axis, y_axis, z_axis,
                )?,
            })
        }
        _ => return None,
    })
}

fn boolean_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let op = if kind == "PartDesign::Boolean" {
        match enumeration_selector(properties, "Type", 0)? {
            0 => cadmpeg_ir::features::BooleanKind::Join,
            1 => cadmpeg_ir::features::BooleanKind::Cut,
            2 => cadmpeg_ir::features::BooleanKind::Intersect,
            _ => return None,
        }
    } else if kind.ends_with("Cut") {
        cadmpeg_ir::features::BooleanKind::Cut
    } else if kind.ends_with("Common") || kind.ends_with("MultiCommon") {
        cadmpeg_ir::features::BooleanKind::Intersect
    } else if kind.ends_with("Fuse") || kind.ends_with("MultiFuse") {
        cadmpeg_ir::features::BooleanKind::Join
    } else {
        return None;
    };
    let (target, tools) = if kind == "PartDesign::Boolean" {
        let group = property(properties, "Group")?;
        if group.links().is_empty() {
            return None;
        }
        if property(properties, "BaseFeature").is_some_and(|property| {
            property
                .links()
                .iter()
                .any(|link| nonempty_link(link.as_ref()))
        }) {
            let base = singular_operand(properties, "BaseFeature")?;
            (
                BodySelection::Native(base.id.clone()),
                BodySelection::Native(group.id.clone()),
            )
        } else {
            let last = group.links().len() - 1;
            (
                BodySelection::Native(format!("{}:link:{last}", group.id)),
                BodySelection::Native(format!("{}:links:0..{last}", group.id)),
            )
        }
    } else if property(properties, "Base").is_some() || property(properties, "Tool").is_some() {
        let base = singular_operand(properties, "Base")?;
        let tool = singular_operand(properties, "Tool")?;
        (
            BodySelection::Native(base.id.clone()),
            BodySelection::Native(tool.id.clone()),
        )
    } else {
        let shapes = property(properties, "Shapes")?;
        if shapes.links().len() < 2 {
            return None;
        }
        (
            BodySelection::Native(format!("{}:link:0", shapes.id)),
            BodySelection::Native(format!("{}:links:1..{}", shapes.id, shapes.links().len())),
        )
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: cadmpeg_ir::features::CombineOperands::new(target, tools).ok()?,

        op,
        keep_tools: false,
    }))
}

fn loft_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profiles = property(properties, "Profile")
        .into_iter()
        .chain(property(properties, "Sections"))
        .flat_map(PropertyRecord::links)
        .filter_map(|link| link.as_ref()?.object())
        .map(|object| {
            sketches.get(object).cloned().map_or_else(
                || ProfileRef::Planar(PlanarProfileRef::Native(object.to_owned())),
                |sketch| ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)),
            )
        })
        .collect::<Vec<_>>();
    if profiles.len() < 2 {
        return None;
    }
    let max_degree = if property(properties, "MaxDegree").is_some() {
        let value = u32::try_from(integer_property(properties, "MaxDegree")?).ok()?;
        Some(std::num::NonZeroU32::new(value)?)
    } else {
        None
    };
    let part_design = kind.starts_with("PartDesign::");
    Some(FeatureDefinition::Operation(FeatureOperation::Loft {
        sections: profiles
            .into_iter()
            .map(cadmpeg_ir::features::LoftSection::Profile)
            .collect(),
        guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
        op: operation_boolean(kind),
        closed: bool_selector(properties, "Closed", false)?,
        solid: if part_design {
            true
        } else {
            bool_selector(properties, "Solid", true)?
        },
        ruled: bool_selector(properties, "Ruled", false)?,
        linearize: if part_design {
            false
        } else {
            bool_selector(properties, "Linearize", false)?
        },
        max_degree,
        allow_multi_profile_faces: if part_design {
            Some(bool_selector(properties, "AllowMultiFace", false)?)
        } else {
            None
        },
    }))
}

fn sweep_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profile_ref = |object: &str| {
        sketches.get(object).cloned().map_or_else(
            || ProfileRef::Planar(PlanarProfileRef::Native(object.to_owned())),
            |sketch| ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)),
        )
    };
    let mut profiles = property(properties, "Profile")
        .into_iter()
        .chain(property(properties, "Sections"))
        .flat_map(PropertyRecord::links)
        .filter_map(|link| link.as_ref()?.object())
        .map(profile_ref)
        .collect::<Vec<_>>();
    profiles.dedup();
    if profiles.is_empty() {
        return None;
    }
    let profile = profiles.remove(0);
    let path_property = property(properties, "Spine")
        .or_else(|| property(properties, "Path"))
        .filter(|property| singular_operand(properties, &property.name).is_some())?;
    let part_design = kind.starts_with("PartDesign::");
    let solid = if part_design {
        true
    } else {
        bool_selector(properties, "Solid", true)?
    };
    let path_tangent = if part_design {
        bool_selector(properties, "SpineTangent", false)?
    } else {
        false
    };
    let auxiliary_spine_tangent = if part_design {
        bool_selector(properties, "AuxiliarySpineTangent", false)?
    } else {
        false
    };
    let auxiliary_curvilinear = if part_design {
        bool_selector(properties, "AuxiliaryCurvilinear", true)?
    } else {
        true
    };
    let transition = match integer_property(properties, "Transition")
        .unwrap_or(u64::from(kind == "Part::Sweep"))
    {
        0 => SweepTransition::Transformed,
        1 => SweepTransition::RightCorner,
        2 => SweepTransition::RoundCorner,
        _ => return None,
    };
    let orientation = if kind == "Part::Sweep" {
        if bool_selector(properties, "Frenet", true)? {
            SweepOrientation::Frenet {}
        } else {
            SweepOrientation::CorrectedFrenet {}
        }
    } else {
        match integer_property(properties, "Mode").unwrap_or(0) {
            0 => SweepOrientation::CorrectedFrenet {},
            1 => SweepOrientation::Fixed {},
            2 => SweepOrientation::Frenet {},
            3 => {
                let auxiliary = property(properties, "AuxiliarySpine")?;
                singular_operand(properties, "AuxiliarySpine")?;
                SweepOrientation::Auxiliary {
                    path: PathRef::Native(auxiliary.id.clone()),
                    tangent: auxiliary_spine_tangent,
                    curvilinear: auxiliary_curvilinear,
                }
            }
            4 => SweepOrientation::Binormal {
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    vector_property(properties, "Binormal")?.get(),
                )?,
            },
            _ => return None,
        }
    };
    let transformation = if kind == "Part::Sweep" {
        SweepTransformation::Constant
    } else {
        match integer_property(properties, "Transformation").unwrap_or(0) {
            0 => SweepTransformation::Constant,
            1 => SweepTransformation::MultiSection,
            2 => SweepTransformation::Linear,
            3 => SweepTransformation::SShape,
            4 => SweepTransformation::Interpolation,
            _ => return None,
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: cadmpeg_ir::features::SweepShape::sheet_sections(
            if solid {
                SweepMode::Solid {
                    op: operation_boolean(kind).try_into().ok()?,
                }
            } else {
                SweepMode::Surface {}
            },
            cadmpeg_ir::features::SweepSection::Profile(profile.planar().cloned()?),
            profiles
                .into_iter()
                .map(|profile| {
                    profile
                        .planar()
                        .cloned()
                        .map(cadmpeg_ir::features::SweepSection::Profile)
                })
                .collect::<Option<Vec<_>>>()?,
        ),

        path: Some(PathRef::Native(path_property.id.clone())),

        orientation: Some(orientation),
        transition: Some(transition),
        transformation: Some(transformation),
        path_tangent,
        linearize: if kind == "Part::Sweep" {
            bool_selector(properties, "Linearize", false)?
        } else {
            false
        },
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: if part_design {
            Some(bool_selector(properties, "AllowMultiFace", false)?)
        } else {
            None
        },
    }))
}

fn hole_definition(
    ctx: &DecodeContext<'_>,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    program_version: Option<&str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let profile = profile_ref(ctx, owner, properties, sketches)?;
    Ok((|| {
    if matches!(profile, ProfileRef::Planar(PlanarProfileRef::Unresolved(_))) {
        return None;
    }
    let filter_bits = integer_selector(properties, "BaseProfileType", 6)?;
    let profile_filter = match filter_bits & 7 {
        1 => HoleProfileFilter::Points,
        2 => HoleProfileFilter::Circles,
        3 => HoleProfileFilter::PointsAndCircles,
        4 => HoleProfileFilter::Arcs,
        5 => HoleProfileFilter::PointsAndArcs,
        6 => HoleProfileFilter::CirclesAndArcs,
        7 => HoleProfileFilter::All,
        _ => return None,
    };
    let positive = |name| scalar_named(properties, name).and_then(PositiveReal::from_finite);
    let diameter = positive("Diameter")?;
    let cut_angle = || {
        positive("HoleCutCountersinkAngle")
            .filter(|value| value.get() < 180.0)
            .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::new(value.get().to_radians()))
    };
    let legacy_cut_types = program_version
        .and_then(freecad_program_version)
        .is_some_and(|version| version < (0, 21));
    let kind = match enumeration_selector(properties, "HoleCutType", 0)? {
        0 => HoleKind::Simple,
        1 => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDiameter",
            )?),
            depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDepth",
            )?),
        },
        2 => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDiameter",
            )?),
            angle: cut_angle()?,
        },
        3 if !legacy_cut_types => HoleKind::Counterdrill {
            diameters: cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                    "HoleCutDiameter",
                )?),
                None,
            )
            .ok()?,

            depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDepth",
            )?),
            angle: cut_angle()?,
        },
        3 | 5 if legacy_cut_types => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDiameter",
            )?),
            depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDepth",
            )?),
        },
        4 if legacy_cut_types => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "HoleCutDiameter",
            )?),
            angle: cut_angle()?,
        },
        _ => return None,
    };
    let extent = match enumeration_selector(properties, "DepthType", 0)? {
        0 => LinearTermination::Blind {
            length: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                "Depth",
            )?)
            .into(),
        },
        1 => LinearTermination::ThroughAll {},
        _ => return None,
    };
    let bottom = match enumeration_selector(properties, "DrillPoint", 1)? {
        0 => HoleBottom::Flat,
        1 => HoleBottom::Angled {
            included_angle: cadmpeg_ir::scalar::InteriorAngle::new(
                positive("DrillPointAngle")?.get().to_radians(),
            )?,
            depth_to_tip: bool_selector(properties, "DrillForDepth", false)?,
        },
        _ => return None,
    };
    let tapered = bool_selector(properties, "Tapered", false)?;
    let taper_angle = tapered
        .then(|| {
            positive("TaperedAngle")
                .filter(|value| value.get() < 180.0)
                .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::new(value.get().to_radians()))
        })
        .flatten();
    if tapered && taper_angle.is_none() {
        return None;
    }
    let thread_type = enumeration_selector(properties, "ThreadType", 0)?;
    let specification = if thread_type == 0 {
        None
    } else {
        let threaded = bool_selector(properties, "Threaded", false)?;
        let standard = cadmpeg_core::text::NonBlankString::new(thread_standard(thread_type)?)?;
        let designation = enumeration_label(properties, "ThreadSize");
        let modeled = if property(properties, "ModelThread").is_some() {
            bool_selector(properties, "ModelThread", false)?
        } else {
            bool_selector(properties, "ModelActualThread", false)?
        };
        let cosmetic = bool_selector(properties, "CosmeticThread", false)?;
        let hand = match enumeration_selector(properties, "ThreadDirection", 0)? {
            0 => ThreadHand::Right,
            1 => ThreadHand::Left,
            _ => return None,
        };
        let depth = match enumeration_selector(properties, "ThreadDepthType", 0)? {
            0 => HoleThreadDepth::HoleDepth,
            1 => HoleThreadDepth::Blind {
                depth: cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(positive(
                    "ThreadDepth",
                )?),
            },
            2 => HoleThreadDepth::TappedStandard,
            _ => return None,
        };
        let clearance = if bool_selector(properties, "UseCustomThreadClearance", false)? {
            Some(Length::from_assigned_real(scalar_named(
                properties,
                "CustomThreadClearance",
            )?))
        } else {
            None
        };
        Some(Box::new(if threaded {
            HoleSpecification::Threaded {
                standard,
                designation,
                class: enumeration_label(properties, "ThreadClass"),
                modeled,
                cosmetic,
                pitch: positive("ThreadPitch")
                    .map(cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real),
                major_diameter: positive("ThreadDiameter")
                    .map(cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real),
                hand,
                depth,
                clearance,
            }
        } else {
            HoleSpecification::Clearance {
                standard,
                designation,
                fit: enumeration_label(properties, "ThreadFit"),
                modeled,
                cosmetic,
                hand,
                depth,
                clearance,
            }
        }))
    };
    let direction = axis_reference(properties, "Profile", objects, properties_by_owner)
        .map(|(_, direction)| cadmpeg_ir::features::FeatureDirection3::from(direction));
    Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: Some(profile.planar().cloned()?),
        profile_filter: Some(profile_filter),
        face: None,
        direction,
        placements: None,
        shape: cadmpeg_ir::features::holes::HoleShape::new(
            HoleConstruction::Form {
                kind,
                specification,
            },
            None,
            Some(cadmpeg_ir::scalar::PositiveLength::from_assigned_positive_real(diameter)),
        )
        .ok()?,

        extent: Some(extent),
        bottom: Some(bottom),
        taper_angle,
        allow_multi_profile_faces: Some(bool_selector(properties, "AllowMultiFace", false)?),
    }))
    })())
}

fn freecad_program_version(value: &str) -> Option<(u64, u64)> {
    value
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .filter(|part| part.contains('.'))
        .find_map(|part| {
            let mut components = part.split('.');
            Some((
                components.next()?.parse().ok()?,
                components.next()?.parse().ok()?,
            ))
        })
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
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((law, axis_origin, axis_direction)) = (|| {
    let law = match enumeration_selector(properties, "Mode", 0)? {
        0 => HelicalSweepLaw::PitchHeightAngle,
        1 => HelicalSweepLaw::PitchTurnsAngle,
        2 => HelicalSweepLaw::HeightTurnsAngle,
        3 => HelicalSweepLaw::HeightTurnsGrowth,
        _ => return None,
    };
    let (axis_origin, axis_direction) =
        match vector_property(properties, "Base").zip(vector_property(properties, "Axis")) {
            Some((origin, direction)) => (
                origin.as_point().get(),
                cadmpeg_ir::units::UnitVector3::normalized(direction.get())?,
            ),
            None => axis_reference(properties, "ReferenceAxis", objects, properties_by_owner)?,
        };
    Some((law, axis_origin, axis_direction))
    })() else { return Ok(None); };
    let profile = profile_ref(ctx, owner, properties, sketches)?;
    Ok((|| {
    if matches!(profile, ProfileRef::Planar(PlanarProfileRef::Unresolved(_))) {
        return None;
    }
    let construction = HelicalSweepConstruction {
        profile: profile.planar().cloned()?,
        axis_origin: cadmpeg_ir::features::FinitePoint3::new(axis_origin)?,
        axis_direction,
        law,
        pitch: cadmpeg_ir::scalar::NonNegativeLength::from_finite_assigned_real(scalar_named(
            properties, "Pitch",
        )?)?,
        travel: cadmpeg_ir::features::HelicalSweepTravel::new(
            Length::from_assigned_real(scalar_named(properties, "Height")?),
            Length::from_assigned_real(scalar_named(properties, "Growth")?),
        )?,
        turns: cadmpeg_ir::scalar::PositiveReal::from_finite(scalar_named(properties, "Turns")?)?,
        cone_angle: cadmpeg_ir::scalar::Angle::new(
            scalar_named(properties, "Angle")?.get().to_radians(),
        )?,
        left_handed: bool_selector(properties, "LeftHanded", false)?,
        reversed: bool_selector(properties, "Reversed", false)?,
        tolerance: Some(cadmpeg_ir::scalar::PositiveReal::from_finite(
            finite_float_selector(
                properties,
                "Tolerance",
                "App::PropertyFloatConstraint",
                FiniteReal::new(DEFAULT_HELICAL_SWEEP_TOLERANCE)?,
            )?,
        )?),
        allow_multi_profile_faces: Some(bool_selector(properties, "AllowMultiFace", false)?),
    };
    let op = if kind.ends_with("SubtractiveHelix") {
        if bool_selector(properties, "Outside", false)? {
            BooleanOp::Intersect
        } else {
            BooleanOp::Cut
        }
    } else {
        BooleanOp::Join
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::HelicalSweep { construction, op },
    ))
    })())
}

fn binder_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let sources = property(properties, "Support")?
        .links()
        .iter()
        .flatten()
        .filter(|link| link.object().is_some())
        .map(|link| {
            Some(BinderSource {
                target: binder_target(link, features)?,
                subelements: link_selectors(link)
                    .map(cadmpeg_core::text::NonBlankString::new)
                    .collect::<Option<Vec<_>>>()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let construction = if kind == "PartDesign::ShapeBinder" {
        BinderConstruction::Shape {
            trace_support: bool_selector(properties, "TraceSupport", false)?,
        }
    } else {
        let distance =
            finite_float_selector(properties, "Offset", "App::PropertyFloat", FiniteReal::ZERO)?;
        let offset_join = enumeration_selector(properties, "OffsetJoinType", 0)?;
        let offset_fill = bool_selector(properties, "OffsetFill", false)?;
        let offset_open_result = bool_selector(properties, "OffsetOpenResult", false)?;
        let offset_intersection = bool_selector(properties, "OffsetIntersection", false)?;
        let offset = if distance.get() == 0.0 {
            None
        } else {
            Some(BinderOffset {
                distance: cadmpeg_ir::scalar::NonZeroLength::from_assigned_real(distance)?,
                join: match offset_join {
                    0 => BinderOffsetJoin::Arcs,
                    1 => BinderOffsetJoin::Tangent,
                    2 => BinderOffsetJoin::Intersection,
                    _ => return None,
                },
                fill: offset_fill,
                open_result: offset_open_result,
                intersection: offset_intersection,
            })
        };
        let context_properties = properties
            .iter()
            .filter(|property| property.name == "Context")
            .copied()
            .collect::<Vec<_>>();
        let context = match context_properties.as_slice() {
            [] => None,
            [property]
                if property.type_name == "App::PropertyXLink"
                    && property.links().len() == 1
                    && property.links()[0]
                        .as_ref()
                        .is_none_or(|link| link.subelements().is_empty()) =>
            {
                property
                    .links()
                    .first()
                    .and_then(Option::as_ref)
                    .filter(|link| link.object().is_some_and(|object| !object.is_empty()))
                    .and_then(|link| binder_target(link, features))
            }
            _ => return None,
        };
        BinderConstruction::SubShape {
            lifecycle: match enumeration_selector(properties, "BindMode", 0)? {
                0 => BinderLifecycle::Synchronized,
                1 => BinderLifecycle::Frozen,
                2 => BinderLifecycle::Detached,
                _ => return None,
            },
            placement: if bool_selector(properties, "Relative", true)? {
                BinderPlacement::Relative
            } else {
                BinderPlacement::Global
            },
            copy_on_change: match enumeration_selector(properties, "BindCopyOnChange", 0)? {
                0 => BinderCopyOnChange::Disabled,
                1 => BinderCopyOnChange::Enabled,
                2 => BinderCopyOnChange::Mutated,
                _ => return None,
            },
            claim_children: bool_selector(properties, "ClaimChildren", false)?,
            fuse: bool_selector(properties, "Fuse", false)?,
            make_face: bool_selector(properties, "MakeFace", true)?,
            partial_load: bool_selector(properties, "PartialLoad", false)?,
            refine: bool_selector(properties, "Refine", true)?,
            offset,
            context,
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Binder {
        sources,
        construction,
    }))
}

fn binder_target(
    link: &crate::native::LinkTarget,
    features: &HashMap<&str, FeatureId>,
) -> Option<BinderTarget> {
    let object = link.object()?;
    if let Some(document) = link.document() {
        return Some(BinderTarget::External {
            document: cadmpeg_core::text::NonBlankString::new(document.as_str())?,
            object: cadmpeg_core::text::NonBlankString::new(object)?,
        });
    }
    Some(match features.get(object).cloned() {
        Some(feature) => BinderTarget::Feature { feature },
        None => BinderTarget::Native {
            reference: cadmpeg_core::text::NonBlankString::new(object)?,
        },
    })
}

fn enumeration_label(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    let property = property(properties, name)?;
    if property.type_name != "App::PropertyEnumeration" {
        return None;
    }
    let document = roxmltree::Document::parse(property.xml.text()).ok()?;
    let root = document.root_element();
    if !root.has_tag_name("Property") {
        return None;
    }
    let values = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let (integer, custom_list) = match values.as_slice() {
        [integer] if integer.has_tag_name("Integer") => (*integer, None),
        [integer, custom_list]
            if integer.has_tag_name("Integer") && custom_list.has_tag_name("CustomEnumList") =>
        {
            (*integer, Some(*custom_list))
        }
        _ => return None,
    };
    if integer.children().any(|child| child.is_element()) {
        return None;
    }
    let custom = match integer.attribute("CustomEnum") {
        None => false,
        Some("true") => true,
        Some(_) => return None,
    };
    if custom != custom_list.is_some() {
        return None;
    }
    let index = integer.attribute("value")?.parse::<usize>().ok()?;
    let custom_list = custom_list?;
    let count = custom_list.attribute("count")?.parse::<usize>().ok()?;
    let enum_values = custom_list
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if enum_values.len() != count {
        return None;
    }
    enum_values
        .into_iter()
        .map(|value| {
            if !value.has_tag_name("Enum") || value.children().any(|child| child.is_element()) {
                return None;
            }
            value.attribute("value").map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>()?
        .get(index)
        .cloned()
}

#[derive(Clone, Copy)]
struct PatternSources<'a, 'b> {
    objects: &'a [ObjectRecord],
    properties_by_owner: &'a HashMap<&'b str, Vec<&'b PropertyRecord>>,
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
        properties_by_owner,
        ..
    } = sources;
    let seeds = (|| -> Option<Vec<FeatureId>> {
        let originals = property(properties, "Originals")
            .filter(|property| !property.links().is_empty())
            .or_else(|| {
                property(properties, "BaseFeature").filter(|property| {
                    property
                        .links()
                        .iter()
                        .flatten()
                        .any(|link| link.object().is_some_and(|object| !object.is_empty()))
                })
            });
        if let Some(originals) = originals {
            let seeds = originals
                .links()
                .iter()
                .filter_map(|link| link.as_ref()?.object())
                .map(|target| {
                    features.get(target).cloned().map(Some).or_else(|| {
                        objects
                            .iter()
                            .find(|object| object.id == target)
                            .filter(|object| {
                                matches!(
                                    object.type_name.as_str(),
                                    "App::Line"
                                        | "App::Plane"
                                        | "App::Point"
                                        | "App::CoordinateSystem"
                                )
                            })
                            .map(|_| None)
                    })
                })
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            if seeds.is_empty() {
                return None;
            }
            Some(seeds)
        } else if let Some(seeds) =
            multi_transform_stage_seeds(owner, features, objects, properties_by_owner)
        {
            Some(seeds)
        } else {
            Some(vec![implicit_body_predecessor(
                owner,
                features,
                objects,
                properties_by_owner,
            )?])
        }
    })();
    let Some(seeds) = seeds else { return Ok(None) };

    let pattern =
        if kind.ends_with("MultiTransform") {
            let Some(transformations) = property(properties, "Transformations") else {
                return Ok(None);
            };
            if transformations.links().is_empty() {
                return Ok(None);
            }
            ctx.charge_collection_items(
                transformations.links().len() as u64,
                "freecad pattern stages",
            )?;
            let mut stages = reserved_vec(
                ctx,
                transformations.links().len(),
                "freecad pattern stages",
            )?;
            for link in transformations.links() {
                let Some((object, owned)) = (|| {
                    let target = link.as_ref()?.object()?;
                    let object = objects.iter().find(|object| object.id == target)?;
                    let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
                    Some((object, owned))
                })() else {
                    return Ok(None);
                };
                let Some(pattern) = pattern_kind::<
                    cadmpeg_ir::features::patterns::NoNestedComposite,
                >(ctx, &object.type_name, owned, sources)?
                else {
                    return Ok(None);
                };
                stages.push(PatternStage {
                    pattern: Box::new(pattern),
                });
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
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds: seeds.into_iter().map(PatternSeed::Feature).collect(),
            pattern,
        },
    )))
}

fn multi_transform_stage_seeds(
    stage: &str,
    features: &HashMap<&str, FeatureId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<Vec<FeatureId>> {
    objects.iter().find_map(|consumer| {
        let owned = properties_by_owner.get(consumer.id.as_str())?;
        let transformations = property(owned, "Transformations")?;
        transformations
            .links()
            .iter()
            .any(|link| link.as_ref().and_then(crate::native::LinkTarget::object) == Some(stage))
            .then_some(())?;
        let originals = property(owned, "Originals")
            .filter(|property| !property.links().is_empty())
            .or_else(|| property(owned, "BaseFeature"))?;
        let seeds = originals
            .links()
            .iter()
            .filter_map(|link| link.as_ref()?.object())
            .map(|object| features.get(object).cloned())
            .collect::<Option<Vec<_>>>()?;
        (!seeds.is_empty()).then_some(seeds)
    })
}

fn implicit_body_predecessor(
    owner: &str,
    features: &HashMap<&str, FeatureId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<FeatureId> {
    objects.iter().find_map(|object| {
        let owned = properties_by_owner.get(object.id.as_str())?;
        let members = body_membership_property(owned)?;
        let position = members.links().iter().position(|link| {
            link.as_ref().and_then(crate::native::LinkTarget::object) == Some(owner)
        })?;
        members.links()[..position]
            .iter()
            .rev()
            .filter_map(|link| link.as_ref()?.object())
            .find_map(|member| features.get(member).cloned())
    })
}

fn pattern_kind<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
    sources: PatternSources<'_, '_>,
) -> Result<Option<PatternKind<C>>, CodecError> {
    let PatternSources {
        objects,
        properties_by_owner,
        entries,
    } = sources;
    if kind.ends_with("Mirrored") {
        return Ok((|| {
            Some(
                if let Some((plane_origin, plane_normal)) =
                    plane_reference(properties, "MirrorPlane", objects, properties_by_owner)
                {
                    PatternKind::new(PatternTransform::Mirror {
                        plane_origin: cadmpeg_ir::features::FinitePoint3::new(plane_origin)?,
                        plane_normal: cadmpeg_ir::features::FeatureDirection3::from(plane_normal),
                    })
                    .ok()?
                } else {
                    PatternKind::new(PatternTransform::MirrorReference {
                        plane: cadmpeg_ir::features::FaceSelection::Native(
                            property(properties, "MirrorPlane")?.id.clone(),
                        ),
                    })
                    .ok()?
                },
            )
        })());
    }

    let Some(count) = (if kind.ends_with("Scaled") {
        integer_selector(properties, "Occurrences", 2)
    } else {
        let absent_default = if kind.ends_with("PolarPattern") { 3 } else { 2 };
        integer_constraint_selector(properties, "Occurrences", absent_default, true)
    }) else {
        return Ok(None);
    };
    if count == 0 || count > MAX_SKETCH_RECORDS as u64 {
        return Ok(None);
    }
    let count = count as u32;
    let Some(mode) = enumeration_selector(properties, "Mode", 0) else {
        return Ok(None);
    };

    if kind.ends_with("Scaled") {
        return Ok((|| {
            let final_factor =
                cadmpeg_ir::scalar::PositiveReal::from_finite(scalar_named(properties, "Factor")?)?;
            (count >= 2).then_some(
                PatternKind::new(PatternTransform::Scale {
                    center: PatternScaleCenter::FirstSeedCentroid,
                    final_factor,
                    count,
                })
                .ok()?,
            )
        })());
    }

    let pattern = if kind.ends_with("LinearPattern") {
        let Some(first) = linear_pattern_axis(ctx, properties, "", count, mode, sources)? else {
            return Ok(None);
        };
        let Some(count2) = integer_constraint_selector(properties, "Occurrences2", 1, false) else {
            return Ok(None);
        };
        if count2 == 0 || count2 > MAX_SKETCH_RECORDS as u64 {
            return Ok(None);
        }
        if count2 > 1 {
            let Some(mode2) = enumeration_selector(properties, "Mode2", 0) else {
                return Ok(None);
            };
            let Some(second) =
                linear_pattern_axis(ctx, properties, "2", count2 as u32, mode2, sources)?
            else {
                return Ok(None);
            };
            let Some(pattern) = (|| {
                PatternKind::new(PatternTransform::Composite {
                    stages: C::rebuild(vec![
                        PatternStage {
                            pattern: Box::new(first),
                        },
                        PatternStage {
                            pattern: Box::new(second),
                        },
                    ])
                    .ok()?,
                })
                .ok()
            })() else {
                return Ok(None);
            };
            pattern
        } else {
            first.widen()
        }
    } else if kind.ends_with("PolarPattern") {
        let Some((axis_origin, mut axis_dir)) =
            axis_reference(properties, "Axis", objects, properties_by_owner)
        else {
            return Ok(None);
        };
        let Some(reversed) = bool_selector(properties, "Reversed", false) else {
            return Ok(None);
        };
        if reversed {
            axis_dir = axis_dir.reversed();
        }
        let Some(axis_origin) = cadmpeg_ir::features::FinitePoint3::new(axis_origin) else {
            return Ok(None);
        };
        let Some(angles) = pattern_locations(
            ctx,
            properties,
            "",
            count,
            mode,
            ("Angle", "Offset"),
            entries,
        )?
        else {
            return Ok(None);
        };
        let Some(pattern) = (|| {
            Some(if let Some(step) = uniform_step(&angles) {
                PatternKind::new(PatternTransform::Circular {
                    axis_origin,
                    axis_dir: cadmpeg_ir::features::FeatureDirection3::from(axis_dir),
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(
                        (step.get() * f64::from(count - 1)).to_radians(),
                    )?,
                    count,
                })
                .ok()?
            } else {
                PatternKind::new(PatternTransform::CircularAngles {
                    axis_origin,
                    axis_dir,
                    angles: angles
                        .into_iter()
                        .map(|angle| cadmpeg_ir::scalar::Angle::new(angle.get().to_radians()))
                        .collect::<Option<Vec<_>>>()?,
                })
                .ok()?
            })
        })() else {
            return Ok(None);
        };
        pattern
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
        objects,
        properties_by_owner,
        entries,
    } = sources;
    let name = |base: &str| format!("{base}{suffix}");
    let mut direction =
        axis_reference(properties, &name("Direction"), objects, properties_by_owner)
            .map(|(_, direction)| direction);
    let Some(reversed) = bool_selector(properties, &name("Reversed"), false) else {
        return Ok(None);
    };
    if reversed {
        direction = direction.map(cadmpeg_ir::units::UnitVector3::reversed);
    }
    let direction = direction.map(cadmpeg_ir::features::FeatureDirection3::from);
    let Some(offsets) = pattern_locations(
        ctx,
        properties,
        suffix,
        count,
        mode,
        ("Length", "Offset"),
        entries,
    )?
    else {
        return Ok(None);
    };
    Ok((|| {
        if let Some(spacing) = uniform_step(&offsets) {
            Some(
                PatternKind::new(PatternTransform::Linear {
                    direction,
                    spacing: cadmpeg_ir::scalar::PositiveLength::from_assigned_real(spacing)?,
                    count,
                    second: None,
                })
                .ok()?,
            )
        } else {
            Some(
                PatternKind::new(PatternTransform::LinearOffsets {
                    direction,
                    offsets: offsets
                        .into_iter()
                        .map(Length::from_assigned_real)
                        .collect(),
                })
                .ok()?,
            )
        }
    })())
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
    let name = |base: &str| format!("{base}{suffix}");
    let intervals = match mode {
        0 => {
            let Some(extent) = scalar_named(properties, &name(extent_base)) else {
                return Ok(None);
            };
            let Some(interval) = FiniteReal::new(extent.get() / f64::from(count - 1)) else {
                return Ok(None);
            };
            ctx.alloc_filled(count as usize - 1, interval, "freecad pattern intervals")?
        }
        1 => {
            let Some(fallback) = scalar_named(properties, &name(offset_base)) else {
                return Ok(None);
            };
            let spacings = match property(properties, &name("Spacings")) {
                Some(property) => numeric_list(ctx, property, entries)?,
                None => Some(Vec::new()),
            };
            let Some(spacings) = spacings else {
                return Ok(None);
            };
            let pattern = match property(properties, &name("SpacingPattern")) {
                Some(property) => numeric_list(ctx, property, entries)?,
                None => Some(Vec::new()),
            };
            let Some(pattern) = pattern else {
                return Ok(None);
            };
            if !spacings.is_empty() && spacings.len() != count as usize - 1 {
                return Ok(None);
            }
            ctx.charge_collection_items(u64::from(count - 1), "freecad pattern intervals")?;
            (0..count as usize - 1)
                .map(|index| {
                    let explicit = spacings
                        .get(index)
                        .copied()
                        .filter(|value| value.get() != -1.0);
                    if let Some(explicit) = explicit {
                        explicit
                    } else if pattern.len() > 1 {
                        pattern[index % pattern.len()]
                    } else {
                        fallback
                    }
                })
                .collect()
        }
        _ => return Ok(None),
    };
    ctx.charge_collection_items(u64::from(count), "freecad pattern locations")?;
    let mut locations = reserved_vec(ctx, count as usize, "freecad pattern locations")?;
    locations.push(FiniteReal::ZERO);
    let mut location = FiniteReal::ZERO;
    for interval in intervals {
        let Some(interval) = cadmpeg_ir::scalar::PositiveReal::from_finite(interval) else {
            return Ok(None);
        };
        let Some(next) = FiniteReal::new(location.get() + interval.get()) else {
            return Ok(None);
        };
        location = next;
        locations.push(location);
    }
    Ok(Some(locations))
}

fn uniform_step(locations: &[FiniteReal]) -> Option<FiniteReal> {
    let step = *locations.get(1)?;
    locations
        .windows(2)
        .all(|pair| {
            (pair[1].get() - pair[0].get() - step.get()).abs() <= f64::EPSILON * step.get().abs()
        })
        .then_some(step)
}

fn axis_reference(
    properties: &[&PropertyRecord],
    name: &str,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<(Point3, cadmpeg_ir::units::UnitVector3)> {
    if let Some(direction) = vector_property(properties, name) {
        return Some((
            Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::units::UnitVector3::normalized(direction.get())?,
        ));
    }
    let (link, selector) = singular_reference_link(property(properties, name)?)?;
    let target = link.object()?;
    let object = objects.iter().find(|object| object.id == target)?;
    let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
    let (origin, z_axis, x_axis, y_axis) = placement_frame(owned)?;
    let direction = match object.type_name.as_str() {
        "PartDesign::Line" | "App::Line" => z_axis,
        "PartDesign::Plane" | "App::Plane" => z_axis,
        "PartDesign::CoordinateSystem" => match selector {
            Some("X_Axis" | "XAxis" | "X") => x_axis,
            Some("Y_Axis" | "YAxis" | "Y") => y_axis,
            Some("Z_Axis" | "ZAxis" | "Z") | None => z_axis,
            _ => return None,
        },
        kind if is_sketch(kind) => match selector {
            Some("H_Axis") => x_axis,
            Some("V_Axis") => y_axis,
            Some("N_Axis") | None => z_axis,
            _ => return None,
        },
        _ => return None,
    };
    Some((
        origin,
        cadmpeg_ir::units::UnitVector3::normalized(direction)?,
    ))
}

fn plane_reference(
    properties: &[&PropertyRecord],
    name: &str,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<(Point3, cadmpeg_ir::units::UnitVector3)> {
    let (link, selector) = singular_reference_link(property(properties, name)?)?;
    let target = link.object()?;
    let object = objects.iter().find(|object| object.id == target)?;
    let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
    let (origin, z_axis, x_axis, y_axis) = placement_frame(owned)?;
    let normal = match object.type_name.as_str() {
        "PartDesign::Plane" | "App::Plane" => z_axis,
        "PartDesign::CoordinateSystem" => match selector {
            Some("XY_Plane" | "XYPlane" | "XY") | None => z_axis,
            Some("XZ_Plane" | "XZPlane" | "XZ") => y_axis,
            Some("YZ_Plane" | "YZPlane" | "YZ") => x_axis,
            _ => return None,
        },
        kind if is_sketch(kind) => match selector {
            None | Some("N_Axis") => z_axis,
            Some("H_Axis") => y_axis,
            Some("V_Axis") => x_axis,
            _ => return None,
        },
        _ => return None,
    };
    Some((origin, cadmpeg_ir::units::UnitVector3::normalized(normal)?))
}

fn link_selectors(link: &crate::native::LinkTarget) -> impl Iterator<Item = &str> {
    link.subelements()
        .iter()
        .flat_map(|selector| selector.split_ascii_whitespace())
        .filter(|selector| !selector.is_empty())
}

fn singular_reference_link(
    property: &PropertyRecord,
) -> Option<(&crate::native::LinkTarget, Option<&str>)> {
    let link = scalar_link(property)?;
    let object = link.object()?;
    if object.is_empty() {
        return None;
    }
    let mut selectors = link_selectors(link);
    let selector = selectors.next();
    if selectors.next().is_some() {
        return None;
    }
    Some((link, selector))
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

fn scalar_named(properties: &[&PropertyRecord], name: &str) -> Option<FiniteReal> {
    property(properties, name).and_then(scalar_value)
}

fn string_property_value(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<String>, CodecError> {
    if property.type_name != "App::PropertyString" {
        return Ok(None);
    }
    direct_root_value(property, "String", "value", |value| {
        retained_string(ctx, value, "fcstd string property value")
    }).transpose()
}

fn integer_property(properties: &[&PropertyRecord], name: &str) -> Option<u64> {
    let value = scalar_named(properties, name)?;
    let value = value.get();
    if value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    Some(if value >= U64_UPPER_EXCLUSIVE {
        u64::MAX
    } else {
        value as u64
    })
}

fn integer_selector(
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: u64,
) -> Option<u64> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != "App::PropertyInteger" {
        return None;
    }
    let value = direct_root_value(property, "Integer", "value", str::parse::<i64>)?.ok()?;
    u64::try_from(value).ok()
}

fn integer_constraint_selector(
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: u64,
    accept_legacy_integer: bool,
) -> Option<u64> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != "App::PropertyIntegerConstraint"
        && !(accept_legacy_integer && property.type_name == "App::PropertyInteger")
    {
        return None;
    }
    let value = direct_root_value(property, "Integer", "value", str::parse::<i64>)?.ok()?;
    u64::try_from(value).ok()
}

fn numeric_list(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    entries: &[EntryRecord],
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    if property.type_name != "App::PropertyFloatList" {
        return Ok(None);
    }
    direct_root(property, "FloatList", |root| {
        let Some(file) = root.attribute("file") else { return Ok(None) };
        if file.is_empty() {
            return Ok(property.side_entries().is_empty().then(Vec::new));
        }
        if property.side_entries() != [file] {
            return Ok(None);
        }
        let Some(data) = entries.iter().find(|entry| entry.name == file).map(|entry| entry.data.as_slice()) else {
            return Ok(None);
        };
        let mut view = View::over_retained(data);
        let Some(count) = view.u32_le().map(|count| count as usize) else { return Ok(None) };
        if count > MAX_SKETCH_RECORDS || view.counted(count as u64, 8).is_none() {
            return Ok(None);
        }
        let mut values = collection_vec(ctx, count, "fcstd numeric-list values")?;
        for _ in 0..count {
            let Some(value) = view.f64_le().and_then(FiniteReal::new) else { return Ok(None) };
            values.push(value);
        }
        Ok(view.is_empty().then_some(values))
    }).unwrap_or(Ok(None))
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
        ctx, "feature", object, format_args!(""),
        "fcstd design feature identity",
    )?).map_err(CodecError::malformed)
}

fn design_identity_text(
    ctx: &DecodeContext<'_>,
    kind: &str,
    object: &ObjectRecord,
    tail: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    retained_format(
        ctx,
        format_args!("fcstd:design:{kind}#{}{tail}", crate::native::id_key(&object.id)),
        operation,
    )
}

fn feature_base_definition(
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let base_properties = properties
        .iter()
        .filter(|property| property.name == "BaseFeature")
        .copied()
        .collect::<Vec<_>>();
    let [property] = base_properties.as_slice() else {
        return None;
    };
    if property.type_name != "App::PropertyLink" || property.links().len() != 1 {
        return None;
    }
    let source = property.links()[0].as_ref()?.object()?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::DerivedGeometry {
            source: feature_ids.get(source)?.clone(),
        },
    ))
}

fn imported_geometry_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    properties: &[&PropertyRecord],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let path = match property(properties, "FileName") {
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
    let Some(path) = path.try_into().ok() else { return Ok(None) };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::ImportedGeometry {
            path,
            format,
        },
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
    if kind == "PartDesign::Boolean" {
        return true;
    }
    ["Cut", "Fuse", "MultiFuse", "Common", "MultiCommon"]
        .iter()
        .any(|operation| kind == format!("Part::{operation}"))
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
    let mut features_by_native = HashMap::new();
    for feature in features {
        if let Some(native_ref) = feature.native_ref.as_deref() {
            insert_hash_map(ctx, &mut features_by_native, native_ref, feature,
                "FreeCAD design census feature index")?;
        }
    }
    let count = objects.iter().filter(|object| is_design_object(&object.type_name)).count();
    let mut census = collection_vec(ctx, count, "FreeCAD design census records")?;
    for object in objects.iter().filter(|object| is_design_object(&object.type_name)) {
            let feature = features_by_native.get(object.id.as_str()).ok_or_else(|| {
                malformed_design(ctx, format_args!(
                    "design object {} has no neutral history projection",
                    object.id
                ))
            })?;
            let (definition, post_processed) = match feature.evaluation.definition() {
                FeatureDefinition::PostProcess { operation, .. } => (operation, true),
                FeatureDefinition::Operation(operation) => (operation, false),
            };
            let value = serde_json::to_value(definition).map_err(|error| {
                malformed_design(ctx, format_args!(
                    "cannot classify design feature {}: {error}",
                    feature.id
                ))
            })?;
            let semantic_kind = value
                .get("definition")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    malformed_design(ctx, format_args!(
                        "design feature {} has no semantic family tag",
                        feature.id
                    ))
                })?;
            let semantic_kind = retained_string(ctx, semantic_kind, "FreeCAD design census semantic kind")?;
            census.push(crate::native::DesignCensusRecord {
                id: crate::native::native_child_id_charged(ctx, "design-census", &object.id, "projection")?,
                object: retained_string(ctx, &object.id, "FreeCAD design census object")?,
                type_name: retained_string(ctx, &object.type_name, "FreeCAD design census type")?,
                feature: retained_string(ctx, feature.id.as_str(), "FreeCAD design census feature")?,
                semantic_kind,
                post_processed,
            });
    }
    census.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(census)
}

#[cfg(test)]
mod profile_tests {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
        SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
    };
    use cadmpeg_ir::spreadsheets::{CellAddress, SpreadsheetRange};

    use super::{endpoints_match_by_roundoff, merged_range, range_contains_address};

    fn build_profiles(
        entities: &[SketchEntity],
        constraints: &[SketchConstraint],
    ) -> Vec<Vec<SketchEntityUse>> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("profile test context");
        super::build_profiles(&ctx, entities, constraints).expect("profile projection")
    }

    #[test]
    fn x64_profile_index_refuses_exhausted_work_before_search() {
        let entities = [entity(
            "test:test:entity#work",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .expect("line geometry"),
        )];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("profile test context");
        let error = super::build_profiles(&ctx, &entities, &[])
            .expect_err("profile construction work must be admitted before scanning");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
        ));
    }

    #[test]
    fn ignores_nonpositive_spans_in_the_neutral_spreadsheet_projection() {
        for xml in [
            r#"<Cell address="A1" rowSpan="0" colSpan="2"/>"#,
            r#"<Cell address="A1" rowSpan="2" colSpan="-7"/>"#,
        ] {
            let document = roxmltree::Document::parse(xml).expect("cell XML");
            assert_eq!(merged_range(document.root_element()).unwrap(), None);
        }
    }

    #[test]
    fn detects_cells_covered_by_a_merged_range() {
        let range = SpreadsheetRange::new(
            CellAddress::parse("A1").expect("A1"),
            CellAddress::parse("I2").expect("I2"),
        )
        .expect("A1:I2");

        assert!(range_contains_address(&range, "B1"));
        assert!(range_contains_address(&range, "I2"));
        assert!(!range_contains_address(&range, "J1"));
        assert!(!range_contains_address(&range, "A3"));
    }

    fn entity(id: &str, geometry: SketchGeometry) -> SketchEntity {
        SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint(id).unwrap(),
            SketchId::mint("test:test:sketch#curved").unwrap(),
            geometry,
        )
    }

    #[test]
    fn profile_chain_refuses_before_use_growth_and_identity_copy() {
        let entities = [entity(
            "test:test:entity#profile-line",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            }).expect("valid line"),
        )];
        crate::test_support::assert_collection_refusal_at(
            &[], "FCStd profile uses", |ctx| super::build_profiles(ctx, &entities, &[]),
        );
        crate::test_support::assert_retained_refusal_at(
            &[], "FCStd profile use identity", |ctx| super::build_profiles(ctx, &entities, &[]),
        );
        crate::test_support::assert_collection_refusal_at(
            &[], "FCStd profile chains", |ctx| super::build_profiles(ctx, &entities, &[]),
        );
    }

    #[test]
    fn curved_segments_chain_by_their_evaluated_endpoints() {
        let entities = [
            entity(
                "test:test:entity#line",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#arc",
                SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                    center: Point2::new(0.0, 0.0),
                    radius: Length::new(1.0).unwrap(),
                    start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
                    end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#line-after-arc",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 1.0),
                    end: Point2::new(1.0, 1.0),
                })
                .unwrap(),
            ),
        ];
        let profiles = build_profiles(&entities, &[]);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 3);
    }

    #[test]
    fn disconnected_profile_seeds_follow_persisted_entity_order() {
        let entities = (1..=11)
            .map(|ordinal| {
                entity(
                    &format!("test:test:entity#{ordinal}"),
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(ordinal as f64 * 10.0, 0.0),
                        end: Point2::new(ordinal as f64 * 10.0 + 1.0, 0.0),
                    })
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();

        let profiles = build_profiles(&entities, &[]);

        assert_eq!(profiles.len(), entities.len());
        assert!(profiles
            .iter()
            .zip(&entities)
            .all(|(profile, entity)| profile[0].entity == entity.id().clone()));
    }

    #[test]
    fn disconnected_profile_seeds_skip_construction_in_persisted_order() {
        let mut construction = entity(
            "test:test:entity#1",
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(-1.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        );
        construction.construction = true;
        let entities = vec![
            construction,
            entity(
                "test:test:entity#2",
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length::new(2.0).unwrap(),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#3",
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(10.0, 0.0),
                    radius: Length::new(2.0).unwrap(),
                })
                .unwrap(),
            ),
        ];

        let profiles = build_profiles(&entities, &[]);

        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile[0].entity.clone())
                .collect::<Vec<_>>(),
            vec![entities[1].id().clone(), entities[2].id().clone()]
        );
    }

    #[test]
    fn coincident_constraint_connects_numerically_separate_endpoints() {
        let entities = [
            entity(
                "test:test:entity#1",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#2",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(2.0, 0.0),
                    end: Point2::new(3.0, 0.0),
                })
                .unwrap(),
            ),
        ];
        let constraint = SketchConstraint {
            id: SketchConstraintId::mint("test:test:constraint#1").unwrap(),
            sketch: entities[0].sketch.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::CoincidentLoci {
                    loci: vec![
                        SketchLocus::End(entities[0].id().clone()),
                        SketchLocus::Start(entities[1].id().clone()),
                    ],
                },
            )
            .unwrap(),
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        };

        let profiles = build_profiles(&entities, &[constraint]);

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 2);
    }

    #[test]
    fn explicit_endpoint_relations_precede_nearby_geometry() {
        let entities = [
            entity(
                "test:test:entity#anchor",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(0.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#nearby",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(32.0 * f64::EPSILON, 0.0),
                    end: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#constrained",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(2.0, 0.0),
                    end: Point2::new(3.0, 0.0),
                })
                .unwrap(),
            ),
        ];
        let constraint = SketchConstraint {
            id: SketchConstraintId::mint("test:test:constraint#explicit-precedence").unwrap(),
            sketch: entities[0].sketch.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::CoincidentLoci {
                    loci: vec![
                        SketchLocus::End(entities[0].id().clone()),
                        SketchLocus::Start(entities[2].id().clone()),
                    ],
                },
            )
            .unwrap(),
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        };

        let profiles = build_profiles(&entities, &[constraint]);

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].len(), 2);
        assert_eq!(profiles[0][0].entity, entities[0].id().clone());
        assert_eq!(profiles[0][1].entity, entities[2].id().clone());
        assert_eq!(
            profiles[1],
            vec![SketchEntityUse {
                entity: entities[1].id().clone(),
                reversed: false,
            }]
        );
    }

    #[test]
    fn profile_junction_uses_the_cadir_roundoff_boundary() {
        let entities = |gap| {
            [
                entity(
                    "test:test:entity#anchor",
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(0.0, 0.0),
                        end: Point2::new(1.0, 0.0),
                    })
                    .unwrap(),
                ),
                entity(
                    "test:test:entity#continuation",
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(1.0 + gap, 0.0),
                        end: Point2::new(2.0, 0.0),
                    })
                    .unwrap(),
                ),
            ]
        };

        let inside = entities(32.0 * f64::EPSILON);
        let inside_profiles = build_profiles(&inside, &[]);
        assert_eq!(inside_profiles.len(), 1);
        assert_eq!(inside_profiles[0].len(), 2);

        let outside = entities(128.0 * f64::EPSILON);
        let outside_profiles = build_profiles(&outside, &[]);
        assert_eq!(outside_profiles.len(), 2);
        assert!(outside_profiles.iter().all(|profile| profile.len() == 1));
    }

    #[test]
    fn multiple_explicit_coincident_continuations_remain_separate_seeds() {
        let entities = [
            entity(
                "test:test:entity#anchor",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(0.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#first-continuation",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                })
                .unwrap(),
            ),
            entity(
                "test:test:entity#second-continuation",
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(0.0, 1.0),
                })
                .unwrap(),
            ),
        ];
        let constraint = SketchConstraint {
            id: SketchConstraintId::mint("test:test:constraint#ambiguous-explicit").unwrap(),
            sketch: entities[0].sketch.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::CoincidentLoci {
                    loci: vec![
                        SketchLocus::End(entities[0].id().clone()),
                        SketchLocus::Start(entities[1].id().clone()),
                        SketchLocus::Start(entities[2].id().clone()),
                    ],
                },
            )
            .unwrap(),
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        };

        let profiles = build_profiles(&entities, &[constraint]);

        assert_eq!(profiles.len(), 3);
        assert!(profiles.iter().all(|profile| profile.len() == 1));
    }

    #[test]
    fn endpoint_roundoff_uses_a_bounded_scale() {
        let exact = Point2::new(1.0, 0.0);
        let inside = Point2::new(1.0 + 32.0 * f64::EPSILON, 0.0);
        let outside = Point2::new(1.0 + 128.0 * f64::EPSILON, 0.0);

        assert!(endpoints_match_by_roundoff(exact, inside));
        assert!(!endpoints_match_by_roundoff(exact, outside));
    }

    #[test]
    fn ambiguous_profile_junctions_remain_separate() {
        let entities = (0..3)
            .map(|ordinal| {
                entity(
                    &format!("test:test:entity#{}", ordinal + 1),
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: Point2::new(0.0, 0.0),
                        end: Point2::new(ordinal as f64 + 1.0, 1.0),
                    })
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();

        let profiles = build_profiles(&entities, &[]);

        assert_eq!(profiles.len(), 3);
        assert!(profiles.iter().all(|profile| profile.len() == 1));
    }
}

#[cfg(test)]
pub(crate) mod tests;
