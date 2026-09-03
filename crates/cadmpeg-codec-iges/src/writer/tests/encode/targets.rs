// SPDX-License-Identifier: Apache-2.0
//! Resolution of a write request against the source: the synthesis catalog, the
//! preservation path, and the two refusals.

use super::*;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::TargetRequest;
use cadmpeg_ir::report::FidelityResolution;

/// The global of [`point_file`] with field 23 set to `flag`.
fn point_file_at_version_flag(flag: u8) -> Vec<u8> {
    let global = format!(
        "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,\
         15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{flag},0,0H,0H;"
    );
    point_file_with_global(global.as_bytes())
}

fn inherit(
    ir: &CadIr,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, CodecError> {
    IgesCodec.plan(EncodeInput::new(ir, fidelity), TargetRequest::Inherit)
}

/// The flagship case: `convert in.igs -o out.igs` on a file that is not the
/// catalog default keeps the file it was handed.
///
/// The command line builds `Inherit` for a same-format conversion, and the
/// resolved dialect is then the source's by construction, so the replay law
/// admits byte replay. Under the catalog default this file would have been
/// silently rewritten as 5.3.
#[test]
fn inherit_replays_a_non_default_version_verbatim() {
    let source = point_file_at_version_flag(9);
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();
    let plan = inherit(decoded.ir(), Some(decoded.source_fidelity())).unwrap();

    assert_eq!(plan.report().write_path, WritePath::VerbatimReplay);
    assert_eq!(
        plan.report().target().map(ToString::to_string),
        Some("iges:5.1-fixed-ascii".to_owned())
    );
    assert!(matches!(
        &plan.report().fidelity,
        FidelityResolution::Replayed
    ));
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(written, source);
}

/// With no retained image there is nothing to preserve, so `Inherit` falls to
/// the semantic writer — at the source's own version, not the catalog default.
#[test]
fn inherit_synthesizes_the_source_version_when_the_image_is_gone() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(point_file_at_version_flag(9)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = inherit(decoded.ir(), None).unwrap();

    assert_eq!(plan.report().write_path, WritePath::Synthesized);
    assert_eq!(
        plan.report().target().map(ToString::to_string),
        Some("iges:5.1-fixed-ascii".to_owned())
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::PreservedSourceUnavailable.kind()));
}

/// A dialect this codec reads but cannot write refuses under `Inherit` once its
/// retained image is gone. There is no fall-through to the catalog default: a
/// same-format conversion never silently changes what the file is, and the
/// refusal names both the source's dialect and the escape.
#[test]
fn inherit_refuses_a_source_dialect_the_writer_cannot_synthesize() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(point_file_at_version_flag(1)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect()
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("iges:1.0-fixed-ascii".to_owned())
    );

    let error = inherit(decoded.ir(), None).expect_err("1.0 is not a synthesis target");
    let CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "iges");
    assert_eq!(refusal.requested(), Some("iges:1.0-fixed-ascii"));
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "iges:5.3-fixed-ascii"),
        "{:?}",
        refusal.available()
    );
}

/// The same source is writable the moment the caller names a target the writer
/// has: an explicit request is always the escape from an inherit refusal.
#[test]
fn an_explicit_target_writes_a_source_the_catalog_cannot_inherit() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(point_file_at_version_flag(1)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).unwrap();

    assert_eq!(plan.report().write_path, WritePath::Synthesized);
    assert_eq!(
        plan.report().target().map(ToString::to_string),
        Some("iges:5.3-fixed-ascii".to_owned())
    );
    assert_eq!(&plan.report().fidelity, &FidelityResolution::NotProvided);
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SourceDialectDisplaced.kind()));
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != IgesLossCode::PreservedSourceUnavailable.kind()));
}

/// The catalog is the five Fixed ASCII rows the semantic writer emits, each
/// reachable by its bare version as well as its id. Compressed ASCII and Binary
/// are deliberately absent: no input makes the writer produce them, and they are
/// written by preserving a source image instead.
#[test]
fn the_catalog_is_the_fixed_ascii_versions_the_writer_emits() {
    let targets = IgesCodec.targets();
    assert_eq!(targets.len(), IgesVersion::ALL.len());
    for version in IgesVersion::ALL {
        assert!(targets.find(version.descriptor().id.as_str()).is_some());
        assert!(targets.find(version.name()).is_some());
    }
    assert!(targets.find("iges:5.3-compressed-ascii").is_none());
    assert!(targets.find("iges:5.3-binary").is_none());
    assert_eq!(
        targets.default().map(|(_, target)| target.id.as_str()),
        Some(IgesVersion::V5_3.descriptor().id.as_str())
    );
}

/// The §8.3 honesty invariant on the synthesis path: re-decoding the output
/// classifies the host layer into exactly the dialect the report named.
///
/// The assertion is against the bytes, not against the report twice. `target`
/// is a claim about what was written, and the only thing that can check a claim
/// about bytes is reading them back through the classifier the codec uses on
/// any other input. For IGES synthesis the whole claim rests on one gate — the
/// global section's version flag, field 23 — and disabling that gate makes this
/// test fail while every other assertion in the crate still passes.
#[test]
fn every_synthesized_target_re_decodes_as_the_dialect_the_report_named() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("cadir:model:point#honesty".into()),
        source_object: None,
        position: Point3::new(1.0, 2.0, 3.0),
    });

    for version in IgesVersion::ALL {
        let plan = IgesCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .unwrap_or_else(|error| panic!("{version:?} is a catalog row, got {error}"));
        let claimed = plan
            .report()
            .target()
            .cloned()
            .expect("an IGES write always names its dialect");
        let mut written = Vec::new();
        plan.write_to(&mut written).unwrap();

        let decoded = IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
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
