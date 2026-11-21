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
