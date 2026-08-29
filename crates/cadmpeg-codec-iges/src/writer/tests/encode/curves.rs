// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use cadmpeg_ir::codec::TargetRequest;

#[test]
fn encode_emits_the_typed_ellipse_form_for_v5_0() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;")),
            &DecodeOptions::default(),
        )
        .expect("the ellipse fixture decodes");
    let plan = IgesEncoder
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_0.target()),
        )
        .expect("V5.0 admits a typed ellipse");
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).expect("V5.0 ellipse writes");
    assert!(!report
        .losses
        .iter()
        .any(|loss| { loss.code.taxonomy() == cadmpeg_ir::LossTaxonomy::GeometryNotTransferred }));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("the V5.0 ellipse output decodes");
    let conic = round_trip
        .ir()
        .native
        .namespace("iges")
        .expect("the output has an IGES namespace")
        .arenas["entities"]
        .iter()
        .find(|record| record.field("entity_type") == Some(104.into()))
        .expect("the output has a Type 104 entity");
    assert_eq!(conic.field("form"), Some(1.into()));
    assert!(round_trip.report().losses.is_empty());
    assert_eq!(
        round_trip.report().dialects().unwrap().primary().declared()["effective_version"],
        "5.0"
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
