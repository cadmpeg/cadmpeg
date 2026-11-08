// SPDX-License-Identifier: Apache-2.0
//! Project persistent construction inputs into feature input states.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Feature, FeatureInputOperand, FeatureInputOperandDefinition, FeatureInputTopology,
    SelectionReference,
};

use super::DesignNative;

pub(super) fn project(
    design: &DesignNative,
    features: &[Feature],
    inputs: &mut Vec<FeatureInputTopology>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let features_by_native: HashMap<_, _> = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect();
    let mut owners = HashMap::new();
    for scope in &design.design_parameter_scopes {
        if let (Some(stream), Some(feature)) = (
            crate::ids::native_stream(&scope.id),
            features_by_native.get(scope.id.as_str()),
        ) {
            owners.insert((stream, scope.record_index), *feature);
        }
    }
    for scope in &design.design_parameter_scopes {
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        let Some(target) = scope.assembly_alignment().and_then(
            crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index,
        ) else {
            continue;
        };
        if let Some(feature) = owners.get(&(stream, target)).copied() {
            owners
                .entry((stream, scope.record_index))
                .or_insert(feature);
        }
    }
    let groups: HashMap<_, _> = design
        .design_construction_operand_groups
        .iter()
        .filter_map(|group| {
            Some((
                (crate::ids::native_stream(&group.id)?, group.record_index),
                group.scope_record_index,
            ))
        })
        .collect();
    let extrude_groups: HashMap<_, _> = design
        .design_extrude_selection_groups
        .iter()
        .filter_map(|group| {
            Some((
                (crate::ids::native_stream(&group.id)?, group.record_index),
                group.scope_record_index,
            ))
        })
        .collect();
    let mut input_by_feature: HashMap<_, _> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| (input.input_of.clone(), index))
        .collect();
    let mut attach = |id: &str, scope: u32| -> Result<(), CodecError> {
        let Some(stream) = crate::ids::native_stream(id) else {
            losses.push(crate::loss::F3dLossCode::FeatureInputOwnerUnresolved.note(format!(
                "Feature input operand {id} has no native stream."
            )));
            return Ok(());
        };
        let Some(feature) = owners.get(&(stream, scope)).copied() else {
            losses.push(crate::loss::F3dLossCode::FeatureInputOwnerUnresolved.note(format!(
                "Feature input operand {id} has no projected feature for scope {scope}."
            )));
            return Ok(());
        };
        let index = if let Some(index) = input_by_feature.get(&feature.id).copied() {
            index
        } else {
            let index = inputs.len();
            inputs.push(FeatureInputTopology {
                id: cadmpeg_ir::ids::FeatureInputTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("f3d", "feature-input", "state"),
                    feature.id.key(),
                ),
                input_of: feature.id.clone(),
                bodies: cadmpeg_ir::features::DistinctMembers::default(),
                faces: cadmpeg_ir::features::DistinctMembers::default(),
                edges: cadmpeg_ir::features::DistinctMembers::default(),
                vertices: cadmpeg_ir::features::DistinctMembers::default(),
                operands: cadmpeg_ir::features::DistinctMembers::default(),
                edge_geometry: Vec::new(),
                native_ref: None,
            });
            input_by_feature.insert(feature.id.clone(), index);
            index
        };
        let native_ref = SelectionReference::new(id.to_owned()).map_err(CodecError::malformed)?;
        let tail = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(id);
        let key = cadmpeg_ir::ids::IdentityKey::from(tail.as_str().len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&tail);
        let operand = FeatureInputOperand {
            id: cadmpeg_ir::ids::FeatureInputOperandId::compose(
                &cadmpeg_ir::identity_namespace!("f3d", "feature-input", "operand"),
                key,
            ),
            definition: FeatureInputOperandDefinition::Native,
            native_ref,
        };
        inputs[index].operands.insert(operand);
        Ok(())
    };
    macro_rules! direct {
        ($($field:ident),* $(,)?) => {$(
            for record in &design.$field {
                attach(&record.id, record.scope_record_index)?;
            }
        )*};
    }
    direct!(
        design_body_recipe_operands,
        design_construction_operand_groups,
        design_edge_identity_operands,
        design_edge_operands,
        design_edge_treatment_vertex_operands,
        design_entity_selection_operands,
        design_face_operands,
        design_extrude_selection_groups,
        design_loft_legacy_body_carriers
    );
    for identity in &design.design_construction_operand_identities {
        if let Some(scope) = crate::ids::native_stream(&identity.id)
            .and_then(|stream| groups.get(&(stream, identity.group_record_index)))
        {
            attach(&identity.id, *scope)?;
        }
    }
    for member in &design.design_extrude_selection_members {
        if let Some(scope) = crate::ids::native_stream(&member.id)
            .and_then(|stream| extrude_groups.get(&(stream, member.group_record_index)))
        {
            attach(&member.id, *scope)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureEvaluation, FeatureOperation, UnresolvedFamily,
    };

    #[test]
    fn operand_without_projected_feature_reports_its_identity() {
        let mut design = DesignNative::default();
        design.design_construction_operand_groups.push(
            serde_json::from_value(serde_json::json!({
                "id": "f3d:Design/BulkStream.dat:construction-group#9",
                "scope_record_index": 7,
                "scope_reference_ordinal": 0,
                "record_index": 9,
                "class_tag": "300",
                "members": [11],
                "frame": {"opaque_index": 1, "opaque_scalar": 1.0, "variant": false},
                "role": 0x10_0000_0000u64,
                "paired_class_tag": "258"
            }))
            .unwrap(),
        );
        let mut inputs = Vec::new();
        let mut losses = Vec::new();
        project(&design, &[], &mut inputs, &mut losses).unwrap();
        assert!(inputs.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::FeatureInputOwnerUnresolved.kind()
        );
        assert!(losses[0].message.contains("construction-group#9"));
    }

    #[test]
    fn construction_inputs_survive_without_history_and_stay_stream_local() {
        let mut design = DesignNative::default();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#7",
            crate::records::feature::scope::DesignFeatureKind::WorkPoint,
            7,
        );
        let feature = Feature {
            product_definition: None,
            needs_recompute: false,
            id: "f3d:model:feature#7".try_into().unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumPoint,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        design.design_parameter_scopes.push(scope);
        let other_scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Other/BulkStream.dat:scope#7",
            crate::records::feature::scope::DesignFeatureKind::WorkPoint,
            7,
        );
        let mut other_feature = feature.clone();
        other_feature.id = "f3d:model:feature#other".try_into().unwrap();
        other_feature.native_ref = Some(other_scope.id.clone());
        design.design_parameter_scopes.push(other_scope);
        for (stream, index) in [
            ("Design/BulkStream.dat", 9),
            ("Design/BulkStream.dat", 10),
            ("Other/BulkStream.dat", 9),
        ] {
            design.design_construction_operand_groups.push(
                serde_json::from_value(serde_json::json!({
                    "id": format!("f3d:{stream}:construction-group#{index}"),
                    "scope_record_index": 7,
                    "scope_reference_ordinal": 0,
                    "record_index": index,
                    "class_tag": "300",
                    "members": [11],
                    "frame": {"opaque_index": 1, "opaque_scalar": 1.0, "variant": false},
                    "role": 0x10_0000_0000u64,
                    "paired_class_tag": "258"
                }))
                .unwrap(),
            );
        }
        let mut inputs = Vec::new();
        let mut losses = Vec::new();
        project(
            &design,
            &[feature.clone(), other_feature.clone()],
            &mut inputs,
            &mut losses,
        )
        .unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].input_of, feature.id);
        assert_eq!(inputs[0].native_ref, None);
        let refs: Vec<_> = inputs[0]
            .operands
            .iter()
            .map(|operand| operand.native_ref.as_str())
            .collect();
        assert_eq!(
            refs,
            [
                "f3d:Design/BulkStream.dat:construction-group#9",
                "f3d:Design/BulkStream.dat:construction-group#10"
            ]
        );
        assert_ne!(inputs[0].operands[0].id, inputs[0].operands[1].id);
        assert_eq!(inputs[1].input_of, other_feature.id);
        assert_eq!(
            inputs[1].operands[0].native_ref.as_str(),
            "f3d:Other/BulkStream.dat:construction-group#9"
        );
        let mut with_history = inputs.clone();
        with_history[0].native_ref = Some("f3d:history:state#4".into());
        let history_id = with_history[0].id.clone();
        project(&design, &[feature, other_feature], &mut with_history, &mut losses).unwrap();
        assert!(losses.is_empty());
        assert_eq!(with_history.len(), 2);
        assert_eq!(with_history[0].id, history_id);
        assert_eq!(
            with_history[0].native_ref.as_deref(),
            Some("f3d:history:state#4")
        );
        assert_eq!(with_history[0].operands.len(), 2);
        let mut model = cadmpeg_ir::document::Model::default();
        model.feature_input_topologies = inputs;
        let mut links = Vec::new();
        cadmpeg_ir::schema::native_links::visit_native_references(
            &model,
            &mut |_, _, id, native| {
                if let Some(native) = native {
                    links.push((id.to_owned(), native.to_owned()));
                }
            },
        );
        assert_eq!(links.len(), 3);
        assert!(links.iter().any(|(id, native)| id
            == model.feature_input_topologies[0].operands[0].id.as_str()
            && native == "f3d:Design/BulkStream.dat:construction-group#9"));
    }
}
