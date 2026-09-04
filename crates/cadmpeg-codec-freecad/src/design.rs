// SPDX-License-Identifier: Apache-2.0
//! Transfer of `FCStd` construction history into neutral design entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{alloc_filled, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    AngularTermination, BinderConstruction, BinderCopyOnChange, BinderLifecycle, BinderOffset,
    BinderOffsetJoin, BinderPlacement, BinderSource, BinderTarget, BodySelection, BooleanOp,
    ChamferSpec, DesignParameter, EdgeSelection, ExtrudeExtent, ExtrudeSide,
    ExtrusionDirectionSource, FaceMaker, Feature, FeatureDefinition, FeatureId,
    FeatureTreeNodeRole, FuzzyTolerance, GeometryImportFormat, HelicalSweepConstruction,
    HelicalSweepLaw, HelixConstructionStyle, HoleBottom, HoleConstruction, HoleKind,
    HoleProfileFilter, HoleSpecification, HoleThreadDepth, InnerWireTaper, Length,
    LinearTermination, ParameterId, ParameterValue, PathRef, PatternKind, PatternScaleCenter,
    PatternSeed, PatternStage, PatternStageCombination, PrimitiveSolid, ProfileRef, RadiusSpec,
    RevolutionAxis, RevolutionFuseOrder, RevolveConstruction, RevolveExtent, RuledCurveOrientation,
    ScaleCenter, ScaleFactors, ShellJoin, ShellMode, SurfaceProjectionMode, SweepMode,
    SweepOrientation, SweepTransformation, SweepTransition, ThreadHand,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchAxis, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    SketchNativeOperand,
};
use cadmpeg_ir::spreadsheets::{
    CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetDimension, SpreadsheetId,
    SpreadsheetRange,
};

use crate::brep::ShapePayloadRecord;
use crate::native::{EntryRecord, ObjectRecord, PropertyRecord};

const MAX_SKETCH_RECORDS: usize = 1_000_000;
const EXTERNAL_GEO_AXIS_COUNT: usize = 2;
const EXTERNAL_GEOMETRY_MISSING_FLAG: u64 = 1 << 3;
const DEFAULT_HELICAL_SWEEP_TOLERANCE: f64 = 0.1;
const DEFAULT_PART_SPIRAL_SEGMENT_TURNS: f64 = 1.0;

pub(crate) fn transfer(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    entries: &[EntryRecord],
    program_version: Option<&str>,
) -> Result<BTreeSet<String>, CodecError> {
    let properties_by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let feature_ids = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .map(|object| (object.id.as_str(), feature_id(object)))
        .collect::<HashMap<_, _>>();
    let parent_by_member = objects
        .iter()
        .filter(|object| is_body(&object.type_name))
        .flat_map(|body| {
            properties_by_owner
                .get(body.id.as_str())
                .and_then(|properties| body_membership_property(properties))
                .into_iter()
                .flat_map(|property| &property.links)
                .filter_map(|link| link.object.as_deref())
                .map(move |member| (member, feature_id(body)))
        })
        .collect::<HashMap<_, _>>();
    let mut sketch_ids = objects
        .iter()
        .filter(|object| is_sketch(&object.type_name))
        .map(|object| {
            (
                object.id.as_str(),
                SketchId(format!("fcstd:design:sketch#{}", object.name)),
            )
        })
        .collect::<HashMap<_, _>>();
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let source_order = objects
        .iter()
        .map(|candidate| (feature_id(candidate), candidate.order))
        .collect::<HashMap<_, _>>();
    let (feature_ordinals, mut cycle_affected) = feature_ordinals(
        objects,
        &properties_by_owner,
        &parent_by_member,
        &source_order,
    );
    let ordinal_by_feature = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .map(|object| (feature_id(object), feature_ordinals[object.id.as_str()]))
        .collect::<HashMap<_, _>>();

    for object in objects {
        if !is_design_object(&object.type_name) {
            continue;
        }
        let owned = properties_by_owner
            .get(object.id.as_str())
            .cloned()
            .unwrap_or_default();
        let id = feature_id(object);
        let mut definition = if is_spreadsheet(&object.type_name) {
            ir.model.spreadsheets.push(append_spreadsheet(
                &mut ir.model.parameters,
                object,
                &owned,
            )?);
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: Vec::new(),
                active_child: None,
            }
        } else if is_body(&object.type_name) {
            body_definition(&owned, &feature_ids).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if is_datum(&object.type_name) {
            datum_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_sketch(&object.type_name) {
            let decoded = parse_sketch(object, &owned)?;
            let sketch = decoded.sketch;
            let sketch_id = sketch.id.clone();
            sketch_ids.insert(object.id.as_str(), sketch_id.clone());
            ir.model.sketches.push(sketch);
            ir.model.sketch_entities.extend(decoded.entities);
            ir.model.sketch_constraints.extend(decoded.constraints);
            ir.model.parameters.extend(decoded.parameters);
            FeatureDefinition::Sketch {
                sketch: Some(sketch_id),
            }
        } else if is_stored_geometry_feature(&object.type_name) {
            FeatureDefinition::StoredGeometry
        } else if object.type_name == "PartDesign::FeatureBase" {
            feature_base_definition(&owned, &feature_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_imported_geometry(&object.type_name) {
            imported_geometry_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_part_construction_geometry(&object.type_name) {
            part_construction_geometry_definition(&object.type_name, &owned, entries)
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                })
        } else if is_primitive(&object.type_name) {
            primitive_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_boolean(&object.type_name) {
            boolean_definition(&object.type_name, &owned)
                .or_else(|| {
                    (object.type_name != "PartDesign::Boolean")
                        .then(|| cached_shape_definition(&owned))
                        .flatten()
                })
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                })
        } else if is_loft(&object.type_name) {
            loft_definition(&object.type_name, &owned, &sketch_ids)
                .or_else(|| cached_shape_definition(&owned))
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                })
        } else if is_sweep(&object.type_name) {
            sweep_definition(&object.type_name, &owned, &sketch_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_helical_sweep(&object.type_name) {
            helical_sweep_definition(
                &object.type_name,
                &object.id,
                &owned,
                &sketch_ids,
                objects,
                &properties_by_owner,
            )
            .unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if matches!(object.type_name.as_str(), "Part::Helix" | "Part::Spiral") {
            parametric_helix_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_binder(&object.type_name) {
            binder_definition(&object.type_name, &owned, &feature_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_pattern(&object.type_name) {
            pattern_definition(
                &object.type_name,
                &object.id,
                &owned,
                &feature_ids,
                objects,
                &properties_by_owner,
                entries,
            )
            .unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if object.type_name == "Part::Scale" {
            scale_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if is_hole(&object.type_name) {
            hole_definition(&object.id, &owned, &sketch_ids, program_version).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_extrusion(&object.type_name) {
            let profile = match profile_ref(&object.id, &owned, &sketch_ids) {
                ProfileRef::Unresolved(_) => ["Profile", "Sketch", "Base", "Source"]
                    .iter()
                    .find_map(|name| property(&owned, name))
                    .map_or_else(
                        || ProfileRef::Unresolved(object.id.clone()),
                        |property| ProfileRef::Native(property.id.clone()),
                    ),
                profile => profile,
            };
            let profile_normal = profile_target(&owned)
                .and_then(|(_, target)| objects.iter().find(|object| object.id == target))
                .map(|profile_object| {
                    let profile_properties = properties_by_owner
                        .get(profile_object.id.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    sketch_frame(profile_properties).map(|frame| frame.1)
                })
                .transpose()?;
            extrusion_definition(
                &object.type_name,
                &owned,
                profile,
                profile_normal,
                &ir.model.sketches,
            )
            .unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if is_revolution(&object.type_name) {
            revolution_definition(&object.type_name, &object.id, &owned, &sketch_ids)
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                })
        } else if matches!(
            object.type_name.as_str(),
            "PartDesign::Thickness" | "Part::Thickness"
        ) {
            thickness_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if matches!(object.type_name.as_str(), "Part::Offset" | "Part::Offset2D") {
            offset_shape_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if matches!(
            object.type_name.as_str(),
            "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
        ) {
            derived_shape_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if object.type_name == "Part::RuledSurface" {
            ruled_surface_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if object.type_name == "Part::Section" {
            section_shape_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if object.type_name == "Part::Mirroring" {
            mirror_shape_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if object.type_name == "Part::ProjectOnSurface" {
            project_on_surface_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            })
        } else if object.type_name == "PartDesign::Draft" {
            draft_definition(&owned, objects, &properties_by_owner).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                }
            })
        } else if is_fillet(&object.type_name) {
            fillet_definition(&object.type_name, &owned, entries)
                .or_else(|| cached_shape_definition(&owned))
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                })
        } else if is_chamfer(&object.type_name) {
            chamfer_definition(&object.type_name, &owned, entries, program_version)
                .or_else(|| cached_shape_definition(&owned))
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone().into(),
                    parameters: native_parameters(&owned),
                })
        } else {
            FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            }
        };
        if cycle_affected.contains(object.id.as_str()) {
            definition = FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(&owned),
            };
        }
        let semantic_dependencies = match &definition {
            FeatureDefinition::Pattern { seeds, .. } => seeds
                .iter()
                .filter_map(|seed| match seed {
                    PatternSeed::Feature(feature) => Some(feature.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let definition = post_processed_definition(definition, &object.type_name, &owned);
        append_operation_parameters(&mut ir.model.parameters, object, &owned);
        let outputs = payloads
            .iter()
            .filter(|payload| owned.iter().any(|property| property.id == payload.property))
            .flat_map(|payload| {
                body_ids
                    .iter()
                    .filter(move |body| {
                        body.0
                            .starts_with(&crate::native::model_id("body", &payload.id, ""))
                    })
                    .cloned()
            })
            .collect();
        let cycle_affected = cycle_affected.contains(object.id.as_str());
        let dependencies = if cycle_affected {
            // The native object and property arenas retain the exact cycle.
            // A neutral edge would require a decoder-owned cycle break and
            // would change when persisted declaration order changes.
            Vec::new()
        } else {
            let mut dependency_objects = object
                .dependencies
                .iter()
                .filter(|_| !is_body(&object.type_name))
                .map(|dependency| (dependency.as_str(), true))
                .chain(
                    owned
                        .iter()
                        .flat_map(|property| &property.links)
                        .filter_map(|link| link.object.as_deref())
                        .map(|dependency| (dependency, false)),
                )
                .collect::<Vec<_>>();
            let mut seen_dependencies = BTreeSet::new();
            dependency_objects.retain(|(dependency, _)| seen_dependencies.insert(*dependency));
            let mut dependencies = dependency_objects
                .into_iter()
                .filter_map(|(dependency, declared)| {
                    feature_ids
                        .get(dependency)
                        .cloned()
                        .map(|feature| (feature, declared))
                })
                .filter(|(dependency, declared)| {
                    *declared
                        || ordinal_by_feature
                            .get(dependency)
                            .is_some_and(|ordinal| *ordinal < feature_ordinals[object.id.as_str()])
                })
                .map(|(dependency, _)| dependency)
                .collect::<Vec<_>>();
            for dependency in semantic_dependencies {
                if !dependencies.contains(&dependency)
                    && ordinal_by_feature
                        .get(&dependency)
                        .is_some_and(|ordinal| *ordinal < feature_ordinals[object.id.as_str()])
                {
                    dependencies.push(dependency);
                }
            }
            dependencies
        };
        ir.model.features.push(Feature {
            id,
            ordinal: feature_ordinals[object.id.as_str()],
            name: Some(object.name.clone()),
            suppressed: bool_property(&owned, "Suppressed"),
            dependencies,
            source_properties: feature_state(&owned),
            source_tag: Some(object.type_name.clone()),
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: Some(object.id.clone()),
        });
    }
    let initial_cycle_affected_features = objects
        .iter()
        .filter(|object| cycle_affected.contains(object.id.as_str()))
        .map(feature_id)
        .collect::<BTreeSet<_>>();
    let parameter_cycle_features = bind_parameter_dependencies(
        &mut ir.model.parameters,
        objects,
        &initial_cycle_affected_features,
    );
    for object in objects {
        if !parameter_cycle_features.contains(&feature_id(object)) {
            continue;
        }
        cycle_affected.insert(object.id.clone());
        if let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(object.id.as_str()))
        {
            feature.definition = FeatureDefinition::Native {
                kind: object.type_name.clone().into(),
                parameters: native_parameters(
                    properties_by_owner
                        .get(object.id.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                ),
            };
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
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    if !body_membership_carrier_is_valid(properties) {
        return None;
    }
    let children = body_membership_property(properties).map_or_else(Vec::new, |property| {
        property
            .links
            .iter()
            .filter_map(|link| link.object.as_deref())
            .filter_map(|target| feature_ids.get(target).cloned())
            .collect()
    });
    let active_child = match body_tip(properties, feature_ids) {
        BodyTipResolution::Valid(active_child) => active_child,
        BodyTipResolution::Invalid => return None,
    };
    Some(FeatureDefinition::TreeNode {
        role: FeatureTreeNodeRole::SolidBodies,
        children,
        active_child,
    })
}

enum BodyTipResolution {
    Valid(Option<FeatureId>),
    Invalid,
}

fn body_tip(
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> BodyTipResolution {
    let Some(property) = property(properties, "Tip") else {
        return BodyTipResolution::Valid(None);
    };
    if property.type_name != "App::PropertyLink" {
        return BodyTipResolution::Invalid;
    }
    match property.links.as_slice() {
        [] => BodyTipResolution::Valid(None),
        [link]
            if link.subelements.is_empty()
                && link.document.is_none()
                && link.document_attribute.is_none() =>
        {
            let Some(target) = link.object.as_deref().filter(|target| !target.is_empty()) else {
                return BodyTipResolution::Valid(None);
            };
            feature_ids
                .get(target)
                .cloned()
                .map(Some)
                .map_or(BodyTipResolution::Invalid, BodyTipResolution::Valid)
        }
        _ => BodyTipResolution::Invalid,
    }
}

fn feature_ordinals<'a>(
    objects: &'a [ObjectRecord],
    properties_by_owner: &HashMap<&'a str, Vec<&'a PropertyRecord>>,
    parent_by_member: &HashMap<&'a str, FeatureId>,
    source_order: &HashMap<FeatureId, usize>,
) -> (HashMap<&'a str, u64>, BTreeSet<String>) {
    let design_objects = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .collect::<Vec<_>>();
    let object_by_id = design_objects
        .iter()
        .map(|object| (object.id.as_str(), *object))
        .collect::<HashMap<_, _>>();
    let object_by_name = design_objects
        .iter()
        .map(|object| (object.name.as_str(), *object))
        .collect::<HashMap<_, _>>();
    let object_by_feature = design_objects
        .iter()
        .map(|object| (feature_id(object), object.id.as_str()))
        .collect::<HashMap<_, _>>();
    let mut source_ordinals = design_objects
        .iter()
        .map(|object| object.order as u64)
        .collect::<Vec<_>>();
    source_ordinals.sort_unstable();
    let mut emitted = BTreeSet::new();
    let mut ordinals = HashMap::new();
    let mut cycle_affected = BTreeSet::new();

    while emitted.len() < design_objects.len() {
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
                    .flat_map(|property| &property.values)
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
                            .links
                            .iter()
                            .map(move |link| (property.name.as_str(), link))
                    })
                    .filter_map(|(property_name, link)| {
                        Some((property_name, link.object.as_deref()?))
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
                            ) || source_order[&feature_id(dependency)] < object.order
                        })
                    })
                    .all(|(_, dependency)| emitted.contains(dependency))
            })
            .min_by_key(|object| object.order);
        let next = if let Some(next) = next {
            next
        } else {
            let remaining = design_objects
                .iter()
                .copied()
                .filter(|object| !emitted.contains(object.id.as_str()))
                .collect::<Vec<_>>();
            cycle_affected.extend(remaining.iter().map(|object| object.id.clone()));
            remaining
                .into_iter()
                .min_by_key(|object| object.order)
                .expect("the loop has at least one un-emitted design object")
        };
        let ordinal = source_ordinals[ordinals.len()];
        emitted.insert(next.id.as_str());
        ordinals.insert(next.id.as_str(), ordinal);
    }

    (ordinals, cycle_affected)
}

/// Apply an operation's shape-refinement and boolean-tolerance controls.
fn post_processed_definition(
    definition: FeatureDefinition,
    kind: &str,
    properties: &[&PropertyRecord],
) -> FeatureDefinition {
    match post_process_controls(properties) {
        PostProcessControlState::Absent => definition,
        PostProcessControlState::Valid {
            refine,
            fuzzy_tolerance,
        } => FeatureDefinition::PostProcess {
            operation: Box::new(definition),
            refine,
            fuzzy_tolerance,
        },
        PostProcessControlState::Malformed => FeatureDefinition::Native {
            kind: kind.to_owned().into(),
            parameters: native_parameters(properties),
        },
    }
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
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| property.name == name);
    let Some(property) = matches.next() else {
        return NamedProperty::Absent;
    };
    if matches.next().is_some() {
        return NamedProperty::Duplicate;
    }
    NamedProperty::Present(property)
}

fn unique_matching_property<'a, F>(
    properties: &[&'a PropertyRecord],
    predicate: F,
    label: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError>
where
    F: Fn(&PropertyRecord) -> bool,
{
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| predicate(property));
    let Some(property) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(malformed(format!(
            "spreadsheet has multiple {label} properties"
        )));
    }
    Ok(Some(property))
}

fn direct_spreadsheet_value<'a, 'input: 'a>(
    xml: &'a roxmltree::Document<'input>,
    tag: &str,
    property_id: &str,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let total = xml
        .descendants()
        .filter(|node| node.has_tag_name(tag))
        .count();
    if total == 0 {
        return Err(malformed(format!("{property_id} has no {tag} value")));
    }
    if total > 1 {
        return Err(malformed(format!(
            "{property_id} has multiple {tag} values"
        )));
    }
    xml.root_element()
        .children()
        .find(|node| node.has_tag_name(tag))
        .ok_or_else(|| malformed(format!("{property_id} has no direct {tag} value")))
}

fn append_spreadsheet(
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<Spreadsheet, CodecError> {
    let property = unique_matching_property(
        properties,
        |property| property.name == "cells" && property.type_name == "Spreadsheet::PropertySheet",
        "cells",
    )?
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "spreadsheet {} has no cells property",
            object.id
        ))
    })?;
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::malformed(format_args!("invalid spreadsheet {}: {error}", property.id))
    })?;
    let cells = direct_spreadsheet_value(&xml, "Cells", &property.id)?;
    let declared = cells
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!("{} has invalid Cells Count", property.id))
        })?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::malformed(format_args!(
            "{} cell count exceeds {MAX_SKETCH_RECORDS}",
            property.id
        )));
    }
    let records = cells
        .children()
        .filter(|node| node.has_tag_name("Cell"))
        .collect::<Vec<_>>();
    if declared != records.len() {
        return Err(CodecError::malformed(format_args!(
            "{} declares {declared} cells but contains {}",
            property.id,
            records.len()
        )));
    }
    let mut cell_ids = Vec::with_capacity(records.len());
    let mut merged_ranges: Vec<SpreadsheetRange> = Vec::new();
    for (index, cell) in records.into_iter().enumerate() {
        let address = cell.attribute("address").ok_or_else(|| {
            CodecError::malformed(format_args!("{} cell has no address", property.id))
        })?;
        let content = cell.attribute("content").unwrap_or_default();
        let name = cell.attribute("alias").unwrap_or(address);
        let mut retained = BTreeMap::from([("address".into(), address.to_owned())]);
        for attribute in [
            "alias",
            "alignment",
            "style",
            "foregroundColor",
            "backgroundColor",
            "displayUnit",
            "rowSpan",
            "colSpan",
        ] {
            if let Some(value) = cell.attribute(attribute) {
                retained.insert(attribute.into(), value.to_owned());
            }
        }
        let id = ParameterId(format!(
            "fcstd:design:parameter#{}:cell:{address}",
            object.name
        ));
        let cell_address = CellAddress::parse(address).ok_or_else(|| {
            CodecError::malformed(format_args!("{} cell has invalid address", property.id))
        })?;
        cell_ids.push(SpreadsheetCell {
            address: cell_address,
            parameter: id.clone(),
        });
        if let Some(range) = merged_range(cell)? {
            if !merged_ranges
                .iter()
                .any(|existing| existing.contains(range.start()))
            {
                merged_ranges.push(range);
            }
        }
        parameters.push(DesignParameter {
            id,
            owner: Some(feature_id(object)),
            ordinal: index as u32,
            name: name.to_owned(),
            expression: content.to_owned(),
            display: None,
            value: (!content.starts_with('='))
                .then(|| content.parse::<f64>().ok().map(ParameterValue::Real))
                .flatten(),
            dependencies: Vec::new(),
            properties: retained,
            pmi: None,
            native_ref: Some(property.id.clone()),
        });
    }
    Ok(Spreadsheet {
        id: SpreadsheetId(format!("fcstd:design:spreadsheet#{}", object.name)),
        feature: feature_id(object),
        cells: cell_ids,
        column_widths: spreadsheet_dimensions(
            properties,
            "Spreadsheet::PropertyColumnWidths",
            "columnWidths",
            "ColumnInfo",
            "Column",
            "width",
        )?,
        row_heights: spreadsheet_dimensions(
            properties,
            "Spreadsheet::PropertyRowHeights",
            "rowHeights",
            "RowInfo",
            "Row",
            "height",
        )?,
        merged_ranges,
        native_ref: Some(object.id.clone()),
    })
}

fn spreadsheet_dimensions(
    properties: &[&PropertyRecord],
    type_name: &str,
    property_name: &str,
    container: &str,
    element: &str,
    value_name: &str,
) -> Result<Vec<SpreadsheetDimension>, CodecError> {
    let Some(property) = unique_matching_property(
        properties,
        |property| property.name == property_name && property.type_name == type_name,
        property_name,
    )?
    else {
        return Ok(Vec::new());
    };
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::malformed(format_args!(
            "invalid spreadsheet dimension {}: {error}",
            property.id
        ))
    })?;
    let root = direct_spreadsheet_value(&xml, container, &property.id)?;
    let records = root
        .children()
        .filter(|node| node.has_tag_name(element))
        .collect::<Vec<_>>();
    let declared = root
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!("{} has invalid dimension count", property.id))
        })?;
    if declared != records.len() || declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::malformed(format_args!(
            "{} dimension count does not match its records",
            property.id
        )));
    }
    records
        .into_iter()
        .map(|record| {
            let name = record.attribute("name").ok_or_else(|| {
                CodecError::malformed(format_args!("{} dimension has no name", property.id))
            })?;
            let pixels = record
                .attribute(value_name)
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "{} dimension has invalid size",
                        property.id
                    ))
                })?;
            let index = if element == "Column" {
                CellAddress::parse(&format!("{name}1"))
                    .map(|address| address.col())
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "{} dimension has invalid column {name}",
                            property.id
                        ))
                    })?
            } else {
                name.parse::<u32>()
                    .ok()
                    .filter(|row| *row > 0)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "{} dimension has invalid row {name}",
                            property.id
                        ))
                    })?
            };
            Ok(SpreadsheetDimension { index, pixels })
        })
        .collect()
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

fn range_contains_address(range: &SpreadsheetRange, address: &str) -> bool {
    CellAddress::parse(address).is_some_and(|address| range.contains(address))
}

fn append_operation_parameters(
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) {
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
    for property in properties
        .iter()
        .copied()
        .filter(|property| NAMES.contains(&property.name.as_str()))
    {
        if parameters.iter().any(|parameter| {
            parameter.owner.as_ref() == Some(&feature_id(object)) && parameter.name == property.name
        }) {
            continue;
        }
        let Some(value) = scalar_value(property) else {
            continue;
        };
        let expression = expression_binding(properties, &property.name);
        let is_angle = property.type_name.contains("Angle");
        let mut retained = BTreeMap::new();
        if let Some((native_ref, _)) = &expression {
            retained.insert("expression_native_ref".into(), native_ref.clone());
        }
        parameters.push(DesignParameter {
            id: ParameterId(format!(
                "fcstd:design:parameter#{}:{}",
                object.name, property.name
            )),
            owner: Some(feature_id(object)),
            ordinal: property.order as u32,
            name: property.name.clone(),
            expression: expression.map_or_else(
                || scalar_text(property).unwrap_or_else(|| value.to_string()),
                |(_, expression)| expression,
            ),
            display: None,
            value: Some(if is_angle {
                ParameterValue::Angle(cadmpeg_ir::features::Angle(value.to_radians()))
            } else {
                ParameterValue::Length(Length(value))
            }),
            dependencies: Vec::new(),
            properties: retained,
            pmi: None,
            native_ref: Some(property.id.clone()),
        });
    }
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
    Err(CodecError::malformed(format_args!(
        "sketch Geometry record {ordinal} declares {kind} but carries <{}>, expected <{expected}>",
        carrier.tag_name().name()
    )))
}

fn external_geometry_metadata(
    node: roxmltree::Node<'_, '_>,
    ordinal: usize,
) -> Result<(Option<String>, bool), CodecError> {
    let extensions = node
        .children()
        .filter(|child| child.has_tag_name("GeoExtensions"))
        .flat_map(|container| container.children())
        .filter(|child| {
            child.has_tag_name("GeoExtension")
                && child.attribute("type") == Some("Sketcher::ExternalGeometryExtension")
        })
        .collect::<Vec<_>>();
    if extensions.len() > 1 {
        return Err(malformed(format!(
            "sketch ExternalGeo Geometry record {ordinal} has multiple ExternalGeometryExtension values"
        )));
    }
    let extension = extensions.first().copied();
    let extension_ref = extension.and_then(|extension| extension.attribute("Ref"));
    let geometry_ref = node.attribute("ref");
    if let (Some(extension_ref), Some(geometry_ref)) = (extension_ref, geometry_ref) {
        if extension_ref != geometry_ref {
            return Err(malformed(format!(
                "sketch ExternalGeo Geometry record {ordinal} has conflicting Ref and ref values"
            )));
        }
    }
    let reference = extension_ref
        .or(geometry_ref)
        .and_then(|value| (!value.is_empty()).then(|| value.to_owned()));

    let extension_flags = extension
        .and_then(|extension| extension.attribute("Flags"))
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                malformed(format!(
                    "sketch ExternalGeo Geometry record {ordinal} has invalid Flags"
                ))
            })
        })
        .transpose()?;
    let geometry_flags = node
        .attribute("flags")
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                malformed(format!(
                    "sketch ExternalGeo Geometry record {ordinal} has invalid flags"
                ))
            })
        })
        .transpose()?;
    if let (Some(extension_flags), Some(geometry_flags)) = (extension_flags, geometry_flags) {
        if extension_flags != geometry_flags {
            return Err(malformed(format!(
                "sketch ExternalGeo Geometry record {ordinal} has conflicting Flags and flags values"
            )));
        }
    }
    let flags = extension_flags.or(geometry_flags).unwrap_or_default();
    Ok((reference, flags & EXTERNAL_GEOMETRY_MISSING_FLAG != 0))
}

fn validate_external_geo_prefix(
    records: &[roxmltree::Node<'_, '_>],
    owner: &str,
) -> Result<(), CodecError> {
    if records.len() < EXTERNAL_GEO_AXIS_COUNT {
        return Err(malformed(format!(
            "{owner} must contain the two reserved ExternalGeo axis records"
        )));
    }
    for (index, (expected_value, expected_label)) in
        [(-1_i64, "-1"), (-2_i64, "-2")].into_iter().enumerate()
    {
        let node = records[index];
        let id = node.attribute("id").ok_or_else(|| {
            malformed(format!(
                "{owner} reserved ExternalGeo record {} has no id",
                index + 1
            ))
        })?;
        if id.parse::<i64>().ok() != Some(expected_value) {
            return Err(malformed(format!(
                "{owner} reserved ExternalGeo record {} has id {id}, expected {expected_label}",
                index + 1
            )));
        }
        let (reference, _) = external_geometry_metadata(node, index + 1)?;
        if reference.is_some() {
            return Err(malformed(format!(
                "{owner} reserved ExternalGeo record {} has an external reference",
                index + 1
            )));
        }
    }
    Ok(())
}

fn external_link_key(reference: &crate::native::LinkTarget) -> Option<String> {
    let object = crate::native::id_key(reference.object.as_deref()?);
    let subelement = reference.subelements.first()?;
    Some(format!("{object}.{subelement}"))
}

fn external_link_indices(
    references: Option<&PropertyRecord>,
) -> Result<HashMap<String, usize>, CodecError> {
    let mut indices = HashMap::new();
    if let Some(references) = references {
        for (index, reference) in references.links.iter().enumerate() {
            let Some(key) = external_link_key(reference) else {
                continue;
            };
            if indices.insert(key.clone(), index).is_some() {
                return Err(malformed(format!(
                    "sketch ExternalGeometry links contain duplicate key {key}"
                )));
            }
        }
    }
    Ok(indices)
}

fn parse_sketch(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<SketchTransfer, CodecError> {
    let id = SketchId(format!("fcstd:design:sketch#{}", object.name));
    let mut entities = Vec::new();
    let mut matched_references = BTreeSet::new();
    if let Some(geometry) = property(properties, "Geometry") {
        if geometry.type_name != "Part::PropertyGeometryList" {
            return Err(CodecError::malformed(format_args!(
                "{} has runtime type {}, expected Part::PropertyGeometryList",
                geometry.id, geometry.type_name
            )));
        }
        let xml = roxmltree::Document::parse(&geometry.raw_xml).map_err(|error| {
            CodecError::malformed(format_args!(
                "invalid sketch geometry {}: {error}",
                geometry.id
            ))
        })?;
        let records = direct_counted_records(&xml, "GeometryList", "Geometry", &geometry.id)?;
        for (index, node) in records.into_iter().enumerate() {
            let carrier = sketch_carrier(node);
            if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
                validate_sketch_carrier(kind, carrier, index + 1)?;
            }
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown")
                .to_owned();
            let attributes = carrier.map_or_else(BTreeMap::new, |child| {
                child
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect()
            });
            let geometry_value = carrier
                .and_then(|carrier| sketch_nurbs(&native_kind, carrier))
                .unwrap_or_else(|| sketch_geometry(&native_kind, &attributes));
            entities.push(
                SketchEntity::new(
                    SketchEntityId(format!(
                        "fcstd:design:sketch-entity#{}:{}",
                        object.name,
                        index + 1
                    )),
                    id.clone(),
                    geometry_value,
                )
                .with_construction(node.descendants().any(|child| {
                    child.has_tag_name("Construction")
                        && child.attribute("value").is_some_and(|value| value != "0")
                }))
                .with_native_ref(Some(geometry.id.clone())),
            );
        }
    }
    if let Some(external_geometry) = property(properties, "ExternalGeo") {
        if external_geometry.type_name != "Part::PropertyGeometryList" {
            return Err(CodecError::malformed(format_args!(
                "{} has runtime type {}, expected Part::PropertyGeometryList",
                external_geometry.id, external_geometry.type_name
            )));
        }
        let xml = roxmltree::Document::parse(&external_geometry.raw_xml).map_err(|error| {
            CodecError::malformed(format_args!(
                "invalid external sketch geometry {}: {error}",
                external_geometry.id
            ))
        })?;
        let records =
            direct_counted_records(&xml, "GeometryList", "Geometry", &external_geometry.id)?;
        validate_external_geo_prefix(&records, &external_geometry.id)?;
        let references = property(properties, "ExternalGeometry");
        if let Some(references) = references {
            if references.type_name != "App::PropertyLinkSubList" {
                return Err(malformed(format!(
                    "{} has runtime type {}, expected App::PropertyLinkSubList",
                    references.id, references.type_name
                )));
            }
        }
        let link_indices = external_link_indices(references)?;
        for (external_index, node) in records
            .into_iter()
            .skip(EXTERNAL_GEO_AXIS_COUNT)
            .enumerate()
        {
            let (cache_reference, missing) = external_geometry_metadata(node, external_index + 3)?;
            let reference_index = cache_reference
                .as_deref()
                .and_then(|cache_reference| link_indices.get(cache_reference).copied());
            if let (Some(cache_reference), None) = (cache_reference.as_deref(), reference_index) {
                if !missing {
                    return Err(malformed(format!(
                        "sketch ExternalGeo Geometry record {} reference {cache_reference} has no matching ExternalGeometry link",
                        external_index + 3
                    )));
                }
            }
            if let Some(reference_index) = reference_index {
                matched_references.insert(reference_index);
            }
            let carrier = sketch_carrier(node);
            if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
                validate_sketch_carrier(kind, carrier, external_index + 3)?;
            }
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown")
                .to_owned();
            let attributes = carrier.map_or_else(BTreeMap::new, |child| {
                child
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect()
            });
            let geometry = carrier
                .and_then(|carrier| sketch_nurbs(&native_kind, carrier))
                .unwrap_or_else(|| sketch_geometry(&native_kind, &attributes));
            entities.push(
                SketchEntity::new(
                    SketchEntityId(format!(
                        "fcstd:design:sketch-entity#{}:external:{external_index}",
                        object.name
                    )),
                    id.clone(),
                    geometry,
                )
                .with_construction(true)
                .with_native_ref(Some(external_geometry.id.clone()))
                .with_geometry_ref(references.map(|property| property.id.clone()))
                .with_endpoint_refs(
                    reference_index
                        .and_then(|index| references.and_then(|property| property.links.get(index)))
                        .map(|reference| reference.subelements.clone())
                        .unwrap_or_default(),
                ),
            );
        }
    }
    if let Some(references) = property(properties, "ExternalGeometry") {
        for (external_index, reference) in references.links.iter().enumerate() {
            if matched_references.contains(&external_index) {
                continue;
            }
            let Some(target_object) = reference.object.clone() else {
                continue;
            };
            let numeric_suffix = format!(":external:{external_index}");
            let entity_suffix = if entities
                .iter()
                .any(|entity| entity.id().0.ends_with(&numeric_suffix))
            {
                format!(":external-link:{external_index}")
            } else {
                numeric_suffix
            };
            entities.push(
                SketchEntity::new(
                    SketchEntityId(format!(
                        "fcstd:design:sketch-entity#{}{}",
                        object.name, entity_suffix
                    )),
                    id.clone(),
                    SketchGeometry::ExternalReference {
                        document: reference.document.clone(),
                        object: target_object,
                        subelements: reference.subelements.clone(),
                    },
                )
                .with_construction(true)
                .with_native_ref(Some(references.id.clone()))
                .with_geometry_ref(Some(references.id.clone()))
                .with_endpoint_refs(reference.subelements.clone()),
            );
        }
    }
    let (horizontal_axis, vertical_axis, root_point) = builtin_reference_usage(properties);
    if horizontal_axis {
        entities.push(
            SketchEntity::new(
                SketchEntityId(format!(
                    "fcstd:design:sketch-entity#{}:reference-horizontal-axis",
                    object.name
                )),
                id.clone(),
                SketchGeometry::ReferenceLine {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                },
            )
            .with_construction(true)
            .with_native_ref(Some(object.id.clone())),
        );
    }
    if vertical_axis {
        entities.push(
            SketchEntity::new(
                SketchEntityId(format!(
                    "fcstd:design:sketch-entity#{}:reference-vertical-axis",
                    object.name
                )),
                id.clone(),
                SketchGeometry::ReferenceLine {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(0.0, 1.0),
                },
            )
            .with_construction(true)
            .with_native_ref(Some(object.id.clone())),
        );
    }
    if root_point {
        entities.push(
            SketchEntity::new(
                SketchEntityId(format!(
                    "fcstd:design:sketch-entity#{}:reference-root-point",
                    object.name
                )),
                id.clone(),
                SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            )
            .with_construction(true)
            .with_native_ref(Some(object.id.clone())),
        );
    }
    let (constraints, parameters) = parse_constraints(object, properties, &id, &entities)?;
    let profiles = build_profiles(&entities, &constraints);
    let (origin, normal, u_axis) = sketch_frame(properties)?;
    Ok(SketchTransfer {
        sketch: Sketch {
            id,
            name: Some(object.name.clone()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin,
                normal,
                u_axis,
            },
            profiles,
            native_ref: Some(object.id.clone()),
        },
        entities,
        constraints,
        parameters,
    })
}

fn builtin_reference_usage(properties: &[&PropertyRecord]) -> (bool, bool, bool) {
    let Some(property) = property(properties, "Constraints") else {
        return (false, false, false);
    };
    let Ok(xml) = roxmltree::Document::parse(&property.raw_xml) else {
        return (false, false, false);
    };
    let mut horizontal = false;
    let mut vertical = false;
    let mut root = false;
    for node in xml
        .descendants()
        .filter(|node| node.has_tag_name("Constrain"))
    {
        let type_code = int_attr(node, "Type");
        let Ok(operands) = constraint_operands(node) else {
            continue;
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
    (horizontal, vertical, root)
}

fn sketch_nurbs(kind: &str, node: roxmltree::Node<'_, '_>) -> Option<SketchGeometry> {
    if !matches!(kind, "Part::GeomBSplineCurve" | "BSplineCurve")
        && !node.has_tag_name("BSplineCurve")
    {
        return None;
    }
    let degree = node.attribute("Degree")?.parse::<u32>().ok()?;
    let periodic = matches!(node.attribute("IsPeriodic")?, "1" | "true" | "True");
    let pole_count = node.attribute("PolesCount")?.parse::<usize>().ok()?;
    let knot_count = node.attribute("KnotsCount")?.parse::<usize>().ok()?;
    if pole_count == 0
        || knot_count == 0
        || pole_count > MAX_SKETCH_RECORDS
        || knot_count > MAX_SKETCH_RECORDS
    {
        return None;
    }
    let poles = node
        .children()
        .filter(|child| child.has_tag_name("Pole"))
        .map(|pole| {
            Some((
                Point2::new(
                    pole.attribute("X")?.parse().ok()?,
                    pole.attribute("Y")?.parse().ok()?,
                ),
                pole.attribute("Z")?.parse::<f64>().ok()?,
                pole.attribute("Weight")?.parse::<f64>().ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let knots = node
        .children()
        .filter(|child| child.has_tag_name("Knot"))
        .map(|knot| {
            Some((
                knot.attribute("Value")?.parse::<f64>().ok()?,
                knot.attribute("Mult")?.parse::<usize>().ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    if poles.len() != pole_count
        || knots.len() != knot_count
        || degree == 0
        || usize::try_from(degree)
            .ok()
            .is_none_or(|degree| degree >= pole_count)
        || poles.iter().any(|(point, z, weight)| {
            !point.u.is_finite()
                || !point.v.is_finite()
                || !z.is_finite()
                || z.abs() > f64::EPSILON
                || !weight.is_finite()
                || *weight <= 0.0
        })
        || knots.iter().any(|(value, multiplicity)| {
            !value.is_finite() || *multiplicity == 0 || *multiplicity > MAX_SKETCH_RECORDS
        })
        || knots.windows(2).any(|pair| pair[0].0 >= pair[1].0)
    {
        return None;
    }
    let expanded_count = knots.iter().try_fold(0_usize, |count, (_, multiplicity)| {
        count.checked_add(*multiplicity)
    })?;
    if expanded_count > MAX_SKETCH_RECORDS {
        return None;
    }
    if !periodic
        && expanded_count
            != pole_count
                .checked_add(usize::try_from(degree).ok()?)?
                .checked_add(1)?
    {
        return None;
    }
    let full_knots = knots
        .iter()
        .flat_map(|(value, multiplicity)| std::iter::repeat_n(*value, *multiplicity))
        .collect();
    let control_points = poles.iter().map(|(point, _, _)| *point).collect();
    let weights = poles
        .iter()
        .map(|(_, _, weight)| *weight)
        .collect::<Vec<_>>();
    Some(SketchGeometry::Nurbs {
        degree,
        knots: full_knots,
        control_points,
        weights: weights
            .iter()
            .any(|weight| (*weight - 1.0).abs() > f64::EPSILON)
            .then_some(weights),
        periodic,
    })
}

fn sketch_frame(properties: &[&PropertyRecord]) -> Result<(Point3, Vector3, Vector3), CodecError> {
    validate_sketch_placement(properties)?;
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
    let value = property(properties, "Placement")
        .or_else(|| property(properties, "AttachmentOffset"))
        .and_then(|property| {
            property
                .values
                .iter()
                .find(|value| value.tag == "PropertyPlacement")
        })?;
    let component = |name: &str| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
    };
    let quaternion = if value.attributes.contains_key("A") {
        let axis = [component("Ox")?, component("Oy")?, component("Oz")?];
        let angle = component("A")?;
        let axis_norm = axis
            .iter()
            .map(|component| component * component)
            .sum::<f64>()
            .sqrt();
        let (axis_x, axis_y, axis_z) = if axis_norm.is_finite() && axis_norm > 0.0 {
            (
                axis[0] / axis_norm,
                axis[1] / axis_norm,
                axis[2] / axis_norm,
            )
        } else if axis_norm == 0.0 {
            (0.0, 0.0, 1.0)
        } else {
            return None;
        };
        let half_angle = angle / 2.0;
        let scale = half_angle.sin();
        [
            axis_x * scale,
            axis_y * scale,
            axis_z * scale,
            half_angle.cos(),
        ]
    } else {
        [
            component("Q0")?,
            component("Q1")?,
            component("Q2")?,
            component("Q3")?,
        ]
    };
    let norm = quaternion
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    Some((
        Point3::new(component("Px")?, component("Py")?, component("Pz")?),
        rotate_vector(quaternion, [0.0, 0.0, 1.0]),
        rotate_vector(quaternion, [1.0, 0.0, 0.0]),
        rotate_vector(quaternion, [0.0, 1.0, 0.0]),
    ))
}

fn validate_sketch_placement(properties: &[&PropertyRecord]) -> Result<(), CodecError> {
    let Some(property) =
        property(properties, "Placement").or_else(|| property(properties, "AttachmentOffset"))
    else {
        return Ok(());
    };
    let error = if property.type_name != "App::PropertyPlacement" {
        Some(format!(
            "sketch {} placement carrier has runtime type {}",
            property.name, property.type_name
        ))
    } else if property.values.len() != 1 || property.values[0].tag != "PropertyPlacement" {
        Some(format!(
            "sketch {} placement carrier requires one PropertyPlacement value",
            property.name
        ))
    } else if placement_frame(properties).is_none() {
        Some(format!(
            "sketch {} placement carrier has incomplete or invalid components",
            property.name
        ))
    } else {
        None
    };
    if let Some(message) = error {
        return Err(CodecError::Malformed(message));
    }
    Ok(())
}

fn rotate_vector(quaternion: [f64; 4], vector: [f64; 3]) -> Vector3 {
    let [x, y, z, w] = quaternion;
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Vector3::new(vector[0], vector[1], vector[2]);
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    let [vx, vy, vz] = vector;
    Vector3::new(
        (1.0 - 2.0 * (y * y + z * z)) * vx
            + 2.0 * (x * y - z * w) * vy
            + 2.0 * (x * z + y * w) * vz,
        2.0 * (x * y + z * w) * vx
            + (1.0 - 2.0 * (x * x + z * z)) * vy
            + 2.0 * (y * z - x * w) * vz,
        2.0 * (x * z - y * w) * vx
            + 2.0 * (y * z + x * w) * vy
            + (1.0 - 2.0 * (x * x + y * y)) * vz,
    )
}

fn feature_state(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
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
    properties
        .iter()
        .filter(|property| STATE_NAMES.contains(&property.name.as_str()))
        .map(|property| {
            let value = property
                .links
                .first()
                .and_then(|link| link.object.clone())
                .or_else(|| scalar_text(property))
                .unwrap_or_else(|| property.raw_xml.clone());
            (property.name.clone(), value)
        })
        .collect()
}

fn bool_property(properties: &[&PropertyRecord], name: &str) -> Option<bool> {
    let value = scalar_text(property(properties, name)?)?;
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
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
    let attributes = direct_root_attributes(property, "Integer")?;
    let value = attributes.get("value")?.parse::<i64>().ok()?;
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

fn float_selector(properties: &[&PropertyRecord], name: &str, absent_default: f64) -> Option<f64> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != "App::PropertyFloat" {
        return None;
    }
    let value = direct_root_attributes(property, "Float")?
        .get("value")?
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

fn float_constraint_selector(
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: f64,
) -> Option<f64> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != "App::PropertyFloatConstraint" {
        return None;
    }
    let value = direct_root_attributes(property, "Float")?
        .get("value")?
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

fn quantity_constraint_selector(
    properties: &[&PropertyRecord],
    name: &str,
    absent_default: f64,
) -> Option<f64> {
    let Some(property) = property(properties, name) else {
        return Some(absent_default);
    };
    if property.type_name != "App::PropertyQuantityConstraint" {
        return None;
    }
    let value = direct_root_attributes(property, "Float")?
        .get("value")?
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

fn direct_bool_value(property: &PropertyRecord) -> Option<bool> {
    if property.type_name != "App::PropertyBool" {
        return None;
    }
    let value = direct_root_attributes(property, "Bool")?.remove("value")?;
    match value.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn direct_fuzzy_tolerance(property: &PropertyRecord) -> Option<FuzzyTolerance> {
    if property.type_name != "App::PropertyFloatConstraint" {
        return None;
    }
    let value = direct_root_attributes(property, "Float")?
        .remove("value")?
        .parse::<f64>()
        .ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(if value < 0.0 {
        FuzzyTolerance::Automatic
    } else if value == 0.0 {
        FuzzyTolerance::KernelDefault
    } else {
        FuzzyTolerance::Explicit(value)
    })
}

fn parse_constraints(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Result<(Vec<SketchConstraint>, Vec<DesignParameter>), CodecError> {
    let Some(property) = property(properties, "Constraints") else {
        return Ok((Vec::new(), Vec::new()));
    };
    if property.type_name != "Sketcher::PropertyConstraintList" {
        return Err(CodecError::malformed(format_args!(
            "{} has runtime type {}, expected Sketcher::PropertyConstraintList",
            property.id, property.type_name
        )));
    }
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::malformed(format_args!(
            "invalid sketch constraints {}: {error}",
            property.id
        ))
    })?;
    let records = direct_counted_records(&xml, "ConstraintList", "Constrain", &property.id)?;
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
        let operands = constraint_operands(node).map_err(|message| {
            CodecError::malformed(format_args!(
                "{} constraint {}: {message}",
                property.id,
                index + 1
            ))
        })?;
        let resolve = |entity, position| {
            if type_code == Some(9) {
                match entity {
                    -1 => return resolve_operand(-1, 0, entities),
                    -2 => return resolve_operand(-2, 0, entities),
                    _ => {}
                }
            }
            resolve_operand(entity, position, entities)
        };
        let mut resolved = operands
            .iter()
            .filter_map(|(entity, position)| resolve(*entity, *position))
            .collect::<Vec<_>>();
        let all_resolved = resolved.len() == operands.len();
        if matches!(type_code, Some(7 | 8)) && operands.len() == 1 && resolved.len() == 1 {
            if let Some(root) = entities
                .iter()
                .find(|entity| entity.id().0.ends_with(":reference-root-point"))
            {
                resolved.insert(0, SketchLocus::Entity(root.id().clone()));
            }
        }
        let parameter = if matches!(type_code, Some(6..=9 | 11 | 16 | 18 | 19)) {
            node.attribute("Value")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| {
                    let id = ParameterId(format!(
                        "fcstd:design:parameter#{}:constraint:{}",
                        object.name,
                        index + 1
                    ));
                    let value = match type_code {
                        Some(9) => ParameterValue::Angle(cadmpeg_ir::features::Angle(value)),
                        Some(16 | 19) => ParameterValue::Real(value),
                        _ => ParameterValue::Length(Length(value)),
                    };
                    let path = format!("Constraints[{index}]");
                    let expression = expression_binding(properties, &path);
                    let mut parameter_properties = [(
                        "is_driving".into(),
                        node.attribute("IsDriving").unwrap_or("1").to_owned(),
                    )]
                    .into_iter()
                    .collect::<BTreeMap<_, _>>();
                    if let Some(name) = node.attribute("Name").filter(|name| !name.is_empty()) {
                        parameter_properties.insert("source_name".into(), name.to_owned());
                    }
                    if let Some((native_ref, _)) = &expression {
                        parameter_properties
                            .insert("expression_native_ref".into(), native_ref.clone());
                    }
                    parameters.push(DesignParameter {
                        id: id.clone(),
                        owner: Some(feature_id(object)),
                        ordinal: index as u32,
                        name: format!("Constraint{}", index + 1),
                        expression: expression.map_or_else(
                            || node.attribute("Value").unwrap_or_default().to_owned(),
                            |(_, expression)| expression,
                        ),
                        display: None,
                        value: Some(value),
                        dependencies: Vec::new(),
                        properties: parameter_properties,
                        pmi: None,
                        native_ref: Some(property.id.clone()),
                    });
                    id
                })
        } else {
            None
        };
        let internal_alignment = || {
            use cadmpeg_ir::sketches::SketchInternalAlignment as Alignment;
            let index = || {
                node.attribute("InternalAlignmentIndex")
                    .and_then(|value| value.parse::<u32>().ok())
            };
            let alignment = match int_attr(node, "InternalAlignmentType")? {
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
            };
            Some(SketchConstraintDefinition::InternalAlignment {
                helper: locus_entity(resolved.first()?).clone(),
                parent: locus_entity(resolved.get(1)?).clone(),
                alignment,
            })
        };
        let grouped_geometry = || {
            if !all_resolved || resolved.is_empty() {
                return None;
            }
            match type_code {
                Some(20) => Some(SketchConstraintDefinition::Group {
                    elements: resolved.clone(),
                }),
                Some(21) => {
                    let metadata = node.attribute("MetaData")?;
                    let metadata: serde_json::Value = serde_json::from_str(metadata).ok()?;
                    Some(SketchConstraintDefinition::Text {
                        elements: resolved.clone(),
                        text: metadata.get("text")?.as_str()?.to_owned(),
                        font: metadata
                            .get("font")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                        is_text_height: metadata
                            .get("isTextHeight")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true),
                    })
                }
                _ => None,
            }
        };
        let midpoint =
            || type_code.and_then(|type_code| midpoint_constraint(type_code, &operands, entities));
        let definition = (type_code == Some(15) && all_resolved)
            .then(internal_alignment)
            .flatten()
            .or_else(grouped_geometry)
            .or_else(midpoint)
            .or_else(|| {
                type_code.and_then(|type_code| {
                    neutral_constraint(type_code, &resolved, parameter.clone(), all_resolved)
                })
            })
            .unwrap_or_else(|| SketchConstraintDefinition::Native {
                native_kind,
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: resolved.iter().map(locus_entity).cloned().collect(),
                parameter,
                operands: operands
                    .iter()
                    .filter_map(|(entity, position)| {
                        if *entity < 0 || resolve(*entity, *position).is_none() {
                            Some(SketchNativeOperand {
                                native_kind: format!("position:{position}"),
                                native_field: None,
                                native_role: None,
                                object_index: u32::try_from(*entity).unwrap_or(u32::MAX),
                                native_ref: None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect(),
            });
        constraints.push(SketchConstraint {
            id: SketchConstraintId(format!(
                "fcstd:design:sketch-constraint#{}:{}",
                object.name,
                index + 1
            )),
            sketch: sketch.clone(),
            definition,
            name: nonempty_attr(node, "Name"),
            driving: bool_attr(node, "IsDriving"),
            active: bool_attr(node, "IsActive"),
            virtual_space: bool_attr(node, "IsInVirtualSpace"),
            visible: bool_attr(node, "IsVisible"),
            orientation: node
                .attribute("Orientation")
                .and_then(|value| value.parse().ok()),
            label_distance: finite_attr(node, "LabelDistance"),
            label_position: finite_attr(node, "LabelPosition"),
            metadata: nonempty_attr(node, "MetaData"),
            native_ref: Some(property.id.clone()),
        });
    }
    Ok((constraints, parameters))
}

fn midpoint_constraint(
    kind: i64,
    operands: &[(i64, i64)],
    entities: &[SketchEntity],
) -> Option<SketchConstraintDefinition> {
    if kind != 1 || operands.len() != 2 {
        return None;
    }
    for (midpoint_index, point_index) in [(0, 1), (1, 0)] {
        let (entity, position) = operands[midpoint_index];
        if position != 3 {
            continue;
        }
        let midpoint = resolve_operand(entity, position, entities)?;
        let bounded = entities
            .iter()
            .find(|candidate| candidate.id() == locus_entity(&midpoint))?;
        if !matches!(bounded.geometry, SketchGeometry::Line { .. }) {
            continue;
        }
        let (entity, position) = operands[point_index];
        let point = resolve_operand(entity, position, entities)?;
        let point_entity = entities
            .iter()
            .find(|candidate| candidate.id() == locus_entity(&point))?;
        if !matches!(point_entity.geometry, SketchGeometry::Point { .. }) {
            continue;
        }
        return Some(SketchConstraintDefinition::Midpoint {
            point,
            entity: bounded.id().clone(),
        });
    }
    None
}

fn bool_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<bool> {
    match node.attribute(name)?.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn finite_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn nonempty_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn expression_binding(properties: &[&PropertyRecord], path: &str) -> Option<(String, String)> {
    let engine = property(properties, "ExpressionEngine")?;
    engine
        .values
        .iter()
        .find(|value| {
            value.tag == "Expression"
                && value
                    .attributes
                    .get("path")
                    .is_some_and(|value| value == path)
        })
        .and_then(|value| {
            Some((
                engine.id.clone(),
                value.attributes.get("expression")?.clone(),
            ))
        })
}

fn bind_parameter_dependencies(
    parameters: &mut Vec<DesignParameter>,
    objects: &[ObjectRecord],
    cycle_affected_features: &BTreeSet<FeatureId>,
) -> BTreeSet<FeatureId> {
    let object_names = objects
        .iter()
        .map(|object| (feature_id(object), object.name.as_str()))
        .collect::<HashMap<_, _>>();
    let candidates = parameters
        .iter()
        .map(|parameter| {
            let mut names = vec![parameter.name.clone()];
            if let Some(source_name) = parameter.properties.get("source_name") {
                if source_name != &parameter.name {
                    names.push(source_name.clone());
                }
            }
            (parameter.id.clone(), parameter.owner.clone(), names)
        })
        .collect::<Vec<_>>();
    let mut local_candidates = HashMap::<(FeatureId, String), Vec<ParameterId>>::new();
    let mut qualified_candidates = HashMap::<String, Vec<ParameterId>>::new();
    for (id, owner, names) in &candidates {
        let Some(owner) = owner else { continue };
        for name in names {
            local_candidates
                .entry((owner.clone(), name.clone()))
                .or_default()
                .push(id.clone());
            if let Some(object) = object_names.get(owner) {
                qualified_candidates
                    .entry(format!("{object}.{name}"))
                    .or_default()
                    .push(id.clone());
            }
        }
    }
    let local = local_candidates
        .into_iter()
        .filter_map(|(key, ids)| (ids.len() == 1).then(|| (key, ids[0].clone())))
        .collect::<HashMap<_, _>>();
    let qualified = qualified_candidates
        .into_iter()
        .filter_map(|(key, ids)| (ids.len() == 1).then(|| (key, ids[0].clone())))
        .collect::<HashMap<_, _>>();
    for parameter in parameters.iter_mut() {
        let mut dependencies = BTreeSet::new();
        for identifier in expression_identifiers(&parameter.expression) {
            let dependency = qualified.get(identifier).or_else(|| {
                parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| local.get(&(owner.clone(), identifier.to_owned())))
            });
            if let Some(dependency) = dependency.filter(|id| **id != parameter.id) {
                dependencies.insert(dependency.clone());
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
            Vec::new()
        } else {
            dependencies.into_iter().collect()
        };
    }
    let mut owner_ordinals = HashMap::<Option<FeatureId>, Vec<u32>>::new();
    for parameter in parameters.iter() {
        owner_ordinals
            .entry(parameter.owner.clone())
            .or_default()
            .push(parameter.ordinal);
    }
    for ordinals in owner_ordinals.values_mut() {
        ordinals.sort_unstable();
    }
    let parameter_cycle_features = order_parameters_by_dependencies(parameters);
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
        let index = next_ordinal.entry(parameter.owner.clone()).or_default();
        parameter.ordinal = owner_ordinals[&parameter.owner][*index];
        *index += 1;
    }
    parameter_cycle_features
}

fn order_parameters_by_dependencies(parameters: &mut Vec<DesignParameter>) -> BTreeSet<FeatureId> {
    let known = parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    let mut remaining = std::mem::take(parameters);
    let mut emitted = BTreeSet::new();
    let mut cycle_features = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(index) = remaining.iter().position(|parameter| {
            parameter
                .dependencies
                .iter()
                .all(|dependency| !known.contains(dependency) || emitted.contains(dependency))
        }) else {
            cycle_features.extend(
                remaining
                    .iter()
                    .filter_map(|parameter| parameter.owner.clone()),
            );
            parameters.append(&mut remaining);
            break;
        };
        let parameter = remaining.remove(index);
        emitted.insert(parameter.id.clone());
        parameters.push(parameter);
    }
    cycle_features
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = &str> {
    expression
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '.'
        })
        .filter(|identifier| !identifier.is_empty())
}

fn neutral_constraint(
    kind: i64,
    loci: &[SketchLocus],
    parameter: Option<ParameterId>,
    complete: bool,
) -> Option<SketchConstraintDefinition> {
    if !complete {
        return None;
    }
    let entity = |index| loci.get(index).map(locus_entity).cloned();
    let pair = || Some((entity(0)?, entity(1)?));
    Some(match kind {
        0 => SketchConstraintDefinition::Disabled,
        1 => SketchConstraintDefinition::CoincidentLoci {
            loci: loci.to_vec(),
        },
        2 => SketchConstraintDefinition::Horizontal { entity: entity(0)? },
        3 => SketchConstraintDefinition::Vertical { entity: entity(0)? },
        4 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Parallel { first, second }
        }
        5 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Tangent { first, second }
        }
        10 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Perpendicular { first, second }
        }
        12 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Equal { first, second }
        }
        13 => SketchConstraintDefinition::PointOnObject {
            point: loci.first()?.clone(),
            entity: entity(1)?,
        },
        17 => SketchConstraintDefinition::Fixed { entity: entity(0)? },
        6 if loci.len() == 2 => SketchConstraintDefinition::DistanceLoci {
            first: loci[0].clone(),
            second: loci[1].clone(),
            parameter: parameter?,
        },
        6 => SketchConstraintDefinition::Distance {
            entities: loci.iter().map(locus_entity).cloned().collect(),
            parameter: parameter?,
        },
        7 => SketchConstraintDefinition::HorizontalDistance {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            parameter: parameter?,
        },
        8 => SketchConstraintDefinition::VerticalDistance {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            parameter: parameter?,
        },
        9 if loci.len() == 2 && sketch_axis(&loci[0]).is_some() => {
            SketchConstraintDefinition::AngleToAxis {
                entity: entity(1)?,
                axis: sketch_axis(&loci[0])?,
                parameter: parameter?,
            }
        }
        9 if loci.len() == 2 && sketch_axis(&loci[1]).is_some() => {
            SketchConstraintDefinition::AngleToAxis {
                entity: entity(0)?,
                axis: sketch_axis(&loci[1])?,
                parameter: parameter?,
            }
        }
        9 if loci.len() == 1 => SketchConstraintDefinition::AngleToAxis {
            entity: entity(0)?,
            axis: SketchAxis::Horizontal,
            parameter: parameter?,
        },
        9 => SketchConstraintDefinition::Angle {
            first: entity(0)?,
            second: entity(1)?,
            parameter: parameter?,
        },
        11 => SketchConstraintDefinition::Radius {
            entity: entity(0)?,
            parameter: parameter?,
        },
        18 => SketchConstraintDefinition::Diameter {
            entity: entity(0)?,
            parameter: parameter?,
        },
        16 => SketchConstraintDefinition::SnellsLaw {
            incident: loci.first()?.clone(),
            refracted: loci.get(1)?.clone(),
            interface: entity(2)?,
            parameter: parameter?,
        },
        19 => SketchConstraintDefinition::Weight {
            entity: entity(0)?,
            parameter: parameter?,
        },
        14 => SketchConstraintDefinition::Symmetric {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            axis: entity(2)?,
        },
        _ => return None,
    })
}

fn sketch_axis(locus: &SketchLocus) -> Option<SketchAxis> {
    let id = locus_entity(locus);
    if id.0.ends_with(":reference-horizontal-axis") {
        Some(SketchAxis::Horizontal)
    } else if id.0.ends_with(":reference-vertical-axis") {
        Some(SketchAxis::Vertical)
    } else {
        None
    }
}

fn constraint_operands(node: roxmltree::Node<'_, '_>) -> Result<Vec<(i64, i64)>, &'static str> {
    match (
        node.attribute("ElementIds"),
        node.attribute("ElementPositions"),
    ) {
        (Some(ids), Some(positions)) => {
            let ids = split_ints(ids)?;
            let positions = split_ints(positions)?;
            if ids.len() != positions.len() {
                return Err("ElementIds and ElementPositions counts differ");
            }
            return Ok(ids
                .into_iter()
                .zip(positions)
                .filter(|(entity, _)| *entity != -2000)
                .collect());
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err("ElementIds and ElementPositions must both be present");
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
                    .map_err(|_| "constraint entity is not an integer")?;
                let position = position
                    .parse::<i64>()
                    .map_err(|_| "constraint position is not an integer")?;
                if entity != -2000 {
                    operands.push((entity, position));
                }
            }
            _ => return Err("constraint entity and position must both be present"),
        }
    }
    Ok(operands)
}

fn direct_counted_records<'a, 'input>(
    xml: &'a roxmltree::Document<'input>,
    container_tag: &str,
    record_tag: &str,
    owner: &str,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, CodecError> {
    let containers = xml
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.has_tag_name(container_tag))
        .collect::<Vec<_>>();
    if containers.len() != 1
        || xml
            .descendants()
            .filter(|node| node.has_tag_name(container_tag))
            .count()
            != 1
    {
        return Err(CodecError::malformed(format_args!(
            "{owner} must contain exactly one direct {container_tag} value"
        )));
    }
    let container = containers[0];
    let declared = container
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!("{owner} has an invalid record count"))
        })?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::malformed(format_args!(
            "{owner} record count exceeds {MAX_SKETCH_RECORDS}"
        )));
    }
    let records = container
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if records.iter().any(|node| !node.has_tag_name(record_tag)) {
        return Err(CodecError::malformed(format_args!(
            "{owner} has a non-{record_tag} direct child"
        )));
    }
    if xml
        .descendants()
        .filter(|node| node.has_tag_name(record_tag))
        .count()
        != records.len()
    {
        return Err(CodecError::malformed(format_args!(
            "{owner} has nested {record_tag} records"
        )));
    }
    if declared != records.len() {
        return Err(CodecError::malformed(format_args!(
            "{owner} declares {declared} records but contains {}",
            records.len()
        )));
    }
    Ok(records)
}

fn split_ints(value: &str) -> Result<Vec<i64>, &'static str> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for group in value.split(',') {
        if group.trim().is_empty() {
            return Err("constraint integer list has an empty item");
        }
        for part in group.split_ascii_whitespace() {
            values.push(
                part.parse::<i64>()
                    .map_err(|_| "constraint integer list has an invalid integer")?,
            );
        }
    }
    Ok(values)
}

fn int_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<i64> {
    node.attribute(name)?.parse().ok()
}

fn resolve_operand(entity: i64, position: i64, entities: &[SketchEntity]) -> Option<SketchLocus> {
    let reference = |suffix: &str| {
        entities
            .iter()
            .find(|candidate| candidate.id().0.ends_with(suffix))
            .map(|candidate| SketchLocus::Entity(candidate.id().clone()))
    };
    match (entity, position) {
        (-1, 0) => return reference(":reference-horizontal-axis"),
        (-1, 1) => return reference(":reference-root-point"),
        (-2, 0) => return reference(":reference-vertical-axis"),
        (-2, 1) => return reference(":reference-root-point"),
        _ => {}
    }
    if entity <= -3 {
        let external_index = usize::try_from(-entity - 3).ok()?;
        let suffix = format!(":external:{external_index}");
        let entity = entities
            .iter()
            .find(|candidate| candidate.id().0.ends_with(&suffix))?;
        return sketch_locus(entity, position);
    }
    sketch_locus(entities.get(usize::try_from(entity).ok()?)?, position)
}

fn sketch_locus(entity: &SketchEntity, position: i64) -> Option<SketchLocus> {
    let id = entity.id().clone();
    if matches!(entity.geometry, SketchGeometry::Point { .. }) && matches!(position, 0..=3) {
        return Some(SketchLocus::Entity(id));
    }
    Some(match position {
        0 => SketchLocus::Entity(id),
        1 => SketchLocus::Start(id),
        2 => SketchLocus::End(id),
        3 => SketchLocus::Center(id),
        _ => return None,
    })
}

fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
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

fn sketch_geometry(kind: &str, attributes: &BTreeMap<String, String>) -> SketchGeometry {
    let number = |name: &str| attributes.get(name).and_then(|value| value.parse().ok());
    let native = || SketchGeometry::Native {
        native_kind: kind.to_owned(),
    };
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
            (Some(start_x), Some(start_y), Some(end_x), Some(end_y)) => SketchGeometry::Line {
                start: Point2::new(start_x, start_y),
                end: Point2::new(end_x, end_y),
            },
            _ => native(),
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
                SketchGeometry::Ellipse {
                    center: Point2::new(x, y),
                    major_angle: cadmpeg_ir::features::Angle(angle),
                    major_radius: Length(major),
                    minor_radius: Length(minor),
                    bounds: bounds.map(|[start, end]| {
                        [
                            cadmpeg_ir::features::Angle(start),
                            cadmpeg_ir::features::Angle(end),
                        ]
                    }),
                }
            }
            _ => native(),
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
                SketchGeometry::Hyperbola {
                    center: Point2::new(x, y),
                    major_angle: cadmpeg_ir::features::Angle(angle),
                    major_radius: Length(major),
                    minor_radius: Length(minor),
                    bounds,
                }
            }
            _ => native(),
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
                SketchGeometry::Parabola {
                    vertex: Point2::new(x, y),
                    axis_angle: cadmpeg_ir::features::Angle(angle),
                    focal_length: Length(focal),
                    bounds,
                }
            }
            _ => native(),
        }
    } else if matches!(kind, "Part::GeomArcOfCircle" | "ArcOfCircle") {
        let frame_angle = number("AngleXU").unwrap_or(0.0);
        match (
            number("CenterX"),
            number("CenterY"),
            number("Radius"),
            number("StartAngle").or_else(|| number("FirstParameter")),
            number("EndAngle").or_else(|| number("LastParameter")),
        ) {
            (Some(x), Some(y), Some(radius), Some(start), Some(end))
                if radius > 0.0
                    && [x, y, radius, start, end, frame_angle]
                        .into_iter()
                        .all(f64::is_finite) =>
            {
                SketchGeometry::Arc {
                    center: Point2::new(x, y),
                    radius: Length(radius),
                    start_angle: cadmpeg_ir::features::Angle(start + frame_angle),
                    end_angle: cadmpeg_ir::features::Angle(end + frame_angle),
                }
            }
            _ => native(),
        }
    } else if matches!(kind, "Part::GeomCircle" | "Circle") {
        match (number("CenterX"), number("CenterY"), number("Radius")) {
            (Some(x), Some(y), Some(radius)) if radius > 0.0 => SketchGeometry::Circle {
                center: Point2::new(x, y),
                radius: Length(radius),
            },
            (Some(x), Some(y), Some(0.0)) => SketchGeometry::Point {
                position: Point2::new(x, y),
            },
            _ => native(),
        }
    } else if kind == "Part::GeomPoint" {
        match (number("X"), number("Y")) {
            (Some(x), Some(y)) => SketchGeometry::Point {
                position: Point2::new(x, y),
            },
            _ => native(),
        }
    } else {
        native()
    }
}

fn build_profiles(
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> Vec<Vec<SketchEntityUse>> {
    // Internal entities arrive in GeometryList order; appended external and built-in reference
    // entities are construction entries. Indices therefore preserve the persisted ordinal for
    // every eligible profile entity.
    let profile_entities = entities
        .iter()
        .enumerate()
        .filter(|(_, entity)| !entity.construction)
        .map(|(index, _)| index)
        .collect::<BTreeSet<_>>();
    let mut unused = profile_entities.clone();
    let explicit_relations = explicit_endpoint_relations(&profile_entities, entities, constraints);
    let mut ambiguous = BTreeSet::new();
    for &entity in &unused {
        for start in [true, false] {
            let matches =
                endpoint_candidates((entity, start), &unused, &explicit_relations, entities);
            if matches.len() > 1 {
                ambiguous.insert(entity);
                ambiguous.extend(matches.into_iter().map(|(candidate, _)| candidate));
            }
        }
    }
    let mut profiles = Vec::new();
    // FreeCAD persists no profile seed. CADIR selects the first remaining persisted ordinal.
    while let Some(first) = unused.pop_first() {
        let mut chain = vec![SketchEntityUse {
            entity: entities[first].id().clone(),
            reversed: false,
        }];
        if ambiguous.contains(&first) {
            profiles.push(chain);
            continue;
        }
        if endpoints(&entities[first]).is_none() {
            profiles.push(chain);
            continue;
        }
        let mut head = (first, true);
        let mut tail = (first, false);
        loop {
            let candidates = endpoint_candidates(tail, &unused, &explicit_relations, entities)
                .into_iter()
                .filter(|(index, _)| !ambiguous.contains(index))
                .collect::<Vec<_>>();
            let Some((index, candidate_start)) = (candidates.len() == 1).then(|| candidates[0])
            else {
                break;
            };
            let (reversed, next_tail) = if candidate_start {
                (false, (index, false))
            } else {
                (true, (index, true))
            };
            unused.remove(&index);
            chain.push(SketchEntityUse {
                entity: entities[index].id().clone(),
                reversed,
            });
            tail = next_tail;
        }
        loop {
            let candidates = endpoint_candidates(head, &unused, &explicit_relations, entities)
                .into_iter()
                .filter(|(index, _)| !ambiguous.contains(index))
                .collect::<Vec<_>>();
            let Some((index, candidate_start)) = (candidates.len() == 1).then(|| candidates[0])
            else {
                break;
            };
            let (reversed, next_head) = if candidate_start {
                (true, (index, false))
            } else {
                (false, (index, true))
            };
            unused.remove(&index);
            chain.insert(
                0,
                SketchEntityUse {
                    entity: entities[index].id().clone(),
                    reversed,
                },
            );
            head = next_head;
        }
        profiles.push(chain);
    }
    profiles
}

fn endpoint_candidates(
    endpoint: (usize, bool),
    available: &BTreeSet<usize>,
    explicit_relations: &BTreeMap<(usize, bool), BTreeSet<(usize, bool)>>,
    entities: &[SketchEntity],
) -> Vec<(usize, bool)> {
    // Active explicit coincident loci override coordinates. Coordinate matching below is the
    // decoder-owned CADIR boundary, not a producer tolerance.
    if let Some(explicit) = explicit_relations.get(&endpoint) {
        return explicit
            .iter()
            .copied()
            .filter(|(candidate, _)| available.contains(candidate))
            .collect();
    }
    available
        .iter()
        .copied()
        .filter(|candidate| *candidate != endpoint.0)
        .flat_map(|candidate| [(candidate, true), (candidate, false)])
        .filter(|candidate| !explicit_relations.contains_key(candidate))
        .filter(|candidate| {
            endpoint_point(endpoint, entities)
                .zip(endpoint_point(*candidate, entities))
                .is_some_and(|(first, second)| endpoints_match_by_roundoff(first, second))
        })
        .collect()
}

fn explicit_endpoint_relations(
    profile_entities: &BTreeSet<usize>,
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> BTreeMap<(usize, bool), BTreeSet<(usize, bool)>> {
    let entity_indices = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.id().0.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut relations = BTreeMap::new();
    for constraint in constraints {
        if constraint.active == Some(false) {
            continue;
        }
        let SketchConstraintDefinition::CoincidentLoci { loci } = &constraint.definition else {
            continue;
        };
        let endpoints = loci
            .iter()
            .filter_map(|locus| match locus {
                SketchLocus::Start(entity) => {
                    Some((entity_indices.get(entity.0.as_str()).copied()?, true))
                }
                SketchLocus::End(entity) => {
                    Some((entity_indices.get(entity.0.as_str()).copied()?, false))
                }
                _ => None,
            })
            .filter(|(entity, _)| profile_entities.contains(entity))
            .collect::<BTreeSet<_>>();
        for first in endpoints.iter().copied() {
            relations.entry(first).or_insert_with(BTreeSet::new).extend(
                endpoints
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != first),
            );
        }
    }
    relations
}

fn endpoint_point(endpoint: (usize, bool), entities: &[SketchEntity]) -> Option<Point2> {
    endpoints(&entities[endpoint.0]).map(|points| if endpoint.1 { points.0 } else { points.1 })
}

fn endpoints(entity: &SketchEntity) -> Option<(Point2, Point2)> {
    match entity.geometry {
        SketchGeometry::Line { start, end } => Some((start, end)),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            Point2::new(
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ),
            Point2::new(
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ),
        )),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds: Some([start, end]),
        } => {
            let major = Point2::new(major_angle.0.cos(), major_angle.0.sin());
            let minor = Point2::new(-major.v, major.u);
            let point = |parameter: f64| {
                let (along_major, along_minor) = (parameter.cos(), parameter.sin());
                Point2::new(
                    center.u
                        + major_radius.0 * along_major * major.u
                        + minor_radius.0 * along_minor * minor.u,
                    center.v
                        + major_radius.0 * along_major * major.v
                        + minor_radius.0 * along_minor * minor.v,
                )
            };
            Some((point(start.0), point(end.0)))
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
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> ProfileRef {
    let Some((property, target)) = profile_target(properties) else {
        return ProfileRef::Unresolved(owner.to_owned());
    };
    sketches.get(target).cloned().map_or_else(
        || ProfileRef::Native(property.id.clone()),
        ProfileRef::Sketch,
    )
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
        let target = link.object.as_deref()?;
        if target.is_empty() || selected.is_some() {
            return None;
        }
        selected = Some((property, target));
    }
    selected
}

fn revolution_axis(properties: &[&PropertyRecord]) -> Option<RevolutionAxis> {
    Some(RevolutionAxis {
        origin: vector_property(properties, "Base").map_or_else(
            || Point3::new(0.0, 0.0, 0.0),
            |vector| Point3::new(vector.x, vector.y, vector.z),
        ),
        direction: vector_property(properties, "Axis")?,
        reference: None,
    })
}

fn revolution_definition(
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profile = match profile_ref(owner, properties, sketches) {
        ProfileRef::Unresolved(_) => None,
        profile => Some(profile),
    };
    let mut axis = revolution_axis(properties)?;
    axis.direction = axis.direction.unit()?;
    let angle = || {
        scalar_named(properties, "Angle")
            .filter(|angle| angle.is_finite() && *angle > 0.0)
            .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()))
    };
    let mode = enumeration_selector(properties, "Type", 0)?;
    let extent = if kind == "Part::Revolution" {
        let angle = angle()?;
        if bool_selector(properties, "Symmetric", false)? {
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
                let angle = angle()?;
                if bool_selector(properties, "Midplane", false)? {
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
                termination: AngularTermination::ThroughAll,
            },
            2 => RevolveExtent::OneSided {
                termination: AngularTermination::ToFirst,
            },
            3 => RevolveExtent::OneSided {
                termination: AngularTermination::ToFace {
                    face: cadmpeg_ir::features::FaceSelection::Native(
                        singular_operand(properties, "UpToFace")?.id.clone(),
                    ),
                    offset: None,
                },
            },
            4 => RevolveExtent::TwoSided {
                first: AngularTermination::Angle { angle: angle()? },
                second: AngularTermination::Angle {
                    angle: cadmpeg_ir::features::Angle(
                        scalar_named(properties, "Angle2")
                            .filter(|angle| angle.is_finite() && *angle > 0.0)?
                            .to_radians(),
                    ),
                },
            },
            _ => return None,
        }
    };
    let reversed = if kind.starts_with("PartDesign::") {
        bool_selector(properties, "Reversed", false)?
    } else {
        false
    };
    if reversed {
        axis.direction = Vector3::new(-axis.direction.x, -axis.direction.y, -axis.direction.z);
    }
    let axis_reference_properties = ["AxisLink", "ReferenceAxis"]
        .iter()
        .filter_map(|name| property(properties, name))
        .collect::<Vec<_>>();
    axis.reference = match axis_reference_properties.as_slice() {
        [] => None,
        [property] => {
            if property.links.iter().any(nonempty_link) {
                singular_reference_link(property)?;
                Some(PathRef::Native(property.id.clone()))
            } else {
                None
            }
        }
        _ => return None,
    };
    let face_maker =
        if kind == "Part::Revolution" && property(properties, "FaceMakerClass").is_some() {
            Some(FaceMaker::new(string_property_value(property(
                properties,
                "FaceMakerClass",
            )?)?)?)
        } else {
            None
        };
    let fuse_order =
        if kind.starts_with("PartDesign::") && property(properties, "FuseOrder").is_some() {
            Some(match integer_property(properties, "FuseOrder")? {
                0 => RevolutionFuseOrder::BaseFirst,
                1 => RevolutionFuseOrder::FeatureFirst,
                _ => return None,
            })
        } else {
            None
        };
    Some(FeatureDefinition::Revolve {
        construction: RevolveConstruction::new(
            profile,
            Some(axis),
            Some(extent),
            Some(if kind == "Part::Revolution" {
                bool_selector(properties, "Solid", false)?
            } else {
                true
            }),
            face_maker,
            fuse_order,
            if kind.starts_with("PartDesign::") {
                Some(bool_selector(properties, "AllowMultiFace", false)?)
            } else {
                None
            },
        ),
        op: if kind == "Part::Revolution" {
            BooleanOp::NewBody
        } else if kind.contains("Groove") {
            BooleanOp::Cut
        } else {
            BooleanOp::Join
        },
    })
}

fn vector_property(properties: &[&PropertyRecord], name: &str) -> Option<Vector3> {
    let property = property(properties, name)?;
    if !is_vector_property_type(&property.type_name) {
        return None;
    }
    let attributes = direct_root_attributes(property, "PropertyVector")?;
    let component = |name: &str| {
        attributes
            .get(name)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
    };
    Some(Vector3::new(
        component("valueX")?,
        component("valueY")?,
        component("valueZ")?,
    ))
}

fn vector_list_property(
    properties: &[&PropertyRecord],
    name: &str,
    entries: &[EntryRecord],
) -> Option<Vec<Point3>> {
    let property = property(properties, name)?;
    if property.type_name != "App::PropertyVectorList" {
        return None;
    }
    let file = direct_root_attributes(property, "VectorList")?
        .get("file")
        .cloned()?;
    if file.is_empty() {
        return property.side_entries.is_empty().then(Vec::new);
    }
    if property.side_entries.as_slice() != [file.as_str()] {
        return None;
    }
    let data = entries
        .iter()
        .find(|entry| entry.name == file)?
        .data
        .as_slice();
    let mut view = View::over_retained(data);
    let count = view.u32_le()? as usize;
    if count > MAX_SKETCH_RECORDS {
        return None;
    }
    let values = view.read_counted(count as u64, 24, |view| {
        Some(Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?))
    })?;
    if !view.is_empty()
        || values
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return None;
    }
    Some(values)
}

fn part_construction_geometry_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<FeatureDefinition> {
    let point = |x: &str, y: &str, z: &str| {
        Some(Point3::new(
            scalar_named(properties, x)?,
            scalar_named(properties, y)?,
            scalar_named(properties, z)?,
        ))
    };
    let angle = |name: &str| {
        scalar_named(properties, name)
            .filter(|value| value.is_finite())
            .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
    };
    match kind {
        "Part::Vertex" => Some(FeatureDefinition::PointGeometry {
            position: point("X", "Y", "Z")?,
        }),
        "Part::Line" => Some(FeatureDefinition::LineSegment {
            start: point("X1", "Y1", "Z1")?,
            end: point("X2", "Y2", "Z2")?,
        }),
        "Part::Circle" => {
            let legacy_angles = property(properties, "Angle0").is_some();
            Some(FeatureDefinition::CircularArc {
                center: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                radius: Length(scalar_named(properties, "Radius").filter(|value| *value > 0.0)?),
                start_angle: angle(if legacy_angles { "Angle0" } else { "Angle1" })?,
                end_angle: angle(if legacy_angles { "Angle1" } else { "Angle2" })?,
            })
        }
        "Part::Ellipse" => {
            let major = scalar_named(properties, "MajorRadius").filter(|value| *value > 0.0)?;
            let minor = scalar_named(properties, "MinorRadius")
                .filter(|value| *value > 0.0 && *value <= major)?;
            Some(FeatureDefinition::EllipticArc {
                center: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                major_axis: Vector3::new(1.0, 0.0, 0.0),
                major_radius: Length(major),
                minor_radius: Length(minor),
                start_angle: angle("Angle1")?,
                end_angle: angle("Angle2")?,
            })
        }
        "Part::Polygon" => {
            let points = vector_list_property(properties, "Nodes", entries)?;
            let closed = bool_property(properties, "Close").unwrap_or(false);
            if points.len() < 2 || (closed && points.len() < 3) {
                return None;
            }
            Some(FeatureDefinition::Polyline { points, closed })
        }
        "Part::RegularPolygon" => Some(FeatureDefinition::RegularPolygonCurve {
            sides: u32::try_from(integer_property(properties, "Polygon")?)
                .ok()
                .filter(|value| *value >= 3)?,
            circumradius: Length(
                scalar_named(properties, "Circumradius").filter(|value| *value > 0.0)?,
            ),
        }),
        "Part::Plane" => Some(FeatureDefinition::PlanarPatch {
            length: Length(scalar_named(properties, "Length").filter(|value| *value > 0.0)?),
            width: Length(scalar_named(properties, "Width").filter(|value| *value > 0.0)?),
        }),
        "Part::Face" => {
            let sources = property(properties, "Sources")?;
            if sources.links.is_empty() {
                return None;
            }
            Some(FeatureDefinition::FaceFromShapes {
                sources: BodySelection::Native(sources.id.clone()),
                face_maker: FaceMaker::new(string_property_value(property(
                    properties,
                    "FaceMakerClass",
                )?)?)?,
            })
        }
        _ => None,
    }
}

fn parametric_helix_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let radius = scalar_named(properties, "Radius").filter(|value| *value > 0.0)?;
    let segment_default = if kind == "Part::Spiral" {
        DEFAULT_PART_SPIRAL_SEGMENT_TURNS
    } else {
        0.0
    };
    let segment_value = quantity_constraint_selector(properties, "SegmentLength", segment_default)?;
    if segment_value < 0.0 {
        return None;
    }
    let segment_turns = (segment_value > 0.0).then_some(segment_value);
    let (shape, revolutions, clockwise, construction_style) = if kind == "Part::Helix" {
        let pitch = scalar_named(properties, "Pitch").filter(|value| *value > 0.0)?;
        let height = scalar_named(properties, "Height").filter(|value| *value > 0.0)?;
        let angle = scalar_named(properties, "Angle").unwrap_or(0.0);
        if !angle.is_finite() || angle.abs() >= 90.0 {
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
                pitch: cadmpeg_ir::features::HelixPitch::new(Length(pitch))?,
            }
        } else {
            cadmpeg_ir::features::HelixShape::Conical {
                pitch: cadmpeg_ir::features::HelixPitch::new(Length(pitch))?,
                cone_angle: cadmpeg_ir::features::Angle(angle.to_radians()),
            }
        };
        (shape, height / pitch, clockwise, construction_style)
    } else {
        let growth = scalar_named(properties, "Growth").filter(|value| *value >= 0.0)?;
        let revolutions = scalar_named(properties, "Rotations").filter(|value| *value > 0.0)?;
        (
            cadmpeg_ir::features::HelixShape::Spiral {
                radial_growth: Length(growth),
            },
            revolutions,
            false,
            None,
        )
    };
    (revolutions.is_finite() && revolutions > 0.0).then_some(FeatureDefinition::Helix {
        axis_origin: Point3::new(0.0, 0.0, 0.0),
        axis_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: Length(radius),
        shape,
        revolutions,
        start_angle: cadmpeg_ir::features::Angle(0.0),
        clockwise,
        segment_turns,
        construction_style,
    })
}

fn extrusion_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    profile: ProfileRef,
    profile_normal: Option<Vector3>,
    sketches: &[Sketch],
) -> Option<FeatureDefinition> {
    if kind == "Part::Extrusion" {
        let raw_direction = vector_property(properties, "Dir");
        let direction_magnitude = raw_direction.map(|direction| {
            (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                .sqrt()
        });
        let direction_mode = enumeration_selector(properties, "DirMode", 0)?;
        let (mut direction, direction_source) = match direction_mode {
            0 => (raw_direction?.unit()?, ExtrusionDirectionSource::Custom),
            1 => {
                let reference = property(properties, "DirLink")?;
                if reference.links.len() != 1 {
                    return None;
                }
                (
                    raw_direction?.unit()?,
                    ExtrusionDirectionSource::Edge {
                        reference: PathRef::Native(reference.id.clone()),
                    },
                )
            }
            2 => {
                let normal = match &profile {
                    ProfileRef::Sketch(sketch_id) => sketches
                        .iter()
                        .find(|sketch| sketch.id == *sketch_id)
                        .and_then(Sketch::resolved_placement)
                        .map(|(_, normal, _)| normal)
                        .or(profile_normal),
                    _ => profile_normal,
                }?;
                (normal.unit()?, ExtrusionDirectionSource::ProfileNormal)
            }
            _ => return None,
        };
        let signed_length = |name| match scalar_named(properties, name) {
            Some(value) if value.is_finite() => Some(value),
            Some(_) => None,
            None => Some(0.0),
        };
        let mut forward = signed_length("LengthFwd")?;
        let reverse = signed_length("LengthRev")?;
        if forward == 0.0 && reverse == 0.0 {
            forward = direction_magnitude.filter(|value| value.is_finite() && *value > 0.0)?;
        }
        let symmetric = bool_selector(properties, "Symmetric", false)?;
        let forward_draft = scalar_named(properties, "TaperAngle").unwrap_or(0.0);
        let reverse_draft = scalar_named(properties, "TaperAngleRev").unwrap_or(0.0);
        if !forward_draft.is_finite() || !reverse_draft.is_finite() {
            return None;
        }
        let to_draft = |degrees: f64| {
            (degrees != 0.0).then_some(cadmpeg_ir::features::Angle(degrees.to_radians()))
        };
        let (extent, reverse_direction) = if symmetric {
            // A symmetric extent mirrors one side across the profile plane, so
            // its single side carries the taper once (from `TaperAngle`).
            (
                ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: Length((forward != 0.0).then_some(forward.abs())?),
                        },
                        draft: to_draft(forward_draft),
                    },
                },
                false,
            )
        } else {
            let forward_travel = (forward != 0.0).then_some((forward, forward_draft));
            let reverse_travel = (reverse != 0.0).then_some((-reverse, reverse_draft));
            let same_side = forward_travel
                .zip(reverse_travel)
                .is_some_and(|((first, _), (second, _))| first.signum() == second.signum());
            if same_side && forward_draft != reverse_draft {
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
                                length: Length(length),
                            },
                            draft: to_draft(draft),
                        },
                    },
                    false,
                ),
                (None, Some((length, draft))) => (
                    ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: Length(-length),
                            },
                            draft: to_draft(draft),
                        },
                    },
                    true,
                ),
                (Some((first, first_draft)), Some((second, second_draft))) => (
                    ExtrudeExtent::TwoSided {
                        first: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: Length(first),
                            },
                            draft: to_draft(first_draft),
                        },
                        second: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: Length(-second),
                            },
                            draft: to_draft(second_draft),
                        },
                    },
                    false,
                ),
                (None, None) => return None,
            }
        };
        if reverse_direction ^ bool_selector(properties, "Reversed", false)? {
            direction = Vector3::new(-direction.x, -direction.y, -direction.z);
        }
        let face_maker = if let Some(class_property) = property(properties, "FaceMakerClass") {
            let maker = FaceMaker::new(string_property_value(class_property)?)?;
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
        return Some(FeatureDefinition::Extrude {
            profile,
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(direction_source),
            },
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent,
            op: BooleanOp::NewBody,
            solid: Some(bool_selector(properties, "Solid", false)?),
            face_maker,
            inner_wire_taper,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        });
    }
    let legacy_two_lengths = property(properties, "SideType").is_none()
        && enumeration_selector(properties, "Type", 0) == Some(4);
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
            Some(Length(scalar_named(properties, &offset_name)?))
        } else {
            None
        };
        match termination_type {
            0 => Some(LinearTermination::Blind {
                length: Length(
                    scalar_named(properties, &length_name).filter(|value| *value != 0.0)?,
                ),
            }),
            1 if kind.contains("Pocket") => Some(LinearTermination::ThroughAll),
            1 => Some(LinearTermination::ToLast),
            2 => Some(LinearTermination::ToFirst),
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
    let side_type = if legacy_two_lengths {
        1
    } else if bool_selector(properties, "Midplane", false)? {
        2
    } else if property(properties, "SideType").is_some() {
        enumeration_selector(properties, "SideType", 0)?
    } else {
        0
    };
    // `TaperAngle2` describes a second, independent side and is read only when
    // the extent actually carries one (`SideType` 1 / two-sided). A symmetric
    // (Midplane) pad mirrors side one, so it has no second side to receive it;
    // the native property remains retained but maps nowhere in the IR.
    let first_draft = scalar_named(properties, "TaperAngle")
        .filter(|angle| *angle != 0.0)
        .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()));
    let extent = match side_type {
        0 => ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: termination(1)?,
                draft: first_draft,
            },
        },
        1 => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: termination(1)?,
                draft: first_draft,
            },
            second: ExtrudeSide {
                termination: termination(2)?,
                draft: scalar_named(properties, "TaperAngle2")
                    .filter(|angle| *angle != 0.0)
                    .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians())),
            },
        },
        2 => ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: termination(1)?,
                draft: first_draft,
            },
        },
        _ => return None,
    };
    let use_custom = bool_selector(properties, "UseCustomVector", false)?;
    let is_nonempty_link = |link: &crate::native::LinkTarget| {
        link.document.is_some()
            || link
                .object
                .as_deref()
                .is_some_and(|object| !object.is_empty())
    };
    let reference_axis = property(properties, "ReferenceAxis")
        .filter(|property| property.links.iter().any(is_nonempty_link));
    if reference_axis.is_some_and(|property| {
        property
            .links
            .iter()
            .filter(|link| is_nonempty_link(link))
            .count()
            != 1
    }) {
        return None;
    }
    let mut direction = if use_custom {
        cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: vector_property(properties, "Direction")?.unit()?,
            source: Some(ExtrusionDirectionSource::Custom),
        }
    } else if let Some(reference_axis) = reference_axis {
        cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: vector_property(properties, "Direction")?.unit()?,
            source: Some(ExtrusionDirectionSource::Edge {
                reference: PathRef::Native(reference_axis.id.clone()),
            }),
        }
    } else {
        match &profile {
            ProfileRef::Sketch(sketch_id) => sketches
                .iter()
                .find(|sketch| sketch.id == *sketch_id)
                .and_then(Sketch::resolved_placement)
                .map(|(_, normal, _)| normal)
                .or(profile_normal)
                .and_then(Vector3::unit)
                .map_or(
                    cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                    |vector| cadmpeg_ir::features::ExtrudeDirection::Explicit {
                        vector,
                        source: Some(ExtrusionDirectionSource::ProfileNormal),
                    },
                ),
            ProfileRef::Native(_) => profile_normal.and_then(Vector3::unit).map_or(
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                |vector| cadmpeg_ir::features::ExtrudeDirection::Explicit {
                    vector,
                    source: Some(ExtrusionDirectionSource::ProfileNormal),
                },
            ),
            ProfileRef::Unresolved(_) => return None,
            _ => return None,
        }
    };
    if bool_selector(properties, "Reversed", false)? {
        let cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } = &mut direction else {
            return None;
        };
        *vector = Vector3::new(-vector.x, -vector.y, -vector.z);
    }
    let length_along_profile_normal = Some(bool_selector(properties, "AlongSketchNormal", true)?);
    let allow_multi_profile_faces = Some(bool_selector(properties, "AllowMultiFace", false)?);
    Some(FeatureDefinition::Extrude {
        profile,
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
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
    })
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
    let factor =
        |name| scalar_named(properties, name).filter(|factor| factor.is_finite() && *factor != 0.0);
    let factors = if bool_selector(properties, "Uniform", true)? {
        ScaleFactors::Uniform(factor("UniformScale")?)
    } else {
        ScaleFactors::PerAxis(Vector3::new(
            factor("XScale")?,
            factor("YScale")?,
            factor("ZScale")?,
        ))
    };
    Some(FeatureDefinition::Scale {
        bodies: BodySelection::Native(base.id.clone()),
        center: Some(ScaleCenter::ModelOrigin),
        factors,
    })
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
        let radius = values.first()?.1;
        values
            .iter()
            .all(|(_, first, second)| {
                *first == radius && *second == radius && radius.is_finite() && radius > 0.0
            })
            .then_some(radius)?
    } else {
        scalar_named(properties, "Radius").filter(|radius| radius.is_finite() && *radius > 0.0)?
    };
    Some(FeatureDefinition::Fillet {
        groups: vec![cadmpeg_ir::features::FilletGroup {
            edges,
            radius: RadiusSpec::Constant {
                radius: Length(radius),
            },
            tangency_weight: None,
        }],
    })
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
        let (_, first, second) = *values.first()?;
        if !first.is_finite() || first <= 0.0 || !second.is_finite() || second <= 0.0 {
            return None;
        }
        if !values.iter().all(|(_, candidate_first, candidate_second)| {
            *candidate_first == first && *candidate_second == second
        }) {
            return None;
        }
        if first == second {
            ChamferSpec::Distance {
                distance: Length(first),
            }
        } else {
            ChamferSpec::TwoDistances {
                first: Length(first),
                second: Length(second),
            }
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
            .is_some_and(|value| value == 1.0 || value == 2.0);
    Some(FeatureDefinition::Chamfer {
        groups: vec![cadmpeg_ir::features::ChamferGroup { edges, spec }],
        flip_direction: if legacy_flip {
            !flip_direction
        } else {
            flip_direction
        },
    })
}

fn part_fillet_edge_values(
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<Vec<(u32, f64, f64)>> {
    let property = property(properties, "Edges")?;
    let entry_name = property.side_entries.as_slice().first()?;
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
    if !thickness.is_finite() || thickness == 0.0 {
        return None;
    }
    let source_name = if kind == "Part::Thickness" {
        "Faces"
    } else {
        "Base"
    };
    let selection = property(properties, source_name)?;
    if selection.links.is_empty() {
        return None;
    }
    Some(FeatureDefinition::Shell {
        bodies: None,
        removed_faces: cadmpeg_ir::features::FaceSelection::Native(selection.id.clone()),
        thickness: Some(Length(thickness.abs())),
        outward: Some(if kind == "Part::Thickness" {
            thickness > 0.0
        } else {
            !bool_property(properties, "Reversed").unwrap_or(false)
        }),
        mode: Some(shell_mode(kind, properties)?),
        join: Some(shell_join(kind, properties)?),
        resolve_intersections: Some(bool_property(properties, "Intersection").unwrap_or(false)),
        allow_self_intersections: Some(
            bool_property(properties, "SelfIntersection").unwrap_or(false),
        ),
    })
}

fn offset_shape_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let source = singular_operand(properties, "Source")?;
    let distance = scalar_named(properties, "Value")
        .filter(|distance| distance.is_finite() && *distance != 0.0)?;
    let mode = shell_mode(kind, properties)?;
    if kind == "Part::Offset2D" && mode == ShellMode::BothSides {
        return None;
    }
    Some(FeatureDefinition::OffsetShape {
        source: BodySelection::Native(source.id.clone()),
        distance: Length(distance),
        mode,
        join: shell_join(kind, properties)?,
        resolve_intersections: bool_property(properties, "Intersection").unwrap_or(false),
        allow_self_intersections: bool_property(properties, "SelfIntersection").unwrap_or(false),
        fill: bool_property(properties, "Fill").unwrap_or(false),
        planar: kind == "Part::Offset2D",
    })
}

fn derived_shape_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    match kind {
        "Part::Compound" | "Part::Compound2" => {
            let Some(links) = property(properties, "Links") else {
                return property(properties, "Shape").map(|_| FeatureDefinition::StoredGeometry);
            };
            if links.links.is_empty() {
                return None;
            }
            Some(FeatureDefinition::Compound {
                members: BodySelection::Native(links.id.clone()),
            })
        }
        "Part::Refine" | "Part::Reverse" => {
            let source = property(properties, "Source")?;
            if source.links.len() != 1 {
                return None;
            }
            let source = BodySelection::Native(source.id.clone());
            Some(if kind == "Part::Refine" {
                FeatureDefinition::RefineShape { source }
            } else {
                FeatureDefinition::ReverseShape { source }
            })
        }
        _ => None,
    }
}

fn cached_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    property(properties, "Shape")
        .filter(|shape| !shape.side_entries.is_empty())
        .map(|_| FeatureDefinition::StoredGeometry)
}

fn ruled_surface_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let curve = |name| {
        let property = property(properties, name)?;
        (property.links.len() == 1).then(|| PathRef::Native(property.id.clone()))
    };
    let orientation = match integer_property(properties, "Orientation").unwrap_or(0) {
        0 => RuledCurveOrientation::Automatic,
        1 => RuledCurveOrientation::Forward,
        2 => RuledCurveOrientation::Reversed,
        _ => return None,
    };
    Some(FeatureDefinition::RuledBetweenCurves {
        first: curve("Curve1")?,
        second: curve("Curve2")?,
        orientation,
    })
}

fn section_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let operand = |name| {
        let property = property(properties, name)?;
        (property.links.len() == 1).then(|| BodySelection::Native(property.id.clone()))
    };
    Some(FeatureDefinition::SectionShape {
        first: operand("Base")?,
        second: operand("Tool")?,
        approximate: Some(bool_property(properties, "Approximation").unwrap_or(false)),
    })
}

fn mirror_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let source = property(properties, "Source")?;
    if source.links.len() != 1 {
        return None;
    }
    let origin = vector_property(properties, "Base")?;
    let plane_reference = property(properties, "MirrorPlane")
        .filter(|property| property.links.iter().any(nonempty_link))
        .map(|property| cadmpeg_ir::features::FaceSelection::Native(property.id.clone()));
    Some(FeatureDefinition::MirrorShape {
        source: BodySelection::Native(source.id.clone()),
        plane_origin: Point3::new(origin.x, origin.y, origin.z),
        plane_normal: vector_property(properties, "Normal")?.unit()?,
        plane_reference,
    })
}

fn project_on_surface_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let sources = property(properties, "Projection")?;
    if sources.links.is_empty() {
        return None;
    }
    let support = property(properties, "SupportFace")?;
    if support.links.len() != 1 {
        return None;
    }
    let mode = match enumeration_selector(properties, "Mode", 0)? {
        0 => SurfaceProjectionMode::All,
        1 => SurfaceProjectionMode::Faces,
        2 => SurfaceProjectionMode::Edges,
        _ => return None,
    };
    let height = if property(properties, "Height").is_some() {
        scalar_named(properties, "Height").filter(|value| *value >= 0.0)?
    } else {
        0.0
    };
    let offset = if property(properties, "Offset").is_some() {
        scalar_named(properties, "Offset")?
    } else {
        0.0
    };
    Some(FeatureDefinition::ProjectOnSurface {
        sources: PathRef::Native(sources.id.clone()),
        support_face: cadmpeg_ir::features::FaceSelection::Native(support.id.clone()),
        direction: vector_property(properties, "Direction")?.unit()?,
        mode,
        height: Length(height),
        offset: Length(offset),
    })
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
    let pull_direction = if property(properties, "PullDirection")
        .is_some_and(|property| property.links.iter().any(nonempty_link))
    {
        axis_reference(properties, "PullDirection", objects, properties_by_owner)
            .map(|(_, direction)| direction)
    } else {
        plane_normal
    };
    let reversed = bool_property(properties, "Reversed").unwrap_or(false);
    let angle = scalar_named(properties, "Angle")?;
    if !angle.is_finite() {
        return None;
    }
    Some(FeatureDefinition::Draft {
        faces: cadmpeg_ir::features::FaceSelection::Native(faces.id.clone()),
        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
            plane: cadmpeg_ir::features::FaceSelection::Native(neutral_plane.id.clone()),
            pull: pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
                direction,
                plane: None,
            }),
        },
        angle: Some(cadmpeg_ir::features::Angle(
            if reversed { -angle } else { angle }.to_radians(),
        )),
        outward: Some(reversed),
    })
}

fn chamfer_spec(properties: &[&PropertyRecord]) -> Option<ChamferSpec> {
    let mode = property(properties, "ChamferType").map_or(Some(0), |property| {
        scalar_value(property).map(|value| value as i64)
    })?;
    let first = property(properties, "Size")
        .and_then(scalar_value)
        .filter(|value| value.is_finite() && *value > 0.0);
    match (mode, first) {
        (0, Some(distance)) => Some(ChamferSpec::Distance {
            distance: Length(distance),
        }),
        (1, Some(first)) => property(properties, "Size2")
            .and_then(scalar_value)
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|second| ChamferSpec::TwoDistances {
                first: Length(first),
                second: Length(second),
            }),
        (2, Some(distance)) => property(properties, "Angle")
            .and_then(scalar_value)
            .filter(|angle| angle.is_finite() && *angle > 0.0 && *angle < 180.0)
            .map(|angle| ChamferSpec::DistanceAngle {
                distance: Length(distance),
                angle: cadmpeg_ir::features::Angle(angle.to_radians()),
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

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

fn nonempty_link(link: &crate::native::LinkTarget) -> bool {
    link.document.is_some()
        || link
            .object
            .as_deref()
            .is_some_and(|object| !object.is_empty())
}

fn singular_operand<'a>(
    properties: &'a [&PropertyRecord],
    name: &str,
) -> Option<&'a PropertyRecord> {
    let property = property(properties, name)?;
    let [link] = property.links.as_slice() else {
        return None;
    };
    link.object
        .as_deref()
        .filter(|object| !object.is_empty())
        .map(|_| property)
}

fn direct_root_attributes(
    property: &PropertyRecord,
    expected_tag: &str,
) -> Option<BTreeMap<String, String>> {
    let document = roxmltree::Document::parse(&property.raw_xml).ok()?;
    let roots = document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.has_tag_name(expected_tag))
        .collect::<Vec<_>>();
    if roots.len() != 1
        || document
            .descendants()
            .filter(|node| node.has_tag_name(expected_tag))
            .count()
            != 1
    {
        return None;
    }
    Some(
        roots[0]
            .attributes()
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect(),
    )
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

fn scalar_value(property: &PropertyRecord) -> Option<f64> {
    let tag = scalar_value_tag(&property.type_name)?;
    if tag == "Bool" {
        return None;
    }
    let attributes = direct_root_attributes(property, tag)?;
    let value = attributes.get("value")?.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn scalar_text(property: &PropertyRecord) -> Option<String> {
    let tag = text_value_tag(&property.type_name)?;
    direct_root_attributes(property, tag)?.remove("value")
}

fn native_parameters(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
    properties
        .iter()
        .filter_map(|property| scalar_text(property).map(|value| (property.name.clone(), value)))
        .collect()
}

fn primitive_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let length = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Length)
    };
    let angle = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .filter(|value| value.is_finite())
            .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
    };
    let signed_length = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .filter(|value| value.is_finite())
            .map(Length)
    };
    let solid = if kind.ends_with("Box") {
        PrimitiveSolid::Box {
            length: length("Length").filter(|value| value.0 > 0.0)?,
            width: length("Width").filter(|value| value.0 > 0.0)?,
            height: length("Height").filter(|value| value.0 > 0.0)?,
        }
    } else if kind.ends_with("Cylinder") {
        PrimitiveSolid::Cylinder {
            radius: length("Radius").filter(|value| value.0 > 0.0)?,
            height: length("Height").filter(|value| value.0 > 0.0)?,
            angle: angle("Angle")?,
        }
    } else if kind.ends_with("Cone") {
        let radius1 = length("Radius1")?;
        let radius2 = length("Radius2")?;
        if radius1.0 == 0.0 && radius2.0 == 0.0 {
            return None;
        }
        PrimitiveSolid::Cone {
            radius1,
            radius2,
            height: length("Height").filter(|value| value.0 > 0.0)?,
            angle: angle("Angle")?,
        }
    } else if kind.ends_with("Sphere") {
        PrimitiveSolid::Sphere {
            radius: length("Radius").filter(|value| value.0 > 0.0)?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Ellipsoid") {
        let x_radius = length("Radius2").filter(|value| value.0 > 0.0)?;
        let y_radius = length("Radius3")?;
        PrimitiveSolid::Ellipsoid {
            x_radius,
            y_radius: if y_radius.0 == 0.0 {
                x_radius
            } else {
                y_radius
            },
            z_radius: length("Radius1").filter(|value| value.0 > 0.0)?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Torus") {
        PrimitiveSolid::Torus {
            major_radius: length("Radius1").filter(|value| value.0 > 0.0)?,
            minor_radius: length("Radius2").filter(|value| value.0 > 0.0)?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Prism") {
        PrimitiveSolid::Prism {
            sides: u32::try_from(integer_property(properties, "Polygon")?).ok()?,
            circumradius: length("Circumradius").filter(|value| value.0 > 0.0)?,
            height: length("Height").filter(|value| value.0 > 0.0)?,
        }
    } else if kind.ends_with("Wedge") {
        PrimitiveSolid::Wedge {
            xmin: signed_length("Xmin")?,
            ymin: signed_length("Ymin")?,
            zmin: signed_length("Zmin")?,
            x2min: signed_length("X2min")?,
            z2min: signed_length("Z2min")?,
            xmax: signed_length("Xmax")?,
            ymax: signed_length("Ymax")?,
            zmax: signed_length("Zmax")?,
            x2max: signed_length("X2max")?,
            z2max: signed_length("Z2max")?,
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
    Some(FeatureDefinition::Primitive { solid, op })
}

fn datum_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let (origin, z_axis, x_axis, y_axis) = placement_frame(properties)?;
    Some(match kind {
        "PartDesign::Plane" => FeatureDefinition::DatumPlane {
            origin,
            normal: z_axis,
            u_axis: x_axis,
        },
        "PartDesign::Line" => FeatureDefinition::DatumAxis {
            origin,
            direction: z_axis,
        },
        "PartDesign::Point" => FeatureDefinition::DatumPoint {
            position: origin,
            construction: None,
        },
        "PartDesign::CoordinateSystem" => FeatureDefinition::DatumCoordinateSystem {
            origin,
            x_axis,
            y_axis,
            z_axis,
        },
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
        if group.links.is_empty() {
            return None;
        }
        if property(properties, "BaseFeature")
            .is_some_and(|property| property.links.iter().any(nonempty_link))
        {
            let base = singular_operand(properties, "BaseFeature")?;
            (
                BodySelection::Native(base.id.clone()),
                BodySelection::Native(group.id.clone()),
            )
        } else {
            let last = group.links.len() - 1;
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
        if shapes.links.len() < 2 {
            return None;
        }
        (
            BodySelection::Native(format!("{}:link:0", shapes.id)),
            BodySelection::Native(format!("{}:links:1..{}", shapes.id, shapes.links.len())),
        )
    };
    Some(FeatureDefinition::Combine {
        target,
        tools,
        op,
        keep_tools: false,
    })
}

fn loft_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profiles = property(properties, "Profile")
        .into_iter()
        .chain(property(properties, "Sections"))
        .flat_map(|property| &property.links)
        .filter_map(|link| link.object.as_deref())
        .map(|object| {
            sketches
                .get(object)
                .cloned()
                .map_or_else(|| ProfileRef::Native(object.to_owned()), ProfileRef::Sketch)
        })
        .collect::<Vec<_>>();
    if profiles.len() < 2 {
        return None;
    }
    let max_degree = if property(properties, "MaxDegree").is_some() {
        let value = u32::try_from(integer_property(properties, "MaxDegree")?).ok()?;
        Some((value > 0).then_some(value)?)
    } else {
        None
    };
    let part_design = kind.starts_with("PartDesign::");
    Some(FeatureDefinition::Loft {
        sections: profiles
            .into_iter()
            .map(cadmpeg_ir::features::LoftSection::Profile)
            .collect(),
        guides: Vec::new(),
        centerline: None,
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
    })
}

fn sweep_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profile_ref = |object: &str| {
        sketches
            .get(object)
            .cloned()
            .map_or_else(|| ProfileRef::Native(object.to_owned()), ProfileRef::Sketch)
    };
    let mut profiles = property(properties, "Profile")
        .into_iter()
        .chain(property(properties, "Sections"))
        .flat_map(|property| &property.links)
        .filter_map(|link| link.object.as_deref())
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
            SweepOrientation::Frenet
        } else {
            SweepOrientation::CorrectedFrenet
        }
    } else {
        match integer_property(properties, "Mode").unwrap_or(0) {
            0 => SweepOrientation::CorrectedFrenet,
            1 => SweepOrientation::Fixed,
            2 => SweepOrientation::Frenet,
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
                direction: vector_property(properties, "Binormal")?.unit()?,
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
    Some(FeatureDefinition::Sweep {
        section: cadmpeg_ir::features::SweepSection::Profile(profile),
        sections: profiles
            .into_iter()
            .map(cadmpeg_ir::features::SweepSection::Profile)
            .collect(),
        path: Some(PathRef::Native(path_property.id.clone())),
        mode: if !solid {
            SweepMode::Surface
        } else if operation_boolean(kind) == BooleanOp::NewBody {
            SweepMode::NewBody
        } else {
            SweepMode::Solid {
                op: operation_boolean(kind).try_into().ok()?,
            }
        },
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
    })
}

fn hole_definition(
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
    program_version: Option<&str>,
) -> Option<FeatureDefinition> {
    let profile = profile_ref(owner, properties, sketches);
    if matches!(profile, ProfileRef::Unresolved(_)) {
        return None;
    }
    let filter_bits = integer_selector(properties, "BaseProfileType", 6)?;
    let profile_filter = HoleProfileFilter {
        points: filter_bits & 1 != 0,
        circles: filter_bits & 2 != 0,
        arcs: filter_bits & 4 != 0,
    };
    if !profile_filter.points && !profile_filter.circles && !profile_filter.arcs {
        return None;
    }
    let positive = |name| scalar_named(properties, name).filter(|value| *value > 0.0);
    let diameter = positive("Diameter")?;
    let cut_angle = || {
        positive("HoleCutCountersinkAngle")
            .filter(|value| *value < 180.0)
            .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
    };
    let legacy_cut_types = program_version
        .and_then(freecad_program_version)
        .is_some_and(|version| version < (0, 21));
    let kind = match enumeration_selector(properties, "HoleCutType", 0)? {
        0 => HoleKind::Simple,
        1 => HoleKind::Counterbore {
            diameter: Length(positive("HoleCutDiameter")?),
            depth: Length(positive("HoleCutDepth")?),
        },
        2 => HoleKind::Countersink {
            diameter: Length(positive("HoleCutDiameter")?),
            angle: cut_angle()?,
        },
        3 if !legacy_cut_types => HoleKind::Counterdrill {
            diameter: Length(positive("HoleCutDiameter")?),
            entry_diameter: None,
            depth: Length(positive("HoleCutDepth")?),
            angle: cut_angle()?,
        },
        3 | 5 if legacy_cut_types => HoleKind::Counterbore {
            diameter: Length(positive("HoleCutDiameter")?),
            depth: Length(positive("HoleCutDepth")?),
        },
        4 if legacy_cut_types => HoleKind::Countersink {
            diameter: Length(positive("HoleCutDiameter")?),
            angle: cut_angle()?,
        },
        _ => return None,
    };
    let extent = match enumeration_selector(properties, "DepthType", 0)? {
        0 => LinearTermination::Blind {
            length: Length(positive("Depth")?),
        },
        1 => LinearTermination::ThroughAll,
        _ => return None,
    };
    let bottom = match enumeration_selector(properties, "DrillPoint", 1)? {
        0 => HoleBottom::Flat,
        1 => HoleBottom::Angled {
            included_angle: cadmpeg_ir::features::Angle(positive("DrillPointAngle")?.to_radians()),
            depth_to_tip: bool_selector(properties, "DrillForDepth", false)?,
        },
        _ => return None,
    };
    let tapered = bool_selector(properties, "Tapered", false)?;
    let taper_angle = tapered
        .then(|| {
            positive("TaperedAngle")
                .filter(|value| *value < 180.0)
                .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
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
        let standard = thread_standard(thread_type)?.into();
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
                depth: Length(positive("ThreadDepth")?),
            },
            2 => HoleThreadDepth::TappedStandard,
            _ => return None,
        };
        let clearance = if bool_selector(properties, "UseCustomThreadClearance", false)? {
            Some(Length(scalar_named(properties, "CustomThreadClearance")?))
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
                pitch: positive("ThreadPitch").map(Length),
                major_diameter: positive("ThreadDiameter").map(Length),
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
    Some(FeatureDefinition::Hole {
        profile: Some(profile),
        profile_filter: Some(profile_filter),
        face: None,
        placements: None,
        construction: HoleConstruction::Form {
            kind,
            specification,
        },
        exit_kind: None,
        diameter: Some(Length(diameter)),
        extent: Some(extent),
        bottom: Some(bottom),
        taper_angle,
        allow_multi_profile_faces: Some(bool_selector(properties, "AllowMultiFace", false)?),
    })
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
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<FeatureDefinition> {
    let law = match enumeration_selector(properties, "Mode", 0)? {
        0 => HelicalSweepLaw::PitchHeightAngle,
        1 => HelicalSweepLaw::PitchTurnsAngle,
        2 => HelicalSweepLaw::HeightTurnsAngle,
        3 => HelicalSweepLaw::HeightTurnsGrowth,
        _ => return None,
    };
    let (axis_origin, axis_direction) = vector_property(properties, "Base")
        .zip(vector_property(properties, "Axis"))
        .map(|(origin, direction)| (Point3::new(origin.x, origin.y, origin.z), direction))
        .or_else(|| axis_reference(properties, "ReferenceAxis", objects, properties_by_owner))?;
    let profile = profile_ref(owner, properties, sketches);
    if matches!(profile, ProfileRef::Unresolved(_)) {
        return None;
    }
    let construction = HelicalSweepConstruction {
        profile,
        axis_origin,
        axis_direction: axis_direction.unit()?,
        law,
        pitch: Length(scalar_named(properties, "Pitch")?),
        height: Length(scalar_named(properties, "Height")?),
        turns: scalar_named(properties, "Turns")?,
        radial_growth: Length(scalar_named(properties, "Growth")?),
        cone_angle: cadmpeg_ir::features::Angle(scalar_named(properties, "Angle")?.to_radians()),
        left_handed: bool_selector(properties, "LeftHanded", false)?,
        reversed: bool_selector(properties, "Reversed", false)?,
        tolerance: Some(float_constraint_selector(
            properties,
            "Tolerance",
            DEFAULT_HELICAL_SWEEP_TOLERANCE,
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
    Some(FeatureDefinition::HelicalSweep { construction, op })
}

fn binder_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let sources = property(properties, "Support")?
        .links
        .iter()
        .filter(|link| {
            link.object
                .as_deref()
                .is_some_and(|object| !object.is_empty())
        })
        .map(|link| {
            Some(BinderSource {
                target: binder_target(link, features)?,
                subelements: link_selectors(link).map(str::to_owned).collect(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let construction = if kind == "PartDesign::ShapeBinder" {
        BinderConstruction::Shape {
            trace_support: bool_selector(properties, "TraceSupport", false)?,
        }
    } else {
        let distance = float_selector(properties, "Offset", 0.0)?;
        let offset_join = enumeration_selector(properties, "OffsetJoinType", 0)?;
        let offset_fill = bool_selector(properties, "OffsetFill", false)?;
        let offset_open_result = bool_selector(properties, "OffsetOpenResult", false)?;
        let offset_intersection = bool_selector(properties, "OffsetIntersection", false)?;
        let offset = if distance == 0.0 {
            None
        } else {
            Some(BinderOffset {
                distance: Length(distance),
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
                    && property.links.len() == 1
                    && property.links[0].subelements.is_empty() =>
            {
                property
                    .links
                    .first()
                    .filter(|link| {
                        link.object
                            .as_deref()
                            .is_some_and(|object| !object.is_empty())
                    })
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
    Some(FeatureDefinition::Binder {
        sources,
        construction,
    })
}

fn binder_target(
    link: &crate::native::LinkTarget,
    features: &HashMap<&str, FeatureId>,
) -> Option<BinderTarget> {
    let object = link.object.as_deref()?;
    if let Some(document) = link.document.as_ref() {
        return Some(BinderTarget::External {
            document: document.clone(),
            object: object.to_owned(),
        });
    }
    Some(features.get(object).cloned().map_or_else(
        || BinderTarget::Native {
            reference: object.to_owned(),
        },
        |feature| BinderTarget::Feature { feature },
    ))
}

fn enumeration_label(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    let property = property(properties, name)?;
    if property.type_name != "App::PropertyEnumeration" {
        return None;
    }
    let document = roxmltree::Document::parse(&property.raw_xml).ok()?;
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

fn pattern_definition(
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    entries: &[EntryRecord],
) -> Option<FeatureDefinition> {
    let originals = property(properties, "Originals")
        .filter(|property| !property.links.is_empty())
        .or_else(|| {
            property(properties, "BaseFeature").filter(|property| {
                property.links.iter().any(|link| {
                    link.object
                        .as_deref()
                        .is_some_and(|object| !object.is_empty())
                })
            })
        });
    let seeds = if let Some(originals) = originals {
        let seeds = originals
            .links
            .iter()
            .filter_map(|link| link.object.as_deref())
            .map(|target| {
                features.get(target).cloned().map(Some).or_else(|| {
                    objects
                        .iter()
                        .find(|object| object.id == target)
                        .filter(|object| {
                            matches!(
                                object.type_name.as_str(),
                                "App::Line" | "App::Plane" | "App::Point" | "App::CoordinateSystem"
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
        seeds
    } else if let Some(seeds) =
        multi_transform_stage_seeds(owner, features, objects, properties_by_owner)
    {
        seeds
    } else {
        vec![implicit_body_predecessor(
            owner,
            features,
            objects,
            properties_by_owner,
        )?]
    };

    let pattern = if kind.ends_with("MultiTransform") {
        let transformations = property(properties, "Transformations")?;
        if transformations.links.is_empty() {
            return None;
        }
        let stages = transformations
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| {
                let target = link.object.as_deref()?;
                let object = objects.iter().find(|object| object.id == target)?;
                let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
                let pattern = pattern_kind(
                    &object.type_name,
                    owned,
                    objects,
                    properties_by_owner,
                    entries,
                )?;
                let combination = if index == 0 {
                    PatternStageCombination::Initialize
                } else if matches!(pattern, PatternKind::Scale { .. }) {
                    PatternStageCombination::AlignedSlices
                } else {
                    PatternStageCombination::CartesianProduct
                };
                Some(PatternStage {
                    pattern: Box::new(pattern),
                    combination,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        PatternKind::Composite { stages }
    } else {
        pattern_kind(kind, properties, objects, properties_by_owner, entries)?
    };
    Some(FeatureDefinition::Pattern {
        seeds: seeds.into_iter().map(PatternSeed::Feature).collect(),
        pattern,
    })
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
            .links
            .iter()
            .any(|link| link.object.as_deref() == Some(stage))
            .then_some(())?;
        let originals = property(owned, "Originals")
            .filter(|property| !property.links.is_empty())
            .or_else(|| property(owned, "BaseFeature"))?;
        let seeds = originals
            .links
            .iter()
            .filter_map(|link| link.object.as_deref())
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
        let position = members
            .links
            .iter()
            .position(|link| link.object.as_deref() == Some(owner))?;
        members.links[..position]
            .iter()
            .rev()
            .filter_map(|link| link.object.as_deref())
            .find_map(|member| features.get(member).cloned())
    })
}

fn pattern_kind(
    kind: &str,
    properties: &[&PropertyRecord],
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    entries: &[EntryRecord],
) -> Option<PatternKind> {
    if kind.ends_with("Mirrored") {
        return Some(
            if let Some((plane_origin, plane_normal)) =
                plane_reference(properties, "MirrorPlane", objects, properties_by_owner)
            {
                PatternKind::Mirror {
                    plane_origin,
                    plane_normal,
                }
            } else {
                PatternKind::MirrorReference {
                    plane: cadmpeg_ir::features::FaceSelection::Native(
                        property(properties, "MirrorPlane")?.id.clone(),
                    ),
                }
            },
        );
    }

    let count = if kind.ends_with("Scaled") {
        integer_selector(properties, "Occurrences", 2)?
    } else {
        let absent_default = if kind.ends_with("PolarPattern") { 3 } else { 2 };
        integer_constraint_selector(properties, "Occurrences", absent_default, true)?
    };
    if count == 0 || count > MAX_SKETCH_RECORDS as u64 {
        return None;
    }
    let count = count as u32;
    let mode = enumeration_selector(properties, "Mode", 0)?;

    if kind.ends_with("Scaled") {
        let final_factor = scalar_named(properties, "Factor")?;
        return (final_factor.is_finite() && final_factor > 0.0 && count >= 2).then_some(
            PatternKind::Scale {
                center: PatternScaleCenter::FirstSeedCentroid,
                final_factor,
                count,
            },
        );
    }

    let pattern = if kind.ends_with("LinearPattern") {
        let first = linear_pattern_axis(
            properties,
            "",
            count,
            mode,
            objects,
            properties_by_owner,
            entries,
        )?;
        let count2 = integer_constraint_selector(properties, "Occurrences2", 1, false)?;
        if count2 == 0 || count2 > MAX_SKETCH_RECORDS as u64 {
            return None;
        }
        if count2 > 1 {
            let mode2 = enumeration_selector(properties, "Mode2", 0)?;
            let second = linear_pattern_axis(
                properties,
                "2",
                count2 as u32,
                mode2,
                objects,
                properties_by_owner,
                entries,
            )?;
            PatternKind::Composite {
                stages: vec![
                    PatternStage {
                        pattern: Box::new(first),
                        combination: PatternStageCombination::Initialize,
                    },
                    PatternStage {
                        pattern: Box::new(second),
                        combination: PatternStageCombination::CartesianProduct,
                    },
                ],
            }
        } else {
            first
        }
    } else if kind.ends_with("PolarPattern") {
        let (axis_origin, mut axis_dir) =
            axis_reference(properties, "Axis", objects, properties_by_owner)?;
        if bool_selector(properties, "Reversed", false)? {
            axis_dir = Vector3::new(-axis_dir.x, -axis_dir.y, -axis_dir.z);
        }
        let angles = pattern_locations(properties, "", count, mode, "Angle", "Offset", entries)?;
        if let Some(step) = uniform_step(&angles) {
            PatternKind::Circular {
                axis_origin,
                axis_dir,
                angle: cadmpeg_ir::features::Angle((step * f64::from(count - 1)).to_radians()),
                count,
            }
        } else {
            PatternKind::CircularAngles {
                axis_origin,
                axis_dir,
                angles: angles
                    .into_iter()
                    .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()))
                    .collect(),
            }
        }
    } else {
        return None;
    };
    Some(pattern)
}

fn linear_pattern_axis(
    properties: &[&PropertyRecord],
    suffix: &str,
    count: u32,
    mode: u64,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    entries: &[EntryRecord],
) -> Option<PatternKind> {
    let name = |base: &str| format!("{base}{suffix}");
    let mut direction =
        axis_reference(properties, &name("Direction"), objects, properties_by_owner)
            .map(|(_, direction)| direction);
    if bool_selector(properties, &name("Reversed"), false)? {
        direction =
            direction.map(|direction| Vector3::new(-direction.x, -direction.y, -direction.z));
    }
    let offsets = pattern_locations(properties, suffix, count, mode, "Length", "Offset", entries)?;
    if let Some(spacing) = uniform_step(&offsets) {
        Some(PatternKind::Linear {
            direction,
            spacing: Length(spacing),
            count,
            second: None,
        })
    } else {
        Some(PatternKind::LinearOffsets {
            direction,
            offsets: offsets.into_iter().map(Length).collect(),
        })
    }
}

fn pattern_locations(
    properties: &[&PropertyRecord],
    suffix: &str,
    count: u32,
    mode: u64,
    extent_base: &str,
    offset_base: &str,
    entries: &[EntryRecord],
) -> Option<Vec<f64>> {
    if count == 0 {
        return None;
    }
    if count == 1 {
        return Some(vec![0.0]);
    }
    let name = |base: &str| format!("{base}{suffix}");
    let intervals = match mode {
        0 => {
            let interval = scalar_named(properties, &name(extent_base))? / f64::from(count - 1);
            alloc_filled(count as usize - 1, interval, "freecad pattern intervals").ok()?
        }
        1 => {
            let fallback = scalar_named(properties, &name(offset_base))?;
            let spacings = property(properties, &name("Spacings")).map_or_else(
                || Some(Vec::new()),
                |property| numeric_list(property, entries),
            )?;
            let pattern = property(properties, &name("SpacingPattern")).map_or_else(
                || Some(Vec::new()),
                |property| numeric_list(property, entries),
            )?;
            if !spacings.is_empty() && spacings.len() != count as usize - 1 {
                return None;
            }
            (0..count as usize - 1)
                .map(|index| {
                    let explicit = spacings.get(index).copied().unwrap_or(-1.0);
                    if explicit != -1.0 {
                        explicit
                    } else if pattern.len() > 1 {
                        pattern[index % pattern.len()]
                    } else {
                        fallback
                    }
                })
                .collect()
        }
        _ => return None,
    };
    let mut locations = Vec::with_capacity(count as usize);
    locations.push(0.0);
    let mut location = 0.0;
    for interval in intervals {
        if !interval.is_finite() || interval <= 0.0 {
            return None;
        }
        location += interval;
        if !location.is_finite() {
            return None;
        }
        locations.push(location);
    }
    Some(locations)
}

fn uniform_step(locations: &[f64]) -> Option<f64> {
    let step = *locations.get(1)?;
    locations
        .windows(2)
        .all(|pair| (pair[1] - pair[0] - step).abs() <= f64::EPSILON * step.abs().max(1.0))
        .then_some(step)
}

fn axis_reference(
    properties: &[&PropertyRecord],
    name: &str,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<(Point3, Vector3)> {
    if let Some(direction) = vector_property(properties, name) {
        return Some((Point3::new(0.0, 0.0, 0.0), direction.unit()?));
    }
    let (link, selector) = singular_reference_link(property(properties, name)?)?;
    let target = link.object.as_deref()?;
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
    Some((origin, direction.unit()?))
}

fn plane_reference(
    properties: &[&PropertyRecord],
    name: &str,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<(Point3, Vector3)> {
    let (link, selector) = singular_reference_link(property(properties, name)?)?;
    let target = link.object.as_deref()?;
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
    Some((origin, normal.unit()?))
}

fn link_selectors(link: &crate::native::LinkTarget) -> impl Iterator<Item = &str> {
    link.subelements
        .iter()
        .flat_map(|selector| selector.split_ascii_whitespace())
        .filter(|selector| !selector.is_empty())
}

fn singular_reference_link(
    property: &PropertyRecord,
) -> Option<(&crate::native::LinkTarget, Option<&str>)> {
    let link = scalar_link(property)?;
    let object = link.object.as_deref()?;
    if object.is_empty() {
        return None;
    }
    let selectors = link_selectors(link).collect::<Vec<_>>();
    let selector = match selectors.as_slice() {
        [] => None,
        [selector] => Some(*selector),
        _ => return None,
    };
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
    let [link] = property.links.as_slice() else {
        return None;
    };
    Some(link)
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

fn scalar_named(properties: &[&PropertyRecord], name: &str) -> Option<f64> {
    property(properties, name).and_then(scalar_value)
}

fn string_property_value(property: &PropertyRecord) -> Option<String> {
    (property.type_name == "App::PropertyString").then_some(())?;
    direct_root_attributes(property, "String")?.remove("value")
}

fn integer_property(properties: &[&PropertyRecord], name: &str) -> Option<u64> {
    let value = scalar_named(properties, name)?;
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0).then_some(value as u64)
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
    let value = direct_root_attributes(property, "Integer")?
        .get("value")?
        .parse::<i64>()
        .ok()?;
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
    let value = direct_root_attributes(property, "Integer")?
        .get("value")?
        .parse::<i64>()
        .ok()?;
    u64::try_from(value).ok()
}

fn numeric_list(property: &PropertyRecord, entries: &[EntryRecord]) -> Option<Vec<f64>> {
    if property.type_name != "App::PropertyFloatList" {
        return None;
    }
    let file = direct_root_attributes(property, "FloatList")?
        .get("file")
        .cloned()?;
    if file.is_empty() {
        return property.side_entries.is_empty().then(Vec::new);
    }
    if property.side_entries.len() != 1 || property.side_entries[0] != file {
        return None;
    }
    let data = entries
        .iter()
        .find(|entry| entry.name == file)?
        .data
        .as_slice();
    let mut view = View::over_retained(data);
    let count = view.u32_le()? as usize;
    if count > MAX_SKETCH_RECORDS {
        return None;
    }
    let values = view.read_counted(count as u64, 8, View::f64_le)?;
    if !view.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(values)
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

fn feature_id(object: &ObjectRecord) -> FeatureId {
    FeatureId(format!("fcstd:design:feature#{}", object.name))
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
    if property.type_name != "App::PropertyLink" || property.links.len() != 1 {
        return None;
    }
    let source = property.links[0].object.as_deref()?;
    Some(FeatureDefinition::DerivedGeometry {
        source: feature_ids.get(source)?.clone(),
    })
}

fn imported_geometry_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let path = property(properties, "FileName").and_then(string_property_value)?;
    if path.is_empty() {
        return None;
    }
    let format = match kind {
        "Part::ImportStep" => GeometryImportFormat::Step,
        "Part::ImportIges" => GeometryImportFormat::Iges,
        "Part::ImportBrep" | "Part::CurveNet" => GeometryImportFormat::Brep,
        _ => return None,
    };
    Some(FeatureDefinition::ImportedGeometry { path, format })
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
    objects: &[ObjectRecord],
    features: &[Feature],
) -> Result<Vec<crate::native::DesignCensusRecord>, CodecError> {
    let features = features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native_ref| (native_ref, feature))
        })
        .collect::<HashMap<_, _>>();
    let mut census = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .map(|object| {
            let feature = features.get(object.id.as_str()).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "design object {} has no neutral history projection",
                    object.id
                ))
            })?;
            let (definition, post_processed) = match &feature.definition {
                FeatureDefinition::PostProcess { operation, .. } => (operation.as_ref(), true),
                definition => (definition, false),
            };
            let value = serde_json::to_value(definition).map_err(|error| {
                CodecError::malformed(format_args!(
                    "cannot classify design feature {}: {error}",
                    feature.id
                ))
            })?;
            let semantic_kind = value
                .get("definition")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "design feature {} has no semantic family tag",
                        feature.id
                    ))
                })?
                .to_owned();
            Ok(crate::native::DesignCensusRecord {
                id: crate::native::native_child_id("design-census", &object.id, "projection"),
                object: object.id.clone(),
                type_name: object.type_name.clone(),
                feature: feature.id.0.clone(),
                neutral: !matches!(definition, FeatureDefinition::Native { .. }),
                semantic_kind,
                post_processed,
            })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    census.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(census)
}

#[cfg(test)]
mod profile_tests {
    use super::*;

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
            cadmpeg_ir::sketches::SketchEntityId(id.into()),
            SketchId("test:sketch#curved".into()),
            geometry,
        )
    }

    #[test]
    fn curved_segments_chain_by_their_evaluated_endpoints() {
        let entities = [
            entity(
                "test:entity#line",
                SketchGeometry::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            ),
            entity(
                "test:entity#arc",
                SketchGeometry::Arc {
                    center: Point2::new(0.0, 0.0),
                    radius: Length(1.0),
                    start_angle: cadmpeg_ir::features::Angle(0.0),
                    end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                },
            ),
            entity(
                "test:entity#line-after-arc",
                SketchGeometry::Line {
                    start: Point2::new(0.0, 1.0),
                    end: Point2::new(1.0, 1.0),
                },
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
                    &format!("test:entity#{ordinal}"),
                    SketchGeometry::Line {
                        start: Point2::new(ordinal as f64 * 10.0, 0.0),
                        end: Point2::new(ordinal as f64 * 10.0 + 1.0, 0.0),
                    },
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
            "test:entity#1",
            SketchGeometry::Line {
                start: Point2::new(-1.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        );
        construction.construction = true;
        let entities = vec![
            construction,
            entity(
                "test:entity#2",
                SketchGeometry::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length(2.0),
                },
            ),
            entity(
                "test:entity#3",
                SketchGeometry::Circle {
                    center: Point2::new(10.0, 0.0),
                    radius: Length(2.0),
                },
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
                "test:entity#1",
                SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            ),
            entity(
                "test:entity#2",
                SketchGeometry::Line {
                    start: Point2::new(2.0, 0.0),
                    end: Point2::new(3.0, 0.0),
                },
            ),
        ];
        let constraint = SketchConstraint {
            id: SketchConstraintId("test:constraint#1".into()),
            sketch: entities[0].sketch.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entities[0].id().clone()),
                    SketchLocus::Start(entities[1].id().clone()),
                ],
            },
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
                "test:entity#anchor",
                SketchGeometry::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(0.0, 0.0),
                },
            ),
            entity(
                "test:entity#nearby",
                SketchGeometry::Line {
                    start: Point2::new(32.0 * f64::EPSILON, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            ),
            entity(
                "test:entity#constrained",
                SketchGeometry::Line {
                    start: Point2::new(2.0, 0.0),
                    end: Point2::new(3.0, 0.0),
                },
            ),
        ];
        let constraint = SketchConstraint {
            id: SketchConstraintId("test:constraint#explicit-precedence".into()),
            sketch: entities[0].sketch.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entities[0].id().clone()),
                    SketchLocus::Start(entities[2].id().clone()),
                ],
            },
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
                    "test:entity#anchor",
                    SketchGeometry::Line {
                        start: Point2::new(0.0, 0.0),
                        end: Point2::new(1.0, 0.0),
                    },
                ),
                entity(
                    "test:entity#continuation",
                    SketchGeometry::Line {
                        start: Point2::new(1.0 + gap, 0.0),
                        end: Point2::new(2.0, 0.0),
                    },
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
                "test:entity#anchor",
                SketchGeometry::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(0.0, 0.0),
                },
            ),
            entity(
                "test:entity#first-continuation",
                SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            ),
            entity(
                "test:entity#second-continuation",
                SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(0.0, 1.0),
                },
            ),
        ];
        let constraint = SketchConstraint {
            id: SketchConstraintId("test:constraint#ambiguous-explicit".into()),
            sketch: entities[0].sketch.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entities[0].id().clone()),
                    SketchLocus::Start(entities[1].id().clone()),
                    SketchLocus::Start(entities[2].id().clone()),
                ],
            },
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
                    &format!("test:entity#{}", ordinal + 1),
                    SketchGeometry::Line {
                        start: Point2::new(0.0, 0.0),
                        end: Point2::new(ordinal as f64 + 1.0, 1.0),
                    },
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
