// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::{test_support, SolidEdgeCodec};

#[test]
fn directory_signature_detects_solid_edge() {
    let bytes = test_support::compound();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::High);
    ctx.finish_session().unwrap();
}

#[test]
fn generic_compound_file_has_no_solid_edge_identity() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::No);
    ctx.finish_session().unwrap();
    assert!(SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}

#[test]
fn inspection_lists_compound_streams() {
    let bytes = test_support::compound();
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
}

#[test]
fn full_decode_reports_pending_bodies() {
    let bytes = test_support::compound();
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    assert!(!result.report().losses.is_empty());
}

#[test]
fn full_decode_states_the_document_unit_system() {
    use cadmpeg_ir::design::units::UnitSymbol;
    use test_support::RecordValue;

    let cluster = test_support::property_set(
        2,
        &[
            ("DefaultDistancePrimary", RecordValue::Word(2, 64)),
            ("DefaultAnglePrimary", RecordValue::Word(2, 357)),
        ],
    );
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_cluster(&cluster)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definitions = &result.ir().model.product_definitions;
    assert_eq!(definitions.len(), 1);
    let units = definitions[0].units.as_ref().unwrap();
    assert_eq!(
        (units.length, units.angle),
        (UnitSymbol::In, UnitSymbol::Deg)
    );
}

#[test]
fn full_decode_without_cluster_reports_unread_units() {
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::compound()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.product_definitions[0].units.is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "units.unread"));
}

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn loss_codes(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

#[test]
fn full_decode_states_the_feature_order_as_one_timeline() {
    use cadmpeg_ir::design::container::{Container, OrderedKind};
    use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
    use cadmpeg_ir::design::refs::Member;
    use test_support::Feature;

    let result = decode(test_support::feature_document(&[
        Feature::Part("JMProtrusionFunction"),
        Feature::Part("JMRevolvedProtrusionFunction"),
        Feature::Group,
        Feature::Part("JMCutoutFunction"),
        Feature::Part("JMMysteryFunction"),
        Feature::Part("JMRoundFunction"),
    ]));
    let design = &result.ir().design;
    assert_eq!(design.containers.len(), 1);
    let Container::Ordered { kind, members, .. } = &design.containers[0] else {
        panic!("the timeline is an ordered container");
    };
    assert_eq!(kind, &OrderedKind::Timeline);
    let ordered: Vec<&FeatureOperation> = members
        .iter()
        .map(|member| {
            let Member::Feature(id) = member else {
                panic!("a timeline member is a feature");
            };
            &design
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .unwrap()
                .definition
                .operation
        })
        .collect();
    let family = |family| FeatureOperation::Unresolved { family };
    assert_eq!(ordered.len(), 6);
    assert_eq!(*ordered[0], family(OperationFamily::Extrude));
    assert_eq!(*ordered[1], family(OperationFamily::Revolve));
    assert_eq!(*ordered[2], family(OperationFamily::Pattern));
    assert_eq!(*ordered[3], family(OperationFamily::Extrude));
    assert!(matches!(
        ordered[4],
        FeatureOperation::Native { kind, .. } if kind.as_str() == "solidedge:JMMysteryFunction"
    ));
    assert_eq!(*ordered[5], family(OperationFamily::Fillet));
    assert_eq!(design.scopes.len(), 1);
    assert_eq!(
        design.scopes[0].definition,
        result.ir().model.product_definitions[0].id
    );
    assert_eq!(
        &design.scopes[0].roots,
        &[design.containers[0].id().clone()]
    );
    let codes = loss_codes(&result);
    for code in [
        "feature.operands-pending",
        "feature.rollback-unread",
        "feature.kind-unknown",
        "body.decode-pending",
    ] {
        assert_eq!(
            codes.iter().filter(|found| *found == code).count(),
            1,
            "{code}"
        );
    }
    assert!(!codes.iter().any(|code| code == "feature.storage-unread"));
}

#[test]
fn full_decode_of_a_document_without_features_states_no_design() {
    let result = decode(test_support::feature_document(&[]));
    assert!(result.ir().design.scopes.is_empty());
    assert!(result.ir().design.features.is_empty());
    let codes = loss_codes(&result);
    assert!(!codes.iter().any(|code| code.starts_with("feature.")));
}

#[test]
fn full_decode_reports_a_psm_stream_that_does_not_parse() {
    let mut streams = vec![
        ("PSMcluster0", vec![0_u8; 40]),
        ("PSMspacemap/0x00000000", b"nope".to_vec()),
        ("PSMtypetable", vec![1_u8; 10]),
    ];
    let result = decode(test_support::document(&streams));
    assert!(loss_codes(&result).contains(&"feature.storage-unread".to_owned()));
    assert!(result.ir().design.features.is_empty());
    // A document with no type table runs no feature read.
    streams.pop();
    let result = decode(test_support::document(&streams));
    assert!(!loss_codes(&result)
        .iter()
        .any(|code| code.starts_with("feature.")));
}

#[test]
fn full_decode_reports_features_without_a_navigator() {
    let classes = ["ReferencePlaneObject", "CLSID_JMPart"];
    let objects = [
        test_support::Object {
            id: 1,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        test_support::Object {
            id: 2,
            form: 1,
            flags: 0,
            body: test_support::part_body("JMHoleFunction"),
        },
    ];
    let (cluster, map) = test_support::cluster_and_map(&objects);
    let streams = [
        ("PSMcluster0", cluster),
        ("PSMspacemap/0x00000000", map),
        ("PSMtypetable", test_support::type_table(&classes, &[])),
    ];
    let result = decode(test_support::document(&streams));
    assert!(loss_codes(&result).contains(&"feature.navigator-absent".to_owned()));
    // The same cluster with no feature class, as an assembly states it, adds
    // no loss.
    let objects = [test_support::Object {
        id: 1,
        form: 0,
        flags: 0,
        body: vec![0; 8],
    }];
    let (cluster, map) = test_support::cluster_and_map(&objects);
    let streams = [
        ("PSMcluster0", cluster),
        ("PSMspacemap/0x00000000", map),
        ("PSMtypetable", test_support::type_table(&classes[..1], &[])),
    ];
    let result = decode(test_support::document(&streams));
    assert!(!loss_codes(&result)
        .iter()
        .any(|code| code.starts_with("feature.")));
}

#[test]
fn full_decode_states_folder_members_as_features_and_reports_the_folder() {
    use test_support::Feature;

    let result = decode(test_support::feature_document(&[
        Feature::Part("JMProtrusionFunction"),
        Feature::Other("CLSID_JMFeatureGroup"),
        Feature::Part("JMCutoutFunction"),
    ]));
    assert_eq!(result.ir().design.features.len(), 2);
    let codes = loss_codes(&result);
    assert!(codes.iter().any(|code| code == "feature.folder-unread"));
    assert!(!codes.iter().any(|code| code == "feature.kind-unknown"));
}

#[test]
fn geometry_depth_decode_states_no_design_records() {
    use cadmpeg_core::decode::DecodeDepth;
    use test_support::Feature;

    let bytes = test_support::feature_document(&[
        Feature::Part("JMProtrusionFunction"),
        Feature::Part("JMRevolvedProtrusionFunction"),
    ]);
    let options = DecodeOptions {
        depth: DecodeDepth::Geometry,
        ..DecodeOptions::default()
    };
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap();
    let ir = result.ir();
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.containers.is_empty());
    assert!(ir.design.features.is_empty());
    assert!(ir.design.sheet_metal_rules.is_empty());
    assert!(ir.check_scope().is_ok());
}
