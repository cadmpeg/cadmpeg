// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn generated_source_less_unit_cube_writes_body_transform() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let expected = cadmpeg_ir::transform::Transform::affine([
        [0.0, -1.0, 0.0, 20.0],
        [1.0, 0.0, 0.0, -30.0],
        [0.0, 0.0, 1.0, 40.0],
    ])
    .expect("affine transform");
    source_less.geometry.bodies[0].transform = Some(expected);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less transformed cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less transformed cube round trip");
    assert_eq!(round_trip.ir().geometry.bodies[0].transform, Some(expected));
    let hints = &f3d_native(round_trip.ir()).transform_hints[0];
    assert!(hints.rotation);
    assert!(!hints.reflection);
    assert!(!hints.shear);
}

#[test]
fn generated_source_less_unit_cube_writes_body_and_face_colors() {
    use cadmpeg_ir::topology::Color;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let body_color = Color::new(0.1, 0.2, 0.3, 1.0).expect("valid color");
    let face_color = Color::new(0.65, 0.45, 0.25, 1.0).expect("valid color");
    source_less.geometry.bodies[0].color = Some(body_color);
    source_less.geometry.faces[2].color = Some(face_color);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less colored cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less colored cube round trip");
    assert_eq!(round_trip.ir().geometry.bodies[0].color, Some(body_color));
    assert_eq!(round_trip.ir().geometry.faces[2].color, Some(face_color));
    assert!(round_trip
        .ir()
        .geometry
        .faces
        .iter()
        .enumerate()
        .all(|(ordinal, face)| ordinal == 2 || face.color.is_none()));
}

#[test]
fn generated_source_less_writes_translucent_direct_color_opaque() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.geometry.bodies[0].color =
        Some(cadmpeg_ir::topology::Color::new(0.1, 0.2, 0.3, 0.5).expect("valid color"));

    let body = source_less.geometry.bodies[0].id.to_string();
    crate::test_support::assert_written_with_loss(
        &source_less,
        None,
        &crate::loss::F3dLossCode::WriterAppearanceOmitted
            .kind()
            .to_string(),
        &body,
    );
}

#[test]
fn generated_source_less_writes_an_empty_display_name_as_no_name() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.geometry.bodies[0].name = Some(String::new());

    let mut encoded = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("an empty display name writes");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("the written file decodes");
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.bodies[0].name, None);
}

#[test]
fn generated_source_less_writes_persistent_body_and_sketch_provenance_attributes() {
    use crate::records::{
        recipes::CreationTimestamp,
        sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink},
    };
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::topology::Color;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.geometry.bodies[0].color =
        Some(Color::new(0.2, 0.4, 0.6, 1.0).expect("valid color"));
    source_less.geometry.faces[0].color =
        Some(Color::new(0.7, 0.3, 0.1, 1.0).expect("valid color"));
    let body_id = source_less.geometry.bodies[0].id.clone();
    let face_id = source_less.geometry.faces[0].id.clone();
    let edge_id = source_less.geometry.edges[0].id.clone();
    let coedge_id = source_less.geometry.coedges[0].id.clone();
    let vertex_id = source_less.geometry.vertices[0].id.clone();
    let mut native = f3d_native_mut(&mut source_less);
    native.persistent_design_links = vec![
        PersistentDesignLink {
            id: "f3d:generated:persistent-design-link#0".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "311".to_owned().try_into().unwrap(),

            design_reference: 7,
            ordinal: 0,
        },
        PersistentDesignLink {
            id: "f3d:generated:persistent-design-link#1".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "322".to_owned().try_into().unwrap(),

            design_reference: 8,
            ordinal: 1,
        },
    ];
    native.persistent_subentity_tags = vec![
        PersistentSubentityTag {
            id: "f3d:generated:persistent-subentity-tag#0".into(),
            target: AttributeTarget::Face(face_id.clone()),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("8").unwrap(),
            token_integer: 4012,
            design_references: vec![301, -314, 411],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:generated:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Edge(edge_id.clone()),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("-1").unwrap(),
            token_integer: 0,
            design_references: vec![511],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:generated:persistent-subentity-tag#2".into(),
            target: AttributeTarget::Face(face_id.clone()),
            selector: 3,
            token: cadmpeg_core::text::NonBlankString::new("42").unwrap(),
            token_integer: 0,
            design_references: Vec::new(),
            ordinal: 1,
        },
    ];
    native.sketch_curve_links = vec![SketchCurveLink {
        id: "f3d:generated:sketch-curve-link#0".into(),
        target: AttributeTarget::Coedge(coedge_id.clone()),
        sketch_curve_id: 113,
        ref_b: 0,
        sense: Some(crate::records::sketch_links::SketchLinkSense::try_from(1).unwrap()),
        role: 2,
        closure: 3,
    }];
    native.creation_timestamps = [
        (AttributeTarget::Body(body_id), 1_579_392_000_000_001.0),
        (AttributeTarget::Face(face_id), 1_579_392_000_000_002.0),
        (AttributeTarget::Edge(edge_id), 1_579_392_000_000_003.0),
        (AttributeTarget::Coedge(coedge_id), 1_579_392_000_000_004.0),
        (AttributeTarget::Vertex(vertex_id), 1_579_392_000_000_005.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (target, unix_microseconds))| CreationTimestamp {
        id: format!("f3d:generated:creation-timestamp#{ordinal}"),
        target,
        record_index: 0,
        unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(unix_microseconds).unwrap(),
    })
    .collect();

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less provenance attribute encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less provenance attribute round trip");
    {
        use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

        fn suffix_index(value: &str) -> i64 {
            value
                .rsplit_once('#')
                .and_then(|(_, suffix)| suffix.parse().ok())
                .expect("generated native id has a numeric record suffix")
        }

        fn attribute_index(attribute: &SourceAttribute) -> i64 {
            suffix_index(attribute.id.as_str())
        }

        fn reference_index(value: Option<&AttributeValue>) -> i64 {
            let Some(AttributeValue::Reference(value)) = value else {
                panic!("generated attribute link is not a reference");
            };
            suffix_index(value)
        }

        fn target_index(target: &AttributeTarget) -> i64 {
            let id = match target {
                AttributeTarget::Body(id) => id.as_str(),
                AttributeTarget::Face(id) => id.as_str(),
                AttributeTarget::Coedge(id) => id.as_str(),
                AttributeTarget::Edge(id) => id.as_str(),
                AttributeTarget::Vertex(id) => id.as_str(),
                _ => panic!("source-less attribute has an unsupported topology owner"),
            };
            suffix_index(id)
        }

        let attributes = &round_trip.ir().model.attributes;
        assert!(!attributes.is_empty());
        for attribute in attributes {
            assert!(attribute.values.len() >= 5);
            assert_eq!(reference_index(attribute.values[0].as_ref()), -1);
            assert_eq!(attribute.values[1], Some(AttributeValue::Integer(-1)));
            assert_eq!(
                reference_index(attribute.values[4].as_ref()),
                target_index(&attribute.target.entity)
            );

            let index = attribute_index(attribute);
            for (field, reciprocal) in [(2usize, 3usize), (3, 2)] {
                let linked = reference_index(attribute.values[field].as_ref());
                if linked < 0 {
                    continue;
                }
                let linked_attribute = attributes
                    .iter()
                    .find(|candidate| attribute_index(candidate) == linked)
                    .expect("generated attribute link resolves");
                assert_eq!(linked_attribute.target, attribute.target);
                assert_eq!(
                    reference_index(linked_attribute.values[reciprocal].as_ref()),
                    index
                );
            }
        }
        for (ordinal, attribute) in attributes.iter().enumerate() {
            if attributes[..ordinal]
                .iter()
                .any(|before| before.target == attribute.target)
            {
                continue;
            }
            let owned = attributes
                .iter()
                .filter(|candidate| candidate.target == attribute.target);
            assert_eq!(
                owned
                    .clone()
                    .filter(|candidate| reference_index(candidate.values[3].as_ref()) == -1)
                    .count(),
                1
            );
            assert_eq!(
                owned
                    .filter(|candidate| reference_index(candidate.values[2].as_ref()) == -1)
                    .count(),
                1
            );
        }
    }
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.persistent_design_links.len(), 2);
    assert_eq!(native.persistent_design_links[0].design_id.as_str(), "311");
    assert_eq!(native.persistent_design_links[0].design_reference, 7);
    assert_eq!(native.persistent_design_links[1].design_id.as_str(), "322");
    assert_eq!(native.persistent_design_links[1].design_reference, 8);
    assert_eq!(
        crate::records::sketch_links::current_persistent_design_links(
            &native.persistent_design_links
        )
        .values()
        .map(|link| link.design_id.as_str())
        .collect::<Vec<_>>(),
        ["322"]
    );
    assert_eq!(native.persistent_subentity_tags.len(), 3);
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.token_integer == 4012
            && tag.design_references == [301, -314, 411]
            && matches!(tag.target, AttributeTarget::Face(_))
    }));
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.token == "-1"
            && tag.design_references == [511]
            && matches!(tag.target, AttributeTarget::Edge(_))
    }));
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.token == "42"
            && tag.design_references.is_empty()
            && matches!(tag.target, AttributeTarget::Face(_))
    }));
    assert_eq!(native.sketch_curve_links.len(), 1);
    assert_eq!(native.sketch_curve_links[0].sketch_curve_id, 113);
    assert_eq!(native.sketch_curve_links[0].sense.map(i64::from), Some(1));
    assert_eq!(native.sketch_curve_links[0].role, 2);
    assert_eq!(native.sketch_curve_links[0].closure, 3);
    assert_eq!(native.creation_timestamps.len(), 5);
    assert!(native.creation_timestamps.iter().any(|timestamp| {
        matches!(timestamp.target, AttributeTarget::Vertex(_))
            && timestamp.unix_microseconds.get() == 1_579_392_000_000_005.0
    }));
    assert_eq!(
        round_trip.ir().geometry.bodies[0].color,
        source_less.geometry.bodies[0].color
    );
    assert_eq!(
        round_trip.ir().geometry.faces[0].color,
        source_less.geometry.faces[0].color
    );

    let duplicate = f3d_native(&source_less).creation_timestamps[0].clone();
    f3d_native_mut(&mut source_less)
        .creation_timestamps
        .push(duplicate);
    crate::test_support::assert_written_with_loss(
        &source_less,
        None,
        "export/native.records-omitted",
        "multiple F3D creation timestamps target the same entity",
    );
}

#[test]
fn generated_source_less_drops_lossy_design_link_metadata() {
    use crate::records::sketch_links::{PersistentDesignLink, SketchCurveLink};
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let body = source_less.geometry.bodies[0].id.clone();
    let coedge = source_less.geometry.coedges[0].id.clone();
    let mut native = f3d_native_mut(&mut source_less);
    native.persistent_design_links = vec![PersistentDesignLink {
        id: "f3d:generated:persistent-design-link#0".into(),
        target: AttributeTarget::Body(body),
        design_id: "311".to_owned().try_into().unwrap(),

        design_reference: 7,
        ordinal: 1,
    }];
    native.sketch_curve_links = [0, 1]
        .map(|ordinal| SketchCurveLink {
            id: format!("f3d:generated:sketch-curve-link#{ordinal}"),
            target: AttributeTarget::Coedge(coedge.clone()),
            sketch_curve_id: 113 + ordinal,
            ref_b: 0,
            sense: Some(crate::records::sketch_links::SketchLinkSense::try_from(1).unwrap()),
            role: 2,
            closure: 3,
        })
        .into();
    drop(native);

    crate::test_support::assert_written_with_loss(
        &source_less,
        None,
        "export/native.records-omitted",
        "contiguous ordinals",
    );
}

#[test]
fn generated_source_less_drops_collapsed_native_topology_metadata() {
    use cadmpeg_asm::brep::records::{EdgeContinuity, TolerantVertexTail};

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let edge = source_less.geometry.edges[0].id.clone();
    let vertex = source_less.geometry.vertices[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.edge_continuities = [0, 1]
            .map(|ordinal| EdgeContinuity {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                edge: edge.clone(),
                record_index: ordinal,
                sense: cadmpeg_ir::topology::Sense::Forward,
                continuity: "unknown".into(),
            })
            .into();
    }
    crate::test_support::assert_written_with_loss(
        &source_less,
        None,
        "export/native.records-omitted",
        "multiple F3D edge-continuity records",
    );

    {
        let mut native = f3d_native_mut(&mut source_less);
        native.edge_continuities.truncate(1);
        native.tolerant_vertex_tails = vec![TolerantVertexTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            vertex,
            record_index: 0,
            leading_tolerances: [
                cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
            ],
            evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Evaluated {
                trailing: Some(0),
            },
        }];
    }
    crate::test_support::assert_written_with_loss(
        &source_less,
        None,
        "export/native.records-omitted",
        "of tolerant-vertex records whose evaluated slot does not agree with the tolerance of their vertex",
    );
}

#[test]
fn generated_source_less_writes_two_independent_cube_bodies() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical cube JSON")
        .replace("synthetic:cube:", "synthetic:cube_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second cube IR");
    second.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 30.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );
    source_less
        .geometry
        .bodies
        .append(&mut second.geometry.bodies);
    source_less
        .geometry
        .regions
        .append(&mut second.geometry.regions);
    source_less
        .geometry
        .shells
        .append(&mut second.geometry.shells);
    source_less
        .geometry
        .faces
        .append(&mut second.geometry.faces);
    source_less
        .geometry
        .loops
        .append(&mut second.geometry.loops);
    source_less
        .geometry
        .coedges
        .append(&mut second.geometry.coedges);
    source_less
        .geometry
        .edges
        .append(&mut second.geometry.edges);
    source_less
        .geometry
        .vertices
        .append(&mut second.geometry.vertices);
    source_less
        .geometry
        .points
        .append(&mut second.geometry.points);
    source_less
        .geometry
        .surfaces
        .append(&mut second.geometry.surfaces);
    source_less
        .geometry
        .curves
        .append(&mut second.geometry.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less two-body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-body round trip");
    assert_eq!(round_trip.ir().geometry.bodies.len(), 2);
    assert_eq!(round_trip.ir().geometry.regions.len(), 2);
    assert_eq!(round_trip.ir().geometry.shells.len(), 2);
    assert_eq!(round_trip.ir().geometry.faces.len(), 12);
    assert_eq!(round_trip.ir().geometry.edges.len(), 24);
    assert_eq!(round_trip.ir().geometry.points.len(), 16);
    assert_eq!(
        round_trip.ir().geometry.bodies[1]
            .transform
            .expect("second body transform")
            .rows()[0][3],
        30.0
    );
    let report = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_source_less_writes_typed_asm_history_graph() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let expected = f3d_native(&source_less).asm_histories[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less history encode");
    let mut preambleless = source_less.clone();
    {
        let mut native = f3d_native_mut(&mut preambleless);
        native.asm_histories[0].preamble = None;
    }
    let mut preambleless_bytes = Vec::new();
    F3dCodec
        .plan(
            EncodeInput::new(&preambleless, None),
            TargetRequest::Unnamed,
        )
        .and_then(|plan| plan.write_to(&mut preambleless_bytes))
        .expect("source-less preambleless history encode");
    let preambleless_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(preambleless_bytes),
            &DecodeOptions::default(),
        )
        .expect("source-less preambleless history round trip");
    assert_eq!(
        f3d_native(preambleless_round_trip.ir()).asm_histories[0].stream_size(),
        None
    );
    assert_eq!(
        f3d_native(preambleless_round_trip.ir()).asm_histories[0].history_entry_count(),
        None
    );
    {
        let mut native = f3d_native_mut(&mut source_less);
        if let Some(preamble) = &mut native.asm_histories[0].preamble {
            preamble.stream_size = 3;
        }
    }
    crate::test_support::assert_written_with_loss(
        &source_less,
        None,
        "export/native.records-omitted",
        "head state_id == stream_size and nonnegative history_entry_count",
    );
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less history round trip");
    let actual = &f3d_native(round_trip.ir()).asm_histories[0];
    assert_eq!(actual.stream_size(), expected.stream_size());
    assert_eq!(actual.history_entry_count(), expected.history_entry_count());
    assert_eq!(actual.states().len(), expected.states().len());
    assert_eq!(actual.states()[0].state_id, expected.states()[0].state_id);
    assert_eq!(actual.states()[0].bulletin_boards.len(), 1);
    assert_eq!(actual.states()[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(actual.states()[0].records.len(), 1);
    assert_eq!(actual.states()[0].records[0].name(), "history_payload");
}

#[test]
fn generated_source_less_drops_lossy_asm_history_graphs() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let mut orphaned = decoded.ir().clone();
    orphaned.source = None;
    orphaned
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let orphan = &mut orphaned
        .native
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .arenas_mut()
        .get_mut("asm_history_records")
        .expect("history-record arena")[0];
    let mut orphan_fields = orphan.fields().unwrap().clone();
    orphan_fields.insert("parent".into(), cadmpeg_core::json!("missing-state"));
    *orphan = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(orphan.id()).expect("valid identity"),
        orphan_fields,
    )
    .expect("valid native identity");
    crate::test_support::assert_written_with_loss(
        &orphaned,
        None,
        "export/native.records-omitted",
        "orphaned or ambiguously parented records",
    );

    let mut duplicate = decoded.ir().clone();
    duplicate.source = None;
    duplicate
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let states = duplicate
        .native
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .arenas_mut()
        .get_mut("asm_delta_states")
        .expect("delta-state arena");
    states.push(states[0].clone());
    crate::test_support::assert_written_with_loss(
        &duplicate,
        None,
        "export/native.records-omitted",
        "asm_delta_states contains duplicate record ids",
    );

    let (mut broken_chain, _, _) = decoded.into_parts();
    broken_chain.source = None;
    broken_chain
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    f3d_native_mut(&mut broken_chain).asm_histories[0].states_mut()[0].next_ref = Some(99);
    crate::test_support::assert_written_with_loss(
        &broken_chain,
        None,
        "export/native.records-omitted",
        "not a coherent doubly linked state chain",
    );
}

#[test]
fn generated_source_less_drops_a_history_skipped_under_the_retained_allowance() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let (mut skipped, _, _) = decoded.into_parts();
    skipped.source = None;
    skipped
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    f3d_native_mut(&mut skipped).asm_histories[0].binding_budget_exceeded = true;
    crate::test_support::assert_written_with_loss(
        &skipped,
        None,
        "export/native.records-omitted",
        "whose bulletin boards and records were not retained",
    );
}
