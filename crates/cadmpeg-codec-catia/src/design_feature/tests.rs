// SPDX-License-Identifier: Apache-2.0
//! Design-feature transfer tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::*;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::units::Units;
use std::io::Cursor;

use crate::native::{
    CatiaDefinitionChainValue, CatiaDefinitionValue, CatiaDesignClass, CatiaDesignObjectRelation,
    CatiaDesignObjectRelationSource, CatiaEntityEvaluation, CatiaEntityEvaluationEncoding,
    CatiaEntityRecord, CatiaEntitySchemaValue, CatiaEntitySuffixPayload,
    CatiaEntitySuffixSchemaValue, CatiaObjectGraph, CatiaObjectOwner,
    CatiaObjectRecordReferenceSource,
};
use crate::object_graph::HeadToken;
use crate::object_graph::ObjectPayload;
use crate::test_support::*;
use crate::CatiaCodec;

mod range;
mod reference_planes;

fn design_object(id: &str, owner_design_object: Option<&str>) -> CatiaDesignObject {
    CatiaDesignObject {
        id: id.to_string(),
        parent: "graph".to_string(),
        ordinal: 0,
        first_field_byte_offset: 0,
        owner_entity_id: 0,
        owner_record: None,
        owner_design_object: owner_design_object.map(str::to_string),
        owner_class: None,
        owner_storage_ref: None,
        fields: Vec::new(),
        field_classes: Vec::new(),
        definition_values: Vec::new(),
        definition_chain_values: Vec::new(),
        relations: Vec::new(),
        parallel_reference_table: None,
    }
}

fn feature(id: &str, native_ref: &str) -> Feature {
    Feature {
        id: FeatureId::from(id),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::StoredGeometry,
        native_ref: Some(native_ref.to_string()),
    }
}

fn payload_relation(target_object: &str, payload_offset: u64) -> CatiaDesignObjectRelation {
    CatiaDesignObjectRelation {
        source_field: "source-field".to_string(),
        source_class: None,
        source: CatiaDesignObjectRelationSource::Payload {
            payload_offset,
            container: CatiaObjectRecordReferenceSource::Field,
        },
        target_entity_id: payload_offset as u32,
        target_field: "target-field".to_string(),
        target_class: None,
        target_design_object: Some(target_object.to_string()),
    }
}

fn storage_relation(target_object: &str) -> CatiaDesignObjectRelation {
    let mut relation = payload_relation(target_object, 0);
    relation.source = CatiaDesignObjectRelationSource::Storage;
    relation
}

fn native_operation_object(
    id: &str,
    owner_design_object: Option<&str>,
    owner_entity_id: u32,
    owner_record: &str,
    class_name: &str,
    class_entry: &str,
) -> CatiaDesignObject {
    let mut object = design_object(id, owner_design_object);
    object.owner_entity_id = owner_entity_id;
    object.owner_record = Some(owner_record.to_string());
    object.owner_class = Some(CatiaDesignClass {
        entry: class_entry.to_string(),
        name: class_name.to_string(),
    });
    object
}

fn object_record(
    id: &str,
    design_object: Option<&str>,
    entity_id: Option<u32>,
    owner: Option<u32>,
    class_name: Option<&str>,
    class_entry: Option<&str>,
) -> CatiaObjectRecord {
    CatiaObjectRecord {
        id: id.to_string(),
        parent: "graph".to_string(),
        design_object: design_object.map(str::to_string),
        entity_record: None,
        entity_id,
        ordinal: 0,
        byte_offset: 0,
        byte_len: 0,
        lead: 0,
        head: Vec::new(),
        inline_body: None,
        owner: owner.map(CatiaObjectOwner::Entity),
        class_ref: None,
        class_name: class_name.map(str::to_string),
        class_entry: class_entry.map(str::to_string),
        storage_ref: None,
        storage_record: None,
        storage_design_object: None,
        payload: ObjectPayload {
            size: 1,
            fields: vec![PayloadField::Terminator],
        },
        repeated_reference_suffix: None,
        repeated_reference_schema_selection: None,
        subtype: PayloadSubtype::Empty,
        references: Vec::new(),
    }
}

fn entity_record(
    id: &str,
    object_record: &str,
    byte_offset: u64,
    entity_id: u32,
) -> CatiaEntityRecord {
    CatiaEntityRecord {
        id: id.to_string(),
        object_graph: "graph".to_string(),
        object_record: object_record.to_string(),
        ordinal: 0,
        byte_offset,
        byte_len: 0,
        lead: 0,
        inline_body: None,
        definition_len: 0,
        definition_prefix: Vec::new(),
        definition_schema_selections: Vec::new(),
        entity_id,
        definition_suffix: Vec::new(),
        value_len: 0,
        value_payload: Vec::new(),
        value_fields: Vec::new(),
        value_schema_selections: Vec::new(),
        relation_expression: None,
        parameter_value: None,
        range_interval: None,
        constraint_range: None,
        definition_value: None,
        definition_chain_value: None,
        relation_program_instance: None,
        schema_configuration_record: None,
        schema_configuration_row_link: None,
        formula_relation: None,
        value_packets: Vec::new(),
        numeric_pair: None,
        reference_signature: None,
        record_suffix: Vec::new(),
        suffix_value: None,
        suffix_framing: None,
        suffix_schema_selection: None,
    }
}

#[test]
fn compact_self_owned_operation_root_remains_an_identity_anchor() {
    let mut object = design_object("operation-object", None);
    object.owner_entity_id = 1;
    object.owner_record = Some("operation-record".to_string());

    let mut record = object_record(
        "operation-record",
        Some("operation-object"),
        Some(1),
        Some(1),
        Some("Prism_ThickThin2"),
        Some("operation-entry"),
    );
    record.lead = 0x1a;
    record.head = vec![
        HeadToken::Lead(0x1a),
        HeadToken::Reference(7),
        HeadToken::Reference(0),
        HeadToken::NullHandle,
        HeadToken::Reference(1),
    ];
    record.class_ref = Some(7);

    let native = CatiaNative {
        design_objects: vec![object],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![record],
        }],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(transfer.native_operation_records.is_empty());
}

#[test]
fn malformed_compact_root_does_not_promote_an_operation() {
    let mut object = design_object("operation-object", None);
    object.owner_entity_id = 1;
    object.owner_record = Some("operation-record".to_string());

    let mut record = object_record(
        "operation-record",
        Some("operation-object"),
        Some(1),
        Some(1),
        Some("Prism_ThickThin2"),
        Some("operation-entry"),
    );
    record.lead = 0x1a;
    record.head = vec![
        HeadToken::Lead(0x1a),
        HeadToken::Reference(7),
        HeadToken::Reference(0),
        HeadToken::NullHandle,
        HeadToken::Reference(1),
        HeadToken::Literal(0),
    ];
    record.class_ref = Some(7);

    let native = CatiaNative {
        design_objects: vec![object],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![record],
        }],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(transfer.native_operation_records.is_empty());
}

fn parameter(id: &str, native_ref: &str) -> cadmpeg_ir::features::DesignParameter {
    cadmpeg_ir::features::DesignParameter {
        id: ParameterId(id.to_string()),
        owner: None,
        ordinal: 99,
        name: id.to_string(),
        expression: "1 mm".to_string(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1.0),
        )),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(native_ref.to_string()),
    }
}

#[test]
fn assigns_parent_from_an_exact_transferred_owner_chain() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("parent-object", None),
            design_object("child-object", Some("parent-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    let mut parent_feature = feature("parent-feature", "parent-object");
    parent_feature.ordinal = 10;
    let mut child_feature = feature("child-feature", "child-object");
    child_feature.ordinal = 20;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "parent-object".to_string(),
                FeatureId::from("parent-feature"),
            ),
            ("child-object".to_string(), FeatureId::from("child-feature")),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert_eq!(ir.model.features[0].parent, None);
    assert_eq!(
        ir.model.features[1].parent,
        Some(FeatureId::from("parent-feature"))
    );
}

#[test]
fn assigns_parent_from_the_nearest_transferred_ancestor() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("parent-object", None),
            design_object("group-object", Some("parent-object")),
            design_object("child-object", Some("group-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    let mut parent_feature = feature("parent-feature", "parent-object");
    parent_feature.ordinal = 10;
    let mut child_feature = feature("child-feature", "child-object");
    child_feature.ordinal = 20;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "parent-object".to_string(),
                FeatureId::from("parent-feature"),
            ),
            ("child-object".to_string(), FeatureId::from("child-feature")),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert_eq!(
        ir.model.features[1].parent,
        Some(FeatureId::from("parent-feature"))
    );
}

#[test]
fn rejects_a_parent_that_does_not_precede_its_child() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("parent-object", None),
            design_object("child-object", Some("parent-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    let mut parent_feature = feature("parent-feature", "parent-object");
    parent_feature.ordinal = 20;
    let mut child_feature = feature("child-feature", "child-object");
    child_feature.ordinal = 10;
    ir.model.features.push(parent_feature);
    ir.model.features.push(child_feature);
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "parent-object".to_string(),
                FeatureId::from("parent-feature"),
            ),
            ("child-object".to_string(), FeatureId::from("child-feature")),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.parent.is_none()));
}

#[test]
fn does_not_assign_a_self_parent() {
    let native = CatiaNative {
        design_objects: vec![design_object("feature-object", Some("feature-object"))],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(feature("feature", "feature-object"));
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([("feature-object".to_string(), FeatureId::from("feature"))]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert_eq!(ir.model.features[0].parent, None);
}

#[test]
fn omits_all_parents_in_an_owner_cycle() {
    let native = CatiaNative {
        design_objects: vec![
            design_object("first-object", Some("second-object")),
            design_object("second-object", Some("first-object")),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .features
        .push(feature("first-feature", "first-object"));
    ir.model
        .features
        .push(feature("second-feature", "second-object"));
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            ("first-object".to_string(), FeatureId::from("first-feature")),
            (
                "second-object".to_string(),
                FeatureId::from("second-feature"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_parents(&mut ir, &native);

    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.parent.is_none()));
}

#[test]
fn assigns_only_prior_payload_feature_dependencies_in_relation_order() {
    let mut source = design_object("source-object", None);
    let mut unresolved = payload_relation("unresolved-object", 6);
    unresolved.target_design_object = None;
    source.relations = vec![
        payload_relation("first-object", 0),
        payload_relation("first-object", 1),
        payload_relation("second-child", 2),
        payload_relation("forward-object", 3),
        storage_relation("storage-object"),
        payload_relation("source-object", 5),
        unresolved,
        payload_relation("broken-object", 7),
        payload_relation("cycle-first", 8),
    ];
    let broken = design_object("broken-object", Some("missing-object"));
    let cycle_first = design_object("cycle-first", Some("cycle-second"));
    let cycle_second = design_object("cycle-second", Some("cycle-first"));
    let native = CatiaNative {
        design_objects: vec![
            design_object("first-object", None),
            design_object("second-object", None),
            design_object("second-child", Some("second-object")),
            design_object("forward-object", None),
            design_object("storage-object", None),
            broken,
            cycle_first,
            cycle_second,
            source,
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    for (id, native_ref, ordinal) in [
        ("first-feature", "first-object", 10),
        ("second-feature", "second-object", 15),
        ("source-feature", "source-object", 20),
        ("forward-feature", "forward-object", 30),
        ("storage-feature", "storage-object", 5),
    ] {
        let mut item = feature(id, native_ref);
        item.ordinal = ordinal;
        ir.model.features.push(item);
    }
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            ("first-object".to_string(), FeatureId::from("first-feature")),
            (
                "second-object".to_string(),
                FeatureId::from("second-feature"),
            ),
            (
                "source-object".to_string(),
                FeatureId::from("source-feature"),
            ),
            (
                "forward-object".to_string(),
                FeatureId::from("forward-feature"),
            ),
            (
                "storage-object".to_string(),
                FeatureId::from("storage-feature"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    transfer.assign_feature_dependencies(&mut ir, &native);

    let source = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id == FeatureId::from("source-feature"))
        .unwrap();
    assert_eq!(
        source.dependencies,
        [
            FeatureId::from("first-feature"),
            FeatureId::from("second-feature")
        ]
    );
    assert!(ir
        .model
        .features
        .iter()
        .filter(|feature| feature.id != FeatureId::from("source-feature"))
        .all(|feature| feature.dependencies.is_empty()));
}

#[test]
fn transfers_admitted_native_operations_with_exact_parentage() {
    let mut parent = native_operation_object(
        "parent-object",
        None,
        1,
        "parent-record",
        "Prism_ThickThin1",
        "parent-entry",
    );
    parent.first_field_byte_offset = 10;
    let mut child = native_operation_object(
        "child-object",
        Some("parent-object"),
        2,
        "child-record",
        "EdgeFillet",
        "child-entry",
    );
    child.first_field_byte_offset = 20;
    let native = CatiaNative {
        design_objects: vec![parent, child],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                object_record(
                    "parent-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("parent-entry"),
                ),
                object_record(
                    "child-record",
                    Some("parent-object"),
                    Some(2),
                    Some(1),
                    Some("EdgeFillet"),
                    Some("child-entry"),
                ),
            ],
        }],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.features[0].ordinal, 10);
    assert_eq!(ir.model.features[1].ordinal, 20);
    assert_eq!(
        ir.model.features[1].parent,
        Some(FeatureId::from("parent-object:feature"))
    );
    assert!(matches!(
        ir.model.features[0].definition,
        FeatureDefinition::ExtrudeUnresolved
    ));
    assert!(matches!(
        ir.model.features[1].definition,
        FeatureDefinition::FilletUnresolved
    ));
    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.source_properties.is_empty()));
    assert_eq!(
        transfer.native_operation_records,
        HashSet::from(["parent-record".to_string(), "child-record".to_string()])
    );
    assert_eq!(
        transfer.consumed_records(),
        transfer.native_operation_records
    );
}

#[test]
fn maps_each_admitted_operation_class_to_its_neutral_family() {
    let cases = [
        ("prism-one", "Prism_ThickThin1", "prism-one-record", 1_u32),
        ("prism-two", "Prism_ThickThin2", "prism-two-record", 2_u32),
        (
            "end-limit",
            "Prism_EndLimit_Length",
            "end-limit-record",
            3_u32,
        ),
        ("revolution", "Revol_ThickThin1", "revolution-record", 4_u32),
        ("sweep", "Sweep_ThickThin1", "sweep-record", 5_u32),
        ("fillet", "EdgeFillet", "fillet-record", 6_u32),
        (
            "circular-pattern",
            "CircPattern_RadialNumber",
            "circular-pattern-record",
            7_u32,
        ),
    ];
    let objects = cases
        .iter()
        .enumerate()
        .map(|(ordinal, (id, kind, record, entity_id))| {
            let mut object = native_operation_object(
                id,
                None,
                *entity_id,
                record,
                kind,
                &format!("{kind}-entry"),
            );
            object.first_field_byte_offset = (ordinal as u64) * 10;
            object
        })
        .collect::<Vec<_>>();
    let records = cases
        .iter()
        .map(|(_id, kind, record, entity_id)| {
            object_record(
                record,
                None,
                Some(*entity_id),
                None,
                Some(kind),
                Some(&format!("{kind}-entry")),
            )
        })
        .collect::<Vec<_>>();
    let native = CatiaNative {
        design_objects: objects,
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records,
        }],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    transfer_design_features(&mut ir, &native, None);

    assert_eq!(ir.model.features.len(), cases.len());
    for feature in &ir.model.features {
        match feature.source_tag.as_deref() {
            Some("Prism_EndLimit_Length" | "Prism_ThickThin1" | "Prism_ThickThin2") => {
                assert!(matches!(
                    feature.definition,
                    FeatureDefinition::ExtrudeUnresolved
                ));
            }
            Some("Revol_ThickThin1") => {
                assert!(matches!(
                    feature.definition,
                    FeatureDefinition::RevolveUnresolved
                ));
            }
            Some("Sweep_ThickThin1") => {
                let FeatureDefinition::Sweep {
                    section,
                    path,
                    mode,
                    ..
                } = &feature.definition
                else {
                    panic!("expected a typed unresolved sweep");
                };
                assert!(matches!(
                    section,
                    cadmpeg_ir::features::SweepSection::Unresolved(Some(_))
                ));
                assert!(matches!(
                    path,
                    Some(cadmpeg_ir::features::PathRef::Unresolved(_))
                ));
                assert!(matches!(mode, cadmpeg_ir::features::SweepMode::Unresolved));
            }
            Some("EdgeFillet") => {
                assert!(matches!(
                    feature.definition,
                    FeatureDefinition::FilletUnresolved
                ));
            }
            Some("CircPattern_RadialNumber") => {
                let FeatureDefinition::Pattern { seeds, pattern } = &feature.definition else {
                    panic!("expected a typed unresolved circular pattern");
                };
                assert!(seeds.is_empty());
                assert!(matches!(
                    pattern,
                    cadmpeg_ir::features::PatternKind::Unresolved {
                        form: Some(cadmpeg_ir::features::PatternForm::Circular)
                    }
                ));
            }
            other => panic!("unexpected operation source tag: {other:?}"),
        }
    }
}

#[test]
fn transfers_exact_definition_values_as_typed_feature_properties() {
    let mut operation = native_operation_object(
        "operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation
        .definition_values
        .push("definition-entity".to_string());
    operation.fields.push("definition-record".to_string());
    let mut definition_entity = entity_record("definition-entity", "definition-record", 20, 1);
    definition_entity.definition_value = Some(CatiaDefinitionValue {
        definition: CatiaEntitySchemaValue {
            offset: 4,
            ordinal: 2,
            entry: "definition-entry".to_string(),
            value: "Mirror".to_string(),
        },
        payload: CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 8,
            evaluation: CatiaEntityEvaluation::Scalar {
                bits: 12.5_f64.to_bits(),
            },
            encoding: CatiaEntityEvaluationEncoding::Direct,
        },
        schema_selection: None,
    });
    let native = CatiaNative {
        design_objects: vec![operation],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                object_record(
                    "operation-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("operation-entry"),
                ),
                object_record(
                    "definition-record",
                    Some("operation-object"),
                    Some(1),
                    Some(1),
                    None,
                    None,
                ),
            ],
        }],
        entity_records: vec![definition_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert!(matches!(
        ir.model.features[0].definition,
        FeatureDefinition::ExtrudeUnresolved
    ));
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                "catia_definition_value_0_definition_entry".to_string(),
                "definition-entry".to_string(),
            ),
            (
                "catia_definition_value_0_definition_offset".to_string(),
                "4".to_string(),
            ),
            (
                "catia_definition_value_0_definition_ordinal".to_string(),
                "2".to_string(),
            ),
            (
                "catia_definition_value_0_definition_value".to_string(),
                "Mirror".to_string(),
            ),
            (
                "catia_definition_value_0_entity".to_string(),
                "definition-entity".to_string(),
            ),
            (
                "catia_definition_value_0_payload_encoding".to_string(),
                "direct".to_string(),
            ),
            (
                "catia_definition_value_0_payload_evaluation".to_string(),
                "scalar".to_string(),
            ),
            (
                "catia_definition_value_0_payload_evaluation_bits".to_string(),
                format!("{:016x}", 12.5_f64.to_bits()),
            ),
            (
                "catia_definition_value_0_payload_kind".to_string(),
                "evaluation".to_string(),
            ),
            (
                "catia_definition_value_0_payload_opcode_offset".to_string(),
                "8".to_string(),
            ),
        ])
    );
    assert_eq!(transfer.native_operation_definition_value_count, 1);
    assert_eq!(
        transfer.native_operation_definition_value_records,
        HashSet::from(["definition-record".to_string()])
    );
    assert_eq!(
        transfer.consumed_records(),
        HashSet::from([
            "operation-record".to_string(),
            "definition-record".to_string()
        ])
    );
}

#[test]
fn transfers_exact_definition_chains_as_typed_feature_properties() {
    let mut operation = native_operation_object(
        "operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation
        .definition_chain_values
        .push("definition-chain-entity".to_string());
    operation.fields.push("definition-chain-record".to_string());
    let mut chain_entity =
        entity_record("definition-chain-entity", "definition-chain-record", 20, 1);
    chain_entity.definition_chain_value = Some(CatiaDefinitionChainValue {
        selector: CatiaEntitySchemaValue {
            offset: 4,
            ordinal: 2,
            entry: "selector-entry".to_string(),
            value: "Length".to_string(),
        },
        role: CatiaEntitySchemaValue {
            offset: 8,
            ordinal: 3,
            entry: "role-entry".to_string(),
            value: "UnsupportedRole".to_string(),
        },
        value: CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset: 12,
            evaluation: CatiaEntityEvaluation::Scalar {
                bits: 12.5_f64.to_bits(),
            },
        },
    });
    let native = CatiaNative {
        design_objects: vec![operation],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                object_record(
                    "operation-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("operation-entry"),
                ),
                object_record(
                    "definition-chain-record",
                    Some("operation-object"),
                    Some(1),
                    Some(1),
                    None,
                    None,
                ),
            ],
        }],
        entity_records: vec![chain_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert!(matches!(
        ir.model.features[0].definition,
        FeatureDefinition::ExtrudeUnresolved
    ));
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                "catia_definition_chain_value_0_entity".to_string(),
                "definition-chain-entity".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_entry".to_string(),
                "role-entry".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_offset".to_string(),
                "8".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_ordinal".to_string(),
                "3".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_value".to_string(),
                "UnsupportedRole".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_entry".to_string(),
                "selector-entry".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_offset".to_string(),
                "4".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_ordinal".to_string(),
                "2".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_value".to_string(),
                "Length".to_string(),
            ),
            (
                "catia_definition_chain_value_0_value_evaluation".to_string(),
                "scalar".to_string(),
            ),
            (
                "catia_definition_chain_value_0_value_evaluation_bits".to_string(),
                format!("{:016x}", 12.5_f64.to_bits()),
            ),
            (
                "catia_definition_chain_value_0_value_kind".to_string(),
                "evaluation".to_string(),
            ),
            (
                "catia_definition_chain_value_0_value_opcode_offset".to_string(),
                "12".to_string(),
            ),
        ])
    );
    assert_eq!(transfer.native_operation_definition_chain_value_count, 1);
    assert_eq!(
        transfer.native_operation_definition_chain_value_records,
        HashSet::from(["definition-chain-record".to_string()])
    );
    assert_eq!(
        transfer.consumed_records(),
        HashSet::from([
            "operation-record".to_string(),
            "definition-chain-record".to_string()
        ])
    );
}

#[test]
fn transfers_definition_chains_from_exact_operation_owner_descendants() {
    let mut operation = native_operation_object(
        "operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation.first_field_byte_offset = 10;
    let mut descendant = design_object("descendant-object", Some("operation-object"));
    descendant.first_field_byte_offset = 20;
    descendant
        .definition_chain_values
        .push("descendant-chain-entity".to_string());
    let mut descendant_entity =
        entity_record("descendant-chain-entity", "descendant-record", 30, 2);
    descendant_entity.definition_chain_value = Some(CatiaDefinitionChainValue {
        selector: CatiaEntitySchemaValue {
            offset: 2,
            ordinal: 4,
            entry: "selector-entry".to_string(),
            value: "Length".to_string(),
        },
        role: CatiaEntitySchemaValue {
            offset: 7,
            ordinal: 5,
            entry: "role-entry".to_string(),
            value: "UnsupportedRole".to_string(),
        },
        value: CatiaEntitySuffixSchemaValue::Atom { value: 3 },
    });
    let native = CatiaNative {
        design_objects: vec![operation, descendant],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![object_record(
                "operation-record",
                None,
                Some(1),
                None,
                Some("Prism_ThickThin1"),
                Some("operation-entry"),
            )],
        }],
        entity_records: vec![descendant_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert!(matches!(
        ir.model.features[0].definition,
        FeatureDefinition::ExtrudeUnresolved
    ));
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                "catia_definition_chain_value_0_entity".to_string(),
                "descendant-chain-entity".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_entry".to_string(),
                "role-entry".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_offset".to_string(),
                "7".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_ordinal".to_string(),
                "5".to_string(),
            ),
            (
                "catia_definition_chain_value_0_role_value".to_string(),
                "UnsupportedRole".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_entry".to_string(),
                "selector-entry".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_offset".to_string(),
                "2".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_ordinal".to_string(),
                "4".to_string(),
            ),
            (
                "catia_definition_chain_value_0_selector_value".to_string(),
                "Length".to_string(),
            ),
            (
                "catia_definition_chain_value_0_value_kind".to_string(),
                "atom".to_string(),
            ),
            (
                "catia_definition_chain_value_0_value_atom".to_string(),
                "3".to_string(),
            ),
        ])
    );
    assert_eq!(transfer.native_operation_definition_chain_value_count, 1);
}

#[test]
fn orders_exact_feature_parameters_by_serialized_field_position() {
    let operation = native_operation_object(
        "operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    let operation_record = object_record(
        "operation-record",
        None,
        Some(1),
        None,
        Some("Prism_ThickThin1"),
        Some("operation-entry"),
    );
    let mut late_parameter_record = object_record(
        "late-parameter-record",
        Some("operation-object"),
        Some(3),
        Some(1),
        None,
        None,
    );
    late_parameter_record.byte_offset = 30;
    late_parameter_record.entity_record = Some("late-parameter-entity".to_string());
    let mut early_parameter_record = object_record(
        "early-parameter-record",
        Some("operation-object"),
        Some(2),
        Some(1),
        None,
        None,
    );
    early_parameter_record.byte_offset = 20;
    early_parameter_record.entity_record = Some("early-parameter-entity".to_string());
    let native = CatiaNative {
        design_objects: vec![operation],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                operation_record,
                late_parameter_record,
                early_parameter_record,
            ],
        }],
        entity_records: vec![
            entity_record("late-parameter-entity", "late-parameter-record", 300, 3),
            entity_record("early-parameter-entity", "early-parameter-record", 200, 2),
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .parameters
        .push(parameter("late-parameter", "late-parameter-entity"));
    ir.model
        .parameters
        .push(parameter("early-parameter", "early-parameter-entity"));
    let mut document_parameter = parameter("document-parameter", "document-parameter-entity");
    document_parameter.ordinal = 2;
    ir.model.parameters.push(document_parameter);

    let transfer = transfer_design_features(&mut ir, &native, None);
    transfer.assign_parameter_owners(&mut ir, &native);

    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (
                parameter.id.clone(),
                parameter.owner.clone(),
                parameter.ordinal
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                ParameterId("late-parameter".to_string()),
                Some(FeatureId::from("operation-object:feature")),
                1,
            ),
            (
                ParameterId("early-parameter".to_string()),
                Some(FeatureId::from("operation-object:feature")),
                0,
            ),
            (ParameterId("document-parameter".to_string()), None, 0,),
        ]
    );
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                "catia_parameter_early-parameter".to_string(),
                "1 mm".to_string()
            ),
            (
                "catia_parameter_late-parameter".to_string(),
                "1 mm".to_string()
            ),
        ])
    );
}

#[test]
fn assigns_a_nested_parameter_to_the_nearest_operation() {
    let mut parent = native_operation_object(
        "parent-operation",
        None,
        1,
        "parent-record",
        "Prism_ThickThin1",
        "parent-entry",
    );
    parent.first_field_byte_offset = 10;
    let mut child = native_operation_object(
        "child-operation",
        Some("parent-operation"),
        2,
        "child-record",
        "Prism_ThickThin2",
        "child-entry",
    );
    child.first_field_byte_offset = 20;
    let parent_record = object_record(
        "parent-record",
        None,
        Some(1),
        None,
        Some("Prism_ThickThin1"),
        Some("parent-entry"),
    );
    let child_record = object_record(
        "child-record",
        Some("parent-operation"),
        Some(2),
        Some(1),
        Some("Prism_ThickThin2"),
        Some("child-entry"),
    );
    let mut parameter_record = object_record(
        "parameter-record",
        Some("child-operation"),
        Some(3),
        Some(2),
        None,
        None,
    );
    parameter_record.entity_record = Some("parameter-entity".to_string());
    let native = CatiaNative {
        design_objects: vec![parent, child],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![parent_record, child_record, parameter_record],
        }],
        entity_records: vec![entity_record("parameter-entity", "parameter-record", 30, 3)],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .parameters
        .push(parameter("parameter", "parameter-entity"));

    let transfer = transfer_design_features(&mut ir, &native, None);
    transfer.assign_parameter_owners(&mut ir, &native);

    let child_feature = FeatureId::from("child-operation:feature");
    assert_eq!(ir.model.parameters[0].owner, Some(child_feature.clone()));
    assert_eq!(
        ir.model.features[1].parent,
        Some(FeatureId::from("parent-operation:feature"))
    );
    assert_eq!(
        ir.model.features[1]
            .source_properties
            .get("catia_parameter_parameter")
            .map(String::as_str),
        Some("1 mm")
    );
}

#[test]
fn native_parameter_map_uses_disambiguated_names_when_source_names_collide() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId::from("feature"),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Prism_ThickThin1".to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Prism_ThickThin1".to_string(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: Some("native-feature".to_string()),
    });
    let mut first = parameter("first", "first-native");
    first.name = "Length".to_string();
    let mut second = parameter("second", "second-native");
    second.name = "Length".to_string();
    ir.model.parameters.extend([first, second]);

    normalize_parameter_names(&mut ir);
    assign_native_operation_parameter_values(
        &mut ir,
        &HashMap::from([
            (ParameterId("first".to_string()), FeatureId::from("feature")),
            (
                ParameterId("second".to_string()),
                FeatureId::from("feature"),
            ),
        ]),
    );

    let FeatureDefinition::Native { parameters, .. } = &ir.model.features[0].definition else {
        panic!("expected an opaque native operation");
    };
    assert_eq!(
        parameters,
        &BTreeMap::from([
            ("Length".to_string(), "1 mm".to_string()),
            ("Length#1".to_string(), "1 mm".to_string()),
        ])
    );
}

#[test]
fn native_parameter_map_retains_circular_pattern_values_in_source_properties() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId::from("pattern-feature"),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("CircPattern_RadialNumber".to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: cadmpeg_ir::features::PatternKind::Unresolved {
                form: Some(cadmpeg_ir::features::PatternForm::Circular),
            },
        },
        native_ref: Some("pattern-feature".to_string()),
    });
    let mut value = parameter("pattern-parameter", "pattern-native");
    value.name = "Number".to_string();
    value.expression = "3".to_string();
    ir.model.parameters.push(value);

    normalize_parameter_names(&mut ir);
    assign_native_operation_parameter_values(
        &mut ir,
        &HashMap::from([(
            ParameterId("pattern-parameter".to_string()),
            FeatureId::from("pattern-feature"),
        )]),
    );

    assert_eq!(
        ir.model.features[0]
            .source_properties
            .get("catia_parameter_Number")
            .map(String::as_str),
        Some("3")
    );
}

#[test]
fn disambiguates_parameter_names_without_hiding_a_later_source_name() {
    let owner = FeatureId::from("feature");
    let mut ir = CadIr::empty(Units::default());
    let mut first = parameter("first", "first-native");
    first.owner = Some(owner.clone());
    first.name = "Angle".to_string();
    let mut second = parameter("second", "second-native");
    second.owner = Some(owner.clone());
    second.name = "Angle".to_string();
    let mut later_source_name = parameter("later", "later-native");
    later_source_name.owner = Some(owner);
    later_source_name.name = "Angle#1".to_string();
    ir.model
        .parameters
        .extend([first, second, later_source_name]);

    normalize_parameter_names(&mut ir);

    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["Angle", "Angle#2", "Angle#1"]
    );
    assert_eq!(
        ir.model.parameters[1].properties.get("source_name"),
        Some(&"Angle".to_string())
    );
    assert!(!ir.model.parameters[2]
        .properties
        .contains_key("source_name"));
}

#[test]
fn does_not_promote_an_unadmitted_helper_owner_class() {
    let object = native_operation_object(
        "helper-object",
        None,
        1,
        "helper-record",
        "Unadmitted_Helper",
        "helper-entry",
    );
    let native = CatiaNative {
        design_objects: vec![object],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![object_record(
                "helper-record",
                None,
                Some(1),
                None,
                Some("Unadmitted_Helper"),
                Some("helper-entry"),
            )],
        }],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(transfer.native_operation_records.is_empty());
}

#[test]
fn pattern_schema_definition_does_not_create_a_feature_instance() {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0];
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &definition,
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    native.entity_records[0]
        .definition_value
        .as_mut()
        .expect("definition value")
        .definition
        .value = "CircPattern".to_string();
    native.object_graphs[0].records[0].class_name = Some("Element1".to_string());

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);
    assert!(ir.model.features.is_empty());
    assert!(transfer.consumed_records().is_empty());
}

#[test]
fn prt_sketch_schema_field_does_not_create_a_feature_instance() {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x83], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "PRTSketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.object_graphs[0].records[1].class_name.as_deref(),
        Some("PRTSketch")
    );

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().is_empty());
}

#[test]
fn exact_sketch_owner_declaration_transfers_identity_without_geometry() {
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &[0x00, 0x08, 0x32, 4, 0, 0, 0],
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    let owner_record = native
        .object_graphs
        .iter()
        .flat_map(|graph| graph.records.iter())
        .find(|record| record.design_object.is_some())
        .expect("synthetic owner declaration record")
        .clone();
    let owner_record_id = owner_record.id.clone();
    let owner_design_object = owner_record.design_object.clone();
    let owner_class_entry = "synthetic-sketch-class".to_string();
    let owner_record_mut = native
        .object_graphs
        .iter_mut()
        .flat_map(|graph| graph.records.iter_mut())
        .find(|record| record.id == owner_record_id)
        .expect("mutable synthetic owner declaration record");
    owner_record_mut.class_name = Some("Sketch".to_string());
    owner_record_mut.class_entry = Some(owner_class_entry.clone());

    let object = native
        .design_objects
        .first_mut()
        .expect("synthetic design object");
    object.owner_record = Some(owner_record_id.clone());
    object.owner_design_object = owner_design_object;
    object.owner_class = Some(crate::native::CatiaDesignClass {
        entry: owner_class_entry,
        name: "Sketch".to_string(),
    });

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    let parameter_entity = native
        .entity_records
        .iter()
        .find(|entity| {
            native
                .object_graphs
                .iter()
                .flat_map(|graph| graph.records.iter())
                .find(|record| record.id == entity.object_record)
                .and_then(|record| record.design_object.as_deref())
                == Some(native.design_objects[0].id.as_str())
        })
        .expect("synthetic feature-owned parameter entity");
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId("synthetic:parameter".to_string()),
            owner: None,
            ordinal: 0,
            name: "Value".to_string(),
            expression: String::new(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(parameter_entity.id.clone()),
        });
    transfer.assign_parameter_owners(&mut ir, &native);

    assert_eq!(ir.model.sketches.len(), 1);
    assert!(matches!(
        ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: Some(_),
        }
    ));
    assert!(ir.model.sketches[0].profiles.is_empty());
    assert_eq!(
        ir.model.sketches[0].placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved
    );
    assert_eq!(
        ir.model.parameters[0].owner,
        Some(cadmpeg_ir::features::FeatureId(
            crate::design_feature::neutral_history_id(&native.design_objects[0].id, "feature"),
        ))
    );
    assert_eq!(
        transfer.sketch_owner_records,
        std::collections::HashSet::from([owner_record_id])
    );
}

#[test]
fn incompatible_exact_feature_candidates_on_one_object_remain_unresolved() {
    let records = [
        object_graph_record(&[0x12, 0x84, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x84, 0x84], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x85, 0x85], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3, 4]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "xy-plane",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);

    let candidate = native
        .design_objects
        .iter()
        .find(|object| object.owner_entity_id == 4)
        .expect("synthetic dual-candidate object");
    assert_eq!(candidate.field_classes[0].name, "xy-plane");
    assert_eq!(candidate.owner_entity_id, 4);
    assert_eq!(
        candidate
            .owner_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Sketch")
    );
    assert_eq!(
        candidate.owner_design_object,
        Some(native.design_objects[1].id.clone())
    );

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().is_empty());
    assert!(transfer.feature_ids.is_empty());
}

#[test]
fn parameter_owner_follows_one_exact_child_design_object() {
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &[0x00, 0x08, 0x32, 4, 0, 0, 0],
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    let owner_record = native
        .object_graphs
        .iter()
        .flat_map(|graph| graph.records.iter())
        .find(|record| record.design_object.is_some())
        .expect("synthetic owner declaration record")
        .clone();
    let owner_record_id = owner_record.id.clone();
    let owner_design_object = owner_record.design_object.clone();
    let owner_class_entry = "synthetic-sketch-class".to_string();
    let owner_record_mut = native
        .object_graphs
        .iter_mut()
        .flat_map(|graph| graph.records.iter_mut())
        .find(|record| record.id == owner_record_id)
        .expect("mutable synthetic owner declaration record");
    owner_record_mut.class_name = Some("Sketch".to_string());
    owner_record_mut.class_entry = Some(owner_class_entry.clone());

    let feature_object = native
        .design_objects
        .first_mut()
        .expect("synthetic design object");
    feature_object.owner_record = Some(owner_record_id);
    feature_object.owner_design_object = owner_design_object.clone();
    feature_object.owner_class = Some(crate::native::CatiaDesignClass {
        entry: owner_class_entry,
        name: "Sketch".to_string(),
    });
    let feature_id = feature_object.id.clone();

    let child_record_id = "synthetic-child-record".to_string();
    let child_entity_id = "synthetic-child-entity".to_string();
    let mut child_record = owner_record.clone();
    child_record.id.clone_from(&child_record_id);
    child_record.entity_record = Some(child_entity_id.clone());
    child_record.entity_id = Some(2);
    child_record.owner = Some(crate::native::CatiaObjectOwner::Entity(2));
    child_record.design_object = Some("synthetic-child-object".to_string());
    native.object_graphs[0].records.push(child_record);

    let mut child_entity = native.entity_records[0].clone();
    child_entity.id.clone_from(&child_entity_id);
    child_entity.object_record = child_record_id.clone();
    child_entity.entity_id = 2;
    child_entity.ordinal = native.entity_records.len() as u64;
    native.entity_records.push(child_entity);

    let mut child_object = native.design_objects[0].clone();
    child_object.id = "synthetic-child-object".to_string();
    child_object.ordinal += 1;
    child_object.first_field_byte_offset += 1;
    child_object.owner_entity_id = 2;
    child_object.owner_record = Some(child_record_id);
    child_object.owner_design_object = Some(feature_id.clone());
    child_object.owner_class = None;
    child_object.owner_storage_ref = None;
    child_object.fields = vec!["synthetic-child-record".to_string()];
    child_object.field_classes.clear();
    child_object.definition_values.clear();
    child_object.definition_chain_values.clear();
    child_object.relations.clear();
    child_object.parallel_reference_table = None;
    native.design_objects.push(child_object);

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId("synthetic:child-parameter".to_string()),
            owner: None,
            ordinal: 0,
            name: "Value".to_string(),
            expression: String::new(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(child_entity_id),
        });

    transfer.assign_parameter_owners(&mut ir, &native);

    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(
        ir.model.parameters[0].owner,
        Some(cadmpeg_ir::features::FeatureId(
            crate::design_feature::neutral_history_id(&feature_id, "feature"),
        ))
    );
}

#[test]
fn complete_standalone_principal_plane_declarations_transfer_one_history_node() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    for (class, plane) in [
        ("xy-plane", PrincipalPlane::Top),
        ("yz-plane", PrincipalPlane::Right),
        ("zx-plane", PrincipalPlane::Front),
    ] {
        let records = [
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        ];
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            class,
        ]));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());

        let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.sketches.is_empty());
        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(
            ir.model.features[0].definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
        assert_eq!(ir.model.features[0].source_tag.as_deref(), Some(class));
        assert_eq!(
            ir.model.features[0].ordinal,
            native.design_objects[0].first_field_byte_offset
        );
        assert_eq!(
            transfer.principal_plane_records,
            native.design_objects[0].fields.iter().cloned().collect()
        );

        let mut excluded_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let excluded = crate::design_feature::transfer_design_features(
            &mut excluded_ir,
            &native,
            Some(&std::collections::HashSet::new()),
        );
        assert!(excluded_ir.model.features.is_empty());
        assert!(excluded.consumed_records().is_empty());
    }
}

#[test]
fn mixed_or_payload_bearing_principal_plane_fields_do_not_transfer() {
    for (records, catalog) in [
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
                "yz-plane",
            ],
        ),
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x84], &[0x80, 0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
            ],
        ),
    ] {
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&catalog));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());

        let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.features.is_empty());
        assert!(transfer.principal_plane_records.is_empty());
    }
}

#[test]
fn design_field_vocabulary_distinguishes_equal_names_from_distinct_entries() {
    let mut bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Feature",
        "Feature",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let classes = &native.design_objects[0].field_classes;

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, classes[1].name);
    assert_ne!(classes[0].entry, classes[1].entry);
}

#[test]
fn visualization_values_do_not_assert_missing_design_intent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_visualization_values_only()),
            &DecodeOptions::default(),
        )
        .expect("decode visualization-only values");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Attribute
            && loss.message.contains("schema-selected presentation value")
    }));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.category() != cadmpeg_ir::report::LossCategory::DesignIntent));
}

#[test]
fn decode_does_not_promote_field_class_names_to_features() {
    for class in [
        "Groove",
        "GSMHelix",
        "CircPattern_RadialNumber",
        "GSMPlaneAngle",
        "GSMPlaneOffset",
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_design_class(class)),
                &DecodeOptions::default(),
            )
            .expect("decode field-class vocabulary");

        assert!(decoded.ir().model.features.is_empty());
        let native = crate::native::CatiaNative::load(
            decoded
                .ir()
                .native
                .namespace("catia")
                .expect("CATIA native namespace"),
        )
        .expect("load retained field-class vocabulary");
        assert_eq!(
            native.design_objects[0]
                .field_classes
                .iter()
                .map(|class| class.name.as_str())
                .collect::<Vec<_>>(),
            ["CurrentFeature", class]
        );
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
                && loss.message.contains("neutral features")
        }));
    }
}
