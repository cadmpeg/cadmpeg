// SPDX-License-Identifier: Apache-2.0
//! Omission of a canonical binding does not remove independent ACT entities.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn omitted_appearance_carrier_keeps_registered_native_only_sibling() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = crate::F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let mut native = crate::test_support::native_test::f3d_native(&ir);
    let original = &native.act_entities[0];
    let original_id = original.id().clone();
    let original_index = original.record_index();
    let sibling_id = format!("{}#9000", original.id().rsplit_once('#').unwrap().0);
    let mut sibling = serde_json::to_value(original).unwrap();
    sibling["id"] = serde_json::json!(sibling_id);
    sibling["record_index"] = serde_json::json!(9000);
    sibling["entity_id"] = serde_json::json!("0_777");
    sibling["channels"] = serde_json::json!({
        "Independent": "01234567-89ab-cdef-0123-456789abcdef",
    });
    native
        .act_entities
        .push(serde_json::from_value(sibling).unwrap());
    let registration = native
        .act_types
        .iter_mut()
        .find(|record| {
            record
                .entities
                .values()
                .any(|value| *value == u64::from(original_index))
        })
        .unwrap();
    let mut entities = registration.entities.values().copied().collect::<Vec<_>>();
    entities.push(9000);
    registration.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
    let guids = native.act_guids.clone();
    ir.model.appearance_bindings.clear();
    let mut losses = Vec::new();
    super::reconcile(&ir, &mut native, &mut losses).unwrap();
    assert_eq!(native.act_entities.len(), 1);
    assert_eq!(native.act_entities[0].id(), &sibling_id);
    assert_eq!(
        native.act_entities[0].channel_group().channels()["Independent"]
            .value
            .as_str(),
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert_eq!(native.act_guids, guids);
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(&original_id)));
}

fn act_content(native: &crate::native::F3dNative) -> serde_json::Value {
    serde_json::json!({
        "tables": native.act_tables,
        "types": native.act_types,
        "entities": native.act_entities,
        "guids": native.act_guids,
        "registry_channels": native.act_registry_channels,
        "roots": native.act_root_components,
        "table_references": native.act_table_references,
    })
}

#[test]
fn excluded_model_scope_preserves_the_complete_valid_native_act_stream() {
    let source = include_bytes!("../../../../tests/golden/fixtures/design_appearances_protein.f3d");
    let decoded = crate::F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode the unchanged ACT fixture");
    let mut native = crate::test_support::native_test::f3d_native(decoded.ir());
    let expected = act_content(&native);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.set_scope(cadmpeg_ir::ContentScope::container())
        .expect("empty neutral arenas can exclude model scope");
    let mut losses = Vec::new();
    super::reconcile(&ir, &mut native, &mut losses)
        .expect("reconcile a valid native-only ACT stream");
    assert_eq!(act_content(&native), expected);
    assert!(
        losses.is_empty(),
        "valid typed ACT needs no reduction: {losses:?}"
    );
}

#[test]
fn unsupported_copy_paste_products_omit_their_whole_dependent_act_stream() {
    use cadmpeg_ir::codec::write::{
        target::TargetRequest, EncodeInput, Encoder, NativeDisposition,
    };
    let source = include_bytes!("../../../../tests/golden/fixtures/design_copy_paste.f3d");
    let decoded = crate::F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode the unchanged copy-paste fixture");
    let ir: cadmpeg_ir::CadIr = serde_json::from_slice(
        &serde_json::to_vec(decoded.ir()).expect("serialize document without fidelity"),
    )
    .expect("reload current canonical content");
    let source_findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate source native graph")
    });
    assert!(
        source_findings.is_empty(),
        "source native findings: {source_findings:?}"
    );
    assert!(!ir.model.product_definitions.is_empty());
    assert_eq!(ir.model.occurrences.len(), 2);
    let plan = crate::F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("write current content without source bytes");
    let omissions = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "writer.act-stream-omitted")
        .collect::<Vec<_>>();
    assert_eq!(omissions.len(), 1);
    assert_eq!(omissions[0].severity, cadmpeg_ir::report::Severity::Error);
    let namespace = ir.native.namespace("f3d").expect("source native namespace");
    for arena in [
        "act_tables",
        "act_types",
        "act_entities",
        "act_guids",
        "act_registry_channels",
        "act_root_components",
        "act_table_references",
    ] {
        for record in &namespace.arenas()[arena] {
            assert!(
                omissions[0].message.contains(record.id()),
                "the stream loss names {}",
                record.id()
            );
            assert_eq!(
                plan.report()
                    .native_dispositions
                    .record_disposition("f3d", arena, record.id()),
                NativeDisposition::Dropped
            );
        }
    }
    for (arena, count) in [
        ("product_definitions", ir.model.product_definitions.len()),
        ("occurrences", 2),
    ] {
        let expected = format!("model.{arena}: {count} record(s) are not encoded");
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "writer.product-omitted"
                && loss.message == expected));
    }
    let mut bytes = Vec::new();
    let report = plan
        .write_to(&mut bytes)
        .expect("encode the supported siblings");
    let written = crate::F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the generated container");
    let native = crate::test_support::native_test::f3d_native(written.ir());
    assert_eq!(
        act_content(&native),
        serde_json::json!({"tables": [], "types": [], "entities": [], "guids": [], "registry_channels": [], "roots": [], "table_references": []})
    );
    assert_eq!(written.ir().model.bodies.len(), ir.model.bodies.len());
    assert!(written.ir().model.product_definitions.is_empty());
    assert!(written.ir().model.occurrences.is_empty());
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &ir, written.ir());
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, written.ir())
            .expect("validate generated native graph")
    });
    assert!(
        findings.is_empty(),
        "generated native findings: {findings:?}"
    );
    let validation = cadmpeg_ir::validate::validate_neutral(written.ir(), Vec::new())
        .expect("validate generated neutral content");
    assert!(
        validation.is_ok(),
        "generated neutral findings: {:?}",
        validation.findings
    );
}
