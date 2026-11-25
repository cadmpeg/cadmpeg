// SPDX-License-Identifier: Apache-2.0
//! Resolution of a write request against the source: the synthesis catalog,
//! preservation, and the refusals.

use cadmpeg_test_support::wire;

use crate::native::DocumentFacts;
use crate::test_support::test_archive::{archive, rewrite_schema_version, CORE_DESIGN_PRODUCT};
use crate::writer::target::UNCARRIED_DIGEST;
use crate::FcstdCodec;
use cadmpeg_core::decode::DecodeDepth;
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
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("fcstd:schema-9"),
        )
        .expect_err("a target that names no catalog or replay row is refused");
    let EncodeError::UnsupportedTarget(refusal) = &unsupported else {
        panic!("expected a target refusal, got {unsupported}");
    };
    assert_eq!(refusal.format(), "fcstd");
    assert_eq!(
        ({
            let wire =
                cadmpeg_core::serde_erase::json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref(),
        Some("fcstd:schema-9")
    );
    assert_eq!(
        serde_json::to_value(refusal).expect("serialize refusal")["refusal"]["kind"],
        "unknown_explicit"
    );

    // A STEP document with no retained FCStd graph gives this writer, which
    // has no synthesis path, nothing to write.
    let mut step_source = cadmpeg_ir::CadIr::empty();
    step_source.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("step:ap242"),
        )),
        std::collections::BTreeMap::new(),
    ));
    let missing_graph = FcstdCodec
        .plan(EncodeInput::new(&step_source, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("missing graph must fail");
    let EncodeError::UnsupportedTarget(refusal) = &missing_graph else {
        panic!("expected a target refusal, got {missing_graph}");
    };
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(refusal).expect("serialize refusal")["refusal"],
        serde_json::json!({
            "kind": "default_unavailable",
            "target": "fcstd:schema-4",
            "source": {"kind": "foreign_format", "value": "step"},
            "reason": "the document holds no FCStd archive entry with a readable Document.xml, \
                       and this writer regenerates no Document.xml, so the target cannot be \
                       written",
        })
    );
    assert_eq!(refusal.format(), "fcstd");

    // An explicit replay row that the document cannot reach is refused
    // naming that row.
    let replay = FcstdCodec
        .plan(
            EncodeInput::new(&step_source, None),
            TargetRequest::Explicit("fcstd:schema-2"),
        )
        .expect_err("a replay row with no retained Document.xml is refused");
    let EncodeError::UnsupportedTarget(refusal) = &replay else {
        panic!("expected a target refusal, got {replay}");
    };
    let wire = serde_json::to_value(refusal).expect("serialize refusal");
    assert_eq!(
        (
            &wire["refusal"]["kind"],
            &wire["refusal"]["target"],
            &wire["refusal"]["requested"],
        ),
        (
            &serde_json::json!("explicit_unavailable"),
            &serde_json::json!("fcstd:schema-2"),
            &serde_json::json!("fcstd:schema-2"),
        )
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
    Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(ir, fidelity),
        TargetRequest::Inherit,
    )
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

/// The retained `Document.xml` is the baseline: a schema-2 source whose
/// `document` record is gone still holds the bytes, which state its
/// declaration, so the write preserves schema 2 entry for entry.
#[test]
fn inherit_writes_a_schema_two_source_without_its_document_record() {
    let source = schema_two_archive();
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
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

    let mut written = Vec::new();
    let report = inherit(&ir, None)
        .expect("the retained Document.xml declares schema 2")
        .write_to(&mut written)
        .expect("write the retained archive");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            &report,
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    assert_eq!(entry_payloads(&written), entry_payloads(&source));
}

/// The source bytes are the `fcstd` archive entries the document holds,
/// whatever format `ir.source` names. A document read from another format
/// that holds them is written as the schema its `Document.xml` declares, and
/// `target.request-displaced` names both where the request names another.
#[test]
fn an_explicit_target_writes_fcstd_entries_under_a_source_of_another_format() {
    let step_source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("step:ap242"),
        )),
        std::collections::BTreeMap::new(),
    ));
    let explicit = |ir: &CadIr| {
        Encoder::plan(
            &FcstdCodec,
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
    };

    let mut schema_four = CORE_DESIGN.ir().clone();
    schema_four.source.clone_from(&step_source);
    let mut written = Vec::new();
    explicit(&schema_four)
        .expect("the retained Document.xml declares schema 4")
        .write_to(&mut written)
        .expect("write the retained archive");
    assert_eq!(
        entry_payloads(&written),
        entry_payloads(CORE_DESIGN_PRODUCT)
    );

    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(schema_two_archive()),
            &DecodeOptions::default(),
        )
        .expect("decode schema 2");
    let mut schema_two = decoded.ir().clone();
    schema_two.source = step_source;
    let plan = explicit(&schema_two).expect("the retained Document.xml declares schema 2");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    let charged: Vec<_> = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code().starts_with("target."))
        .map(|loss| (loss.code.local_code(), loss.message.as_str()))
        .collect();
    let [(code, message)] = charged.as_slice() else {
        panic!("expected one target loss, got {charged:?}");
    };
    assert_eq!(*code, "target.request-displaced");
    assert!(
        message.contains("fcstd:schema-4") && message.contains("fcstd:schema-2"),
        "{message}"
    );
}

/// An explicit `--to` that the retained graph cannot deliver is written as the
/// dialect the retained graph declares (§3.4, Refusals).
///
/// This writer patches the retained `Document.xml` and regenerates none, so a
/// schema-2 source asked for schema 4 is written as schema 2. The report names
/// that dialect, `fcstd/target.request-displaced` names both, and the output
/// decodes back as schema 2.
#[test]
fn an_explicit_schema_four_target_writes_a_schema_two_source_as_schema_two() {
    let source = schema_two_archive();
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("decode schema 2");
    let plan = Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(decoded.ir(), None),
        TargetRequest::Explicit("fcstd:schema-4"),
    )
    .expect("the retained schema-2 document is written");
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).expect("write");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            &report,
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    let displaced: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code().starts_with("target."))
        .map(|loss| {
            (
                loss.code.namespace(),
                loss.code.local_code(),
                loss.severity,
                loss.message.as_str(),
            )
        })
        .collect();
    let [(namespace, code, severity, message)] = displaced.as_slice() else {
        panic!("expected one target loss, got {displaced:?}");
    };
    assert_eq!(
        (*namespace, *code, *severity),
        (
            "fcstd",
            "target.request-displaced",
            cadmpeg_ir::report::Severity::Warning
        )
    );
    assert!(
        message.contains("fcstd:schema-4") && message.contains("fcstd:schema-2"),
        "{message}"
    );
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

/// Writes `ir` for `request` and decodes the file. Returns the dialect the
/// output reads back as, which the report names as its target, and the
/// `target.*` losses of the report, as code and message.
fn write_and_read_dialect(
    ir: &CadIr,
    request: TargetRequest<'_>,
) -> (String, Vec<(String, String)>) {
    let mut written = Vec::new();
    let report = Encoder::plan(&FcstdCodec, EncodeInput::new(ir, None), request)
        .expect("the retained Document.xml is written")
        .write_to(&mut written)
        .expect("write");
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("decode output");
    let read = round_trip
        .ir()
        .source
        .as_ref()
        .and_then(|source| source.dialect())
        .map(cadmpeg_core::dialect::DialectMatch::dialect)
        .map(ToString::to_string)
        .expect("the output records its dialect");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            &report,
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string)
        .as_deref(),
        Some(read.as_str()),
        "the report names the dialect the file reads back as"
    );
    let charged = report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code().starts_with("target."))
        .map(|loss| (loss.code.local_code().to_owned(), loss.message.clone()))
        .collect();
    (read, charged)
}

/// An explicit replay row names a schema this writer reaches by patching a
/// retained `Document.xml` that declares it. A source of that schema is
/// written as it, and the report charges no target loss.
#[test]
fn an_explicit_replay_row_writes_a_source_of_that_schema() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(schema_two_archive()),
            &DecodeOptions::default(),
        )
        .expect("decode schema 2");
    for token in ["fcstd:schema-2", "schema-2"] {
        assert_eq!(
            write_and_read_dialect(decoded.ir(), TargetRequest::Explicit(token)),
            ("fcstd:schema-2".to_owned(), vec![]),
            "{token}"
        );
    }
}

/// A same-format source asked for a replay row of another schema is written
/// as the schema its retained `Document.xml` declares, and
/// `target.request-displaced` names the requested and the written dialect
/// (§3.4, Refusals).
#[test]
fn an_explicit_replay_row_of_another_schema_writes_the_declared_schema() {
    let (read, charged) =
        write_and_read_dialect(CORE_DESIGN.ir(), TargetRequest::Explicit("fcstd:schema-3"));
    assert_eq!(read, "fcstd:schema-4");
    let [(code, message)] = charged.as_slice() else {
        panic!("expected one target loss, got {charged:?}");
    };
    assert_eq!(code, "target.request-displaced");
    assert!(
        message.contains("fcstd:schema-3") && message.contains("fcstd:schema-4"),
        "{message}"
    );
}

/// A schema-3 source is written as schema 3, under `Inherit` and under an
/// explicit `fcstd:schema-3`, and charges no target loss.
#[test]
fn a_schema_three_source_is_written_as_schema_three() {
    let ir = FcstdCodec
        .decode(
            &mut Cursor::new(rewrite_schema_version(CORE_DESIGN_PRODUCT, "3")),
            &DecodeOptions::default(),
        )
        .expect("decode schema 3")
        .ir()
        .clone();
    for request in [
        TargetRequest::Inherit,
        TargetRequest::Explicit("fcstd:schema-3"),
    ] {
        assert_eq!(
            write_and_read_dialect(&ir, request),
            ("fcstd:schema-3".to_owned(), vec![])
        );
    }
}

/// A document of another source format, or of none, that holds `fcstd`
/// archive entries is written under `Inherit` as the schema its retained
/// `Document.xml` declares. That schema is not the newest row `Inherit`
/// resolved to, so the one target loss is `target.source-dialect-displaced`
/// naming both.
#[test]
fn inherit_writes_fcstd_entries_under_a_source_of_another_format_as_their_schema() {
    let mut ir = FcstdCodec
        .decode(
            &mut Cursor::new(schema_two_archive()),
            &DecodeOptions::default(),
        )
        .expect("decode schema 2")
        .ir()
        .clone();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("step:ap242"),
        )),
        std::collections::BTreeMap::new(),
    ));
    let (read, charged) = write_and_read_dialect(&ir, TargetRequest::Inherit);
    assert_eq!(read, "fcstd:schema-2");
    let [(code, message)] = charged.as_slice() else {
        panic!("expected one target loss, got {charged:?}");
    };
    assert_eq!(code, "target.source-dialect-displaced");
    assert!(
        message.contains("source is step")
            && message.contains("resolved to fcstd:schema-4")
            && message.contains("written as fcstd:schema-2"),
        "{message}"
    );

    ir.source = None;
    let (read, charged) = write_and_read_dialect(&ir, TargetRequest::Inherit);
    assert_eq!(read, "fcstd:schema-2");
    let [(code, message)] = charged.as_slice() else {
        panic!("expected one target loss, got {charged:?}");
    };
    assert_eq!(code, "target.source-dialect-displaced");
    assert!(
        message.contains("records no source")
            && message.contains("resolved to fcstd:schema-4")
            && message.contains("written as fcstd:schema-2"),
        "{message}"
    );
}

/// `Inherit` on an `FCStd` source that records no dialect writes the
/// retained `Document.xml` as the dialect it declares (§3.4, Refusals: a
/// writer refuses only where it can write no file). The report names that
/// dialect, and the one target loss is `fcstd/target.source-dialect-displaced`.
#[test]
fn inherit_writes_a_source_that_records_no_dialect_as_its_declared_dialect() {
    let source = schema_two_archive();
    let mut ir = FcstdCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("decode schema 2")
        .ir()
        .clone();
    let meta = ir.source.take().expect("the decode records a source");
    ir.source = Some(
        cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
            "identity": {"kind": "unclassified", "format": meta.format()},
            "attributes": meta.attributes,
        }))
        .unwrap(),
    );

    let mut written = Vec::new();
    let report = inherit(&ir, None)
        .expect("a source with no recorded dialect is written")
        .write_to(&mut written)
        .expect("write");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            &report,
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    let charged: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code().starts_with("target."))
        .map(|loss| (loss.code.local_code(), loss.message.as_str()))
        .collect();
    let [(code, message)] = charged.as_slice() else {
        panic!("expected one target loss, got {charged:?}");
    };
    assert_eq!(*code, "target.source-dialect-displaced");
    assert!(
        message.contains("records no dialect") && message.contains("fcstd:schema-2"),
        "{message}"
    );
    assert_eq!(entry_payloads(&written), entry_payloads(&source));
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
        let plan = Encoder::plan(
            &FcstdCodec,
            EncodeInput::new(&ir, Some(fidelity)),
            TargetRequest::Inherit,
        )
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

/// A held uncarried digest that no longer computes shows an edit of a record
/// it covers, which the writer has no serializer for. The document is
/// patched, and `export:edit.dropped` at Error names the document and the
/// record the digest does not read (§3.4, Paths item 4).
#[test]
fn a_document_whose_held_digest_does_not_compute_reports_the_edit_dropped() {
    let mut ir = CORE_DESIGN.ir().clone();
    let unknown = crate::native::native_id("unknown", "unread");
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("fcstd"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "unknowns",
            &[serde_json::json!({"id": unknown, "field": "value"})],
        )
        .expect("set the unknowns arena");
    let (report, bytes) = patched(&ir, Some(CORE_DESIGN.source_fidelity()));
    assert_eq!(
        export_losses(&report),
        [("edit.dropped", cadmpeg_ir::report::Severity::Error)]
    );
    let dropped = report
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "edit.dropped")
        .expect("edit.dropped");
    assert!(
        dropped.message.contains(unknown.as_str())
            && dropped
                .message
                .contains(crate::native::native_id("document", "0").as_str()),
        "{}",
        dropped.message
    );
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode output");
    assert_eq!(
        round_trip
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.dialect())
            .map(|dialect| dialect.dialect().to_string()),
        Some("fcstd:schema-4".to_owned())
    );
}

/// The writer patches retained native records only, so a neutral edit has no
/// serializer. The file holds the document as the source stores it, and the
/// report names the document, since the digest does not isolate the edited
/// record (§3.4, Paths item 4).
#[test]
fn a_neutral_edit_is_left_as_stored_and_reported() {
    let decoded = &*CORE_DESIGN;
    let (_, control) = patched(decoded.ir(), Some(decoded.source_fidelity()));
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
    let (report, bytes) = patched(&edited, Some(decoded.source_fidelity()));
    assert_eq!(bytes, control);
    assert_eq!(
        export_losses(&report),
        [("edit.dropped", cadmpeg_ir::report::Severity::Error)]
    );
    let document = crate::native::native_id("document", "0");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(document.as_str())));
}
