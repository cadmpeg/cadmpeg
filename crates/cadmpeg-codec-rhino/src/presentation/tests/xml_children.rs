// SPDX-License-Identifier: Apache-2.0
//! XML child searches stop at the immutable parent's actual last child.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::FramingError;
use crate::wire::Uuid;

fn assert_missing_child(xml: &str, operation: &'static str, message: &str, empty: bool) {
    let bytes = super::legacy_rdk_payload(xml, false, &[]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    // An empty parent must never execute a positive child-search visit.
    let probe = empty.then(|| RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None));
    let error = crate::presentation::classify_rdk_material_payload(&ctx, &bytes, 0..bytes.len()).unwrap_err();
    drop(probe);
    assert!(matches!(error, FramingError::Structural { offset: 0, message: actual } if actual == message));
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_xml_root_executes_no_render_data_child_visit() {
    assert_missing_child("<xml/>", "Rhino RDK render data search",
        "legacy RDK XML has no render-content-manager-data element", true);
}

#[test]
fn empty_render_data_executes_no_material_child_visit() {
    assert_missing_child("<xml><render-content-manager-data/></xml>", "Rhino RDK material search",
        "legacy RDK XML has no material element", true);
}

#[test]
fn unmatched_render_data_children_preserve_the_structural_error() {
    assert_missing_child("<xml><!--first--><other/>tail</xml>", "Rhino RDK render data search",
        "legacy RDK XML has no render-content-manager-data element", false);
}

#[test]
fn unmatched_material_children_preserve_the_structural_error() {
    assert_missing_child("<xml><render-content-manager-data><!--first--><other/>tail</render-content-manager-data></xml>",
        "Rhino RDK material search", "legacy RDK XML has no material element", false);
}

#[test]
fn xml_child_searches_preserve_first_and_last_matching_material_identity() {
    for xml in [
        "<xml><render-content-manager-data><material instance-id=\"11111111-1111-1111-1111-111111111111\"/><material instance-id=\"22222222-2222-2222-2222-222222222222\"/></render-content-manager-data><unused/></xml>",
        "<xml><!--first--><other/><render-content-manager-data><!--first--><other/><material instance-id=\"11111111-1111-1111-1111-111111111111\"/></render-content-manager-data></xml>",
    ] {
        let bytes = super::legacy_rdk_payload(xml, false, &[]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        assert_eq!(crate::presentation::parse_legacy_rdk_material_instance_id(&ctx, &bytes, 0..bytes.len()).unwrap(),
            Some(Uuid::from_canonical([0x11; 16])));
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_xml_child_search_preserves_the_original_refusal() {
    let bytes = super::legacy_rdk_payload("<xml/>", false, &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "Rhino original XML child refusal").unwrap_err()
        else { panic!("original work refusal"); };
    assert!(matches!(crate::presentation::classify_rdk_material_payload(&ctx, &bytes, 0..bytes.len()),
        Err(FramingError::Resource(refusal)) if refusal == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
