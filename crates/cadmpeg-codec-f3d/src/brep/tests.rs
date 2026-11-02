// SPDX-License-Identifier: Apache-2.0

use super::{persistent_design_links, persistent_subentity_tags, Brep};
use crate::records::recipes::CreationTimestamp;
use crate::records::sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink};
use cadmpeg_asm::brep::annotations::AnnotationRecord;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::{FaceId, RegionId};
use cadmpeg_ir::topology::{Body, BodyKind, Region};
use std::collections::{HashMap, HashSet};

fn with_limits<T>(
    max_retained: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn with_context<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    with_limits(u64::MAX, run)
}

fn asm_with_face(face: &str, index: u32) -> AsmBrep {
    use cadmpeg_asm::brep::records::{identity::NativeRecordNamespace, FaceNativeKey};
    let key = FaceNativeKey {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        record_index: index,
        asm_face_key: Some(7),
    };
    let mut model_face = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .faces
        .remove(0);
    model_face.id = face.try_into().expect("face id");
    model_face.native_ref = Some(key.id());
    AsmBrep {
        faces: vec![model_face],
        face_native_keys: vec![key],
        ..AsmBrep::default()
    }
}

fn asm_with_body(body: &str, index: u32) -> AsmBrep {
    use cadmpeg_asm::brep::records::{identity::NativeRecordNamespace, BodyNativeKey};
    let key = BodyNativeKey {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        record_index: index,
        body_ordinal: 0,
        source_brep: None,
        asm_body_key: Some(11),
    };
    let mut model_body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .bodies
        .remove(0);
    model_body.id = body.try_into().expect("body id");
    model_body.native_ref = Some(key.id());
    AsmBrep {
        bodies: vec![model_body],
        body_native_keys: vec![key],
        ..AsmBrep::default()
    }
}

fn generic_tag_attribute(
    target: AttributeTarget,
    versions: (i64, i64),
    group_count: i64,
    groups: Vec<AttributeValue>,
) -> SourceAttribute {
    SourceAttribute {
        native_ref: None,
        id: "f3d:brep:attribute#1".try_into().expect("valid identity"),
        target,
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: ([
            AttributeValue::String("generic_tag_attrib_def".into()),
            AttributeValue::Integer(versions.0),
            AttributeValue::Integer(versions.1),
            AttributeValue::Integer(-1),
            AttributeValue::String("generic_tag_attrib_def ".into()),
            AttributeValue::Integer(group_count),
        ]
        .into_iter()
        .chain(groups)
        .collect::<Vec<_>>())
        .into_iter()
        .map(Some)
        .collect(),
    }
}

#[test]
fn generic_tag_payload_accepts_both_equal_envelope_versions() {
    for (version, groups) in [
        (
            2,
            vec![
                AttributeValue::Integer(7),
                AttributeValue::String("97".into()),
                AttributeValue::Integer(0),
                AttributeValue::Integer(1),
                AttributeValue::Integer(302),
            ],
        ),
        (
            3,
            vec![
                AttributeValue::Integer(7),
                AttributeValue::String("97".into()),
                AttributeValue::Integer(0),
                AttributeValue::Integer(1),
                AttributeValue::Integer(302),
                AttributeValue::Integer(0),
            ],
        ),
    ] {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
            (version, version),
            1,
            groups,
        );
        let asm = asm_with_face("f3d:test:face#1", 7);
        assert_eq!(persistent_subentity_tags(&asm, &attribute).unwrap().len(), 1);
        assert_eq!(
            persistent_subentity_tags(&asm, &attribute).unwrap()[0].design_references,
            [302]
        );
    }
}

#[test]
fn generic_tag_payload_rejects_mixed_or_unsupported_envelope_versions() {
    let groups = vec![
        AttributeValue::Integer(7),
        AttributeValue::String("97".into()),
        AttributeValue::Integer(0),
        AttributeValue::Integer(0),
        AttributeValue::Integer(0),
    ];
    for versions in [(2, 3), (1, 1), (4, 4)] {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
            versions,
            1,
            groups.clone(),
        );
        let asm = asm_with_face("f3d:test:face#1", 7);
        assert!(persistent_subentity_tags(&asm, &attribute).unwrap().is_empty());
    }
}

#[test]
fn generic_tag_payload_binds_modern_body_design_links() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
        (2, 2),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
        ],
    );
    let asm = asm_with_body("f3d:test:body#1", 9);
    let links = persistent_design_links(&asm, &attribute).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].design_id.as_str(), "301");
    assert_eq!(links[0].design_reference, 1);
}

#[test]
fn generic_tag_payload_binds_legacy_body_design_links() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
        (3, 3),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
            AttributeValue::Integer(0),
        ],
    );
    let asm = asm_with_body("f3d:test:body#1", 9);
    assert_eq!(persistent_design_links(&asm, &attribute).unwrap().len(), 1);
}

#[test]
fn brep_qualification_rewrites_owned_ids_and_cross_references() {
    let key = || BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        record_index: 1,
        body_ordinal: 0,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(7),
    };
    let body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let region = RegionId::mint("f3d:brep:entity#2").expect("identity grammar");
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![Body {
                native_ref: Some(key().id()),
                id: body.clone(),
                kind: BodyKind::default(),
                regions: vec![region.clone()],
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            regions: vec![Region {
                native_ref: None,
                id: region,
                body: body.clone(),
                shells: Vec::new(),
            }],

            body_native_keys: vec![key()],
            annotation_records: vec![AnnotationRecord {
                id: "f3d:brep:entity#1".into(),
                stream: "asset/BREP.source.smbh".into(),
                offset: 10,
                tag: cadmpeg_asm::brep::annotations::AnnotationTag::Record("body".into()),
                derived_fields: Vec::new(),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source"))
        .expect("qualify BREP");

    let qualified = BodyId::mint("f3d:brep/source/brep:entity#1").expect("identity grammar");
    assert_eq!(brep.asm.bodies[0].id, qualified);
    assert_eq!(brep.asm.regions[0].body, qualified);
    assert_eq!(
        brep.asm.bodies[0].native_ref.as_deref(),
        Some(brep.asm.body_native_keys[0].id().as_str())
    );
    assert_eq!(
        brep.asm
            .body_native_keys
            .iter()
            .find(|record| {
                brep.asm.bodies[0].native_ref.as_deref() == Some(record.id().as_str())
            })
            .and_then(|record| record.asm_body_key.as_ref()),
        Some(&7)
    );
    assert_eq!(brep.asm.annotation_records[0].id, qualified.as_str());
    assert_eq!(
        brep.asm.body_native_keys[0].source_brep.as_deref(),
        Some("BREP.source.smbh")
    );
}

#[test]
fn body_key_retention_keeps_only_the_selected_connected_graph() {
    let native_key = |index, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let keys = vec![native_key(1, 10), native_key(3, 20)];
    let body = |index: u32, region| Body {
        native_ref: keys
            .iter()
            .find(|key| key.record_index == index)
            .map(|key| key.id()),
        id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: vec![
            RegionId::mint(format!("f3d:brep:entity#{region}")).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![
                Body {
                    name: Some("f3d:brep:entity#3".into()),
                    ..body(1, 2)
                },
                body(3, 4),
            ],
            regions: vec![
                Region {
                    native_ref: None,
                    id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
                    body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
                    shells: Vec::new(),
                },
                Region {
                    native_ref: None,
                    id: RegionId::mint("f3d:brep:entity#4").expect("identity grammar"),
                    body: BodyId::mint("f3d:brep:entity#3").expect("identity grammar"),
                    shells: Vec::new(),
                },
            ],

            body_native_keys: keys,
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([20])))
        .expect("retain body graph");

    assert_eq!(brep.asm.bodies.len(), 1);
    assert_eq!(brep.asm.bodies[0].id.as_str(), "f3d:brep:entity#3");
    assert_eq!(brep.asm.regions.len(), 1);
    assert_eq!(brep.asm.regions[0].id.as_str(), "f3d:brep:entity#4");
    assert_eq!(brep.asm.body_native_keys.len(), 1);
    assert_eq!(
        brep.asm
            .body_native_keys
            .iter()
            .filter(|record| record.asm_body_key.is_some())
            .count(),
        1
    );
}

#[test]
fn body_key_retention_preserves_derived_links_for_reachable_targets() {
    let native_key = |index, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let keys = vec![native_key(1, 10), native_key(3, 20)];
    let body = |index: u32| Body {
        native_ref: keys
            .iter()
            .find(|key| key.record_index == index)
            .map(|key| key.id()),
        id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let target = |index: u32| {
        cadmpeg_asm::brep::records::identity::NativeRowId::from(
            keys
                .iter()
                .find(|key| key.record_index == index)
                .map(|key| key.id())
                .expect("key row"),
        )
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![body(1), body(3)],

            body_native_keys: keys.clone(),
            ..AsmBrep::default()
        },
        sketch_curve_links: vec![
            SketchCurveLink {
                id: "link-retained".into(),
                target: target(1),
                target_coedge: None,
                sketch_curve_id: 1,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            },
            SketchCurveLink {
                id: "link-dropped".into(),
                target: target(3),
                target_coedge: None,
                sketch_curve_id: 3,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            },
        ],
        persistent_design_links: vec![
            PersistentDesignLink {
                id: "design-retained".into(),
                target: target(1),
                target_coedge: None,
                design_id: "301".to_owned().try_into().unwrap(),
                design_reference: 1,
                ordinal: 0,
            },
            PersistentDesignLink {
                id: "design-dropped".into(),
                target: target(3),
                target_coedge: None,
                design_id: "303".to_owned().try_into().unwrap(),
                design_reference: 3,
                ordinal: 0,
            },
        ],
        persistent_subentity_tags: vec![
            PersistentSubentityTag {
                id: "tag-retained".into(),
                target: target(1),
                target_coedge: None,
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![1],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "tag-dropped".into(),
                target: target(3),
                target_coedge: None,
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![3],
                ordinal: 0,
            },
        ],
        creation_timestamps: vec![
            CreationTimestamp {
                id: "time-retained".into(),
                target: target(1),
                target_coedge: None,
                record_index: 1,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
            },
            CreationTimestamp {
                id: "time-dropped".into(),
                target: target(3),
                target_coedge: None,
                record_index: 3,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
            },
        ],
        sketch_region_bodies_omitted: 0,
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
        .expect("retain body graph");

    assert_eq!(brep.sketch_curve_links.len(), 1);
    assert_eq!(brep.persistent_design_links.len(), 1);
    assert_eq!(brep.persistent_subentity_tags.len(), 1);
    assert_eq!(brep.creation_timestamps.len(), 1);
    assert_eq!(brep.persistent_subentity_tags[0].id, "tag-retained");
}

#[test]
fn body_key_retention_preserves_selectorless_neutral_roots() {
    let native_body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let projected_body = BodyId::mint("f3d:brep:saved-edge-body#5").expect("identity grammar");
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![
                Body {
                    native_ref: None,
                    id: native_body.clone(),
                    kind: BodyKind::Solid,
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                },
                Body {
                    native_ref: None,
                    id: projected_body.clone(),
                    kind: BodyKind::Wire,
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                },
            ],

            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                record_index: 1,
                body_ordinal: 0,
                source_brep: Some("BREP.source.smbh".into()),
                asm_body_key: Some(10),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
        .expect("retain body graph");

    assert_eq!(brep.asm.bodies.len(), 2);
    assert!(brep.asm.bodies.iter().any(|body| body.id == projected_body));
}

#[test]
fn body_selectors_use_ordinals_only_for_an_all_null_key_lane() {
    let native_key = |ordinal, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: key,
    };
    let body = |ordinal: u32| Body {
        native_ref: Some(
            cadmpeg_asm::brep::records::identity::NativeRowId::row_id(
                &cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                "body-native-key",
                ordinal,
            )
            .to_string(),
        ),
        id: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![body(0), body(1)],
            body_native_keys: vec![native_key(0, None), native_key(1, None)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    assert_eq!(brep.body_selectors().len(), 2);
    assert_eq!(
        brep.body_selectors()[&BodyId::mint("f3d:brep:entity#1").expect("identity grammar")],
        1
    );

    brep.asm.body_native_keys[1].asm_body_key = Some(7);
    assert_eq!(
        brep.body_selectors(),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            7
        )])
    );
}

#[test]
fn design_body_selectors_prefer_exact_keys_then_fall_back_to_ordinals() {
    let native_key = |ordinal, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: Some(key),
    };
    let body = |ordinal: u32| Body {
        native_ref: Some(
            cadmpeg_asm::brep::records::identity::NativeRowId::row_id(
                &cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                "body-native-key",
                ordinal,
            )
            .to_string(),
        ),
        id: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![body(0), body(1)],
            body_native_keys: vec![native_key(0, 1), native_key(1, 0)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    assert_eq!(
        brep.body_selectors_for(&HashSet::from([0])).unwrap(),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            0
        )])
    );

    brep.asm.body_native_keys = vec![native_key(0, 436)];
    assert_eq!(
        brep.body_selectors_for(&HashSet::from([0])).unwrap(),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#0").expect("identity grammar"),
            0
        )])
    );
}

#[test]
fn source_curve_keys_retain_distinct_qualifiers_in_every_payload_form() {
    for qualifier in [0u64, 165, 26302, u64::MAX] {
        let mut forms = vec![vec![
            AttributeValue::Integer(1),
            AttributeValue::Integer(1),
            AttributeValue::Integer(3),
            AttributeValue::String(format!("149 {qualifier} 1 0 2 1")),
        ]];
        if let Ok(qualifier) = i64::try_from(qualifier) {
            for form in [0, 2] {
                let mut values = vec![
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(form),
                    AttributeValue::Integer(149),
                    AttributeValue::Integer(qualifier),
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(2),
                    AttributeValue::Integer(1),
                ];
                if form == 2 {
                    values.push(AttributeValue::Integer(0));
                }
                forms.push(values);
            }
        }
        for values in forms {
            let payload =
                super::sketch_link_payload(&values.into_iter().map(Some).collect::<Vec<_>>())
                    .expect("source-curve key");
            assert_eq!((payload.sketch_curve_id, payload.ref_b), (149, qualifier));
        }
    }
}

/// Topology of one body in a sketch-region fixture.
#[derive(Clone, Copy)]
struct RegionBody {
    edges: u32,
    linked_edges: u32,
    coedge_links: bool,
    face_attribute: bool,
    wire_edge: bool,
}

const SKETCH_REGION: RegionBody = RegionBody {
    edges: 4,
    linked_edges: 4,
    coedge_links: false,
    face_attribute: false,
    wire_edge: false,
};

const SOLID: RegionBody = RegionBody {
    edges: 4,
    linked_edges: 0,
    coedge_links: true,
    face_attribute: true,
    wire_edge: false,
};

fn sketch_region_brep(bodies: &[RegionBody], key: Option<u64>) -> Brep {
    use cadmpeg_asm::brep::records::identity::{NativeRecordNamespace, NativeRowId};
    use cadmpeg_asm::brep::records::EdgeOwnership;

    let mut asm = AsmBrep::default();
    let mut links = Vec::new();
    for (ordinal, spec) in (0_u32..).zip(bodies) {
        let entity = |slot: u32, index: u32| {
            format!(
                "f3d:brep:entity#{}",
                1000 * (ordinal + 1) + 100 * slot + index
            )
        };
        let namespace = NativeRecordNamespace::new(crate::ids::ID_FORMAT);
        let body_key = BodyNativeKey {
            source_namespace: namespace.clone(),
            record_index: ordinal + 1,
            body_ordinal: ordinal,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: key,
        };
        let body = entity(0, 1);
        let region = entity(1, 1);
        let shell = entity(2, 1);
        let face = entity(3, 1);
        let ring = entity(4, 1);
        asm.bodies.push(
            serde_json::from_value(serde_json::json!({
                "id": body,
                "regions": [region],
                "native_ref": body_key.id(),
            }))
            .unwrap(),
        );
        asm.regions.push(Region {
            native_ref: None,
            id: RegionId::mint(region.clone()).unwrap(),
            body: BodyId::mint(body.clone()).unwrap(),
            shells: vec![cadmpeg_ir::ids::ShellId::mint(shell.clone()).unwrap()],
        });
        asm.shells.push(
            cadmpeg_ir::topology::Shell::new(
                cadmpeg_ir::ids::ShellId::mint(shell.clone()).unwrap(),
                RegionId::mint(region).unwrap(),
                vec![FaceId::mint(face.clone()).unwrap()],
                if spec.wire_edge {
                    vec![cadmpeg_ir::ids::EdgeId::mint(entity(5, 90)).unwrap()]
                } else {
                    Vec::new()
                },
                Vec::new(),
            )
            .unwrap(),
        );
        asm.faces.push(
            serde_json::from_value(serde_json::json!({
                "id": face, "shell": shell, "surface": entity(6, 1), "sense": "forward",
                "loops": {"classification": "unspecified", "loops": if spec.edges == 0 { vec![] } else { vec![ring.clone()] }},
            }))
            .unwrap(),
        );
        let coedges = (0..spec.edges)
            .map(|index| entity(7, index))
            .collect::<Vec<_>>();
        if spec.edges != 0 {
            asm.loops.push(
                serde_json::from_value(serde_json::json!({
                    "id": ring, "face": face, "boundary": {"kind": "ring", "coedges": coedges},
                }))
                .unwrap(),
            );
        }
        for index in 0..spec.edges {
            let edge = entity(5, index);
            asm.coedges.push(
                serde_json::from_value(serde_json::json!({
                    "id": coedges[usize::try_from(index).unwrap()], "owner_loop": ring, "edge": edge,
                    "radial_next": coedges[usize::try_from(index).unwrap()], "sense": "forward",
                }))
                .unwrap(),
            );
            // The neutral edge resolves through its ownership row, which the
            // link names; a coedge link names the owning edge's row with the
            // coedge source index.
            let ownership = EdgeOwnership {
                source_namespace: namespace.clone(),
                record_index: (ordinal + 1) * 100 + index,
                owner_coedge_index: None,
            };
            asm.edge_ownerships.push(ownership);
            let row = NativeRowId::row_id(
                &namespace,
                "edge-ownership",
                (ordinal + 1) * 100 + index,
            );
            asm.edges.push(cadmpeg_ir::topology::Edge {
                native_ref: Some(row.as_str().to_owned()),
                id: cadmpeg_ir::ids::EdgeId::mint(edge).unwrap(),
                carrier: cadmpeg_ir::topology::EdgeCarrier::Free,
                start: cadmpeg_ir::ids::VertexId::mint(entity(9, 2 * index)).unwrap(),
                end: cadmpeg_ir::ids::VertexId::mint(entity(9, 2 * index + 1)).unwrap(),
                tolerance: None,
            });
            let link_target = if index < spec.linked_edges {
                Some((row, None))
            } else if spec.coedge_links {
                let coedge_index =
                    cadmpeg_asm::brep::records::neutral_record_index(
                        coedges[usize::try_from(index).unwrap()].as_str(),
                    )
                    .unwrap();
                Some((row, Some(coedge_index)))
            } else {
                None
            };
            if let Some((target, target_coedge)) = link_target {
                links.push(SketchCurveLink {
                    id: format!("link-{ordinal}-{index}"),
                    target,
                    target_coedge,
                    sketch_curve_id: 1,
                    ref_b: 0,
                    sense: None,
                    role: 0,
                    closure: 0,
                });
            }
        }
        if spec.face_attribute {
            asm.attributes.push(
                serde_json::from_value(serde_json::json!({
                    "id": entity(8, 1), "target": {"kind": "face", "id": face},
                    "name": "generic_tag_attrib_def", "values": [],
                }))
                .unwrap(),
            );
        }
        asm.body_native_keys.push(body_key);
    }
    Brep {
        asm,
        sketch_curve_links: links,
        ..Brep::default()
    }
}

#[test]
fn sketch_regions_leave_a_keyless_blob_and_selectors_count_the_remaining_bodies() {
    let mut brep = sketch_region_brep(&[SKETCH_REGION, SOLID, SKETCH_REGION, SOLID], None);
    assert_eq!(
        with_context(|ctx| brep.omit_sketch_region_bodies(ctx, 2)).unwrap(),
        2
    );
    let ordinals = brep
        .asm
        .body_native_keys
        .iter()
        .map(|key| (key.record_index, key.body_ordinal))
        .collect::<Vec<_>>();
    assert_eq!(ordinals, [(2, 0), (4, 1)]);
    assert_eq!(brep.asm.bodies.len(), 2);
    assert_eq!(brep.asm.faces.len(), 2);
    let selected = brep.body_selectors_for(&HashSet::from([0])).unwrap();
    assert_eq!(
        selected.keys().map(BodyId::as_str).collect::<Vec<_>>(),
        ["f3d:brep:entity#2001"]
    );
    assert!(brep
        .sketch_curve_links
        .iter()
        .all(|link| link.target_coedge.is_some()));
}

#[test]
fn only_a_keyless_body_with_fully_linked_edges_and_unattributed_faces_is_a_sketch_region() {
    let near_misses = [
        // A planar surface made from a Sketch: linked edges and a face attribute.
        RegionBody {
            face_attribute: true,
            ..SKETCH_REGION
        },
        // One edge stores no link.
        RegionBody {
            linked_edges: 3,
            ..SKETCH_REGION
        },
        // Links on coedges only, the form a solid made from a Sketch stores.
        RegionBody {
            linked_edges: 0,
            coedge_links: true,
            ..SKETCH_REGION
        },
        // No edges to prove the form.
        RegionBody {
            edges: 0,
            linked_edges: 0,
            ..SKETCH_REGION
        },
        // A wire member is outside the region form.
        RegionBody {
            wire_edge: true,
            ..SKETCH_REGION
        },
    ];
    for (case, spec) in near_misses.into_iter().enumerate() {
        let mut brep = sketch_region_brep(&[spec, SOLID], None);
        assert_eq!(
            with_context(|ctx| brep.omit_sketch_region_bodies(ctx, 1)).unwrap(),
            0,
            "{case}"
        );
        assert_eq!(brep.asm.bodies.len(), 2, "{case}");
        assert_eq!(brep.asm.body_native_keys[1].body_ordinal, 1, "{case}");
    }
    // A stored body key selects by key, so the form has no effect.
    let mut keyed = sketch_region_brep(&[SKETCH_REGION, SOLID], Some(7));
    assert_eq!(
        with_context(|ctx| keyed.omit_sketch_region_bodies(ctx, 1)).unwrap(),
        0
    );
    assert_eq!(keyed.asm.bodies.len(), 2);
    // The body map must hold one selector per remaining body.
    for selector_count in [0, 2] {
        let mut unmatched = sketch_region_brep(&[SKETCH_REGION, SOLID], None);
        assert_eq!(
            with_context(|ctx| unmatched.omit_sketch_region_bodies(ctx, selector_count)).unwrap(),
            0,
            "{selector_count}"
        );
        assert_eq!(unmatched.asm.bodies.len(), 2, "{selector_count}");
    }
}

#[test]
fn a_blob_of_only_sketch_regions_keeps_no_model_body() {
    let mut brep = sketch_region_brep(&[SKETCH_REGION], None);
    assert_eq!(
        with_context(|ctx| brep.omit_sketch_region_bodies(ctx, 0)).unwrap(),
        1
    );
    assert!(brep.asm.bodies.is_empty());
    assert!(brep.asm.body_native_keys.is_empty());
}
