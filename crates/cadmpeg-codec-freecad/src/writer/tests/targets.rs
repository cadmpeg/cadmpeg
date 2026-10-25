// SPDX-License-Identifier: Apache-2.0
//! Target identity, guarded source layout, and synthesis fallback contracts.
use crate::native::DocumentFacts;
use crate::test_support::test_archive::{archive, rewrite_schema_version, CORE_DESIGN_PRODUCT};
use crate::FcstdCodec;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::TargetRequest, EncodeInput, Encoder, ExportPlan, SynthesisCause,
};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions, DecodeResult, SourceFidelity};
use cadmpeg_test_support::wire;
use std::io::Cursor;
fn decode(source: &[u8]) -> DecodeResult {
    FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode FCStd source")
}
fn plan(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    target: TargetRequest<'_>,
) -> Result<ExportPlan, CodecError> {
    FcstdCodec.plan(EncodeInput::new(ir, fidelity), target)
}
fn inherited(decoded: &DecodeResult) -> ExportPlan {
    plan(
        decoded.ir(),
        Some(decoded.source_fidelity()),
        TargetRequest::Inherit,
    )
    .expect("represented unchanged source writes")
}
fn target(plan: &ExportPlan) -> String {
    wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
        plan.report(),
        "identity/target",
    )
    .expect("FCStd target identity")
    .to_string()
}
fn bytes(plan: ExportPlan) -> Vec<u8> {
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("write planned archive");
    output
}
fn entry_payloads(archive: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive.to_vec())).expect("readable ZIP");
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).expect("entry");
            let name = entry.name().expect("entry name").into_owned();
            let mut payload = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut payload).expect("inflate entry");
            (name, payload)
        })
        .collect()
}
fn schema_two_archive() -> Vec<u8> {
    archive(
        r#"<Document SchemaVersion="2" ProgramVersion="0.13"><Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Document"/></Property></Properties><Features Count="1"><Feature type="App::Feature" name="First"/></Features><FeatureData Count="1"><Feature name="First"><Properties Count="0"/></Feature></FeatureData></Document>"#,
    )
}
#[test]
pub(crate) fn write_target_and_source_requirements_are_explicit() {
    let ir = CadIr::empty();
    for target_id in ["fcstd:schema-2", "fcstd:schema-3", "fcstd:schema-4"] {
        assert!(
            FcstdCodec.targets().find(target_id).is_some(),
            "{target_id} is synthesized"
        );
        let output = plan(&ir, None, TargetRequest::Explicit(target_id)).expect("catalog target");
        assert!(matches!(
            output.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        assert_eq!(target(&output), target_id);
        assert_eq!(
            decode(&bytes(output))
                .report()
                .dialects()
                .unwrap()
                .primary()
                .dialect()
                .as_str(),
            target_id
        );
    }
    let default = plan(&ir, None, TargetRequest::Inherit).expect("source-less default");
    assert_eq!(target(&default), "fcstd:schema-4");
    let error = plan(&ir, None, TargetRequest::Explicit("fcstd:schema-99"))
        .expect_err("unknown output vocabulary");
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("expected target refusal: {error}")
    };
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(refusal.available().len(), 3);
    let mut foreign = CadIr::empty();
    foreign.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("step:ap242"),
        )),
        std::collections::BTreeMap::default(),
    ));
    let output = plan(&foreign, None, TargetRequest::Inherit)
        .expect("foreign input uses schema-4 synthesis");
    assert_eq!(target(&output), "fcstd:schema-4");
    assert!(matches!(
        output.report().write_path(),
        WritePath::Synthesized { .. }
    ));
}
#[test]
fn unchanged_represented_sources_patch_their_own_layout() {
    let two = schema_two_archive();
    for (source, target_id) in [
        (CORE_DESIGN_PRODUCT, "fcstd:schema-4"),
        (two.as_slice(), "fcstd:schema-2"),
    ] {
        let decoded = decode(source);
        assert!(
            decoded.source_fidelity().unrepresented().is_empty(),
            "unrepresented: {:?}; losses: {:?}",
            decoded.source_fidelity().unrepresented(),
            decoded.report().losses
        );
        let output = inherited(&decoded);
        assert!(
            matches!(output.report().write_path(), WritePath::Patched { .. }),
            "unchanged represented source {target_id}: {:#?}",
            output.report()
        );
        assert_eq!(target(&output), target_id);
        assert_eq!(entry_payloads(&bytes(output)), entry_payloads(source));
    }
}
#[test]
fn no_fidelity_or_missing_layout_uses_synthesis_in_the_selected_schema() {
    let decoded = decode(&schema_two_archive());
    let output = plan(decoded.ir(), None, TargetRequest::Inherit)
        .expect("typed schema-2 content synthesizes");
    assert_eq!(target(&output), "fcstd:schema-2");
    assert!(matches!(
        output.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let (mut ir, _, fidelity) = decoded.into_parts();
    ir.native
        .namespace_mut("fcstd")
        .set_arena::<DocumentFacts>(
            &cadmpeg_test_support::service_decode_context(),
            "document",
            &[],
        )
        .expect("remove optional source layout");
    let output =
        plan(&ir, Some(&fidelity), TargetRequest::Inherit).expect("missing layout falls back");
    assert_eq!(target(&output), "fcstd:schema-2");
    assert!(matches!(
        output.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(
        matches!(
            output.report().synthesis_cause(),
            Some(SynthesisCause::PatchDeclined { .. })
        ),
        "{:?}",
        output.report().synthesis_cause()
    );
    assert_eq!(
        decode(&bytes(output))
            .report()
            .dialects()
            .unwrap()
            .primary()
            .dialect()
            .as_str(),
        "fcstd:schema-2"
    );
}
#[test]
fn explicit_cross_schema_output_synthesizes_and_reports_the_emitted_vocabulary() {
    let decoded = decode(&schema_two_archive());
    for target_id in ["fcstd:schema-3", "fcstd:schema-4"] {
        let output = plan(
            decoded.ir(),
            Some(decoded.source_fidelity()),
            TargetRequest::Explicit(target_id),
        )
        .expect("cross-schema synthesis");
        assert!(matches!(
            output.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        assert_eq!(target(&output), target_id);
        assert_eq!(
            decode(&bytes(output))
                .report()
                .dialects()
                .unwrap()
                .primary()
                .dialect()
                .as_str(),
            target_id
        );
    }
}
#[test]
fn unknown_source_declaration_is_preserved_only_with_eligible_layout() {
    let source = rewrite_schema_version(CORE_DESIGN_PRODUCT, "04");
    let decoded = decode(&source);
    let primary = decoded.report().dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "fcstd:unknown");
    assert_eq!(primary.declared()["schema_version"], "04");
    let output = inherited(&decoded);
    assert_eq!(target(&output), "fcstd:unknown");
    assert!(matches!(
        output.report().write_path(),
        WritePath::Patched { .. }
    ));
    assert_eq!(entry_payloads(&bytes(output)), entry_payloads(&source));
    let error = plan(decoded.ir(), None, TargetRequest::Inherit)
        .expect_err("no synthesis vocabulary for residual declaration");
    assert!(matches!(error, CodecError::UnsupportedTarget(_)), "{error}");
    let explicit = plan(
        decoded.ir(),
        None,
        TargetRequest::Explicit("fcstd:schema-4"),
    )
    .expect("explicit catalog vocabulary can synthesize");
    assert_eq!(target(&explicit), "fcstd:schema-4");
    assert!(matches!(
        explicit.report().write_path(),
        WritePath::Synthesized { .. }
    ));
}
#[test]
fn a_source_without_a_recorded_dialect_remains_an_unresolvable_target() {
    let decoded = decode(CORE_DESIGN_PRODUCT);
    let (mut ir, _, fidelity) = decoded.into_parts();
    let source = ir.source.take().expect("source metadata");
    let format = source.format().to_owned();
    ir.source=Some(serde_json::from_value(serde_json::json!({"identity":{"classification":"unclassified","format":format},"attributes":source.attributes})).unwrap());
    let error =
        plan(&ir, Some(&fidelity), TargetRequest::Inherit).expect_err("unrecorded source target");
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("target refusal expected: {error}")
    };
    assert_eq!(
        serde_json::to_value(refusal).unwrap()["refusal"]["kind"],
        "unrecorded_source"
    );
    let explicit = plan(&ir, None, TargetRequest::Explicit("fcstd:schema-4"))
        .expect("explicit target resolves missing declaration");
    assert_eq!(target(&explicit), "fcstd:schema-4");
}
#[test]
fn neutral_edits_and_unrepresented_source_never_reach_the_patch_path() {
    let decoded = decode(CORE_DESIGN_PRODUCT);
    let control = bytes(inherited(&decoded));
    let mut edited = decoded.ir().clone();
    edited.model.bodies.first_mut().expect("fixture body").name =
        Some("Renamed current body".into());
    let output = plan(
        &edited,
        Some(decoded.source_fidelity()),
        TargetRequest::Inherit,
    )
    .expect("neutral edit synthesizes");
    assert!(matches!(
        output.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    // #214 B1 now derives this source's unrepresented classes from genuine
    // unread spans. That law takes precedence over the later patch gate.
    let unrepresented = decoded.source_fidelity().unrepresented();
    assert!(!unrepresented.is_empty());
    assert_eq!(
        output.report().synthesis_cause(),
        Some(&SynthesisCause::SourceUnrepresented {
            classes: unrepresented
        })
    );
    let reread = decode(&bytes(output));
    assert!(reread
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.name.as_deref() == Some("Renamed current body")));
    let mut fidelity = decoded.source_fidelity().clone();
    fidelity
        .mark_untyped(
            crate::fidelity::SOURCE_IMAGE,
            [cadmpeg_ir::ArenaClass::Design],
        )
        .unwrap();
    let output = plan(decoded.ir(), Some(&fidelity), TargetRequest::Inherit)
        .expect("unrepresented source synthesizes");
    assert!(matches!(
        output.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert_eq!(
        output.report().synthesis_cause(),
        Some(&SynthesisCause::SourceUnrepresented {
            classes: fidelity.unrepresented()
        })
    );
    assert_eq!(
        bytes(inherited(&decoded)),
        control,
        "planning edited copies never mutates original layout"
    );
}
#[test]
fn every_emitted_target_redecodes_as_the_reported_target() {
    let source = schema_two_archive();
    let decoded = decode(&source);
    for target_id in ["fcstd:schema-2", "fcstd:schema-3", "fcstd:schema-4"] {
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let output = plan(decoded.ir(), fidelity, TargetRequest::Explicit(target_id))
                .expect("target writes");
            let claimed = target(&output);
            let reread = decode(&bytes(output));
            assert_eq!(
                reread
                    .report()
                    .dialects()
                    .unwrap()
                    .primary()
                    .dialect()
                    .as_str(),
                claimed
            );
        }
    }
}

#[test]
fn malformed_typed_native_records_are_input_preconditions() {
    let decoded = decode(CORE_DESIGN_PRODUCT);
    let mut ir = decoded.ir().clone();
    let records = ir
        .native
        .namespace_mut("fcstd")
        .arenas_mut()
        .get_mut("properties")
        .expect("typed properties");
    let record = records.first_mut().expect("fixture property");
    let mut fields = record.fields().expect("typed field map");
    fields.insert("type_name".into(), serde_json::json!(false));
    *record = cadmpeg_ir::native::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(record.id()).expect("existing identity"),
        fields,
    )
    .expect("generic native record");
    let error = plan(&ir, None, TargetRequest::Explicit("fcstd:schema-4"))
        .expect_err("invalid own typed schema cannot supply synthesis content");
    assert!(matches!(error, CodecError::Malformed(_)), "{error}");
}

mod fallback;
