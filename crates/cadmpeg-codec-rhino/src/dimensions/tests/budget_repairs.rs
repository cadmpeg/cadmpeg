// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn annotation(text: &str, fallback: &str) -> super::super::V2Annotation {
    let bytes = v2_payload(7, &[], text, fallback, false, None);
    with_test_context(&bytes, |ctx| {
        v2_annotation_direct(
            ctx,
            &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
            MillimeterScale::IDENTITY,
        )
    })
    .expect("valid V2 text")
}

#[test]
fn v2_text_trimming_admits_discarded_characters_and_preserves_unicode() {
    for (text, fallback, expected) in [
        ("\0\t\u{2003}  ", "fallback", ""),
        ("\u{2003}\0é中\n\u{2003}", "fallback", "é中"),
        ("", "\n défaut \0", "défaut"),
        ("", "", ""),
    ] {
        let annotation = annotation(text, fallback);
        assert_eq!(
            v2_effective_text(&cadmpeg_test_support::service_decode_context(), &annotation)
                .unwrap(),
            expected
        );
    }
    for (text, operation) in [
        ("\0\t\u{2003}  ", "Rhino V2 effective text leading boundary"),
        (" é\n\u{2003}", "Rhino V2 effective text trailing boundary"),
    ] {
        let annotation = annotation(text, "fallback");
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let result = v2_effective_text(&ctx, &annotation);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            },
        );
    }
}

#[test]
fn v2_coordinate_checks_do_not_charge_fixed_arrays() {
    let bytes = v2_payload(7, &[[1.0, 2.0]; 32], "", "", false, None);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "Rhino v2 annotation direct traversal",
        None,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let annotation = v2_annotation_direct(
        &ctx,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
        MillimeterScale::IDENTITY,
    )
    .expect("fixed coordinate predicates need no admission");
    assert_eq!(annotation.points.len(), 32);
    assert!(annotation
        .points
        .iter()
        .all(|point| point.get() == [1.0, 2.0]));
}
