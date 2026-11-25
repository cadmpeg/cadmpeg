// SPDX-License-Identifier: Apache-2.0
//! Resolution of a write request against the source: the synthesis catalog,
//! preservation, and the refusals.

use cadmpeg_test_support::wire;

use crate::native::DocumentFacts;
use crate::test_support::test_archive::{archive, rewrite_schema_version, CORE_DESIGN_PRODUCT};
use crate::writer::target::UNCARRIED_DIGEST;
use crate::FcstdCodec;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput};
use cadmpeg_ir::codec::write::{EncodeError, Encoder};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::io::Cursor;
use std::sync::LazyLock;

/// The full decode of `CORE_DESIGN_PRODUCT`, shared by the tests that only
/// read it or edit a clone of it.
static CORE_DESIGN: LazyLock<cadmpeg_ir::DecodeResult> =
    LazyLock::new(|| decode_at(DecodeDepth::Full));

#[test]
pub(crate) fn write_target_and_source_requirements_are_explicit() {
    let decoded = &*CORE_DESIGN;
    let unsupported = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None)
                .with_target(TargetRequest::Explicit("fcstd:schema-3")),
        )
        .expect_err("unsupported target must fail");
    let EncodeError::UnsupportedTarget(refusal) = &unsupported else {
        panic!("expected a target refusal, got {unsupported}");
    };
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(
        ({
            let wire = serde_json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref(),
        Some("fcstd:schema-3")
    );

    // A STEP document has no retained FCStd graph this writer can patch.
    // The catalog intentionally has no cross-format default, so `plan`
    // refuses without pretending schema 4 was requested and then naming it
    // as both requested and available.
    let mut step_source = cadmpeg_ir::CadIr::empty();
    step_source.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("step:ap242"),
        )),
        std::collections::BTreeMap::new(),
    ));
    let missing_graph = FcstdCodec
        .plan(EncodeInput::new(&step_source, None))
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("missing graph must fail");
    let EncodeError::UnsupportedTarget(refusal) = &missing_graph else {
        panic!("expected a target refusal, got {missing_graph}");
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("serialize refusal")["refusal"],
        serde_json::json!({"kind": "no_default", "source": {"kind": "foreign_format", "value": "step"}})
    );
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(
        missing_graph.to_string(),
        "fcstd cannot inherit a write target from source format step: this encoder declares no cross-format default; available targets: fcstd:schema-4"
    );
}

/// A schema-2 `Document.xml`, in the `Features`/`FeatureData` vocabulary
/// that schema declares, wrapped in an archive.
fn schema_two_archive() -> Vec<u8> {
    archive(
        r#"<Document SchemaVersion="2" ProgramVersion="0.13">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Document"/></Property></Properties>
<Features Count="1"><Feature type="App::Feature" name="First"/></Features>
<FeatureData Count="1"><Feature name="First"><Properties Count="0"/></Feature></FeatureData>
</Document>"#,
    )
}

fn inherit(
    ir: &CadIr,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, EncodeError> {
    Encoder::plan(&FcstdCodec, EncodeInput::new(ir, fidelity))
}

/// Names every archive entry with its payload, so preservation can be
/// asserted against the source rather than against the writer's own output.
fn entry_payloads(archive: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive.to_vec())).expect("readable ZIP");
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).expect("archive entry");
            let name = entry.name().expect("entry name").into_owned();
            let mut payload = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut payload).expect("inflate entry");
            (name, payload)
        })
        .collect()
}

/// `Inherit` on a schema-4 source states schema 4 and writes every retained
/// entry back byte for byte, `Document.xml` included.
///
/// This is the catalog dialect, and the report states it.
#[test]
fn inherit_preserves_a_schema_four_source_entry_for_entry() {
    let decoded = &*CORE_DESIGN;
    let plan =
        inherit(decoded.ir(), Some(decoded.source_fidelity())).expect("schema 4 is preserved");

    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:schema-4".to_owned())
    );
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("write");
    assert_eq!(
        entry_payloads(&written),
        entry_payloads(CORE_DESIGN_PRODUCT)
    );
}

/// The canonical §8.2 case, in its preserving half: a schema-2 source with a
/// usable retained document graph writes back as schema 2.
///
/// `fcstd:schema-2` is not in `targets()` and never will be — this writer
/// regenerates no `Document.xml`. Preservation is the other capability, and
/// it reaches every dialect the codec reads. Before the resolution existed,
/// `plan` once hardcoded schema 4, and this source was either rewritten as
/// schema 4 or refused.
#[test]
fn inherit_preserves_a_schema_two_source_outside_the_catalog() {
    let source = schema_two_archive();
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("decode schema 2");
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.dialect())
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    assert!(
        Encoder::targets(&FcstdCodec)
            .find("fcstd:schema-2")
            .is_none(),
        "schema 2 is preserved, never synthesized"
    );

    let plan =
        inherit(decoded.ir(), Some(decoded.source_fidelity())).expect("schema 2 is preserved");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("write");
    assert_eq!(entry_payloads(&written), entry_payloads(&source));

    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("decode output");
    assert_eq!(
        round_trip
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.dialect())
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
}

/// A declaration outside the registry remains writable when the retained
/// document graph proves the exact residual identity. Preservation does not
/// reinterpret `"04"` as the registered schema-4 declaration.
#[test]
fn inherit_preserves_an_unknown_schema_declaration_exactly() {
    let source = rewrite_schema_version(CORE_DESIGN_PRODUCT, "04");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("decode the residual declaration");
    assert_eq!(
        decoded
            .report()
            .dialects()
            .unwrap()
            .primary()
            .dialect()
            .as_str(),
        "fcstd:unknown"
    );
    assert_eq!(
        decoded.report().dialects().unwrap().primary().declared()["schema_version"],
        "04"
    );

    let plan = inherit(decoded.ir(), Some(decoded.source_fidelity()))
        .expect("the retained residual dialect is preservable");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:unknown".to_owned())
    );
    let mut written = Vec::new();
    plan.write_to(&mut written)
        .expect("write the retained graph");
    assert_eq!(entry_payloads(&written), entry_payloads(&source));

    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("decode the preserved residual declaration");
    let primary = round_trip.report().dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "fcstd:unknown");
    assert_eq!(primary.declared()["schema_version"], "04");
}

/// The canonical §8.2 case, in its refusing half: a schema-2 source whose
/// retained document graph cannot be written back is refused, not quietly
/// rewritten as schema 4.
///
/// There is no fall-through to the catalog default. The refusal names the
/// source's own dialect and the catalog, so the caller can reach the file
/// with an explicit `--to` from the message alone.
#[test]
fn inherit_refuses_a_schema_two_source_with_no_usable_baseline() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(schema_two_archive()),
            &DecodeOptions::default(),
        )
        .expect("decode schema 2");
    let (mut ir, _, _) = decoded.into_parts();
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("fcstd"))
        .set_arena::<DocumentFacts>(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "document",
            &[],
        )
        .expect("drop the document record");

    let error = inherit(&ir, None).expect_err("a schema-2 source with no baseline is refused");
    let EncodeError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(
        ({
            let wire = serde_json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref(),
        Some("fcstd:schema-2")
    );
    assert_eq!(
        serde_json::to_value(refusal).expect("serialize refusal")["refusal"]["kind"],
        "inherited_unavailable"
    );
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "fcstd:schema-4"),
        "{:?}",
        refusal.available()
    );
}

/// An explicit `--to` is the escape from the inherit refusal only where the
/// retained graph can deliver the target. Where it cannot, `plan` refuses
/// by name, with the catalog, before any byte is written.
///
/// This is where the codec's synthesis gap is visible: it patches the
/// retained `Document.xml` and regenerates none, so schema 2 to schema 4 is
/// a transcode it cannot perform at any request. A degraded schema-4 write
/// built from schema-2 records is not the alternative — there is no
/// synthesis path to degrade to, so the honest answer is the same typed
/// refusal every sibling gives, not a bare message string from deep inside
/// `write`.
#[test]
fn an_explicit_schema_four_target_refuses_a_schema_two_source_by_name() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(schema_two_archive()),
            &DecodeOptions::default(),
        )
        .expect("decode schema 2");
    let error = Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(decoded.ir(), None).with_target(TargetRequest::Explicit("fcstd:schema-4")),
    )
    .expect_err("this writer regenerates no Document.xml");
    let EncodeError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(
        ({
            let wire = serde_json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref(),
        Some("fcstd:schema-4")
    );
    assert_eq!(
        serde_json::to_value(refusal).expect("serialize refusal")["refusal"]["kind"],
        "explicit_unavailable"
    );
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "fcstd:schema-4"),
        "{:?}",
        refusal.available()
    );
}

/// An `FCStd` source that records no dialect refuses `Inherit`, uniformly
/// with every other encoder, and quotes no dialect id because none exists.
#[test]
fn inherit_refuses_a_source_that_records_no_dialect() {
    let mut ir = CORE_DESIGN.ir().clone();
    let source = ir.source.take().expect("the decode records a source");
    let format = source.format().to_owned();
    ir.source = Some(
        serde_json::from_value(serde_json::json!({
            "identity": {"kind": "unclassified", "format": format},
            "attributes": source.attributes,
        }))
        .unwrap(),
    );

    let error = inherit(&ir, None).expect_err("a source with no recorded dialect is refused");
    let EncodeError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(
        ({
            let wire = serde_json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref(),
        None
    );
    assert_eq!(
        serde_json::to_value(refusal).expect("serialize refusal")["refusal"]["kind"],
        "unrecorded_source"
    );
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "fcstd:schema-4"),
        "{:?}",
        refusal.available()
    );
}

/// The §8.3 honesty invariant on this codec's only write path: re-decoding
/// the output classifies the host layer into exactly the dialect the report
/// named.
///
/// The assertion is against the bytes, not against the report twice, and
/// not against entry payloads. `target` comes from the resolution's write
/// options; the `SchemaVersion` in the output comes from the retained
/// `Document.xml`, which this writer patches and never regenerates. Those
/// values originate in the same native scan; the CADIR reader rejects a wire
/// document that makes them disagree, and the writer consumes the resolved
/// source witness without classifying the retained record again. Both bands
/// are covered: the catalog dialect and the schema-2 dialect that is preserved
/// but never synthesized.
#[test]
fn every_preserved_write_re_decodes_as_the_dialect_the_report_named() {
    let schema_two = schema_two_archive();
    for (label, source) in [
        ("schema 4", CORE_DESIGN_PRODUCT),
        ("schema 2", schema_two.as_slice()),
    ] {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source.to_vec()), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{label} source must decode, got {error}"));
        let plan = inherit(decoded.ir(), Some(decoded.source_fidelity()))
            .unwrap_or_else(|error| panic!("{label} is preserved, got {error}"));
        let claimed = wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target",
        )
        .clone()
        .expect("an FCStd write always names its dialect");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("the plan writes");

        let round_trip = FcstdCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{label} output must decode, got {error}"));
        let classified = round_trip
            .report()
            .dialects()
            .unwrap_or_else(|| panic!("{label} output must classify a host dialect"))
            .primary()
            .dialect()
            .clone();
        assert_eq!(
            classified, claimed,
            "{label}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}

/// A sidecar that marks a retained record `untyped` patches as an unmarked
/// one does, and the report carries both carried-source codes naming the
/// record.
#[test]
fn an_untyped_retained_record_is_patched_and_reported() {
    let decoded = decode_at(DecodeDepth::Full);
    let (ir, _, mut fidelity) = decoded.into_parts();
    let write = |fidelity: &cadmpeg_ir::SourceFidelity| {
        let plan = Encoder::plan(&FcstdCodec, EncodeInput::new(&ir, Some(fidelity)))
            .expect("the retained document patches");
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).expect("the patch writes");
        (report, bytes)
    };
    let (unmarked, unmarked_bytes) = write(&fidelity);
    assert!(matches!(
        unmarked.write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    assert!(!unmarked
        .losses
        .iter()
        .any(|loss| loss.code.namespace() == "export"));
    fidelity
        .insert_retained_record(
            "fcstd:test:skipped#0"
                .to_owned()
                .try_into()
                .expect("record id"),
            cadmpeg_ir::RetainedSourceRecord::whole("", b"skipped".to_vec())
                .with_untyped([cadmpeg_core::decode::ArenaClass::Design]),
        )
        .expect("a new record id");
    let (marked, marked_bytes) = write(&fidelity);
    assert_eq!(marked_bytes, unmarked_bytes);
    let carried: Vec<_> = marked
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export")
        .map(|loss| (loss.code.local_code(), loss.severity, loss.message.as_str()))
        .collect();
    assert_eq!(carried.len(), 2, "{carried:?}");
    assert_eq!(carried[0].0, "source.carried");
    assert_eq!(carried[0].1, cadmpeg_ir::report::Severity::Info);
    assert_eq!(carried[1].0, "source.unchecked");
    assert_eq!(carried[1].1, cadmpeg_ir::report::Severity::Warning);
    assert!(carried
        .iter()
        .all(|(_, _, message)| message.contains("fcstd:test:skipped#0 (design)")));
}

fn decode_at(depth: DecodeDepth) -> cadmpeg_ir::DecodeResult {
    FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions {
                depth,
                ..DecodeOptions::default()
            },
        )
        .expect("decode source")
}

fn recorded_digest(
    decoded: &cadmpeg_ir::DecodeResult,
) -> Option<&cadmpeg_ir::hash::digest::Sha256Digest> {
    decoded
        .source_fidelity()
        .codec_digests
        .get(UNCARRIED_DIGEST)
}

#[test]
fn only_a_full_decode_records_the_digest() {
    let full = &*CORE_DESIGN;
    assert_eq!(full.ir().scope(), DecodeDepth::Full.scope());
    assert!(recorded_digest(full).is_some());
    for depth in [DecodeDepth::Geometry, DecodeDepth::Container] {
        let decoded = decode_at(depth);
        assert_eq!(decoded.ir().scope(), depth.scope());
        assert_eq!(recorded_digest(&decoded), None, "{depth}");
    }
}

/// Writes `ir` through `Inherit` and returns the report and the file.
fn patched(
    ir: &CadIr,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
) -> (cadmpeg_ir::report::export::ExportReport, Vec<u8>) {
    let mut bytes = Vec::new();
    let report = inherit(ir, fidelity)
        .expect("the retained document patches")
        .write_to(&mut bytes)
        .expect("the patch writes");
    assert!(matches!(
        report.write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    (report, bytes)
}

/// The `export:` losses of `report`, as code and severity.
fn export_losses(
    report: &cadmpeg_ir::report::export::ExportReport,
) -> Vec<(&str, cadmpeg_ir::report::Severity)> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export")
        .map(|loss| (loss.code.local_code(), loss.severity))
        .collect()
}

/// A `Geometry` document records no digest, so its edits are not known. The
/// writer patches the retained document as it patches the unedited `Full` one
/// and reports `export:edit.unverified`, edited or not.
#[test]
fn a_geometry_document_is_patched_and_its_edits_reported_unverified() {
    let (full_report, full_bytes) = patched(CORE_DESIGN.ir(), Some(CORE_DESIGN.source_fidelity()));
    assert_eq!(export_losses(&full_report), []);
    let decoded = decode_at(DecodeDepth::Geometry);
    let mut edited = decoded.ir().clone();
    edited
        .source
        .as_mut()
        .expect("the fixture records a source")
        .attributes
        .insert(
            cadmpeg_core::nonblank_const!("edited_since_decode"),
            "yes".to_owned(),
        );
    for ir in [decoded.ir(), &edited] {
        let (report, bytes) = patched(ir, Some(decoded.source_fidelity()));
        assert_eq!(bytes, full_bytes);
        assert_eq!(
            export_losses(&report),
            [("edit.unverified", cadmpeg_ir::report::Severity::Warning)]
        );
    }
}

/// A `Container` document retains no archive entry, so this writer, which has
/// no synthesis path, holds no source bytes to write.
#[test]
fn a_container_document_is_an_unsupported_target() {
    let decoded = decode_at(DecodeDepth::Container);
    let error =
        inherit(decoded.ir(), Some(decoded.source_fidelity())).expect_err("no entry is retained");
    let EncodeError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "fcstd");
}

/// A `Full` document whose sidecar lost its digest cannot prove that it is unedited:
/// it is patched and reported as a document below `Full` is.
#[test]
fn a_full_document_without_its_digest_is_patched_and_reported_unverified() {
    let (_, unedited) = patched(CORE_DESIGN.ir(), Some(CORE_DESIGN.source_fidelity()));
    let mut stripped = CORE_DESIGN.source_fidelity().clone();
    assert!(stripped.codec_digests.remove(UNCARRIED_DIGEST).is_some());
    let (report, bytes) = patched(CORE_DESIGN.ir(), Some(&stripped));
    assert_eq!(
        bytes, unedited,
        "the stripped document patches as the unedited one"
    );
    assert_eq!(
        export_losses(&report),
        [("edit.unverified", cadmpeg_ir::report::Severity::Warning)]
    );
}

/// The writer patches retained native records only, so a neutral edit has no
/// serializer. It is refused rather than written as the unedited source.
#[test]
fn a_neutral_edit_is_refused_and_the_unedited_document_still_writes() {
    let decoded = &*CORE_DESIGN;
    let mut control = Vec::new();
    inherit(decoded.ir(), Some(decoded.source_fidelity()))
        .expect("an unedited document is preserved")
        .write_to(&mut control)
        .expect("write control");
    assert_eq!(
        entry_payloads(&control),
        entry_payloads(CORE_DESIGN_PRODUCT)
    );

    let mut edited = decoded.ir().clone();
    let body = edited
        .geometry
        .bodies
        .first_mut()
        .expect("the fixture decodes a body");
    body.name = Some(format!(
        "{} renamed",
        body.name.as_deref().unwrap_or("body")
    ));
    let Err(EncodeError::Backend(CodecError::NotImplemented(message))) =
        inherit(&edited, Some(decoded.source_fidelity()))
    else {
        panic!("a neutral edit must be refused");
    };
    assert!(
        message.contains("neutral or uncarried native edits"),
        "{message}"
    );

    let mut rewritten = Vec::new();
    inherit(decoded.ir(), Some(decoded.source_fidelity()))
        .expect("the unedited document is still preserved")
        .write_to(&mut rewritten)
        .expect("write unedited");
    assert_eq!(rewritten, control);
}
