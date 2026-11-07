// SPDX-License-Identifier: Apache-2.0
//! Author Design feature scopes and their scalar owners.

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};
use crate::records::parameters::{
    DesignParameter, DesignParameterContent, DesignParameterDiscriminator, DesignParameterOwner,
    DesignParameterOwnerContent, DesignParameterSource,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BooleanOp, Dimension, DimensionValue, FeatureDefinition, FeatureId, FeatureOperation,
    ParameterId, ParameterValue, PrimitiveSolidKind, RadialDimension,
};

struct Scalar {
    kind: &'static str,
    value: f64,
    parameter: Option<ParameterId>,
    name: Option<String>,
    expression: Option<String>,
}

struct Extrusion {
    ordinal: u64,
    id: FeatureId,
    container: u32,
    distance: Scalar,
    current_state: u32,
    previous_state: u32,
}

struct EmptyBaseFeature {
    ordinal: u64,
    id: FeatureId,
    reference: u32,
    state: u32,
}

struct NativeBaseFlange {
    ordinal: u64,
    id: FeatureId,
    references: [u32; 4],
    thickness: f64,
}

fn select_native_base_flanges(
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> Vec<NativeBaseFlange> {
    target
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters }) =
                feature.evaluation.definition()
            else {
                return None;
            };
            if kind.as_str() != "BaseFlange"
                || !parameters.is_empty()
                || feature
                    .source_tag
                    .as_deref()
                    .is_some_and(|tag| tag != "BaseFlange")
                || feature.suppressed == Some(true)
                || feature.needs_recompute
                || feature.product_definition.is_some()
                || !feature.dependencies.is_empty()
                || feature.source_text.is_some()
                || !feature.source_content.is_empty()
                || !feature.evaluation.outputs().is_empty()
                || feature.source_properties.len() != 4
            {
                return None;
            }
            let scope = native
                .design_parameter_scopes
                .iter()
                .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
            let operation = scope.base_flange_operation()?;
            let references = [
                operation.profile_group_record_index,
                operation.profile_record_index,
                operation.thickness_record_index,
                operation.settings_record_index,
            ];
            if !scope.reference_members().values().copied().eq(references)
                || references.iter().enumerate().any(|(index, value)| {
                    feature
                        .source_properties
                        .get(format!("reference:{index}").as_str())
                        != Some(&value.to_string())
                })
                || scope.history_state_id().is_some()
            {
                return None;
            }
            Some(NativeBaseFlange {
                ordinal: feature.ordinal,
                id: feature.id.clone(),
                references,
                thickness: operation.thickness.get(),
            })
        })
        .collect()
}

fn select_empty_base_features(
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> Vec<EmptyBaseFeature> {
    target
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies:
                    cadmpeg_ir::features::BodySelection::Resolved {
                        bodies,
                        native: selection,
                    },
            }) = feature.evaluation.definition()
            else {
                return None;
            };
            if !bodies.is_empty()
                || feature.native_ref.as_deref() != Some(selection.as_str())
                || feature.suppressed == Some(true)
                || feature.needs_recompute
                || feature.product_definition.is_some()
                || !feature.dependencies.is_empty()
                || !feature.source_properties.is_empty()
                || feature.source_text.is_some()
                || !feature.source_content.is_empty()
                || !feature.evaluation.outputs().is_empty()
                || feature
                    .source_tag
                    .as_deref()
                    .is_some_and(|tag| tag != "Base Feature")
            {
                return None;
            }
            let scope = native
                .design_parameter_scopes
                .iter()
                .find(|scope| scope.id == *selection)?;
            if scope
                .base_feature_construction()?
                .body_entity_suffixes()
                .len()
                != 0
            {
                return None;
            }
            let mut references = scope.reference_members().values();
            let reference = *references.next()?;
            if references.next().is_some() {
                return None;
            }
            let state = u32::try_from(scope.history_state_id()?).ok()?;
            Some(EmptyBaseFeature {
                ordinal: feature.ordinal,
                id: feature.id.clone(),
                reference,
                state,
            })
        })
        .collect()
}

struct SketchDimension {
    constraint: cadmpeg_ir::sketches::SketchConstraintId,
    scope: u32,
    ordinal: u32,
    source_ordinal: u32,
    source_kind: String,
    construction: DimensionConstruction,
    scalar: Scalar,
}

enum DimensionConstruction {
    Points([u32; 2]),
    BodyRecipe(u32),
}

fn select_sketch_dimensions(
    target: &CadIr,
    native: &crate::native::F3dNative,
    sketch: Option<&super::sketches::SketchStream>,
) -> Vec<SketchDimension> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchGeometryDefinition, SketchLocus,
    };
    let Some(sketch) = sketch else {
        return Vec::new();
    };
    target.model.sketch_constraints.iter().filter_map(|constraint| {
        if constraint.name.is_some() || constraint.driving.is_some() || constraint.active.is_some()
            || constraint.virtual_space.is_some() || constraint.visible.is_some()
            || constraint.orientation.is_some() || constraint.label_distance.is_some()
            || constraint.label_position.is_some() || constraint.metadata.is_some() { return None; }
        let (parameter_id, construction, source_kind) = match constraint.definition.kind() {
            SketchConstraintDefinitionInput::HorizontalDistance {
                first: SketchLocus::Entity(first), second: SketchLocus::Entity(second), parameter: Dimension::Parameter(id),
            } => {
                let position = |id| {
                    let entity = target.model.sketch_entities.iter().find(|entity| entity.id() == id && entity.sketch == constraint.sketch)?;
                    let SketchGeometryDefinition::Point { position } = entity.geometry.definition() else { return None };
                    Some(position.get())
                };
                let a = position(first)?;
                let b = position(second)?;
                let expected = target.dimension_value(&Dimension::<cadmpeg_ir::scalar::Length>::Parameter(id.clone()))?.get();
                let tolerance = target.tolerances.linear_or_default().get();
                if (a.v - b.v).abs() > tolerance || ((a.u - b.u).abs() - expected.abs()).abs() > tolerance { return None; }
                let first = *sketch.point_indices.get(first)?;
                let second = *sketch.point_indices.get(second)?;
                if first == second { return None; }
                (id, DimensionConstruction::Points([first, second]), "Linear Dimension".to_owned())
            }
            SketchConstraintDefinitionInput::Native { native_kind, native_state: None, native_flags: None,
                native_properties, entities, parameter: Some(id), operands } if native_properties.is_empty() && operands.len() == 1 => {
                let operand = &operands[0];
                let record = native.design_dimension_recipe_records.iter().find(|record| operand.native_ref.as_deref() == Some(record.id.as_str()))?;
                if !native_kind.as_str().starts_with("Linear Dimension") || operand.object_index != Some(record.record_index)
                    || operand.native_kind.as_str() != "construction_recipe"
                    || !operand.field.as_ref().is_some_and(|field| field.name.as_str() == "recipe" && field.role.is_none())
                    || record.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Body
                    || record.program != [0, 0] || !record.references.is_empty() || record.prefix_bytes.byte_len() != 9
                    || record.prefix_bytes.digest() != &cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0; 9])
                    || entities.iter().any(|id| !sketch.point_indices.contains_key(id)) { return None; }
                (id, DimensionConstruction::BodyRecipe(record.record_index), native_kind.as_str().to_owned())
            }
            _ => return None,
        };
        let parameter_row = target.model.parameters.iter().find(|row| &row.id == parameter_id)?;
        let owner = parameter_row.owner.as_ref()?;
        let (_, _, scope) = sketch.feature_scopes.iter().find(|(_, feature, _)| feature == owner)?;
        let feature = target.model.features.iter().find(|feature| &feature.id == owner)?;
        if !matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(id)),
        }) if id == &constraint.sketch) { return None; }
        let parameter = Dimension::<cadmpeg_ir::scalar::Length>::Parameter(parameter_id.clone());
        let scalar = scalar(target, owner, "Linear Dimension", parameter_row.ordinal, &parameter)?;
        Some(SketchDimension { constraint: constraint.id.clone(), scope: *scope,
            ordinal: parameter_row.ordinal, source_ordinal: parameter_row.ordinal,
            source_kind, construction, scalar })
    }).collect()
}

fn select_extrusions(
    target: &CadIr,
    native: &crate::native::F3dNative,
    sketch: Option<&super::sketches::SketchStream>,
) -> Vec<Extrusion> {
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrudeExtent, ExtrudeStart, LinearTermination, PlanarProfileRef,
        ProfileRef, SketchFeatureBinding,
    };
    let Some(sketch) = sketch else {
        return Vec::new();
    };
    target.model.features.iter().filter_map(|feature| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)),
            direction: ExtrudeDirection::ProfileNormal {},
            start: ExtrudeStart::ProfilePlane {},
            extent: ExtrudeExtent::OneSided { side },
            op: BooleanOp::NewBody,
            solid: Some(true), face_maker: None, inner_wire_taper: None,
            length_along_profile_normal: None, allow_multi_profile_faces: None,
        }) = feature.evaluation.definition() else { return None };
        let LinearTermination::Blind { length } = &side.termination else { return None };
        if !matches!(length, Dimension::Parameter(_)) { return None; }
        if side.draft.is_some() || feature.suppressed == Some(true) || feature.needs_recompute
            || feature.product_definition.is_some()
            || !feature.source_properties.is_empty() || feature.source_text.is_some()
            || !feature.source_content.is_empty() || !feature.evaluation.outputs().is_empty()
            || feature.source_tag.as_deref().is_some_and(|tag| tag != "Extrude") { return None; }
        let owner = target.model.features.iter().find(|candidate| matches!(candidate.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(id)) }) if id == sketch_id))?;
        if feature.dependencies.len() != 1 || feature.dependencies.iter().any(|id| id != &owner.id) { return None; }
        let mut inputs = target.model.feature_input_topologies.iter().filter(|input| input.input_of == feature.id);
        let input = inputs.next()?;
        if inputs.next().is_some() || !input.bodies.is_empty() || !input.faces.is_empty()
            || !input.edges.is_empty() || !input.vertices.is_empty() || !input.edge_geometry.is_empty()
            || input.operands.len() != 1 { return None; }
        let scope = native.design_parameter_scopes.iter().find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
        let current_state = u32::try_from(scope.history_state_id()?).ok()?;
        let previous_state = u32::try_from(scope.previous_history_state_id()?).ok()?;
        if current_state == u32::MAX || previous_state == u32::MAX { return None; }
        let container = *sketch.feature_containers.get(&owner.id)?;
        let distance = scalar(target, &feature.id, "AlongDistance", 2, length)?;
        if distance.value <= 0.0 { return None; }
        Some(Extrusion { ordinal: feature.ordinal, id: feature.id.clone(), container, distance, current_state, previous_state })
    }).collect()
}

fn scalar<Q: DimensionValue>(
    target: &CadIr,
    feature: &FeatureId,
    kind: &'static str,
    ordinal: u32,
    dimension: &Dimension<Q>,
) -> Option<Scalar> {
    let ParameterValue::Length(value) = target.dimension_value(dimension)?.to_parameter_value()?
    else {
        return None;
    };
    let source_value = value.get() / 10.0;
    if value.get() != 0.0 && source_value == 0.0 {
        return None;
    }
    let parameter = match dimension {
        Dimension::Literal(_) => None,
        Dimension::Parameter(id) => Some(
            target
                .model
                .parameters
                .iter()
                .find(|parameter| &parameter.id == id)?,
        ),
    };
    if parameter.is_some_and(|parameter| {
        parameter.owner.as_ref() != Some(feature)
            || parameter.ordinal != ordinal
            || !parameter.driving
            || parameter.display.is_some()
            || parameter.pmi.is_some()
            || parameter.product_definition.is_some()
            || !parameter.properties.is_empty()
            || parameter.name.trim().is_empty()
    }) {
        return None;
    }
    let expression = match parameter {
        Some(parameter) => Some(parameter.expression.render_with(|id| {
            target
                .model
                .parameters
                .iter()
                .find(|candidate| &candidate.id == id)
                .map(|candidate| candidate.name.clone())
        })?),
        None => None,
    };
    Some(Scalar {
        kind,
        value: source_value,
        parameter: parameter.map(|parameter| parameter.id.clone()),
        name: parameter.map(|parameter| parameter.name.clone()),
        expression,
    })
}

type Primitive = (u64, &'static str, Vec<Scalar>, u32, FeatureId);

struct CopyFeature {
    ordinal: u64,
    id: FeatureId,
    bodies: Vec<(u64, u32, u32)>,
}

fn select_copies(
    target: &CadIr,
    native: &crate::native::F3dNative,
    attributes: &super::attributes::AttributeIndex<'_>,
) -> Result<Vec<CopyFeature>, CodecError> {
    let mut selected = Vec::new();
    for feature in &target.model.features {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::InsertBodies {
                bodies: cadmpeg_ir::features::InsertedBodies::Resolved { .. }
            })
        ) || feature.suppressed == Some(true)
            || !feature.dependencies.is_empty()
            || !feature.source_properties.is_empty()
            || feature.source_text.is_some()
            || !feature.source_content.is_empty()
        {
            continue;
        }
        let Some(scope) = native
            .design_parameter_scopes
            .iter()
            .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        let Some(operation) = scope.copy_paste_bodies_operation() else {
            continue;
        };
        let mut bodies = Vec::new();
        for copied in operation.bodies() {
            let mut bindings = native.design_body_bindings.iter().filter(|binding| {
                crate::ids::native_stream(binding.id()) == crate::ids::native_stream(&scope.id)
                    && binding.entity_suffix == u64::from(copied.copied.value)
            });
            let Some(binding) = bindings.next() else {
                break;
            };
            let Some(row) = binding.body.as_ref() else {
                break;
            };
            if bindings.any(|candidate| candidate.body.as_ref() != Some(row)) {
                break;
            }
            let Some((ordinal, body)) = target.model.bodies.iter().enumerate().find(|(_, body)| {
                body.native_ref.as_deref() == Some(row.as_str())
                    && feature.evaluation.outputs().contains(&body.id)
            }) else {
                break;
            };
            let key = u64::try_from(super::attributes::source_less_body_key(
                attributes, body, ordinal,
            )?)
            .map_err(CodecError::malformed)?;
            bodies.push((key, copied.source.value, copied.copied.value));
        }
        if !bodies.is_empty()
            && bodies.len() == operation.bodies().len()
            && bodies.len() == feature.evaluation.outputs().len()
        {
            selected.push(CopyFeature {
                ordinal: feature.ordinal,
                id: feature.id.clone(),
                bodies,
            });
        }
    }
    Ok(selected)
}

fn select(target: &CadIr) -> Vec<Primitive> {
    let mut primitives = Vec::new();
    for feature in &target.model.features {
        if feature.suppressed == Some(true)
            || !feature.dependencies.is_empty()
            || !feature.source_properties.is_empty()
            || feature.source_text.is_some()
            || !feature.source_content.is_empty()
            || feature.product_definition.is_some()
        {
            continue;
        }
        let (kind, values, op) = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Primitive {
                placement: None,
                solid,
                op,
            }) => {
                let PrimitiveSolidKind::Cylinder {
                    radius,
                    height,
                    angle: Dimension::Literal(angle),
                } = solid.kind()
                else {
                    continue;
                };
                if *angle != cadmpeg_ir::scalar::Angle::FULL_TURN {
                    continue;
                }
                let diameter = match radius {
                    RadialDimension::Diameter(diameter) => {
                        scalar(target, &feature.id, "Diameter", 1, diameter)
                    }
                    RadialDimension::Radius(Dimension::Literal(radius)) => {
                        let value = radius.get() / 10.0 * 2.0;
                        (value.is_finite() && value != 0.0 && (value * 10.0).is_finite()).then_some(
                            Scalar {
                                kind: "Diameter",
                                value,
                                parameter: None,
                                name: None,
                                expression: None,
                            },
                        )
                    }
                    RadialDimension::Radius(_) => None,
                };
                (
                    "CylinderPrimitive",
                    vec![
                        match scalar(target, &feature.id, "Height", 0, height) {
                            Some(value) => value,
                            None => continue,
                        },
                        match diameter {
                            Some(value) => value,
                            None => continue,
                        },
                    ],
                    op,
                )
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some([length, width, height]),
                placement: Some(placement),
                op,
            }) => {
                let rows = placement.rows();
                if rows[0][0..3] != [1.0, 0.0, 0.0]
                    || rows[1][0..3] != [0.0, 1.0, 0.0]
                    || rows[2] != [0.0, 0.0, 1.0, 0.0]
                    || [rows[0][3], rows[1][3]]
                        .into_iter()
                        .any(|value| value != 0.0 && value / 10.0 == 0.0)
                {
                    continue;
                }
                (
                    "BoxPrimitive",
                    vec![
                        match scalar(target, &feature.id, "Length", 0, length) {
                            Some(value) => value,
                            None => continue,
                        },
                        match scalar(target, &feature.id, "Width", 1, width) {
                            Some(value) => value,
                            None => continue,
                        },
                        match scalar(target, &feature.id, "Height", 2, height) {
                            Some(value) => value,
                            None => continue,
                        },
                        Scalar {
                            kind: "OffsetX",
                            value: rows[0][3] / 10.0,
                            parameter: None,
                            name: None,
                            expression: None,
                        },
                        Scalar {
                            kind: "OffsetY",
                            value: rows[1][3] / 10.0,
                            parameter: None,
                            name: None,
                            expression: None,
                        },
                    ],
                    op,
                )
            }
            _ => continue,
        };
        if feature.source_tag.as_deref().is_some_and(|tag| tag != kind) {
            continue;
        }
        let operation = match op {
            BooleanOp::Join => 1u32,
            BooleanOp::Cut => 2,
            BooleanOp::Intersect => 3,
            BooleanOp::NewBody => 4,
            BooleanOp::Unresolved => continue,
        };
        primitives.push((feature.ordinal, kind, values, operation, feature.id.clone()));
    }
    primitives
}

pub(in crate::writer) fn authored_feature_ids(
    target: &CadIr,
) -> std::collections::BTreeSet<FeatureId> {
    select(target)
        .into_iter()
        .map(|(_, _, _, _, id)| id)
        .collect()
}

pub(super) struct EncodedFeatures {
    pub(super) stream: Option<(Vec<u8>, Vec<u8>)>,
    pub(super) features: Vec<FeatureId>,
    pub(super) parameters: Vec<ParameterId>,
    pub(super) collection_index: u32,
    pub(super) append: Option<DesignAppend>,
}

pub(super) struct DesignAppend {
    pub(super) bulk: Vec<u8>,
    pub(super) primary: Vec<crate::metastream::RecordIndexEntry>,
    pub(super) secondary: Vec<crate::metastream::RecordIndexEntry>,
    pub(super) types: Vec<GeneratedDesignType>,
    pub(super) body_pairs: Vec<(u64, u64)>,
}

pub(super) fn encode(
    target: &CadIr,
    native: &crate::native::F3dNative,
    attributes: &super::attributes::AttributeIndex<'_>,
    brep_name: &str,
    design_registry: &GeneratedDesignRegistry,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut sketch: Option<&mut super::sketches::SketchStream>,
) -> Result<Option<EncodedFeatures>, CodecError> {
    let mut primitives = select(target);
    let mut extrusions = select_extrusions(target, native, sketch.as_deref());
    let base_features = select_empty_base_features(target, native);
    let base_flanges = select_native_base_flanges(target, native);
    let mut dimensions = select_sketch_dimensions(target, native, sketch.as_deref());
    extrusions.sort_by_key(|extrusion| extrusion.ordinal);
    let mut copies = select_copies(target, native, attributes)?;
    let joined = sketch
        .as_ref()
        .is_some_and(|sketch| !sketch.feature_scopes.is_empty());
    if joined && design_registry.body_map.is_some() {
        copies.clear();
    }
    if design_registry.body_map.is_none()
        && (!target.model.appearance_bindings.is_empty()
            || copies.iter().map(|copy| copy.bodies.len()).sum::<usize>()
                != target.model.bodies.len())
    {
        copies.clear();
    }
    let append_main = !copies.is_empty() && design_registry.body_map.is_some();
    if primitives.is_empty()
        && copies.is_empty()
        && base_features.is_empty()
        && base_flanges.is_empty()
        && !joined
    {
        return Ok(None);
    }
    primitives.sort_by_key(|primitive| primitive.0);
    let type_start = if joined {
        u32::try_from(
            sketch
                .as_ref()
                .map_or(0, |sketch| sketch.registry.types.len()),
        )
        .ok()
        .and_then(|value| value.checked_add(256))
        .ok_or_else(|| CodecError::InvalidInput("F3D feature type count exceeds u32".into()))?
    } else if append_main {
        u32::try_from(design_registry.types.len())
            .ok()
            .and_then(|n| n.checked_add(256))
            .ok_or_else(|| CodecError::InvalidInput("F3D feature type count exceeds u32".into()))?
    } else {
        256
    };
    let tags = (0..15u32)
        .map(|offset| {
            type_start
                .checked_add(offset)
                .map(|value| value.to_string())
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CodecError::InvalidInput("F3D feature type count exceeds u32".into()))?;
    let mut occupied = sketch
        .as_ref()
        .filter(|_| joined)
        .map_or_else(std::collections::BTreeSet::new, |sketch| {
            sketch.occupied.clone()
        });
    let mut next = sketch
        .as_ref()
        .filter(|_| joined)
        .map_or(1, |sketch| sketch.next);
    dimensions.retain(|dimension| match dimension.construction {
        DimensionConstruction::Points(_) => true,
        DimensionConstruction::BodyRecipe(index) => index != 0 && occupied.insert(index),
    });
    occupied.extend(base_flanges.iter().flat_map(|feature| feature.references));
    if append_main {
        occupied.extend(
            design_registry
                .types
                .iter()
                .flat_map(|kind| kind.entity_ids.iter())
                .filter_map(|id| u32::try_from(*id).ok()),
        );
        next = occupied
            .last()
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .unwrap_or(0);
    }
    let mut bases = Vec::new();
    for _ in &primitives {
        bases.push(reserve_indices(&mut occupied, &mut next, 16)?);
    }
    let mut extrusion_bases = Vec::new();
    for _ in &extrusions {
        extrusion_bases.push(reserve_indices(&mut occupied, &mut next, 11)?);
    }
    let mut base_feature_indices = Vec::new();
    for _ in &base_features {
        base_feature_indices.push(reserve_indices(&mut occupied, &mut next, 1)?);
    }
    let mut base_flange_indices = Vec::new();
    for _ in &base_flanges {
        base_flange_indices.push(reserve_indices(&mut occupied, &mut next, 1)?);
    }
    let mut dimension_bases = Vec::new();
    for _ in &dimensions {
        dimension_bases.push(reserve_indices(&mut occupied, &mut next, 4)?);
    }
    let mut copy_bases = Vec::new();
    for copy in &copies {
        let count = u32::try_from(copy.bodies.len())
            .ok()
            .and_then(|n| n.checked_add(3))
            .ok_or_else(|| CodecError::InvalidInput("too many copied bodies".into()))?;
        copy_bases.push(reserve_indices(&mut occupied, &mut next, count)?);
    }
    let body_map_index = if copies.is_empty() || append_main {
        None
    } else {
        Some(reserve_indices(&mut occupied, &mut next, 1)?)
    };
    let timeline = reserve_indices(&mut occupied, &mut next, 1)?;
    let collection_index = reserve_indices(&mut occupied, &mut next, 1)?;
    let mut out = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut scopes = Vec::new();
    let mut timeline_scopes = sketch
        .as_ref()
        .filter(|_| joined)
        .map_or_else(Vec::new, |sketch| {
            sketch
                .feature_scopes
                .iter()
                .map(|(ordinal, _, index)| (*ordinal, u64::from(*index)))
                .collect()
        });
    let mut owners = Vec::new();
    let mut parameters = Vec::new();
    let mut family_ordinals = std::collections::BTreeMap::<&str, u32>::new();
    for ((source_ordinal, kind, values, operation, _), base) in primitives.iter().zip(bases) {
        let family_ordinal = family_ordinals.entry(kind).or_default();
        *family_ordinal = family_ordinal.checked_add(1).ok_or_else(|| {
            CodecError::InvalidInput("F3D feature family ordinal exceeds u32".into())
        })?;
        scopes.push(u64::from(base));
        timeline_scopes.push((*source_ordinal, u64::from(base)));
        mark_primary(ctx, &mut primary, base, out.len())?;
        header(&mut out, &tags[1], base)?;
        out.extend_from_slice(&[0; 9]);
        out.extend_from_slice(&operation.to_le_bytes());
        out.push(0);
        out.push(1);
        out.extend_from_slice(
            &u32::try_from(values.len())
                .map_err(|_| CodecError::InvalidInput("too many F3D feature parameters".into()))?
                .to_le_bytes(),
        );
        for local in 0..values.len() {
            reference(
                &mut out,
                base + 1
                    + 3 * u32::try_from(local).map_err(|_| {
                        CodecError::InvalidInput("too many F3D feature parameters".into())
                    })?,
            );
        }
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        native_lp_utf16(&mut out, kind)?;
        out.extend_from_slice(&family_ordinal.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&[0; 7]);
        out.extend_from_slice(&[1, 15]);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&252u32.to_le_bytes());
        out.extend_from_slice(&0f64.to_le_bytes());
        out.extend_from_slice(&252u32.to_le_bytes());
        out.extend_from_slice(&[1, 14]);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 1, 13]);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0; 3]);
        ctx.charge_entities(1, "F3D feature record")?;
        mark(&mut secondary, base, out.len());
        header(&mut out, &tags[0], base)?;
        for (local, scalar) in values.iter().enumerate() {
            let local = u32::try_from(local)
                .map_err(|_| CodecError::InvalidInput("too many F3D feature parameters".into()))?;
            let value = scalar.value;
            let owner_index = base + 1 + 3 * local;
            let parameter_index = owner_index + 1;
            owners.push(u64::from(owner_index));
            parameters.push(u64::from(parameter_index));
            let owner = DesignParameterOwner::try_from(DesignParameterOwnerContent {
                id: format!("f3d:generated:owner#{owner_index}"),
                class_tag: tags[2].clone().try_into().map_err(CodecError::Malformed)?,
                record_index: owner_index,
                scope_record_index: Some(base),
                local_ordinal: Some(local),
                evaluated_value: value,
                parameter_record_index: parameter_index,
                owned_ordinal: local,
                variant: Some(0),
                companion_record_index: owner_index + 2,
            })
            .map_err(CodecError::Malformed)?;
            mark_primary(ctx, &mut primary, owner_index, out.len())?;
            ctx.charge_entities(1, "F3D feature record")?;
            super::parameter_links::encode(&mut out, &[owner], &[])?;
            mark(
                &mut secondary,
                owner_index,
                out.len() - crate::layout::indexed_design_record_header::LEN,
            );
            let parameter = DesignParameter::authored(DesignParameterContent {
                id: format!("f3d:generated:parameter#{parameter_index}"),
                class_tag: tags[3].clone().try_into().map_err(CodecError::Malformed)?,
                record_index: parameter_index,
                source_ordinal: local,
                source: DesignParameterSource::authored(
                    scalar.kind.into(),
                    Some(owner_index),
                    Some(DesignParameterDiscriminator::Code3),
                )
                .map_err(CodecError::Malformed)?,
                expression: scalar
                    .expression
                    .clone()
                    .unwrap_or_else(|| format!("{value} cm")),
                unit: Some("cm".into()),
                name: scalar
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{}{base}", scalar.kind)),
                evaluated_value: value,
            })
            .map_err(CodecError::Malformed)?;
            mark_primary(ctx, &mut primary, parameter_index, out.len())?;
            super::parameters::encode_document_parameter(&mut out, &parameter, collection_index)?;
        }
    }
    let mut extrusion_scopes = Vec::new();
    let mut base_feature_scopes = Vec::new();
    let mut base_flange_scopes = Vec::new();
    let mut extrusion_companions = Vec::new();
    let mut dimension_loci = Vec::new();
    let mut dimension_recipes = Vec::new();
    let mut copy_groups = Vec::new();
    for (feature, index) in base_flanges.iter().zip(base_flange_indices) {
        let family = family_ordinals.entry("BaseFlange").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D base flanges".into()))?;
        base_flange_scopes.push(u64::from(index));
        timeline_scopes.push((feature.ordinal, u64::from(index)));
        mark_primary(ctx, &mut primary, index, out.len())?;
        let start = out.len();
        header(&mut out, &tags[14], index)?;
        out.resize(start + 258, 0);
        out[start + 73..start + 77].copy_from_slice(&1u32.to_le_bytes());
        out[start + 81] = 1;
        out[start + 82..start + 86].copy_from_slice(&feature.references[3].to_le_bytes());
        out[start + 92..start + 96].copy_from_slice(&1u32.to_le_bytes());
        out[start + 112] = 1;
        out[start + 113..start + 117].copy_from_slice(&feature.references[2].to_le_bytes());
        out[start + 123..start + 131].copy_from_slice(&feature.thickness.to_le_bytes());
        out[start + 141..start + 145].copy_from_slice(&1u32.to_le_bytes());
        out[start + 145] = 1;
        out[start + 146..start + 150].copy_from_slice(&feature.references[0].to_le_bytes());
        out.extend_from_slice(&4u32.to_le_bytes());
        for reference_index in feature.references {
            reference(&mut out, reference_index);
        }
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        native_lp_utf16(&mut out, "BaseFlange")?;
        out.extend_from_slice(&family.to_le_bytes());
        out.extend_from_slice(&[0; 78]);
        mark(&mut secondary, index, out.len());
        header(&mut out, &tags[0], index)?;
    }
    for (feature, index) in base_features.iter().zip(base_feature_indices) {
        let family = family_ordinals.entry("Base Feature").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D base features".into()))?;
        base_feature_scopes.push(u64::from(index));
        timeline_scopes.push((feature.ordinal, u64::from(index)));
        mark_primary(ctx, &mut primary, index, out.len())?;
        let start = out.len();
        header(&mut out, &tags[13], index)?;
        out.resize(start + 138, 0);
        out.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut out, feature.reference);
        out.extend_from_slice(&feature.state.to_le_bytes());
        native_lp_utf16(&mut out, "Base Feature")?;
        out.extend_from_slice(&family.to_le_bytes());
        out.extend_from_slice(&[0; 78]);
        mark(&mut secondary, index, out.len());
        header(&mut out, &tags[0], index)?;
    }
    for (extrusion, base) in extrusions.iter().zip(extrusion_bases) {
        let group = base + 1;
        let profile = base + 4;
        let owner_index = base + 8;
        let parameter_index = base + 9;
        let companion_index = base + 10;
        let family = family_ordinals.entry("Extrude").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D extrusions".into()))?;
        extrusion_scopes.push(u64::from(base));
        timeline_scopes.push((extrusion.ordinal, u64::from(base)));
        mark_primary(ctx, &mut primary, base, out.len())?;
        let start = out.len();
        header(&mut out, &tags[8], base)?;
        let end = start
            .checked_add(208)
            .ok_or_else(|| CodecError::InvalidInput("F3D scope length overflows".into()))?;
        out.resize(end, 0);
        out[start + 21..start + 25].copy_from_slice(&4u32.to_le_bytes());
        out[start + 25..start + 29].copy_from_slice(&2u32.to_le_bytes());
        out[start + 30..start + 34].copy_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&3u32.to_le_bytes());
        for index in [group, profile, owner_index] {
            reference(&mut out, index);
        }
        out.extend_from_slice(&extrusion.current_state.to_le_bytes());
        native_lp_utf16(&mut out, "Extrude")?;
        let mut tail = [0u8; 78];
        tail[..4].copy_from_slice(&family.to_le_bytes());
        tail[31..35].copy_from_slice(&extrusion.previous_state.to_le_bytes());
        out.extend_from_slice(&tail);
        mark(&mut secondary, base, out.len());
        header(&mut out, &tags[0], base)?;
        mark_primary(ctx, &mut primary, group, out.len())?;
        header(&mut out, &tags[6], group)?;
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut out, profile);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0x0000_0041_0000_0000u64.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&1f64.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut out, group + 2);
        out.extend_from_slice(&[0; 2]);
        reference(&mut out, group + 1);
        out.push(0);
        reference(&mut out, base);
        mark(&mut secondary, group, out.len());
        header(&mut out, &tags[0], group)?;
        copy_groups.push(u64::from(group));
        mark_primary(ctx, &mut primary, profile, out.len())?;
        header(&mut out, &tags[6], profile)?;
        out.extend_from_slice(&[0; 10]);
        reference(&mut out, profile + 3);
        out.extend_from_slice(&1u32.to_le_bytes());
        native_lp_utf16(&mut out, "00000000-0000-4000-8000-000000000001")?;
        native_lp_utf16(&mut out, &extrusion.container.to_string())?;
        out.extend_from_slice(&[0; 94]);
        mark(&mut secondary, profile, out.len());
        header(&mut out, &tags[6], profile)?;
        copy_groups.push(u64::from(profile));
        let scalar = &extrusion.distance;
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerContent {
            id: format!("f3d:generated:owner#{owner_index}"),
            class_tag: tags[2].clone().try_into().map_err(CodecError::Malformed)?,
            record_index: owner_index,
            scope_record_index: Some(base),
            local_ordinal: Some(2),
            evaluated_value: scalar.value,
            parameter_record_index: parameter_index,
            owned_ordinal: 2,
            variant: Some(0),
            companion_record_index: companion_index,
        })
        .map_err(CodecError::Malformed)?;
        owners.push(u64::from(owner_index));
        parameters.push(u64::from(parameter_index));
        mark_primary(ctx, &mut primary, owner_index, out.len())?;
        super::parameter_links::encode(&mut out, &[owner], &[])?;
        mark(
            &mut secondary,
            owner_index,
            out.len() - crate::layout::indexed_design_record_header::LEN,
        );
        let parameter = DesignParameter::authored(DesignParameterContent {
            id: format!("f3d:generated:parameter#{parameter_index}"),
            class_tag: tags[3].clone().try_into().map_err(CodecError::Malformed)?,
            record_index: parameter_index,
            source_ordinal: 2,
            source: DesignParameterSource::authored(
                "AlongDistance".into(),
                Some(owner_index),
                Some(DesignParameterDiscriminator::Code3),
            )
            .map_err(CodecError::Malformed)?,
            expression: scalar
                .expression
                .clone()
                .unwrap_or_else(|| format!("{} cm", scalar.value)),
            unit: Some("cm".into()),
            name: scalar
                .name
                .clone()
                .unwrap_or_else(|| format!("Depth{base}")),
            evaluated_value: scalar.value,
        })
        .map_err(CodecError::Malformed)?;
        mark_primary(ctx, &mut primary, parameter_index, out.len())?;
        super::parameters::encode_document_parameter(&mut out, &parameter, collection_index)?;
        mark_primary(ctx, &mut primary, companion_index, out.len())?;
        header(&mut out, &tags[9], companion_index)?;
        out.extend_from_slice(&[0; 20]);
        reference(&mut out, owner_index);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        extrusion_companions.push(u64::from(companion_index));
    }
    for (dimension, base) in dimensions.iter().zip(dimension_bases) {
        let parameter_index = base + 1;
        let companion_index = base + 2;
        let locus_index = base + 3;
        let scalar = &dimension.scalar;
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerContent {
            id: format!("f3d:generated:owner#{base}"),
            class_tag: tags[2].clone().try_into().map_err(CodecError::Malformed)?,
            record_index: base,
            scope_record_index: Some(dimension.scope),
            local_ordinal: Some(dimension.ordinal),
            evaluated_value: scalar.value,
            parameter_record_index: parameter_index,
            owned_ordinal: dimension.source_ordinal,
            variant: Some(1),
            companion_record_index: companion_index,
        })
        .map_err(CodecError::Malformed)?;
        owners.push(u64::from(base));
        parameters.push(u64::from(parameter_index));
        mark_primary(ctx, &mut primary, base, out.len())?;
        super::parameter_links::encode(&mut out, &[owner], &[])?;
        mark(
            &mut secondary,
            base,
            out.len() - crate::layout::indexed_design_record_header::LEN,
        );
        let parameter = DesignParameter::authored(DesignParameterContent {
            id: format!("f3d:generated:parameter#{parameter_index}"),
            class_tag: tags[3].clone().try_into().map_err(CodecError::Malformed)?,
            record_index: parameter_index,
            source_ordinal: dimension.source_ordinal,
            source: DesignParameterSource::authored(
                dimension.source_kind.clone(),
                Some(base),
                None,
            )
            .map_err(CodecError::Malformed)?,
            expression: scalar
                .expression
                .clone()
                .unwrap_or_else(|| format!("{} cm", scalar.value)),
            unit: Some("cm".into()),
            name: scalar
                .name
                .clone()
                .unwrap_or_else(|| format!("Dimension{base}")),
            evaluated_value: scalar.value,
        })
        .map_err(CodecError::Malformed)?;
        mark_primary(ctx, &mut primary, parameter_index, out.len())?;
        super::parameters::encode_document_parameter(&mut out, &parameter, collection_index)?;
        mark_primary(ctx, &mut primary, companion_index, out.len())?;
        header(&mut out, &tags[9], companion_index)?;
        out.extend_from_slice(&[0; 20]);
        reference(&mut out, base);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        extrusion_companions.push(u64::from(companion_index));
        match dimension.construction {
            DimensionConstruction::Points(points) => {
                mark_primary(ctx, &mut primary, locus_index, out.len())?;
                let start = out.len();
                header(&mut out, &tags[10], locus_index)?;
                out.resize(start + 80, 0);
                out[start + 19] = 1;
                out[start + 20..start + 24].copy_from_slice(&3u32.to_le_bytes());
                out[start + 24] = 1;
                out[start + 35..start + 39].copy_from_slice(&4u32.to_le_bytes());
                out[start + 39] = 1;
                out[start + 40..start + 44].copy_from_slice(&points[0].to_le_bytes());
                out[start + 54] = 1;
                out[start + 55..start + 59].copy_from_slice(&points[1].to_le_bytes());
                out[start + 65..start + 69].copy_from_slice(&1u32.to_le_bytes());
                mark(&mut secondary, locus_index, out.len());
                header(&mut out, &tags[11], locus_index)?;
                dimension_loci.push(u64::from(locus_index));
            }
            DimensionConstruction::BodyRecipe(index) => {
                mark_primary(ctx, &mut primary, index, out.len())?;
                header(&mut out, &tags[12], index)?;
                out.extend_from_slice(&[0; 9]);
                native_lp_ascii(&mut out, "body_recipe_data")?;
                out.extend_from_slice(&0i32.to_le_bytes());
                out.extend_from_slice(&0i32.to_le_bytes());
                dimension_recipes.push(u64::from(index));
            }
        }
    }
    let mut body_map = std::collections::BTreeMap::new();
    let mut body_pairs = Vec::new();
    for (copy, base) in copies.iter().zip(copy_bases) {
        let group = base + 1;
        let relation = base + 2;
        let count = u32::try_from(copy.bodies.len()).map_err(CodecError::malformed)?;
        let family = family_ordinals.entry("CopyPasteBodies").or_default();
        *family += 1;
        scopes.push(u64::from(base));
        timeline_scopes.push((copy.ordinal, u64::from(base)));
        mark_primary(ctx, &mut primary, base, out.len())?;
        header(&mut out, &tags[1], base)?;
        out.extend_from_slice(&[0; 18]);
        reference(&mut out, group);
        reference(&mut out, relation);
        out.extend_from_slice(&(count + 1).to_le_bytes());
        reference(&mut out, group);
        for i in 0..count {
            reference(&mut out, base + 3 + i);
        }
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        native_lp_utf16(&mut out, "CopyPasteBodies")?;
        let mut tail = [0u8; 110];
        tail[..4].copy_from_slice(&family.to_le_bytes());
        tail[53..57].copy_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&tail);
        mark(&mut secondary, base, out.len());
        header(&mut out, &tags[0], base)?;
        mark_primary(ctx, &mut primary, group, out.len())?;
        header(&mut out, &tags[6], group)?;
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&count.to_le_bytes());
        for i in 0..count {
            reference(&mut out, base + 3 + i);
        }
        copy_groups.push(u64::from(group));
        for (i, (_, source, _)) in copy.bodies.iter().enumerate() {
            let index = base + 3 + u32::try_from(i).map_err(CodecError::malformed)?;
            scopes.push(u64::from(index));
            mark_primary(ctx, &mut primary, index, out.len())?;
            header(&mut out, &tags[1], index)?;
            out.extend_from_slice(&[0; 10]);
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&u64::from(*source).to_le_bytes());
        }
        mark_primary(ctx, &mut primary, relation, out.len())?;
        header(&mut out, &tags[6], relation)?;
        out.extend_from_slice(&[0; 8]);
        out.push(1);
        let reference_count = count.checked_mul(2).ok_or_else(|| {
            CodecError::InvalidInput("F3D copy body reference count exceeds u32".into())
        })?;
        out.extend_from_slice(&reference_count.to_le_bytes());
        for (i, (key, source, copied)) in copy.bodies.iter().enumerate() {
            out.push(1);
            out.extend_from_slice(&source.to_le_bytes());
            out.extend_from_slice(&[0; 10]);
            out.push(1);
            out.extend_from_slice(&copied.to_le_bytes());
            out.extend_from_slice(&[0; 6]);
            if i + 1 != copy.bodies.len() {
                out.extend_from_slice(&[0; 4]);
            }
            body_map.insert(*key, u64::from(*copied));
            body_pairs.push((*key, u64::from(*copied)));
        }
        copy_groups.push(u64::from(relation));
    }
    if let Some(index) = body_map_index {
        mark_primary(ctx, &mut primary, index, out.len())?;
        header(&mut out, &tags[7], index)?;
        out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
        out.extend_from_slice(
            &u32::try_from(body_map.len())
                .map_err(CodecError::malformed)?
                .to_le_bytes(),
        );
        for (key, suffix) in body_map {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        native_lp_utf16(&mut out, brep_name)?;
    }
    mark_primary(ctx, &mut primary, timeline, out.len())?;
    timeline_scopes.sort_by_key(|(ordinal, _)| *ordinal);
    native_lp_ascii(&mut out, &tags[4])?;
    out.extend_from_slice(&u64::from(timeline).to_le_bytes());
    native_lp_ascii(&mut out, "Timeline")?;
    out.extend_from_slice(&[0, 0, 1]);
    out.extend_from_slice(&timeline_scopes[0].1.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out.extend_from_slice(
        &u32::try_from(timeline_scopes.len())
            .map_err(|_| CodecError::InvalidInput("too many F3D features".into()))?
            .to_le_bytes(),
    );
    for (_, scope) in &timeline_scopes {
        out.push(1);
        out.extend_from_slice(&scope.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
    }
    mark_primary(ctx, &mut primary, collection_index, out.len())?;
    let parameter_indices = parameters
        .iter()
        .map(|value| {
            u32::try_from(*value)
                .map_err(|_| CodecError::InvalidInput("F3D parameter index exceeds u32".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    super::parameters::encode_collection(&mut out, &tags[5], collection_index, &parameter_indices)?;
    let types = [
        (
            "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
            Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
            0,
            "CommonData",
            vec![],
        ),
        (
            "6AA4C36C-52A2-48D2-A062-69FE123B84D1",
            Some("2EDC2536-D95B-4469-8ACA-DC07124D9C43"),
            0,
            "Fusion",
            scopes,
        ),
        (
            "D91D429C-1BB5-4640-B49F-69AF14F463E9",
            Some("B24D5130-8751-4272-8143-D230D8757873"),
            0,
            "Fusion",
            owners,
        ),
        (
            "7A6A3D31-BE74-4E19-9115-20642944C2E4",
            Some("63D6AD42-466A-40B7-9CCD-EB21BC500EF6"),
            5,
            "CommonData",
            parameters,
        ),
        (
            "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B",
            Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
            2,
            "Fusion",
            vec![u64::from(timeline)],
        ),
        (
            "B0556F58-EE09-46A7-B18F-E296F48E5B13",
            Some("A7AEA631-985B-4DD1-8CE2-DE2C-14B54081"),
            3,
            "CommonData",
            vec![u64::from(collection_index)],
        ),
        (
            "00000000-0000-4000-8000-000000000264",
            None,
            0,
            "Geometry",
            copy_groups,
        ),
        (
            crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
            Some(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID),
            crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
            "Body",
            body_map_index.into_iter().map(u64::from).collect(),
        ),
        (
            "DD405BC2-D673-44F0-8833-5CB2A1C186C7",
            None,
            0,
            "Fusion",
            extrusion_scopes,
        ),
        (
            "00000000-0000-4000-8000-000000000272",
            None,
            0,
            "Dimension",
            extrusion_companions,
        ),
        (
            "00000000-0000-0000-0000-000000001304",
            None,
            1,
            "Dimension",
            dimension_loci,
        ),
        (
            "00000000-0000-0000-0000-000000001303",
            None,
            1,
            "Dimension",
            vec![],
        ),
        (
            "00000000-0000-0000-0000-000000001305",
            None,
            1,
            "Dimension",
            dimension_recipes,
        ),
        (
            "D7015B81-AB4D-4C89-BED7-AA4411E1964A",
            None,
            0,
            "Fusion",
            base_feature_scopes,
        ),
        (
            "00000000-0000-0000-0000-000000001400",
            None,
            1,
            "Fusion",
            base_flange_scopes,
        ),
    ]
    .into_iter()
    .map(|(guid, base, version, module, entity_ids)| {
        Ok(GeneratedDesignType {
            type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
            base_type_guid: base
                .map(|value| value.to_owned().try_into())
                .transpose()
                .map_err(CodecError::Malformed)?,
            version,
            module: module.into(),
            entity_ids,
        })
    })
    .collect::<Result<Vec<_>, CodecError>>()?;
    let mut registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let mut append = None;
    let stream = if append_main {
        append = Some(DesignAppend {
            bulk: out,
            primary,
            secondary,
            types: registry.types,
            body_pairs,
        });
        None
    } else if joined {
        let sketch = sketch
            .as_mut()
            .ok_or_else(|| CodecError::Malformed("missing generated sketch stream".into()))?;
        let offset = cadmpeg_core::decode::u64_from_index(sketch.bulk.len());
        for record in primary.iter_mut().chain(&mut secondary) {
            record.bulk_offset = record
                .bulk_offset
                .checked_add(offset)
                .ok_or_else(|| CodecError::InvalidInput("F3D Design stream exceeds u64".into()))?;
        }
        sketch.bulk.extend_from_slice(&out);
        sketch.constraints.extend(
            dimensions
                .iter()
                .map(|dimension| dimension.constraint.clone()),
        );
        sketch.registry.types.append(&mut registry.types);
        sketch.primary.extend(primary.iter().copied());
        sketch.secondary.extend(secondary.iter().copied());
        let combined =
            encode_design_metastream(&sketch.registry, &sketch.primary, &sketch.secondary)?;
        sketch.meta = combined
            .ok_or_else(|| CodecError::Malformed("missing generated Design metadata".into()))?;
        None
    } else {
        let meta = encode_design_metastream(&registry, &primary, &secondary)?
            .ok_or_else(|| CodecError::Malformed("missing generated feature metadata".into()))?;
        Some((out, meta))
    };
    let mut written_parameters: Vec<_> = primitives
        .iter()
        .flat_map(|(_, _, values, _, _)| values.iter().filter_map(|value| value.parameter.clone()))
        .collect();
    written_parameters.extend(
        extrusions
            .iter()
            .filter_map(|extrusion| extrusion.distance.parameter.clone()),
    );
    written_parameters.extend(
        dimensions
            .iter()
            .filter_map(|dimension| dimension.scalar.parameter.clone()),
    );
    let mut written = sketch
        .as_ref()
        .filter(|_| joined)
        .map_or_else(Vec::new, |sketch| {
            sketch
                .feature_scopes
                .iter()
                .map(|(_, id, _)| id.clone())
                .collect()
        });
    written.extend(primitives.into_iter().map(|(_, _, _, _, id)| id));
    written.extend(copies.into_iter().map(|copy| copy.id));
    written.extend(extrusions.into_iter().map(|extrusion| extrusion.id));
    written.extend(base_features.into_iter().map(|feature| feature.id));
    written.extend(base_flanges.into_iter().map(|feature| feature.id));
    Ok(Some(EncodedFeatures {
        stream,
        features: written,
        parameters: written_parameters,
        collection_index,
        append,
    }))
}

fn reserve_indices(
    occupied: &mut std::collections::BTreeSet<u32>,
    next: &mut u32,
    count: u32,
) -> Result<u32, CodecError> {
    loop {
        let start = *next;
        let end = start
            .checked_add(count - 1)
            .filter(|_| start != 0)
            .ok_or_else(|| {
                CodecError::InvalidInput("F3D feature record indices exceed u32".into())
            })?;
        if let Some(collision) = occupied.range(start..=end).next().copied() {
            *next = collision.checked_add(1).unwrap_or(0);
            continue;
        }
        occupied.extend(start..=end);
        *next = end.checked_add(1).unwrap_or(0);
        return Ok(start);
    }
}

fn mark_primary(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &mut Vec<crate::metastream::RecordIndexEntry>,
    id: u32,
    offset: usize,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, "F3D feature record")?;
    mark(records, id, offset);
    Ok(())
}

fn mark(records: &mut Vec<crate::metastream::RecordIndexEntry>, id: u32, offset: usize) {
    records.push(crate::metastream::RecordIndexEntry {
        entity_id: u64::from(id),
        bulk_offset: cadmpeg_core::decode::u64_from_index(offset),
    });
}
fn header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&id.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, id: u32) {
    out.push(1);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}
