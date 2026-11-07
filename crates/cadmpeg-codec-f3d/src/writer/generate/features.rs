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
    scope_index: u32,
    parameter_index: Option<u32>,
    profile_asset: Option<crate::records::mesh::DesignRelaxedGuidText>,
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

struct NativeSurfaceStitch {
    ordinal: u64,
    id: FeatureId,
    member: u32,
    state: u32,
    gap_cm: f64,
    opaque_index: u32,
    opaque_scalar: f64,
}

struct NativeForm {
    ordinal: u64,
    id: FeatureId,
    reference: u32,
}

fn select_native_forms(target: &CadIr, native: &crate::native::F3dNative) -> Vec<NativeForm> {
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
            if kind.as_str() != "Form"
                || !parameters.is_empty()
                || feature.suppressed == Some(true)
                || feature.needs_recompute
                || feature.product_definition.is_some()
                || !feature.dependencies.is_empty()
                || feature.source_text.is_some()
                || !feature.source_content.is_empty()
                || !feature.evaluation.outputs().is_empty()
                || feature
                    .source_tag
                    .as_deref()
                    .is_some_and(|tag| tag != "Form")
                || feature.source_properties.len() != 1
            {
                return None;
            }
            let (key, value) = feature.source_properties.iter().next()?;
            if key.as_str() != "reference:0" {
                return None;
            }
            let reference = value.parse::<u32>().ok().filter(|value| *value != 0)?;
            let scope = native
                .design_parameter_scopes
                .iter()
                .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
            if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Form
                || !scope.reference_members().values().copied().eq([reference])
            {
                return None;
            }
            Some(NativeForm {
                ordinal: feature.ordinal,
                id: feature.id.clone(),
                reference,
            })
        })
        .collect()
}

struct NativeComponentCopy {
    ordinal: u64,
    id: FeatureId,
    component: String,
    occurrence_keys: [String; 2],
    transforms: [[[f64; 4]; 4]; 2],
    definition: cadmpeg_ir::ids::ProductDefinitionId,
    occurrences: [cadmpeg_ir::ids::OccurrenceId; 2],
}

fn select_component_copies(
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> Vec<NativeComponentCopy> {
    use cadmpeg_ir::products::{OccurrenceParent, ProductDefinitionKind, PrototypeReference};
    let copies: Vec<_> = target
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Operation(FeatureOperation::InsertComponent { occurrence }) =
                feature.evaluation.definition()
            else {
                return None;
            };
            if feature.suppressed == Some(true)
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
                    .is_some_and(|tag| tag != "CopyPaste")
            {
                return None;
            }
            let scope = native
                .design_parameter_scopes
                .iter()
                .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
            let operation = scope.copy_paste_component_operation()?;
            let keys = [
                &operation.source_occurrence_guid,
                &operation.copied_occurrence_guid,
            ];
            if keys.iter().any(|key| key.as_str().len() != 36)
                || *occurrence != crate::ids::neutral_component_occurrence_id(keys[1])
            {
                return None;
            }
            let definition = crate::ids::neutral_component_id(&operation.component_guid);
            let product = target
                .model
                .product_definitions
                .iter()
                .find(|product| product.id == definition)?;
            if product.kind != ProductDefinitionKind::Part
                || product.label.is_some()
                || product.source_name.is_some()
                || product.description.is_some()
                || product.part_number.is_some()
                || !product.bom_properties.is_empty()
                || !product.bodies.is_empty()
                || !product.sketches.is_empty()
                || product.native_ref.is_some()
            {
                return None;
            }
            let mut transforms = [[[0.0; 4]; 4]; 2];
            for (index, key) in keys.iter().enumerate() {
                let id = crate::ids::neutral_component_occurrence_id(*key);
                let placed = target
                    .model
                    .occurrences
                    .iter()
                    .find(|candidate| candidate.id == id)?;
                if placed.prototype
                    != (PrototypeReference::Local {
                        definition: definition.clone(),
                    })
                    || !matches!(placed.parent, OccurrenceParent::Root {})
                    || usize::try_from(placed.ordinal).ok()? != index
                    || placed.linked_prototype.is_some()
                    || placed.name.is_some()
                    || placed.visible.is_some()
                    || placed.link.is_some()
                    || !placed.native_ref.as_deref().is_some_and(|reference| {
                        crate::ids::native_stream(reference) == crate::ids::native_stream(&scope.id)
                    })
                {
                    return None;
                }
                let mut rows = placed.transform?.rows();
                if (0..3)
                    .any(|row| (0..3).any(|col| rows[row][col] != f64::from(u8::from(row == col))))
                {
                    return None;
                }
                for row in rows.iter_mut().take(3) {
                    row[3] /= 10.0;
                }
                transforms[index] = rows;
            }
            Some(NativeComponentCopy {
                ordinal: feature.ordinal,
                id: feature.id.clone(),
                component: operation.component_guid.as_str().to_owned(),
                occurrence_keys: keys.map(|key| key.as_str().to_owned()),
                transforms,
                definition,
                occurrences: keys.map(crate::ids::neutral_component_occurrence_id),
            })
        })
        .collect();
    if copies.len() == 1 {
        copies
    } else {
        Vec::new()
    }
}

fn select_native_surface_stitches(
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> Vec<NativeSurfaceStitch> {
    use cadmpeg_ir::features::FaceSelection;
    target
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                faces: FaceSelection::Native(selection),
                merge_entities: Some(true),
                create_solid: Some(true),
                gap_tolerance: Some(Dimension::Literal(gap)),
            }) = feature.evaluation.definition()
            else {
                return None;
            };
            if feature.native_ref.as_deref() != Some(selection.as_str())
                || feature
                    .source_tag
                    .as_deref()
                    .is_some_and(|tag| tag != "SurfaceStitch")
                || feature.suppressed == Some(true)
                || feature.needs_recompute
                || feature.product_definition.is_some()
                || !feature.dependencies.is_empty()
                || !feature.source_properties.is_empty()
                || feature.source_text.is_some()
                || !feature.source_content.is_empty()
                || !feature.evaluation.outputs().is_empty()
            {
                return None;
            }
            let scope = native
                .design_parameter_scopes
                .iter()
                .find(|scope| scope.id == *selection)?;
            let operation = scope.surface_stitch_operation()?;
            let mut inputs = target
                .model
                .feature_input_topologies
                .iter()
                .filter(|input| input.input_of == feature.id);
            let input = inputs.next()?;
            if inputs.next().is_some()
                || !input.bodies.is_empty()
                || !input.faces.is_empty()
                || !input.edges.is_empty()
                || !input.vertices.is_empty()
                || !input.edge_geometry.is_empty()
                || input.operands.len() != 1
            {
                return None;
            }
            let input_ref = input.operands.iter().next()?.native_ref.as_str();
            let group = native
                .design_construction_operand_groups
                .iter()
                .find(|group| group.id == input_ref)?;
            if crate::ids::native_stream(&group.id) != crate::ids::native_stream(&scope.id)
                || group.scope_record_index != scope.record_index
                || group.scope_reference_ordinal != 0
                || group.role()
                    != crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5
                || group.members().len() != 1
                || !group.lost_edge_references.is_empty()
                || group.frame.variant
                || group.frame.closing_flag
            {
                return None;
            }
            let member = group.members()[0].value;
            if member == 0
                || !scope.reference_members().values().copied().eq([
                    group.record_index,
                    member,
                    operation.tolerance_record_index,
                    operation.settings_record_index,
                ])
            {
                return None;
            }
            let gap_cm = gap.get() / 10.0;
            if gap_cm <= 0.0 {
                return None;
            }
            Some(NativeSurfaceStitch {
                ordinal: feature.ordinal,
                id: feature.id.clone(),
                member,
                state: u32::try_from(scope.history_state_id()?).ok()?,
                gap_cm,
                opaque_index: group.frame.opaque_index.get(),
                opaque_scalar: group.frame.opaque_scalar().get(),
            })
        })
        .collect()
}

struct NativeRemoveBody {
    ordinal: u64,
    id: FeatureId,
    member: u32,
    state: u32,
    opaque_index: u32,
    opaque_scalar: f64,
    recipe: Option<NativeBodyRecipe>,
}

struct NativeBodyRecipe {
    design_reference: u64,
    asset: String,
    context: String,
}

fn select_native_remove_bodies(
    target: &CadIr,
    native: &crate::native::F3dNative,
) -> Vec<NativeRemoveBody> {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection};
    target.model.features.iter().filter_map(|feature| {
        let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(selection), mode: BodyRetentionMode::DeleteSelected,
        }) = feature.evaluation.definition() else { return None };
        if feature.source_tag.as_deref().is_some_and(|tag| tag != "RemoveBody")
            || feature.suppressed == Some(true) || feature.needs_recompute || feature.product_definition.is_some()
            || !feature.dependencies.is_empty() || !feature.source_properties.is_empty()
            || feature.source_text.is_some() || !feature.source_content.is_empty() || !feature.evaluation.outputs().is_empty() { return None; }
        let scope = native.design_parameter_scopes.iter().find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
        let group = native.design_construction_operand_groups.iter().find(|group| &group.id == selection)?;
        if crate::ids::native_stream(&group.id) != crate::ids::native_stream(&scope.id)
            || group.scope_record_index != scope.record_index || group.scope_reference_ordinal != 0
            || group.role() != crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A
            || group.members().len() != 1 || !group.lost_edge_references.is_empty()
            || group.frame.variant || group.frame.closing_flag { return None; }
        let member = group.members()[0].value;
        if !scope.reference_members().values().copied().eq([group.record_index, member]) { return None; }
        let mut inputs = target.model.feature_input_topologies.iter().filter(|input| input.input_of == feature.id);
        let input = inputs.next()?;
        if inputs.next().is_some() || !input.bodies.is_empty() || !input.faces.is_empty()
            || !input.edges.is_empty() || !input.vertices.is_empty() || !input.edge_geometry.is_empty()
            || !input.operands.iter().any(|operand| operand.native_ref.as_str() == selection) { return None; }
        let recipe = if input.operands.len() == 1 { None } else {
        if input.operands.len() != 2 { return None; }
        let operand = native.design_body_recipe_operands.iter().find(|record| record.record_index() == member
            && input.operands.iter().any(|input| input.native_ref.as_str() == record.id))?;
        if operand.scope_record_index != scope.record_index
            || !matches!(operand.owner, crate::records::topology::body_recipe::DesignOperandOwner::Group { group_record_index, group_member_ordinal: 0 } if group_record_index == group.record_index)
            || operand.references().len() != 1 || operand.references()[0].form != 3
            || !operand.references()[0].candidate_faces.is_empty() || !operand.references()[0].preceding_candidate_faces.is_empty()
            || !operand.references()[0].preceding_body_slots.is_empty()
            || operand.resolved_body_state_id.is_some() || operand.resolved_body_slot.is_some() || operand.resolved_face_slot.is_some()
            || !operand.resolved_body_face_slots.is_empty() || member.checked_add(4) != Some(operand.next_record_index())
            || !operand.selector_tail().is_some_and(|tail| tail.byte_len() == 4 && tail.digest() == &cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0; 4]))
            || !native.construction_recipes.iter().any(|recipe| recipe.id == operand.recipe_id
                && recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body
                && recipe.recipe_index == 0 && recipe.record_index.as_ref().is_some_and(|index| index.value == 0)
                && recipe.design.as_ref().is_some_and(|design| design.id.value == (member + 3).to_string()
                    && design.selector.as_ref().is_some_and(|selector| selector.value == member + 4))) { return None; }
        Some(NativeBodyRecipe { design_reference: operand.references()[0].design_reference,
            asset: operand.asset_id.as_str().to_owned(), context: operand.context_id.as_str().to_owned() })
        };
        Some(NativeRemoveBody { ordinal: feature.ordinal, id: feature.id.clone(), member,
            state: u32::try_from(scope.history_state_id()?).ok()?,
            opaque_index: group.frame.opaque_index.get(), opaque_scalar: group.frame.opaque_scalar().get(),
            recipe })
    }).collect()
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
        let parameter_index = distance.parameter.as_ref().and_then(|id| {
            let stream = crate::ids::native_stream(&scope.id)?;
            let zero = crate::ids::neutral_parameter_id_parts(stream, 0);
            let prefix = zero.as_str().strip_suffix('0')?;
            let index = id.as_str().strip_prefix(prefix)?.parse::<u32>().ok()?;
            (crate::ids::neutral_parameter_id_parts(stream, index) == *id).then_some(index)
        });
        let profile_asset = scope.extrude_profile().map(|profile| profile.asset_id.clone());
        Some(Extrusion { ordinal: feature.ordinal, id: feature.id.clone(), container, distance, scope_index: scope.record_index, parameter_index, profile_asset, current_state, previous_state })
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
    pub(super) component_definitions: Vec<cadmpeg_ir::ids::ProductDefinitionId>,
    pub(super) component_occurrences: Vec<cadmpeg_ir::ids::OccurrenceId>,
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
    let mut stitches = select_native_surface_stitches(target, native);
    let mut removals = select_native_remove_bodies(target, native);
    let mut component_copies = select_component_copies(target, native);
    let mut forms = select_native_forms(target, native);
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
        && removals.is_empty()
        && stitches.is_empty()
        && component_copies.is_empty()
        && forms.is_empty()
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
    if type_start.checked_add(18).is_none_or(|tag| tag > 999) {
        component_copies.clear();
    }
    let tags = (0..19u32)
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
    forms.retain(|feature| occupied.insert(feature.reference));
    stitches.retain(|feature| occupied.insert(feature.member));
    removals.retain(|feature| {
        let indices =
            feature.member..=feature.member + if feature.recipe.is_some() { 4 } else { 0 };
        if indices.clone().any(|index| occupied.contains(&index)) {
            return false;
        }
        occupied.extend(indices);
        true
    });
    if primitives.is_empty()
        && copies.is_empty()
        && base_features.is_empty()
        && base_flanges.is_empty()
        && removals.is_empty()
        && stitches.is_empty()
        && component_copies.is_empty()
        && forms.is_empty()
        && !joined
    {
        return Ok(None);
    }
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
    let extrusion_identities = extrusions
        .iter()
        .map(|extrusion| {
            let scope = (extrusion.scope_index != 0 && occupied.insert(extrusion.scope_index))
                .then_some(extrusion.scope_index);
            let frame = extrusion.parameter_index.and_then(|index| {
                let owner = index.checked_sub(1).filter(|owner| *owner != 0)?;
                let companion = index.checked_add(1)?;
                let indices = [owner, index, companion];
                if indices.iter().any(|index| occupied.contains(index)) {
                    return None;
                }
                occupied.extend(indices);
                Some(indices)
            });
            (scope, frame)
        })
        .collect::<Vec<_>>();
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
    let mut removal_bases = Vec::new();
    for _ in &removals {
        removal_bases.push(reserve_indices(&mut occupied, &mut next, 4)?);
    }
    let mut stitch_bases = Vec::new();
    for _ in &stitches {
        stitch_bases.push(reserve_indices(&mut occupied, &mut next, 6)?);
    }
    let mut component_copy_bases = Vec::new();
    for _ in &component_copies {
        component_copy_bases.push(reserve_indices(&mut occupied, &mut next, 5)?);
    }
    let mut form_bases = Vec::new();
    for _ in &forms {
        form_bases.push(reserve_indices(&mut occupied, &mut next, 1)?);
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
    let mut removal_scopes = Vec::new();
    let mut stitch_scopes = Vec::new();
    let mut extrusion_companions = Vec::new();
    let mut dimension_loci = Vec::new();
    let mut dimension_recipes = Vec::new();
    let mut copy_groups = Vec::new();
    for (form, index) in forms.iter().zip(form_bases) {
        let family = family_ordinals.entry("Form").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D forms".into()))?;
        mark_primary(ctx, &mut primary, index, out.len())?;
        header(&mut out, &tags[1], index)?;
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&1_u32.to_le_bytes());
        reference(&mut out, form.reference);
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        native_lp_utf16(&mut out, "Form")?;
        out.extend_from_slice(&family.to_le_bytes());
        out.extend_from_slice(&[0; 78]);
        scopes.push(u64::from(index));
        timeline_scopes.push((form.ordinal, u64::from(index)));
        mark(&mut secondary, index, out.len());
        header(&mut out, &tags[0], index)?;
    }
    let mut component_sources = Vec::new();
    let mut component_results = Vec::new();
    for (copy, base) in component_copies.iter().zip(component_copy_bases) {
        let relation = base + 1;
        let sources = [base + 2, base + 3];
        let component = u64::from(base + 4);
        let family = family_ordinals.entry("CopyPaste").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D component copies".into()))?;
        for (index, record) in sources.into_iter().enumerate() {
            mark_primary(ctx, &mut primary, record, out.len())?;
            let start = out.len();
            header(&mut out, &tags[17 + index], record)?;
            out.resize(start + if index == 0 { 229 } else { 357 }, 0);
            out[start + 19] = 1;
            out[start + 20..start + 24].copy_from_slice(&1_u32.to_le_bytes());
            out[start + 24] = 1;
            out[start + 25..start + 33].copy_from_slice(&component.to_le_bytes());
            out[start + 40..start + 44].copy_from_slice(
                &(u32::try_from(index).map_err(CodecError::malformed)? + 1).to_le_bytes(),
            );
            for (at, value) in [
                (44, copy.component.as_str()),
                (120, copy.occurrence_keys[index].as_str()),
            ] {
                out[start + at..start + at + 4].copy_from_slice(&36_u32.to_le_bytes());
                for (offset, unit) in value.encode_utf16().enumerate() {
                    out[start + at + 4 + offset * 2..start + at + 6 + offset * 2]
                        .copy_from_slice(&unit.to_le_bytes());
                }
            }
            out[start + 197] = 1;
            out[start + 198..start + 206].copy_from_slice(&component.to_le_bytes());
            if index == 0 {
                out[start + 208] = 1;
                out[start + 218] = 1;
            } else {
                for (offset, value) in copy.transforms[index].iter().flatten().enumerate() {
                    out[start + 209 + offset * 8..start + 217 + offset * 8]
                        .copy_from_slice(&value.to_le_bytes());
                }
                out[start + 346] = 1;
            }
        }
        component_sources.push(u64::from(sources[0]));
        component_results.push(u64::from(sources[1]));
        mark_primary(ctx, &mut primary, relation, out.len())?;
        header(&mut out, &tags[6], relation)?;
        out.extend_from_slice(&[0; 10]);
        reference(&mut out, sources[1]);
        out.extend_from_slice(&[0; 2]);
        reference(&mut out, sources[0]);
        out.push(0);
        reference(&mut out, base);
        copy_groups.push(u64::from(relation));
        mark(&mut secondary, relation, out.len());
        header(&mut out, &tags[0], relation)?;
        mark_primary(ctx, &mut primary, base, out.len())?;
        let start = out.len();
        header(&mut out, &tags[1], base)?;
        out.resize(start + 529, 0);
        for (at, matrix) in [(38, &copy.transforms[0]), (194, &copy.transforms[1])] {
            for (offset, value) in matrix.iter().flatten().enumerate() {
                out[start + at + offset * 8..start + at + 8 + offset * 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        out[start + 378..start + 382].copy_from_slice(&1_u32.to_le_bytes());
        out[start + 382] = 1;
        out[start + 383..start + 391].copy_from_slice(&u64::from(relation).to_le_bytes());
        out[start + 397..start + 401].copy_from_slice(&9_u32.to_le_bytes());
        for (offset, unit) in "CopyPaste".encode_utf16().enumerate() {
            out[start + 401 + offset * 2..start + 403 + offset * 2]
                .copy_from_slice(&unit.to_le_bytes());
        }
        out[start + 419..start + 423].copy_from_slice(&family.to_le_bytes());
        scopes.push(u64::from(base));
        timeline_scopes.push((copy.ordinal, u64::from(base)));
        mark(&mut secondary, base, out.len());
        header(&mut out, &tags[0], base)?;
    }
    for (feature, index) in stitches.iter().zip(stitch_bases) {
        let group = index + 1;
        let tolerance = index + 4;
        let settings = index + 5;
        let family = family_ordinals.entry("SurfaceStitch").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D surface stitches".into()))?;
        stitch_scopes.push(u64::from(index));
        timeline_scopes.push((feature.ordinal, u64::from(index)));
        mark_primary(ctx, &mut primary, index, out.len())?;
        header(&mut out, &tags[16], index)?;
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&4u32.to_le_bytes());
        for id in [group, feature.member, tolerance, settings] {
            reference(&mut out, id);
        }
        out.extend_from_slice(&feature.state.to_le_bytes());
        native_lp_utf16(&mut out, "SurfaceStitch")?;
        out.extend_from_slice(&family.to_le_bytes());
        out.extend_from_slice(&[0; 78]);
        mark(&mut secondary, index, out.len());
        header(&mut out, &tags[0], index)?;
        mark_primary(ctx, &mut primary, group, out.len())?;
        header(&mut out, &tags[6], group)?;
        out.extend_from_slice(&[0; 9]);
        out.push(1);
        out.extend_from_slice(&1u32.to_le_bytes());
        native_lp_ascii(&mut out, "DcFeatureOperationIdFlag")?;
        native_lp_ascii(&mut out, "IntrinsicMetaTypeuint64")?;
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut out, feature.member);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0x0000_0005_0000_0000u64.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&feature.opaque_index.to_le_bytes());
        out.extend_from_slice(&feature.opaque_scalar.to_le_bytes());
        out.extend_from_slice(&feature.opaque_index.to_le_bytes());
        reference(&mut out, group + 2);
        out.extend_from_slice(&[0; 2]);
        reference(&mut out, group + 1);
        out.push(0);
        reference(&mut out, index);
        mark(&mut secondary, group, out.len());
        header(&mut out, &tags[0], group)?;
        copy_groups.push(u64::from(group));
        mark_primary(ctx, &mut primary, tolerance, out.len())?;
        let start = out.len();
        header(&mut out, &tags[2], tolerance)?;
        out.resize(start + 100, 0);
        out[start + 19..start + 24].copy_from_slice(&[1, 1, 0, 0, 0]);
        out[start + 24] = 1;
        out[start + 25..start + 29].copy_from_slice(&index.to_le_bytes());
        out[start + 40..start + 48].copy_from_slice(&feature.gap_cm.to_le_bytes());
        mark(&mut secondary, tolerance, out.len());
        header(&mut out, &tags[2], tolerance)?;
        owners.push(u64::from(tolerance));
        mark_primary(ctx, &mut primary, settings, out.len())?;
        header(&mut out, &tags[6], settings)?;
        out.extend_from_slice(&[0; 20]);
        copy_groups.push(u64::from(settings));
    }
    for (feature, index) in removals.iter().zip(removal_bases) {
        let group = index + 1;
        let family = family_ordinals.entry("RemoveBody").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D body removals".into()))?;
        removal_scopes.push(u64::from(index));
        timeline_scopes.push((feature.ordinal, u64::from(index)));
        mark_primary(ctx, &mut primary, index, out.len())?;
        header(&mut out, &tags[15], index)?;
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&2u32.to_le_bytes());
        reference(&mut out, group);
        reference(&mut out, feature.member);
        out.extend_from_slice(&feature.state.to_le_bytes());
        native_lp_utf16(&mut out, "RemoveBody")?;
        out.extend_from_slice(&family.to_le_bytes());
        out.extend_from_slice(&[0; 78]);
        mark(&mut secondary, index, out.len());
        header(&mut out, &tags[0], index)?;
        mark_primary(ctx, &mut primary, group, out.len())?;
        header(&mut out, &tags[6], group)?;
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut out, feature.member);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&feature.opaque_index.to_le_bytes());
        out.extend_from_slice(&feature.opaque_scalar.to_le_bytes());
        out.extend_from_slice(&feature.opaque_index.to_le_bytes());
        reference(&mut out, group + 2);
        out.extend_from_slice(&[0; 2]);
        reference(&mut out, group + 1);
        out.push(0);
        reference(&mut out, index);
        mark(&mut secondary, group, out.len());
        header(&mut out, &tags[0], group)?;
        copy_groups.push(u64::from(group));
        if let Some(recipe) = &feature.recipe {
            mark_primary(ctx, &mut primary, feature.member, out.len())?;
            header(&mut out, &tags[15], feature.member)?;
            out.extend_from_slice(&[0; 10]);
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&recipe.design_reference.to_le_bytes());
            out.extend_from_slice(&3u32.to_le_bytes());
            reference(&mut out, feature.member + 3);
            out.extend_from_slice(&1u32.to_le_bytes());
            native_lp_utf16(&mut out, &recipe.asset)?;
            native_lp_utf16(&mut out, &recipe.context)?;
            out.extend_from_slice(&2u32.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            mark(&mut secondary, feature.member, out.len());
            header(&mut out, &tags[0], feature.member)?;
            removal_scopes.push(u64::from(feature.member));
            for index in feature.member + 1..=feature.member + 4 {
                mark_primary(ctx, &mut primary, index, out.len())?;
                header(&mut out, &tags[6], index)?;
                if index == feature.member + 3 {
                    native_lp_ascii(&mut out, &index.to_string())?;
                    out.extend_from_slice(&(feature.member + 4).to_le_bytes());
                    out.extend_from_slice(&[0; 12]);
                    native_lp_ascii(&mut out, "body_recipe_data")?;
                }
                copy_groups.push(u64::from(index));
            }
        }
    }
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
    for ((extrusion, base), (scope, frame)) in extrusions
        .iter()
        .zip(extrusion_bases)
        .zip(extrusion_identities)
    {
        let scope = scope.unwrap_or(base);
        let group = base + 1;
        let profile = base + 4;
        let [owner_index, parameter_index, companion_index] =
            frame.unwrap_or([base + 8, base + 9, base + 10]);
        let family = family_ordinals.entry("Extrude").or_default();
        *family = family
            .checked_add(1)
            .ok_or_else(|| CodecError::InvalidInput("too many F3D extrusions".into()))?;
        extrusion_scopes.push(u64::from(scope));
        timeline_scopes.push((extrusion.ordinal, u64::from(scope)));
        mark_primary(ctx, &mut primary, scope, out.len())?;
        let start = out.len();
        header(&mut out, &tags[8], scope)?;
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
        mark(&mut secondary, scope, out.len());
        header(&mut out, &tags[0], scope)?;
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
        reference(&mut out, scope);
        mark(&mut secondary, group, out.len());
        header(&mut out, &tags[0], group)?;
        copy_groups.push(u64::from(group));
        mark_primary(ctx, &mut primary, profile, out.len())?;
        header(&mut out, &tags[6], profile)?;
        out.extend_from_slice(&[0; 10]);
        reference(&mut out, profile + 3);
        out.extend_from_slice(&1u32.to_le_bytes());
        native_lp_utf16(
            &mut out,
            extrusion
                .profile_asset
                .as_ref()
                .map_or("00000000-0000-4000-8000-000000000001", |asset| {
                    asset.as_str()
                }),
        )?;
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
            scope_record_index: Some(scope),
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
        (
            "00000000-0000-0000-0000-000000001401",
            None,
            1,
            "Fusion",
            removal_scopes,
        ),
        (
            "00000000-0000-0000-0000-000000001402",
            None,
            1,
            "Fusion",
            stitch_scopes,
        ),
        (
            "00000000-0000-0000-0000-000000001403",
            None,
            1,
            "Fusion",
            component_sources,
        ),
        (
            "00000000-0000-0000-0000-000000001404",
            None,
            1,
            "Fusion",
            component_results,
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
    written.extend(removals.into_iter().map(|feature| feature.id));
    written.extend(stitches.into_iter().map(|feature| feature.id));
    written.extend(forms.into_iter().map(|feature| feature.id));
    let component_definitions = component_copies
        .iter()
        .map(|feature| feature.definition.clone())
        .collect();
    let component_occurrences = component_copies
        .iter()
        .flat_map(|feature| feature.occurrences.iter().cloned())
        .collect();
    written.extend(component_copies.into_iter().map(|feature| feature.id));
    Ok(Some(EncodedFeatures {
        stream,
        features: written,
        parameters: written_parameters,
        collection_index,
        append,
        component_definitions,
        component_occurrences,
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
