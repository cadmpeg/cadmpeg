// SPDX-License-Identifier: Apache-2.0
//! Design-feature transfer tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::{transfer_design_features, DesignFeatureTransfer};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use crate::native::{
    CatiaDesignClass, CatiaDesignObject, CatiaDesignObjectRelation,
    CatiaDesignObjectRelationSource, CatiaNative, CatiaObjectGraph, CatiaObjectOwner,
    CatiaObjectRecord, CatiaObjectRecordReferenceSource,
};
use crate::object_graph::HeadToken;
use crate::object_graph::ObjectPayload;
use crate::object_graph::PayloadField;
use crate::test_support::test_formula::standard_catpart_with_definition_value;
use crate::test_support::test_object_graph::{
    catalog_stream, design_object, entity_backed_object_graph, object_graph_from_records,
    object_graph_record, standard_catpart_with_design_class,
    standard_catpart_with_visualization_values_only, value_block_stream,
};
use crate::CatiaCodec;

mod reference_planes;

fn feature(id: &str, native_ref: &str) -> Feature {
    Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{id}")).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
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
        target_entity_id: u32::try_from(payload_offset).expect("fixture value fits u32"),
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

fn declared_object(
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
        entity: entity_id.map(|id| crate::native::CatiaObjectEntity {
            record: String::new(),
            id,
        }),
        ordinal: 0,
        byte_offset: 0,
        byte_len: 0,
        lead: 0,
        head: Vec::new(),
        inline_body: None,
        owner: owner.map(CatiaObjectOwner::Entity),
        class: match (class_name, class_entry) {
            (None, None) => None,
            (class_name, class_entry) => Some(crate::native::CatiaObjectClass {
                ordinal: 0,
                name: class_name.map(str::to_string),
                entry: class_entry.map(str::to_string),
            }),
        },
        storage: None,
        payload: ObjectPayload {
            size: 1,
            fields: vec![PayloadField::Terminator],
        },
        repeated_reference_schema_selection: None,
        references: Vec::new(),
    }
}

#[test]
fn compact_self_owned_operation_root_remains_an_identity_anchor() {
    let mut object = design_object("synthetic:test:object#operation-object", None);
    object.owner_entity_id = 1;
    object.owner_record = Some("operation-record".to_string());

    let mut record = object_record(
        "operation-record",
        Some("synthetic:test:object#operation-object"),
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
    if let Some(class) = &mut record.class {
        class.ordinal = 7;
    } else {
        record.class = Some(crate::native::CatiaObjectClass {
            ordinal: 7,
            name: None,
            entry: None,
        });
    }

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
    let mut ir = CadIr::empty();

    let transfer = crate::test_support::with_service_context(|ctx| {
        transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert!(ir.model.features.is_empty());
    assert_eq!(transfer.consumed_records().count(), 0);
}

#[test]
fn malformed_compact_root_does_not_promote_an_operation() {
    let mut object = design_object("synthetic:test:object#operation-object", None);
    object.owner_entity_id = 1;
    object.owner_record = Some("operation-record".to_string());

    let mut record = object_record(
        "operation-record",
        Some("synthetic:test:object#operation-object"),
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
    if let Some(class) = &mut record.class {
        class.ordinal = 7;
    } else {
        record.class = Some(crate::native::CatiaObjectClass {
            ordinal: 7,
            name: None,
            entry: None,
        });
    }

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
    let mut ir = CadIr::empty();

    let transfer = crate::test_support::with_service_context(|ctx| {
        transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert!(ir.model.features.is_empty());
    assert_eq!(transfer.consumed_records().count(), 0);
}

#[test]
fn principal_and_reference_features_refuse_collection_limit() {
    let object = design_object("synthetic:test:object#plane", None);
    let record = object_record("plane-record", None, None, None, None, None);
    let principal = super::PrincipalPlaneCandidate {
        object: &object,
        declarations: vec![&record],
        plane: cadmpeg_ir::features::PrincipalPlane::Top,
        declaration_class: "xy-plane",
    };
    let mut ir = CadIr::empty();
    let mut transfer = DesignFeatureTransfer::default();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        super::transfer_principal_plane(ctx, &mut ir, &mut transfer, principal)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_principal_features")
    );
    let principal = super::PrincipalPlaneCandidate {
        object: &object,
        declarations: vec![&record],
        plane: cadmpeg_ir::features::PrincipalPlane::Top,
        declaration_class: "xy-plane",
    };
    crate::test_support::with_service_context(|ctx| {
        super::transfer_principal_plane(ctx, &mut ir, &mut transfer, principal)
    })
    .expect("service profile admits principal plane");
    assert_eq!(ir.model.features.len(), 1);

    let reference = super::ReferencePlaneCandidate {
        object: &object,
        owner_record: &record,
        kind: "GSMPlaneOffset",
    };
    let mut reference_ir = CadIr::empty();
    let mut reference_transfer = DesignFeatureTransfer::default();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        super::transfer_reference_plane(ctx, &mut reference_ir, &mut reference_transfer, &reference)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_reference_features")
    );
    crate::test_support::with_service_context(|ctx| {
        super::transfer_reference_plane(ctx, &mut reference_ir, &mut reference_transfer, &reference)
    })
    .expect("service profile admits reference plane");
    assert_eq!(reference_ir.model.features.len(), 1);
}

#[test]
fn principal_plane_declaration_list_refuses_collection_limit() {
    let mut object = design_object("synthetic:test:object#plane", None);
    object.owner_record = Some("plane-record".to_string());
    object.fields.push("plane-record".to_string());
    let record = object_record("plane-record", None, None, None, None, None);
    let records = HashMap::from([(record.id.as_str(), &record)]);
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        super::principal_plane_candidate(ctx, &object, &records)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_principal_plane_declarations")
    );
    let service = crate::test_support::with_service_context(|ctx| {
        super::principal_plane_candidate(ctx, &object, &records)
    })
    .expect("service profile admits declaration scan");
    assert!(service.is_none());
}

#[test]
fn sketch_feature_rows_refuse_collection_limit() {
    let native = sketch_owner_native();
    let sketch_object = &native.design_objects[0];
    let sketch_record = &native.object_graphs[0].records[0];
    let mut ir = CadIr::empty();
    let mut transfer = DesignFeatureTransfer::default();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        super::transfer_sketch(ctx, &mut ir, &mut transfer, sketch_object, sketch_record)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_design_sketches")
    );
    crate::test_support::with_service_context(|ctx| {
        super::transfer_sketch(ctx, &mut ir, &mut transfer, sketch_object, sketch_record)
    })
    .expect("service profile admits sketch feature");
    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.features.len(), 1);
}

#[test]
fn feature_dependency_lookup_refuses_collection_limit() {
    let object_id = "synthetic:test:object#owner";
    let native = CatiaNative {
        design_objects: vec![design_object(object_id, None)],
        ..CatiaNative::default()
    };
    let transfer = DesignFeatureTransfer::default();
    let mut ir = CadIr::empty();
    ir.model.features.push(feature("owner", object_id));
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        transfer.assign_feature_dependencies(ctx, &mut ir, &native)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_feature_dependency_objects")
    );
    crate::test_support::with_service_context(|ctx| {
        transfer.assign_feature_dependencies(ctx, &mut ir, &native)
    })
    .expect("service profile admits dependency indexes");
    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn assigns_only_prior_payload_feature_dependencies_in_relation_order() {
    let mut source = design_object("synthetic:test:object#source-object", None);
    let mut unresolved = payload_relation("unresolved-object", 6);
    unresolved.target_design_object = None;
    source.relations = vec![
        payload_relation("synthetic:test:object#first-object", 0),
        payload_relation("synthetic:test:object#first-object", 1),
        payload_relation("synthetic:test:object#second-child", 2),
        payload_relation("synthetic:test:object#forward-object", 3),
        storage_relation("synthetic:test:object#storage-object"),
        payload_relation("synthetic:test:object#source-object", 5),
        unresolved,
        payload_relation("synthetic:test:object#broken-object", 7),
        payload_relation("synthetic:test:object#cycle-first", 8),
    ];
    let broken = design_object(
        "synthetic:test:object#broken-object",
        Some("missing-object"),
    );
    let cycle_first = design_object(
        "synthetic:test:object#cycle-first",
        Some("synthetic:test:object#cycle-second"),
    );
    let cycle_second = design_object(
        "synthetic:test:object#cycle-second",
        Some("synthetic:test:object#cycle-first"),
    );
    let native = CatiaNative {
        design_objects: vec![
            design_object("synthetic:test:object#first-object", None),
            design_object("synthetic:test:object#second-object", None),
            design_object(
                "synthetic:test:object#second-child",
                Some("synthetic:test:object#second-object"),
            ),
            design_object("synthetic:test:object#forward-object", None),
            design_object("synthetic:test:object#storage-object", None),
            broken,
            cycle_first,
            cycle_second,
            source,
        ],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();
    for (id, native_ref, ordinal) in [
        ("first-feature", "synthetic:test:object#first-object", 10),
        ("second-feature", "synthetic:test:object#second-object", 15),
        ("source-feature", "synthetic:test:object#source-object", 20),
        (
            "forward-feature",
            "synthetic:test:object#forward-object",
            30,
        ),
        ("storage-feature", "synthetic:test:object#storage-object", 5),
    ] {
        let mut item = feature(id, native_ref);
        item.ordinal = ordinal;
        ir.model.features.push(item);
    }
    let transfer = DesignFeatureTransfer {
        feature_ids: HashMap::from([
            (
                "synthetic:test:object#first-object".to_string(),
                FeatureId::mint("synthetic:test:id#first-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#second-object".to_string(),
                FeatureId::mint("synthetic:test:id#second-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#source-object".to_string(),
                FeatureId::mint("synthetic:test:id#source-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#forward-object".to_string(),
                FeatureId::mint("synthetic:test:id#forward-feature").expect("identity grammar"),
            ),
            (
                "synthetic:test:object#storage-object".to_string(),
                FeatureId::mint("synthetic:test:id#storage-feature").expect("identity grammar"),
            ),
        ]),
        ..DesignFeatureTransfer::default()
    };

    crate::test_support::with_service_context(|ctx| {
        transfer.assign_feature_dependencies(ctx, &mut ir, &native)
    })
    .unwrap();

    let source = ir
        .model
        .features
        .iter()
        .find(|feature| {
            feature.id
                == FeatureId::mint("synthetic:test:id#source-feature").expect("identity grammar")
        })
        .unwrap();
    assert_eq!(
        source.dependencies.as_slice(),
        [
            FeatureId::mint("synthetic:test:id#first-feature").expect("identity grammar"),
            FeatureId::mint("synthetic:test:id#second-feature").expect("identity grammar")
        ]
    );
    assert!(ir
        .model
        .features
        .iter()
        .filter(|feature| feature.id
            != FeatureId::mint("synthetic:test:id#source-feature").expect("identity grammar"))
        .all(|feature| feature.dependencies.is_empty()));
}

fn sketch_owner_native() -> CatiaNative {
    CatiaNative {
        design_objects: vec![declared_object(
            "synthetic:test:object#sketch-object",
            None,
            1,
            "sketch-record",
            "Sketch",
            "sketch-entry",
        )],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![object_record(
                "sketch-record",
                None,
                Some(1),
                None,
                Some("Sketch"),
                Some("sketch-entry"),
            )],
        }],
        ..CatiaNative::default()
    }
}

#[test]
fn design_sketch_entity_limit_refuses_before_sketch_push() {
    let native = sketch_owner_native();
    let mut ir = CadIr::empty();
    let error = crate::test_support::with_entity_limit(0, |ctx| {
        transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    });
    let Err(error) = error else {
        panic!("one sketch exceeds zero entities");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && limit.operation == "admit CATIA design sketch")
    );
    assert!(ir.model.sketches.is_empty());
}

#[test]
fn design_sketch_feature_limit_refuses_before_feature_push() {
    let native = sketch_owner_native();
    let mut ir = CadIr::empty();
    let error = crate::test_support::with_entity_limit(1, |ctx| {
        transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    });
    let Err(error) = error else {
        panic!("sketch and feature exceed one entity");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && limit.operation == "admit CATIA design feature")
    );
    assert_eq!(ir.model.sketches.len(), 1);
    assert!(ir.model.features.is_empty());
}

#[test]
fn operation_schema_owner_declarations_do_not_establish_features() {
    let cases = [
        (
            "synthetic:test:object#prism-one",
            "Prism_ThickThin1",
            "prism-one-record",
            1_u32,
        ),
        (
            "synthetic:test:object#prism-two",
            "Prism_ThickThin2",
            "prism-two-record",
            2_u32,
        ),
        (
            "synthetic:test:object#end-limit",
            "Prism_EndLimit_Length",
            "end-limit-record",
            3_u32,
        ),
        (
            "synthetic:test:object#revolution",
            "Revol_ThickThin1",
            "revolution-record",
            4_u32,
        ),
        (
            "synthetic:test:object#sweep",
            "Sweep_ThickThin1",
            "sweep-record",
            5_u32,
        ),
        (
            "synthetic:test:object#fillet",
            "EdgeFillet",
            "fillet-record",
            6_u32,
        ),
        (
            "synthetic:test:object#circular-pattern",
            "CircPattern_RadialNumber",
            "circular-pattern-record",
            7_u32,
        ),
    ];
    let objects = cases
        .iter()
        .enumerate()
        .map(|(ordinal, (id, kind, record, entity_id))| {
            let mut object =
                declared_object(id, None, *entity_id, record, kind, &format!("{kind}-entry"));
            object.first_field_byte_offset = (cadmpeg_core::decode::u64_from_index(ordinal)) * 10;
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
    let mut ir = CadIr::empty();

    crate::test_support::with_service_context(|ctx| {
        transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert!(ir.model.features.is_empty());
}

#[test]
fn does_not_promote_an_unadmitted_helper_owner_class() {
    let object = declared_object(
        "synthetic:test:object#helper-object",
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
    let mut ir = CadIr::empty();

    let transfer = crate::test_support::with_service_context(|ctx| {
        transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert!(ir.model.features.is_empty());
    assert_eq!(transfer.consumed_records().count(), 0);
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
        .definition_value_mut()
        .expect("definition value")
        .definition
        .value = "CircPattern".to_string();
    if let Some(class) = &mut native.object_graphs[0].records[0].class {
        class.name = Some("Element1".to_string());
    } else {
        native.object_graphs[0].records[0].class = Some(crate::native::CatiaObjectClass {
            ordinal: 0,
            name: Some("Element1".to_string()),
            entry: None,
        });
    }

    let mut ir = CadIr::empty();
    let transfer = crate::test_support::with_service_context(|ctx| {
        crate::design_feature::transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();
    assert!(ir.model.features.is_empty());
    assert!(transfer.consumed_records().next().is_none());
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
        native.object_graphs[0].records[1].class_name(),
        Some("PRTSketch")
    );

    let mut ir = CadIr::empty();
    let transfer = crate::test_support::with_service_context(|ctx| {
        crate::design_feature::transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().next().is_none());
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
    if let Some(class) = &mut owner_record_mut.class {
        class.name = Some("Sketch".to_string());
        class.entry = Some(owner_class_entry.clone());
    } else {
        owner_record_mut.class = Some(crate::native::CatiaObjectClass {
            ordinal: 0,
            name: Some("Sketch".to_string()),
            entry: Some(owner_class_entry.clone()),
        });
    }

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

    let mut ir = CadIr::empty();
    let transfer = crate::test_support::with_service_context(|ctx| {
        crate::design_feature::transfer_design_features(
            ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .unwrap();

    assert_eq!(ir.model.sketches.len(), 1);
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(_))
            }
        )
    ));
    assert!(ir.model.sketches[0].profiles.is_empty());
    assert_eq!(
        ir.model.sketches[0].placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved {}
    );
    assert_eq!(
        transfer.sketch_owner_records,
        std::collections::HashSet::from([owner_record_id])
    );
}

mod feature_identity;
mod parentage;
