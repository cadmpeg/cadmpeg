// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::{StepCodec, StepSchema};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn unknown_occurrence_pose_is_omitted_and_known_sibling_round_trips() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for (key, bodies) in [
        ("root", vec![]),
        ("part", vec![ir.model.bodies[0].id.clone()]),
    ] {
        ir.model.product_definitions.push(ProductDefinition {
            id: format!("test:model:product#{key}").try_into().unwrap(),
            kind: ProductDefinitionKind::Part,
            source_name: Some(key.to_owned()),
            label: None,
            description: None,
            part_number: None,
            bom_properties: Default::default(),
            bodies,
            native_ref: None,
        });
    }
    let root: cadmpeg_ir::ids::OccurrenceId = "test:model:occurrence#root".try_into().unwrap();
    for (ordinal, (key, transform)) in [
        ("root", Some(Transform::identity())),
        ("unknown", None),
        ("known", Some(Transform::identity())),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{key}").try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: ir.model.product_definitions[usize::from(key != "root")]
                    .id
                    .clone(),
            },
            parent: if key == "root" {
                OccurrenceParent::Root {}
            } else {
                OccurrenceParent::Occurrence {
                    occurrence: root.clone(),
                }
            },
            ordinal: u32::try_from(ordinal.saturating_sub(1)).unwrap(),
            transform,
            linked_prototype: None,
            name: Some(key.to_owned()),
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let plan = StepCodec::default().plan(
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit(StepSchema::Ap242Edition3.descriptor().id.as_str()),
    );
    let plan = plan.unwrap();
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::StepLossCode::OccurrencePlacementUnknown.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("test:model:occurrence#unknown"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .occurrences
        .iter()
        .any(|occurrence| occurrence.name.as_deref() == Some("unknown")));
    let known = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("known"))
        .expect("known sibling");
    assert_eq!(known.transform, Some(Transform::identity()));
    assert_eq!(ir.model.occurrences[1].transform, None);
}

#[test]
fn unknown_ancestor_omits_descendants_and_keeps_known_local_frames() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for (key, bodies) in [
        ("root", vec![]),
        ("part", vec![ir.model.bodies[0].id.clone()]),
    ] {
        ir.model.product_definitions.push(ProductDefinition {
            id: format!("test:model:product#{key}").try_into().unwrap(),
            kind: ProductDefinitionKind::Part,
            source_name: Some(key.to_owned()),
            label: None,
            description: None,
            part_number: None,
            bom_properties: Default::default(),
            bodies,
            native_ref: None,
        });
    }
    let root: cadmpeg_ir::ids::OccurrenceId = "test:model:occurrence#root".try_into().unwrap();
    for (ordinal, (key, transform)) in [
        ("root", Some(Transform::identity())),
        ("unknown", None),
        ("known", Some(Transform::identity())),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.occurrences.push(Occurrence {
            id: format!("test:model:occurrence#{key}").try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: ir.model.product_definitions[usize::from(key != "root")]
                    .id
                    .clone(),
            },
            parent: if key == "root" {
                OccurrenceParent::Root {}
            } else {
                OccurrenceParent::Occurrence {
                    occurrence: root.clone(),
                }
            },
            ordinal: u32::try_from(ordinal.saturating_sub(1)).unwrap(),
            transform,
            linked_prototype: None,
            name: Some(key.to_owned()),
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    let mut leaf_definition = ir.model.product_definitions[1].clone();
    leaf_definition.id = "test:model:product#leaf".try_into().unwrap();
    leaf_definition.source_name = Some("leaf".into());
    ir.model.product_definitions.push(leaf_definition);
    let unknown = ir.model.occurrences[1].clone();
    let known = ir.model.occurrences[2].clone();
    let translated = |x| {
        Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap()
    };
    let mut child = known.clone();
    child.id = "test:model:occurrence#child".try_into().unwrap();
    child.name = Some("child".into());
    child.parent = OccurrenceParent::Occurrence {
        occurrence: unknown.id.clone(),
    };
    child.ordinal = 0;
    child.transform = Some(translated(2.0));
    let mut grandchild = child.clone();
    grandchild.id = "test:model:occurrence#grandchild".try_into().unwrap();
    grandchild.name = Some("grandchild".into());
    grandchild.parent = OccurrenceParent::Occurrence {
        occurrence: child.id.clone(),
    };
    let mut known_child = child.clone();
    known_child.id = "test:model:occurrence#known_child".try_into().unwrap();
    known_child.name = Some("known_child".into());
    known_child.prototype = PrototypeReference::Local {
        definition: ir.model.product_definitions[2].id.clone(),
    };
    known_child.parent = OccurrenceParent::Occurrence {
        occurrence: known.id.clone(),
    };
    known_child.transform = Some(translated(3.0));
    ir.model.occurrences[2].transform = Some(translated(10.0));
    ir.model
        .occurrences
        .extend([child, grandchild, known_child]);
    let plan = StepCodec::default().plan(
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit(StepSchema::Ap242Edition3.descriptor().id.as_str()),
    );
    let plan = plan.unwrap();
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::StepLossCode::OccurrencePlacementUnknown.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 3);
    for key in ["unknown", "child", "grandchild"] {
        let id = format!("test:model:occurrence#{key}");
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.message.contains(&id))
                .count(),
            1
        );
    }

    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .occurrences
        .iter()
        .any(|occurrence| occurrence.name.as_deref() == Some("unknown")));
    let known = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("known"))
        .expect("known sibling");
    assert_eq!(known.transform, Some(translated(10.0)));
    let known_child = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("known_child"))
        .expect("known nested sibling");
    assert_eq!(known_child.transform, Some(translated(3.0)));
    for key in ["child", "grandchild"] {
        assert!(!decoded
            .ir()
            .model
            .occurrences
            .iter()
            .any(|occurrence| occurrence.name.as_deref() == Some(key)));
    }
    assert_eq!(ir.model.occurrences[1].transform, None);
}
