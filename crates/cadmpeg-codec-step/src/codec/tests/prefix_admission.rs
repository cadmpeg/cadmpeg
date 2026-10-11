// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn empty_detection_has_no_input_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(!crate::codec::starts_with_step_magic(&ctx, b"").unwrap());
    assert!(!crate::codec::is_part28_xml(b""));
    assert!(!crate::codec::is_ap242_bo_model_xml(b""));
    ctx.finish_session().unwrap();
}


#[test]
fn magic_trivia_has_no_exhaustion_visit() {
    for (input, work) in [(b" ".as_slice(), 2), (b"\\N\\", 1), (b"/*x*/", 3)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // A comment visits the outer cursor and two actual delimiter windows.
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).unwrap();
        assert!(!crate::codec::starts_with_step_magic(&ctx, input).unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn xml_preamble_has_no_exhaustion_visit() {
    for input in [b"<!--x-->".as_slice(), b"<?x?>", b"<!X>"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).unwrap();
        assert!(!crate::codec::is_part28_xml(input));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn bounded_xml_probes_need_no_work_or_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(crate::codec::is_part28_xml(b"<iso_10303_28/>"));
    assert!(crate::codec::is_part28_xml(
        b"<?xml version='1.0'?><!-- ignored --><other xmlns='urn:oid:1.0.10303.28.2.1.1'/>"
    ));
    assert!(crate::codec::is_ap242_bo_model_xml(
        b"<!-- ignored --><s:Uos xmlns:s='http://standards.iso.org/iso/ts/10303/-3001/-ed-1/tech/xml-schema/bo_model'/>"
    ));
    assert!(!crate::codec::is_part28_xml(b"<other xmlns='unrelated'/><iso_10303_28/>"));
    assert!(!crate::codec::is_ap242_bo_model_xml(b"<Uos xmlns='unrelated'/>"));
    ctx.finish_session().unwrap();
}

#[test]
fn xml_probe_enforces_its_4096_byte_window() {
    let mut prefix = vec![b' '; super::super::XML_PROBE_BYTES];
    prefix.extend_from_slice(b"<iso_10303_28/>");
    assert!(!crate::codec::is_part28_xml(&prefix));
    let root = b"<iso_10303_28/>";
    let mut ending_at_limit = vec![b' '; super::super::XML_PROBE_BYTES - root.len()];
    ending_at_limit.extend_from_slice(root);
    assert!(crate::codec::is_part28_xml(&ending_at_limit));
    ending_at_limit.extend_from_slice(&[b'x'; 8192]);
    assert!(crate::codec::is_part28_xml(&ending_at_limit));
    let mut truncated = vec![b' '; super::super::XML_PROBE_BYTES - root.len() + 1];
    truncated.extend_from_slice(root);
    assert!(!crate::codec::is_part28_xml(&truncated));
}
