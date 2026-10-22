// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_test_support::wire;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
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
        &(Encoder::plan(&encoder, EncodeInput::new(ir, None), request)
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

/// An unrecorded source selects the catalog default and names its displacement.
#[test]
fn inherit_defaults_a_source_that_records_no_dialect() {
    let mut ir = CadIr::empty();
    ir.source = Some(
        serde_json::from_value(serde_json::json!({
            "identity": {"classification": "unclassified", "format": "rhino"},
            "attributes": {},
        }))
        .unwrap(),
    );
    let plan = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect("§5.0 selects catalog-default synthesis");
    assert_default_synthesis(plan, "unrecorded");
}

/// An explicit target that is not the source archive version charges a loss.
#[test]
fn a_dialect_changing_explicit_write_charges_displacement_by_name() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("rhino:archive-70"),
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
        .find(|loss| loss.code == crate::loss::RhinoLossCode::SourceDialectDisplaced.kind())
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
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("rhino:archive-50"),
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

/// A source outside the synthesis catalog selects and reports the actual default.
#[test]
fn inherit_synthesizes_the_default_for_a_source_archive_version_outside_the_catalog() {
    let ir = source_in("rhino:archive-3");
    let encoder = RhinoCodec;
    let plan = Encoder::plan(
        &encoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect("§5.0 selects catalog-default synthesis");
    assert_default_synthesis(plan, "rhino:archive-3");
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
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#honesty").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
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
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
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

/// `displaced` is the displaced source dialect id, or `unrecorded`.
fn assert_default_synthesis(plan: cadmpeg_ir::codec::write::ExportPlan, displaced: &str) {
    use cadmpeg_ir::{Codec, DecodeOptions};
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert_eq!(
        serde_json::to_value(plan.report()).expect("report")["identity"]["target"],
        "rhino:archive-80"
    );
    let displacement: Vec<_> = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::RhinoLossCode::SourceDialectDisplaced.kind())
        .collect();
    assert_eq!(displacement.len(), 1);
    assert!(displacement[0].message.contains(displaced));
    assert!(displacement[0].message.contains("rhino:archive-80"));
    assert!(plan.report().synthesis_cause().is_none());
    assert_eq!(
        wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        cadmpeg_ir::report::export::FidelityResolution::NotProvided {}
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the actual selected target");
    let decoded = RhinoCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("written catalog-default grammar reads");
    assert_eq!(
        decoded
            .report()
            .dialects()
            .expect("native classification")
            .primary()
            .dialect()
            .as_str(),
        "rhino:archive-80"
    );
}

mod fallback;
