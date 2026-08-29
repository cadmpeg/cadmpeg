// SPDX-License-Identifier: Apache-2.0
//! Resolution of a write request against the source: the synthesis catalog,
//! preservation, and the refusals.

use super::super::target::resolve;
use super::super::*;
use crate::native::DocumentFacts;
use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::codec::{EncodeInput, TargetRequest};
use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
use std::io::Cursor;

#[test]
pub(crate) fn write_target_and_source_requirements_are_explicit() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let unsupported = FcstdCodec
        .encode_with_options(
            decoded.ir(),
            &mut Vec::new(),
            crate::FcstdWriteOptions {
                schema_version: 3,
                file_version: 1,
            },
        )
        .expect_err("unsupported target must fail");
    let CodecError::UnsupportedTarget {
        format, requested, ..
    } = &unsupported
    else {
        panic!("expected a target refusal, got {unsupported}");
    };
    assert_eq!(format, "fcstd");
    assert_eq!(requested.as_deref(), Some("fcstd:schema-3"));

    // `FileVersion` is not part of a dialect id, so the catalog cannot
    // refuse this one. The resolution's second half does, at the same
    // layer and with the same typed refusal.
    let wrong_file_version = FcstdCodec
        .encode_with_options(
            decoded.ir(),
            &mut Vec::new(),
            crate::FcstdWriteOptions {
                schema_version: 4,
                file_version: 2,
            },
        )
        .expect_err("a FileVersion the retained graph does not carry must fail");
    let CodecError::UnsupportedTarget { requested, .. } = &wrong_file_version else {
        panic!("expected a target refusal, got {wrong_file_version}");
    };
    assert_eq!(requested.as_deref(), Some("fcstd:schema-4"));

    // A document with no retained graph has nothing this writer can patch,
    // so `plan` refuses by name with the catalog rather than failing deep
    // inside `write`. The request is irrelevant to the outcome: with
    // nothing to inherit the catalog default stands in, and the retained
    // graph cannot deliver that either.
    let source_less = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let missing_graph = FcstdCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("missing graph must fail");
    let CodecError::UnsupportedTarget {
        format, requested, ..
    } = &missing_graph
    else {
        panic!("expected a target refusal, got {missing_graph}");
    };
    assert_eq!(format, "fcstd");
    assert_eq!(requested.as_deref(), Some("fcstd:schema-4"));
}

/// An explicit target this writer does not produce is refused by `plan`
/// itself, with the catalog in the message.
///
/// The check runs before any synthesis, so an empty document is enough:
/// what is under test is that the request reaches the encoder at all. This
/// writer reaches one schema through the trait, which is exactly why the
/// refusal must exist — every other id is a claim it cannot honour.
#[test]
fn plan_refuses_an_explicit_target_outside_the_catalog() {
    let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let error = Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("fcstd:nonesuch"),
    )
    .err()
    .expect("an id outside the catalog is refused");

    let cadmpeg_core::CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "fcstd");
    assert_eq!(requested.as_deref(), Some("fcstd:nonesuch"));
    for target in Encoder::targets(&FcstdCodec) {
        assert!(available.contains(target.id), "{available}");
    }
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

fn inherit(ir: &CadIr) -> Result<cadmpeg_ir::codec::ExportPlan<'_>, CodecError> {
    Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(ir, None),
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
            let name = entry.name().to_owned();
            let mut payload = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut payload).expect("inflate entry");
            (name, payload)
        })
        .collect()
}

/// `Inherit` on a schema-4 source states schema 4 and writes every retained
/// entry back byte for byte, `Document.xml` included.
///
/// This is the catalog dialect, so the resolution and the old hardcoded
/// `FcstdWriteOptions::default()` agree on what to write. What is new is
/// that the report states it.
#[test]
fn inherit_preserves_a_schema_four_source_entry_for_entry() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let plan = inherit(decoded.ir()).expect("schema 4 is preserved");

    assert_eq!(plan.write_path(), cadmpeg_ir::WritePath::Patched);
    assert_eq!(
        plan.report().target().map(ToString::to_string),
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
/// `plan` hardcoded `FcstdWriteOptions::default()` and this source was
/// either rewritten as schema 4 or refused.
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
            .and_then(|source| source.dialect.as_ref())
            .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
    assert!(
        cadmpeg_ir::codec::find_target(Encoder::targets(&FcstdCodec), "fcstd:schema-2").is_none(),
        "schema 2 is preserved, never synthesized"
    );

    let plan = inherit(decoded.ir()).expect("schema 2 is preserved");
    assert_eq!(
        plan.report().target().map(ToString::to_string),
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
            .and_then(|source| source.dialect.as_ref())
            .map(ToString::to_string),
        Some("fcstd:schema-2".to_owned())
    );
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
        .namespace_mut("fcstd")
        .set_arena("document", &[] as &[DocumentFacts])
        .expect("drop the document record");

    let error = inherit(&ir)
        .err()
        .expect("a schema-2 source with no baseline is refused");
    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "fcstd");
    assert_eq!(requested.as_deref(), Some("fcstd:schema-2"));
    assert!(available.contains("fcstd:schema-4"), "{available}");
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
    assert!(
        resolve(decoded.ir(), TargetRequest::Explicit("fcstd:schema-4")).is_err(),
        "the retained schema-2 graph cannot deliver schema 4"
    );

    let error = Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(decoded.ir(), None),
        TargetRequest::Explicit("fcstd:schema-4"),
    )
    .err()
    .expect("this writer regenerates no Document.xml");
    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "fcstd");
    assert_eq!(requested.as_deref(), Some("fcstd:schema-4"));
    assert!(available.contains("fcstd:schema-4"), "{available}");
}

/// An `FCStd` source that records no dialect refuses `Inherit`, uniformly
/// with every other encoder, and quotes no dialect id because none exists.
#[test]
fn inherit_refuses_a_source_that_records_no_dialect() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source
        .as_mut()
        .expect("the decode records a source")
        .dialect = None;

    let error = inherit(&ir)
        .err()
        .expect("a source with no recorded dialect is refused");
    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "fcstd");
    assert_eq!(*requested, None);
    assert!(available.contains("fcstd:schema-4"), "{available}");
}

/// The §8.3 honesty invariant on this codec's only write path: re-decoding
/// the output classifies the host layer into exactly the dialect the report
/// named.
///
/// The assertion is against the bytes, not against the report twice, and
/// not against entry payloads. `target` comes from the resolution's write
/// options; the `SchemaVersion` in the output comes from the retained
/// `Document.xml`, which this writer patches and never regenerates. Those
/// are two independent sources for one fact, and the equality gate in
/// `resolve` is the only thing that ties them together — disabling it makes
/// this test fail. Both bands are covered: the catalog dialect and the
/// schema-2 dialect that is preserved but never synthesized.
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
        let plan = inherit(decoded.ir())
            .unwrap_or_else(|error| panic!("{label} is preserved, got {error}"));
        let claimed = plan
            .report()
            .target()
            .cloned()
            .expect("an FCStd write always names its dialect");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("the plan writes");

        let round_trip = FcstdCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{label} output must decode, got {error}"));
        let classified = round_trip
            .report()
            .dialects
            .as_ref()
            .map(cadmpeg_core::dialect::DialectLayers::primary)
            .and_then(|entry| entry.dialect.clone())
            .unwrap_or_else(|| panic!("{label} output must classify a host dialect"));
        assert_eq!(
            classified, claimed,
            "{label}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}
