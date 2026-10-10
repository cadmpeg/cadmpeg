// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

fn cage_payload(count: i32) -> Vec<u8> {
    use crate::test_support::test_dump as bytes;
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    for value in [1, 0, 3, 1, 2, 2, 2, 2, 2, count] {
        bytes::push_i32(&mut body, value);
    }
    for knots in [2, 2, count] {
        for index in 0..knots {
            bytes::push_f64(&mut body, f64::from(index));
        }
    }
    for index in 0..(4 * count) {
        for coordinate in [f64::from(index), 0.0, 0.0, 1.0] {
            bytes::push_f64(&mut body, coordinate);
        }
    }
    bytes::crc_chunk(archive, 0x4000_8000, &body)
}

fn cage_scan(count: i32) -> crate::container::Scan<'static> {
    use crate::test_support::test_dump as bytes;
    let archive = ArchiveVersion::V5;
    let payload = cage_payload(count);
    scan_with_objects(&[bytes::object_record_with_payload(
        archive, 1, crate::cage::CLASS.to_wire(), &payload,
    )])
}

#[test]
fn cage_feature_text_preserves_coordinate_and_weight_order() {
    let scan = cage_scan(2);
    with_expand(&scan, |expand| {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        transaction.decode_geometry().unwrap();
        let features = &transaction.session.document().model.features;
        assert_eq!(features.len(), 1);
        let properties = &features[0].source_properties;
        assert_eq!(properties.get("u_knots").unwrap(), "0,1");
        assert_eq!(properties.get("v_knots").unwrap(), "0,1");
        assert_eq!(properties.get("w_knots").unwrap(), "0,1");
        assert_eq!(properties.get("control_points").unwrap(), "0,0,0;1,0,0;2,0,0;3,0,0;4,0,0;5,0,0;6,0,0;7,0,0");
        assert_eq!(properties.get("weights").unwrap(), "1,1,1,1,1,1,1,1");
    });
}

#[test]
fn rejected_cage_feature_releases_payload_fields() {
    let mut rejection_cost = None;
    for count in [2, 32] {
        let scan = cage_scan(count);
        let run = |cap, reject| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            let object = transaction.object(0).unwrap().clone();
            transaction.decode_cage(0, &object)?;
            assert_eq!(transaction.session.document().model.features.len(), 1);
            if reject {
                transaction.decode_cage(0, &object)?;
                assert_eq!(transaction.session.document().model.features.len(), 1);
                assert!(transaction.report.phase_warnings.iter().any(|warning| warning.contains("NURBS cage candidate rejected")));
            }
            ctx.copy_retained_text("x", "cage rejection checkpoint")?;
            drop(transaction);
            ctx.finish_session()
        };
        let used = |reject| {
            let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "cage rejection checkpoint", |cap| run(cap, reject));
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("checkpoint refusal"); };
            limit.used
        };
        let cost = used(true) - used(false);
        if let Some(previous) = rejection_cost {
            assert_eq!(cost, previous, "discarded cage fields do not consume retained bytes");
        }
        rejection_cost = Some(cost);
        run(u64::MAX, true).unwrap();
    }
}

#[test]
fn rejected_morph_feature_keeps_unresolved_reference_losses() {
    use crate::test_support::test_dump as bytes;
    let archive = ArchiveVersion::V5;
    let captive = crate::wire::Uuid::from_canonical([9; 16]);
    let mut ids = Vec::new();
    for value in [1, 0, 1] { bytes::push_i32(&mut ids, value); }
    ids.extend(captive.to_wire());
    let mut body = Vec::new();
    for value in [1, 0] { bytes::push_i32(&mut body, value); }
    body.extend(cage_payload(2));
    body.extend(bytes::crc_chunk(archive, 0x4000_8000, &ids));
    for value in [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
                  0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        bytes::push_f64(&mut body, value);
    }
    let payload = bytes::crc_chunk(archive, 0x4000_8000, &body);
    let scan = scan_with_objects(&[bytes::object_record_with_payload(archive, 1, crate::morph::CLASS.to_wire(), &payload)]);
    with_expand(&scan, |expand| {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        let object = transaction.object(0).unwrap().clone();
        transaction.decode_morph(0, &object).unwrap();
        assert_eq!(transaction.session.document().model.features.len(), 1);
        assert_eq!(transaction.report.typed_losses.len(), 1);
        transaction.decode_morph(0, &object).unwrap();
        assert_eq!(transaction.session.document().model.features.len(), 1);
        assert!(transaction.report.phase_warnings.iter().any(|warning| warning.contains("morph candidate rejected")));
        assert_eq!(transaction.report.typed_losses.len(), 2);
        for loss in &transaction.report.typed_losses {
            assert_eq!(loss.code, RhinoLossCode::ReferenceMemberUnresolved.kind());
            assert_eq!(loss.message, format!("morph captive in object record 0 references object {captive}"));
        }
    });
}
