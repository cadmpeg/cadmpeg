use super::{
    circle_dimension_handle_driver, feature_intervals, is_solver_point_operand,
    relation_declaration_candidates, relation_instances, relation_signature,
    relation_uses_dynamic_operands, UNKNOWN_FEATURE_SPAN,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::FeatureInputOperand;
use crate::records::FeatureInputOperandKind;
use crate::records::FeatureInputRelationFamily;
use crate::records::FeatureInputScalar;
use crate::records::FeatureInputScalarRole;
use crate::records::FeatureSource;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName,
    FeatureInputRelationBinding,
};
use std::collections::BTreeMap;

fn class(offset: u64, name: &str) -> FeatureInputClass {
    FeatureInputClass {
        id: format!("class-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        name: name.into(),
    }
}

fn scalar(offset: u64, role: FeatureInputScalarRole) -> FeatureInputScalar {
    let operands = [0_u16, 1]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_index)| FeatureInputOperand {
            offset: offset + cadmpeg_core::decode::u64_from_index(ordinal),
            reference_ref: format!("reference-{offset}-{ordinal}"),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
            entity_index,
            entity_ref: None,
        })
        .collect();
    FeatureInputScalar {
        id: format!("scalar-{offset}"),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_id: 1,
        name: "dimension".into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
        role,

        operands,
    }
}

fn lane(classes: Vec<FeatureInputClass>, scalars: Vec<FeatureInputScalar>) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes,
        names: Vec::new(),
        scalars,
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn sketch_history() -> Vec<FeatureHistory> {
    vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "sketch".into(),
            parent: "history".into(),
            xml_tag: "Sketch".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "Sketch".into(),
            kind: "Sketch".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }]
}

fn dynamic_scalar(
    offset: u64,
    kind: FeatureInputOperandKind,
    indices: &[u16],
    value: f64,
) -> FeatureInputScalar {
    let mut scalar = scalar(offset, FeatureInputScalarRole::Driving);
    scalar.value = cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite test scalar");

    scalar.operands = indices
        .iter()
        .enumerate()
        .map(|(ordinal, entity_index)| FeatureInputOperand {
            offset: offset + cadmpeg_core::decode::u64_from_index(ordinal),
            reference_ref: format!("reference-{offset}-{ordinal}"),
            kind,
            entity_index: *entity_index,
            entity_ref: None,
        })
        .collect();
    scalar
}

mod circles;
mod declarations;
mod dynamic_operands;
