// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::{transform::Transform, CadIr};

#[test]
fn absent_occurrence_placement_omits_only_that_reference_with_a_named_loss() {
    let mut ir = CadIr::empty();
    let definition = "test:model:product#part".try_into().unwrap();
    ir.model.product_definitions.push(ProductDefinition {
        id: definition,
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies: vec![],
        sketches: Vec::new(),
        native_ref: None,
    });
    for (ordinal, (key, transform)) in [("unknown", None), ("known", Some(Transform::identity()))]
        .into_iter()
        .enumerate()
    {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{key}").try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: ir.model.product_definitions[0].id.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal: u32::try_from(ordinal).unwrap(),
            transform,
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let exported = super::references(
        &ir,
        &super::super::materials::MaterialPlan::new(&ir).expect("material plan"),
    )
    .unwrap();
    let references = exported.records;
    let losses = exported.losses;
    assert_eq!(references.len(), 1);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("test:model:occurrence#unknown"));
    assert!(!losses[0].message.contains("test:model:occurrence#known"));
    let known = crate::writer::neutral_object_uuid("test:model:occurrence#known");
    let unknown = crate::writer::neutral_object_uuid("test:model:occurrence#unknown");
    assert!(references[0]
        .windows(known.len())
        .any(|bytes| bytes == known));
    assert!(!references[0]
        .windows(unknown.len())
        .any(|bytes| bytes == unknown));
    assert_eq!(ir.model.occurrences[0].transform, None);
    assert_eq!(
        ir.model.occurrences[1].transform,
        Some(Transform::identity())
    );
}

#[test]
fn unknown_ancestor_cascades_without_changing_known_local_reference_frames() {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(ProductDefinition {
        id: "test:model:product#part".try_into().unwrap(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies: vec![],
        sketches: Vec::new(),
        native_ref: None,
    });
    let mut leaf = ir.model.product_definitions[0].clone();
    leaf.id = "test:model:product#leaf".try_into().unwrap();
    ir.model.product_definitions.push(leaf);
    let translated = |x| {
        Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap()
    };
    for (key, parent, transform, ordinal) in [
        ("unknown", None, None, 0),
        ("child", Some("unknown"), Some(translated(2.0)), 0),
        ("grandchild", Some("child"), Some(translated(5.0)), 0),
        ("known", None, Some(translated(10.0)), 1),
        ("known_child", Some("known"), Some(translated(3.0)), 0),
    ] {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{key}").try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: ir.model.product_definitions[usize::from(parent.is_some())]
                    .id
                    .clone(),
            },
            parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
                OccurrenceParent::Occurrence {
                    occurrence: format!("test:model:occurrence#{parent}")
                        .try_into()
                        .unwrap(),
                }
            }),
            ordinal,
            transform,
            linked_prototype: None,
            name: Some(key.into()),
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let graph = cadmpeg_ir::products::AssemblyGraph::new(&ir.model.occurrences).unwrap();
    let children = super::children(&ir, &graph, &ir.model.product_definitions[0]);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, ir.model.occurrences[4].id);
    let exported = super::references(
        &ir,
        &super::super::materials::MaterialPlan::new(&ir).expect("material plan"),
    )
    .unwrap();
    let references = exported.records;
    let losses = exported.losses;
    assert_eq!(references.len(), 2);
    assert_eq!(losses.len(), 3);
    let definitions = super::definitions(
        &ir,
        &std::collections::BTreeSet::default(),
        crate::RhinoArchiveVersion::V5,
    )
    .unwrap();
    for key in ["unknown", "child", "grandchild"] {
        let id = format!("test:model:occurrence#{key}");
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.message.contains(&id))
                .count(),
            1
        );
        let uuid = crate::writer::neutral_object_uuid(&id);
        assert!(!definitions
            .iter()
            .any(|record| record.windows(uuid.len()).any(|bytes| bytes == uuid)));
        assert!(!references
            .iter()
            .any(|record| record.windows(uuid.len()).any(|bytes| bytes == uuid)));
    }
    for (key, x) in [("known", 10.0), ("known_child", 3.0)] {
        let uuid = crate::writer::neutral_object_uuid(&format!("test:model:occurrence#{key}"));
        let record = references
            .iter()
            .find(|record| record.windows(uuid.len()).any(|bytes| bytes == uuid))
            .unwrap();
        let expected = translated(x)
            .rows()
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        assert!(record
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }
    assert_eq!(ir.model.occurrences[1].transform, Some(translated(2.0)));
}

#[test]
fn overflowing_occurrence_placement_names_only_the_unwritable_record() {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(ProductDefinition {
        id: "test:model:product#part".try_into().unwrap(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies: vec![],
        sketches: Vec::new(),
        native_ref: None,
    });
    let huge = Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    for (ordinal, name) in ["overflow", "valid"].into_iter().enumerate() {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{name}").try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: ir.model.product_definitions[0].id.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal: u32::try_from(ordinal).unwrap(),
            transform: Some(if name == "overflow" {
                huge
            } else {
                Transform::identity()
            }),
            linked_prototype: (name == "overflow").then_some(huge),
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let plan = super::super::materials::MaterialPlan::new(&ir).unwrap();
    let output = super::references(&ir, &plan).unwrap();
    assert_eq!(output.records.len(), 1);
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:occurrence#overflow")));
    assert!(output.written.contains("test:model:occurrence#valid"));
    let mut bytes = Vec::new();
    let report = crate::RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:occurrence#overflow")));
    let valid = crate::writer::neutral_object_uuid("test:model:occurrence#valid");
    assert!(bytes.windows(valid.len()).any(|window| window == valid));
}

#[test]
fn invalid_occurrence_graph_names_removed_records_and_keeps_sibling() {
    let mut ir = CadIr::empty();
    let definition: cadmpeg_ir::ids::ProductDefinitionId =
        "test:model:product#part".try_into().unwrap();
    ir.model.product_definitions.push(ProductDefinition {
        id: definition.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies: vec![],
        sketches: Vec::new(),
        native_ref: None,
    });
    for (name, parent) in [
        ("valid", None),
        ("missing", Some("absent")),
        ("cycle-a", Some("cycle-b")),
        ("cycle-b", Some("cycle-a")),
        ("duplicate", None),
        ("duplicate", None),
    ] {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{name}").try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: definition.clone(),
            },
            parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
                OccurrenceParent::Occurrence {
                    occurrence: format!("test:model:occurrence#{parent}")
                        .try_into()
                        .unwrap(),
                }
            }),
            ordinal: 0,
            transform: Some(Transform::identity()),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let plan = super::super::materials::MaterialPlan::new(&ir).unwrap();
    let output = super::references(&ir, &plan).unwrap();
    assert!(output.written.contains("test:model:occurrence#valid"));
    for name in ["missing", "cycle-a", "cycle-b", "duplicate"] {
        let id = format!("test:model:occurrence#{name}");
        assert!(
            output.losses.iter().any(|loss| loss.message.contains(&id)),
            "{id}"
        );
        assert!(!output.written.contains(&id));
    }
}

#[test]
fn unrepresentable_product_metadata_names_definition_and_keeps_sibling() {
    let mut ir = CadIr::empty();
    let native_id = "rhino:product:definition#bad";
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "product_definitions".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            native_id.try_into().unwrap(),
            serde_json::from_value(
                serde_json::json!({"unit_system": 4_294_967_296_u64,"archive_index": 4_294_967_296_i64}),
            )
            .unwrap(),
        )
        .unwrap()],
    );
    for (id, native_ref) in [("bad", Some(native_id)), ("good", None)] {
        ir.model.product_definitions.push(ProductDefinition {
            id: format!("test:model:product#{id}").try_into().unwrap(),
            kind: ProductDefinitionKind::Part,
            source_name: Some(id.into()),
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![],
            sketches: Vec::new(),
            native_ref: native_ref.map(str::to_owned),
        });
    }
    let mut bytes = Vec::new();
    let report = crate::RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::WriterProductMetadataReduced.kind()
        && loss.message.contains("test:model:product#bad")
        && loss.message.contains(native_id)
        && loss.message.contains("unit_system")
        && loss.message.contains("archive_index")));
    let bad = crate::writer::neutral_object_uuid("test:model:product#bad");
    let good = crate::writer::neutral_object_uuid("test:model:product#good");
    assert!(bytes.windows(bad.len()).any(|window| window == bad));
    assert!(bytes.windows(good.len()).any(|window| window == good));
}
