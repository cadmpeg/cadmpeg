// SPDX-License-Identifier: Apache-2.0
//! Pcurve decoding observes the workspace constructor's session.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;

use crate::reader::geometry::{PcurveSources, PcurveWalk, PcurveWorkspace};

use super::{decode_pcurve_geometry, BTreeMap, BTreeSet};

const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=LINE('',#3,#4);#2=UNKNOWN_CURVE();#3=CARTESIAN_POINT('',(0.,0.));#4=VECTOR('',#5,1.);#5=DIRECTION('',(1.,0.));ENDSEC;END-ISO-10303-21;";

fn preserves_original_refusal(id: u64) {
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
        .expect("valid pcurve records");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::service())
        .expect("source fits policy");
    let mut workspace = PcurveWorkspace::new(&ctx, "test pcurve workspace")
        .expect("empty workspace");
    let mut active = BTreeSet::new();
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let points = BTreeMap::from([(3, Point2::new(0.0, 0.0))]);
    let vectors = BTreeMap::from([(4, Point2::new(1.0, 0.0))]);
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(u64::MAX, "test original pcurve refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    assert!(matches!(decode_pcurve_geometry(id, &exchange, PcurveSources {
        points: &points, vectors: &vectors,
        placements: &BTreeMap::new(), transformations: &BTreeMap::new(), angle_scale: 1.0,
    }, (&mut losses, &mut loss_storage), &mut PcurveWalk { active: &mut active, workspace: &mut workspace }, 0),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(workspace.records.is_empty());
    assert!(active.is_empty());
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(losses);
    drop(active);
    drop(workspace);
    drop(loss_storage);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn pcurve_recognized_record_preserves_original_session_refusal() {
    preserves_original_refusal(1);
}

#[test]
fn pcurve_unrecognized_record_preserves_original_session_refusal() {
    preserves_original_refusal(2);
}

#[test]
fn pcurve_missing_record_preserves_original_session_refusal() {
    preserves_original_refusal(99);
}
