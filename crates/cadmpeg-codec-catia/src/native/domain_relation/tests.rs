// SPDX-License-Identifier: Apache-2.0
//! Native retention, exact wire validation, and unresolved operand behavior.

use super::{derive, CatiaConsolidatedDomainRelation};
use crate::native::CatiaNative;
use crate::wire::records::consolidated_records;

fn stream() -> Vec<u8> {
    let mut payload = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 10.0, -0.25, 0.0, 5.0, 2.0, -0.0, 0.25] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = vec![
        0xb2,
        0x03,
        0x24,
        u8::try_from(payload.len()).expect("small fixture"),
        0x05,
    ];
    bytes.extend_from_slice(&payload);
    bytes
}

#[test]
fn native_class24_domain_relation_retains_exact_frame_and_wire() {
    let native = CatiaNative::decode(&stream());
    let [relation] = native.consolidated_domain_relations.as_slice() else {
        panic!("one relation");
    };
    let wire = serde_json::to_value(relation).expect("serialize relation");
    assert_eq!(wire["class"], 0x24);
    assert_eq!(wire["data"]["kind"], "domain24");
    assert_eq!(wire["data"]["selector"], 0x0d87);
    assert_eq!(wire["data"]["sites"]["kind"], "absent");
    assert_eq!(wire["data"]["source"]["kind"], "affine");
    assert_eq!(
        wire["payload"],
        serde_json::json!(relation.definition.frame().payload)
    );
    assert_eq!(
        serde_json::from_value::<CatiaConsolidatedDomainRelation>(wire.clone())
            .expect("load relation"),
        *relation
    );
    let mut contradictory = wire.clone();
    contradictory["data"]["source"]["offset"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<CatiaConsolidatedDomainRelation>(contradictory).is_err());
    let mut contradictory = wire;
    contradictory["data"]["selector"] = serde_json::json!(0x0587);
    assert!(serde_json::from_value::<CatiaConsolidatedDomainRelation>(contradictory).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store domain relation");
    let mut owned_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .clone()
        .store_owned(
            &cadmpeg_test_support::service_decode_context(),
            &mut owned_namespace,
        )
        .expect("store owned domain relation");
    assert_eq!(owned_namespace, namespace);
    assert_eq!(
        CatiaNative::load(&namespace).expect("load native namespace"),
        native
    );
}

#[test]
fn native_class24_domain_relation_refuses_retention_and_collection_limits() {
    let bytes = stream();
    let records = consolidated_records(&bytes);
    let error = crate::test_support::with_collection_limit(0, |ctx| derive(ctx, &bytes, &records))
        .expect_err("relation output is charged");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "catia_domain_relations")
    );
    let error =
        crate::test_support::with_retained_refusal(&[], "catia_domain_relation_payload", |ctx| {
            derive(ctx, &bytes, &records)
        })
        .expect_err("frame bytes are charged");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "catia_domain_relation_payload")
    );
    let error = crate::test_support::with_work_limit(0, |ctx| derive(ctx, &bytes, &records))
        .expect_err("parse work is charged");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "catia_domain_relation_parse")
    );
}

#[test]
fn unresolved_class24_relation_creates_no_neutral_geometry() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let input = crate::test_support::test_container::standard_catpart_from_streams(&stream(), &[]);
    let decoded = crate::CatiaCodec
        .decode(&mut std::io::Cursor::new(input), &DecodeOptions::default())
        .expect("decode unresolved relation");
    assert!(decoded.ir().model.curves.is_empty());
    assert!(decoded.ir().model.procedural_curves.is_empty());
    assert!(decoded.ir().model.edges.is_empty());
    assert_eq!(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("native namespace")
            .arenas()["consolidated_domain_relations"]
            .len(),
        1
    );
}

#[test]
fn native_class25_maps_round_trip_and_decline_conflicting_sample_pairs() {
    for bytes in [
        crate::test_support::test_class25::affine_stream(),
        crate::test_support::test_class25::sampled_stream(),
    ] {
        let native = CatiaNative::decode(&bytes);
        let [relation] = native.consolidated_domain_relations.as_slice() else {
            panic!("one map")
        };
        let wire = serde_json::to_value(relation).expect("serialize map");
        assert_eq!(wire["class"], 0x25);
        assert_eq!(wire["data"]["kind"], "domain25");
        for flag in [0x13, 0x83] {
            let mut wrong_header = wire.clone();
            wrong_header["flag"] = serde_json::json!(flag);
            assert!(
                serde_json::from_value::<CatiaConsolidatedDomainRelation>(wrong_header).is_err()
            );
            let mut wrong_frame = bytes.clone();
            wrong_frame[1] = flag;
            assert!(CatiaNative::decode(&wrong_frame)
                .consolidated_domain_relations
                .is_empty());
        }
        assert_eq!(
            serde_json::from_value::<CatiaConsolidatedDomainRelation>(wire.clone())
                .expect("load complete map"),
            *relation
        );
        let mut contradictory = wire;
        if contradictory["data"]["source"]["kind"] == "sampled" {
            contradictory["data"]["source"]["samples"]["source"][0] = serde_json::json!(-1.0);
        } else {
            contradictory["data"]["source"]["offset"] = serde_json::json!(1.0);
        }
        assert!(serde_json::from_value::<CatiaConsolidatedDomainRelation>(contradictory).is_err());
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        native.store(&mut namespace).expect("borrowed store");
        let mut owned = cadmpeg_ir::NativeNamespace::default();
        native
            .clone()
            .store_owned(&cadmpeg_test_support::service_decode_context(), &mut owned)
            .expect("owned store");
        assert_eq!(owned, namespace);
        assert_eq!(CatiaNative::load(&namespace).expect("load maps"), native);
    }
}

#[test]
fn a8_class25_payload_never_enters_b_family_parameter_map_arena() {
    let b_frame = crate::test_support::test_class25::sampled_stream();
    let payload = &b_frame[5..];
    let mut a8 = vec![0xa8, 0x03, 0x25];
    a8.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("small payload")
            .to_le_bytes(),
    );
    a8.extend_from_slice(&123_u32.to_le_bytes());
    a8.extend_from_slice(payload);
    let native = CatiaNative::decode(&a8);
    assert!(native.consolidated_domain_relations.is_empty());
}

#[test]
fn unresolved_class25_maps_create_no_neutral_geometry() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    for bytes in [
        crate::test_support::test_class25::affine_stream(),
        crate::test_support::test_class25::sampled_stream(),
    ] {
        let input = crate::test_support::test_container::standard_catpart_from_streams(&bytes, &[]);
        let decoded = crate::CatiaCodec
            .decode(&mut std::io::Cursor::new(input), &DecodeOptions::default())
            .expect("decode unresolved class25 map");
        let model = &decoded.ir().model;
        assert!(model.curves.is_empty() && model.procedural_curves.is_empty());
        assert!(model.surfaces.is_empty() && model.procedural_surfaces.is_empty());
        assert!(model.edges.is_empty() && model.faces.is_empty());
        assert_eq!(
            decoded
                .ir()
                .native
                .namespace("catia")
                .expect("native namespace")
                .arenas()["consolidated_domain_relations"]
                .len(),
            1
        );
    }
}
