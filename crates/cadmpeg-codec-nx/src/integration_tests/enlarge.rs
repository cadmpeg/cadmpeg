// SPDX-License-Identifier: Apache-2.0
//! ENLARGE extraction, unresolved semantics, and registered native admission.

use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};

fn decode(selector: u8) -> cadmpeg_ir::codec::DecodeResult {
    // Independent bounded grammar: two payload references, a compact middle
    // index, one shifted binary64 and three zero atoms, then four references.
    let mut prefix = vec![selector, 0, 0, 1, 0, 0xf0, 0, 0xf1, 1, 0, 0x80, 0];
    prefix.extend([0, 0, 0, 0, 0, 0, 0xff, 0, 5, 2, 0xa0, 0x4c, 0x4a, 1, 5]);
    prefix.extend([0x2f, 0xf0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    prefix.extend([1, 3, 8, 2, 0xf0, 2, 0xf0, 3, 0xf0, 4, 0xf0, 5]);
    prefix.extend([
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0,
    ]);
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "ENLARGE", prefix)],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn enlarge_pipeline_resolves_the_owning_class_without_inventing_face_roles() {
    let result = decode(3);
    let namespace = result.ir().native.namespace("nx").unwrap();
    let records = &namespace.arenas()["feature_enlarge_construction_envelopes"];
    assert_eq!(records.len(), 1);
    let wire = serde_json::to_value(&records[0]).unwrap();
    assert_eq!(wire["parameter_class_index"], 3);
    assert_eq!(wire["scalars"], serde_json::json!([1.0, 0.0, 0.0, 0.0]));
    assert_eq!(
        wire["raw_scalars"],
        serde_json::json!(["2ff0000000000000", "00", "00", "00"])
    );
    assert_eq!(wire["byte_len"], 63);
    let feature = &result.ir().model.features[0];
    let definition = serde_json::to_value(feature.evaluation.definition()).unwrap();
    assert_eq!(
        definition,
        serde_json::json!({"definition":"extend_surface", "faces":{"kind":"unresolved"}, "distance":null, "method":"unresolved"})
    );
    let (mut ir, report, fidelity) = result.into_parts();
    let validation =
        cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    crate::test_support::with_decode_context(|ctx| {
        assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
            .unwrap()
            .is_empty());
        let mut invalid = wire;
        invalid["scalars"][0] = serde_json::json!(2.0);
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "feature_enlarge_construction_envelopes", &[invalid])
            .unwrap();
        let findings = <crate::NxCodec as CodecBackend>::validate_native(ctx, &ir).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("values must match raw_values"));
    });
    for selector in [0, 1, 4] {
        let result = decode(selector);
        assert!(!result
            .ir()
            .native
            .namespace("nx")
            .unwrap()
            .arenas()
            .contains_key("feature_enlarge_construction_envelopes"));
    }
}
