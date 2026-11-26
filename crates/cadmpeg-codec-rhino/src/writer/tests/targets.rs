// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::serde_erase::json;
use cadmpeg_test_support::wire;

use cadmpeg_ir::codec::write::{
    target::{target_loss_kind, TargetRequest, SOURCE_DIALECT_DISPLACED},
    EncodeInput, Encoder,
};
use cadmpeg_ir::document::CadIr;

use crate::RhinoCodec;

/// An empty document that a Rhino decode of `dialect` would have produced.
fn source_in(dialect: &'static str) -> CadIr {
    let mut ir = CadIr::empty();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect::DialectId::parse(dialect).expect("test id has dialect grammar"),
        )),
        std::collections::BTreeMap::new(),
    ));
    ir
}

/// The resolved target `plan` reports for one request.
fn resolved(ir: &CadIr, encoder: RhinoCodec, request: TargetRequest<'_>) -> String {
    wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
        &(Encoder::plan(&encoder, EncodeInput::new(ir, None).with_target(request))
            .expect("the request resolves")
            .report()),
        "identity/target",
    )
    .as_ref()
    .expect("a Rhino write always names its archive version")
    .as_str()
    .to_owned()
}

/// `convert old.3dm -o new.3dm` with no target flag keeps the archive version
/// the file already is.
///
/// The encoder carries no version at all, and the source is archive 50, so the
/// answer can only come from the source. This is the defect the resolution
/// closes — the round trip used to hand a Rhino 5 user a file their own Rhino
/// cannot open.
#[test]
fn inherit_resolves_to_the_source_archive_version() {
    let ir = source_in("rhino:archive-50");
    assert_eq!(
        resolved(&ir, RhinoCodec, TargetRequest::Inherit),
        "rhino:archive-50"
    );
}

/// An explicit target overrides the source: the flag is the escape from
/// preservation, and it says which archive version to write.
#[test]
fn an_explicit_target_wins_over_the_source_archive_version() {
    let ir = source_in("rhino:archive-50");
    assert_eq!(
        resolved(&ir, RhinoCodec, TargetRequest::Explicit("rhino:archive-70"),),
        "rhino:archive-70"
    );
}

/// With no Rhino source there is nothing to inherit, so `Inherit` falls back to
/// the catalog default — never to encoder state, which no longer exists.
///
/// This is the cross-format request the application and library callers send.
/// It changes no existing file's identity because there is no same-format
/// source whose identity could change.
#[test]
fn inherit_falls_back_to_the_catalog_default_with_nothing_to_inherit() {
    let ir = CadIr::empty();
    assert_eq!(
        resolved(&ir, RhinoCodec, TargetRequest::Inherit),
        "rhino:archive-80"
    );
}

/// A Rhino source that records no dialect is written as the catalog default
/// under `Inherit`, and `rhino/target.source-dialect-displaced` says the source
/// recorded none (§3.4, Refusals).
#[test]
fn inherit_writes_a_source_that_records_no_dialect_as_the_default() {
    let mut ir = CadIr::empty();
    ir.source = Some(
        json::from_value(serde_json::json!({
            "identity": {"kind": "unclassified", "format": "rhino"},
            "attributes": {},
        }))
        .unwrap(),
    );
    let message = written_as_the_default(&ir);
    assert!(message.contains("records no dialect"), "{message}");
    assert!(message.contains("rhino:archive-80"), "{message}");
}

/// Writes `ir` under `Inherit`, asserts the report and the bytes both name the
/// catalog default, and returns the message of the one displacement loss.
fn written_as_the_default(ir: &CadIr) -> String {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let plan = Encoder::plan(&RhinoCodec, EncodeInput::new(ir, None))
        .expect("the source is written as the catalog default");
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).expect("write");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            &report,
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("rhino:archive-80".to_owned())
    );
    let charged: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code().starts_with("target."))
        .collect();
    let [loss] = charged.as_slice() else {
        panic!("expected one target loss, got {charged:#?}");
    };
    assert_eq!(
        loss.code,
        target_loss_kind(
            cadmpeg_ir::loss_namespace!("rhino"),
            SOURCE_DIALECT_DISPLACED
        )
    );
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    let round_trip = RhinoCodec
        .decode(
            &mut std::io::Cursor::new(written),
            &DecodeOptions::default(),
        )
        .expect("decode output");
    assert_eq!(
        round_trip
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.dialect())
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("rhino:archive-80".to_owned())
    );
    loss.message.clone()
}

/// An explicit target that is not the source archive version charges a loss.
#[test]
fn a_dialect_changing_explicit_write_charges_displacement_by_name() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit("rhino:archive-70")),
    )
    .expect("archive 70 is in the catalog");
    assert_eq!(
        wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        cadmpeg_ir::report::export::FidelityResolution::NotProvided {}
    );
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code
                == target_loss_kind(
                    cadmpeg_ir::loss_namespace!("rhino"),
                    SOURCE_DIALECT_DISPLACED,
                )
        })
        .expect("dialect displacement is charged");
    assert!(loss.message.contains("rhino:archive-50"));
    assert!(loss.message.contains("rhino:archive-70"));
}

/// An explicit target that is the source's own archive version changes nothing,
/// so it is not degraded.
#[test]
fn an_explicit_write_at_the_source_dialect_is_not_degraded() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit("rhino:archive-50")),
    )
    .expect("archive 50 is in the catalog");
    assert_eq!(
        wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        cadmpeg_ir::report::export::FidelityResolution::NotProvided {}
    );
}

/// A source archive version outside the synthesis catalog is written as the
/// catalog default under `Inherit` (§3.4, Refusals).
///
/// Archives 2, 3, 4 and 90 decode and have no writer, and 3DM has no
/// byte-replay path that could preserve them. The displacement loss names the
/// source dialect and the row written; an explicit `--to rhino:<archive>`
/// selects another row.
#[test]
fn inherit_writes_a_source_archive_version_outside_the_catalog_as_the_default() {
    let message = written_as_the_default(&source_in("rhino:archive-3"));
    assert!(message.contains("rhino:archive-3"), "{message}");
    assert!(message.contains("rhino:archive-80"), "{message}");
}

/// The §8.3 honesty invariant on the synthesis path: re-decoding the output
/// classifies the host layer into exactly the dialect the report named.
///
/// The assertion is against the bytes, not against the report twice. `target`
/// is a claim about what was written, and the only thing that can check a claim
/// about bytes is reading them back through the classifier the codec uses on
/// any other input. For 3DM the whole claim rests on the archive version word
/// in the file header, and writing a fixed word there makes this test fail.
#[test]
fn every_synthesized_target_re_decodes_as_the_dialect_the_report_named() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let mut ir = CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#honesty").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 2.0, 3.0,
        ))
        .expect("a finite position is a point"),
        None,
    ));

    for version in [
        crate::RhinoArchiveVersion::V5,
        crate::RhinoArchiveVersion::V6,
        crate::RhinoArchiveVersion::V7,
        crate::RhinoArchiveVersion::V8,
    ] {
        let plan = Encoder::plan(
            &RhinoCodec,
            EncodeInput::new(&ir, None)
                .with_target(TargetRequest::Explicit(version.descriptor().id.as_str())),
        )
        .unwrap_or_else(|error| panic!("{version:?} is a catalog row, got {error}"));
        let claimed = wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target",
        )
        .clone()
        .expect("a Rhino write always names its archive version");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("the plan writes");

        let decoded = crate::RhinoCodec
            .decode(
                &mut std::io::Cursor::new(written),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{version:?} output must decode, got {error}"));
        let classified = decoded
            .report()
            .dialects()
            .unwrap_or_else(|| panic!("{version:?} output must classify a host dialect"))
            .primary()
            .dialect()
            .clone();
        assert_eq!(
            classified, claimed,
            "{version:?}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}
