// SPDX-License-Identifier: Apache-2.0
//! A design-intent failure keeps the geometry and product document.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};

use super::{assert_valid, decode};
use crate::decode::design_intent::inject_failure;
use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{f3d_without_brep, XREF_ROLE};
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::{
    synthetic_geometry_smbh, synthetic_geometry_with_history_smbh,
};
use crate::test_support::zip_test::{
    f3d_with_damaged_design_bulkstream, f3d_with_damaged_protein_asset, f3d_with_smbh,
    f3d_with_smbh_and_protein,
};
use crate::F3dCodec;

fn design_archive() -> Vec<u8> {
    f3d_with_smbh_and_protein(&synthetic_geometry_with_history_smbh())
}

fn malformed_design() -> CodecError {
    CodecError::malformed("injected design-intent failure")
}

fn recursion_refusal() -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        scope: cadmpeg_core::decode::LimitScope::Part,
        dimension: ResourceDimension::RecursionDepth,
        reason: ResourceFailure::BudgetExceeded,
        limit: 1,
        used: 1,
        additional: 1,
        operation: "injected design-intent recursion",
    })
}

#[test]
fn a_design_intent_failure_keeps_geometry_and_reports_one_error_loss() {
    let full = decode(design_archive());
    assert!(!f3d_native(full.ir()).asm_histories.is_empty());

    inject_failure(malformed_design);
    let contained = decode(design_archive());

    assert_eq!(contained.ir().model.bodies, full.ir().model.bodies);
    assert_eq!(contained.ir().model.faces, full.ir().model.faces);
    assert_eq!(contained.ir().model.surfaces, full.ir().model.surfaces);
    assert_eq!(
        contained.ir().model.appearances,
        full.ir().model.appearances
    );
    assert!(f3d_native(contained.ir()).asm_histories.is_empty());
    assert!(contained.ir().model.features.is_empty());
    assert!(contained.ir().model.sketches.is_empty());
    let undecoded = contained
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::DesignIntentUndecoded.kind())
        .collect::<Vec<_>>();
    assert_eq!(undecoded.len(), 1);
    assert_eq!(undecoded[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(undecoded[0]
        .message
        .contains("injected design-intent failure"));
    assert!(undecoded[0]
        .message
        .contains("dimension-companion and configuration reports were not produced"));
    assert!(full
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::DesignIntentUndecoded.kind()));
    assert_valid(&contained);
}

#[test]
fn typed_history_headers_do_not_represent_opaque_history_payload() {
    let full = decode(design_archive());
    let native = f3d_native(full.ir());
    assert!(native
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .flat_map(|state| &state.records)
        .any(|record| !record.raw_bytes.is_empty()));
    assert!(full.ir().scope.design);
    assert!(full
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
}

#[test]
fn a_cut_and_a_total_design_failure_hold_identical_prerequisites() {
    let inputs = [
        design_archive(),
        include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d").to_vec(),
    ];
    for source in inputs {
        let cut = F3dCodec
            .decode(
                &mut Cursor::new(&source),
                &DecodeOptions {
                    depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                    ..DecodeOptions::default()
                },
            )
            .expect("geometry cut delivers its prerequisites");
        inject_failure(malformed_design);
        let contained = F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("total phase-2 failure is contained");
        assert!(!cut.ir().scope.design);
        assert_eq!(cut.ir().scope, contained.ir().scope);
        assert_eq!(
            contained.report().scope(),
            cadmpeg_ir::document::ContentScope::geometry()
        );
        assert_eq!(cut.ir().model, contained.ir().model);
        assert_eq!(cut.ir().native, contained.ir().native);
        assert_eq!(cut.source_fidelity(), contained.source_fidelity());
        assert!(contained
            .source_fidelity()
            .unrepresented()
            .contains(&cadmpeg_ir::document::ArenaClass::Design,));
        assert_eq!(
            contained
                .ir()
                .native
                .namespace("f3d")
                .expect("F3D native namespace")
                .arenas()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            crate::native::F3D_ARENA_NAMES
        );
        assert!(contained.report().losses.iter().any(|loss| {
            loss.code.local_code() == "depth.not-delivered" && loss.message.contains("design")
        }));
    }
}

#[test]
fn strict_mode_refuses_a_contained_design_intent_failure() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    inject_failure(malformed_design);

    let failure = F3dCodec
        .decode(&mut Cursor::new(design_archive()), &options)
        .expect_err("strict mode refuses the design-intent loss");

    let DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(
        rejection.loss().code,
        F3dLossCode::DesignIntentUndecoded.kind()
    );
}

fn moved_first_point(ir: &cadmpeg_ir::document::CadIr) -> cadmpeg_ir::document::CadIr {
    let mut edited = ir.clone();
    let moved = edited.model.points[0].position().get();
    edited.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 1.0,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    edited
}

#[test]
fn cut_and_failed_design_are_never_resurrected_by_native_write_back() {
    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::document::ArenaClass;
    use cadmpeg_ir::report::export::WritePath;
    for depth in [DecodeDepth::Geometry, DecodeDepth::Full] {
        let source = design_archive();
        if depth == DecodeDepth::Full {
            inject_failure(malformed_design);
        }
        let result = F3dCodec
            .decode(
                &mut Cursor::new(&source),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .expect("the held geometry is delivered");
        assert!(!result.ir().scope.design);
        assert!(result
            .source_fidelity()
            .unrepresented()
            .contains(&ArenaClass::Design));
        assert!(!EncodeInput::new(result.ir(), Some(result.source_fidelity())).can_reuse_source());
        for edited in [result.ir().clone(), moved_first_point(result.ir())] {
            let plan = F3dCodec
                .plan(
                    EncodeInput::new(&edited, Some(result.source_fidelity())),
                    TargetRequest::Inherit,
                )
                .expect("a declined patch synthesizes held content");
            assert!(matches!(
                plan.report().write_path(),
                WritePath::Synthesized { .. }
            ));
            assert!(
                matches!(plan.report().synthesis_cause(), Some(cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented { classes }) if classes.contains(&ArenaClass::Design))
            );
            let mut output = Vec::new();
            plan.write_to(&mut output)
                .expect("write synthesized archive");
            assert_ne!(source, output);
            let reloaded = F3dCodec
                .decode(&mut Cursor::new(output), &DecodeOptions::default())
                .expect("synthesized archive decodes");
            assert_eq!(reloaded.ir().model.bodies.len(), edited.model.bodies.len());
            assert_eq!(reloaded.ir().model.faces.len(), edited.model.faces.len());
            assert!(reloaded.ir().model.features.is_empty());
            assert!(reloaded.ir().model.sketches.is_empty());
            assert!(f3d_native(reloaded.ir()).asm_histories.is_empty());
        }
    }
}

#[test]
fn a_bodyless_design_intent_failure_keeps_the_xref_product_structure() {
    let archive = || f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let full = decode(archive());
    assert!(!f3d_native(full.ir()).xref_references.is_empty());

    inject_failure(malformed_design);
    let contained = decode(archive());

    assert_eq!(
        f3d_native(contained.ir()).xref_references,
        f3d_native(full.ir()).xref_references
    );
    assert_eq!(
        f3d_native(contained.ir()).xref_designs,
        f3d_native(full.ir()).xref_designs
    );
    assert_eq!(
        contained
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::DesignIntentUndecoded.kind())
            .count(),
        1
    );
    assert_valid(&contained);
}

fn native_findings(ir: &cadmpeg_ir::document::CadIr) -> Vec<String> {
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, ir).expect("validate the native namespace")
    })
    .into_iter()
    .map(|finding| finding.message)
    .collect()
}

/// A full decode whose design intent fails keeps the product scopes and
/// owners that design intent binds. Its scope tells native validation that
/// the design bindings are absent.
#[test]
fn native_validation_skips_design_intent_links_after_a_contained_failure() {
    let source =
        include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d").to_vec();
    inject_failure(malformed_design);
    let contained = decode(source.clone());

    assert!(!contained.ir().scope.design);
    assert!(contained
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
    assert_eq!(native_findings(contained.ir()), Vec::<String>::new());
    let full = decode(source);
    assert!(full.ir().scope.design);
    assert_eq!(native_findings(full.ir()), Vec::<String>::new());
}

#[test]
fn a_resource_refusal_in_design_intent_still_refuses_the_document() {
    inject_failure(recursion_refusal);

    let failure = F3dCodec
        .decode(
            &mut Cursor::new(design_archive()),
            &DecodeOptions::default(),
        )
        .expect_err("a resource refusal is not contained");

    assert!(
        matches!(
            &failure,
            DecodeFailure::Codec(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
        ),
        "{failure:?}"
    );
}

/// E3 branch (iii): geometry only. Clearing design and restricting scope
/// synthesizes the held bodies without the failed design intent.
#[test]
fn geometry_only_scope_synthesizes_held_bodies_without_failed_design() {
    use cadmpeg_ir::codec::write::Encoder as _;
    use cadmpeg_ir::document::{ArenaClass, ArenaName, ContentScope};
    use cadmpeg_ir::report::export::WritePath;
    inject_failure(malformed_design);
    let contained = decode(design_archive());
    assert!(contained
        .source_fidelity()
        .unrepresented()
        .contains(&ArenaClass::Design));
    let mut value = serde_json::to_value(contained.ir()).expect("design document JSON");
    for arena in ArenaName::ALL {
        if arena.class() == ArenaClass::Design {
            value["model"][arena.as_str()] = serde_json::json!([]);
        }
    }
    let mut cleared: cadmpeg_ir::document::CadIr =
        serde_json::from_value(value).expect("geometry-only document");
    cleared.native = cadmpeg_ir::native::Native::default();
    cleared.set_scope(ContentScope::geometry()).unwrap();
    let plan = F3dCodec
        .plan(
            EncodeInput::new(&cleared, Some(contained.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("geometry-only scope synthesizes");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(
        matches!(plan.report().synthesis_cause(), Some(cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented { classes }) if classes.contains(&ArenaClass::Design))
    );
    let mut output = Vec::new();
    plan.write_to(&mut output)
        .expect("write geometry-only archive");
    let reloaded = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("geometry-only archive decodes");
    assert_eq!(
        reloaded.ir().model.bodies.len(),
        contained.ir().model.bodies.len()
    );
    assert_eq!(
        reloaded.ir().model.faces.len(),
        contained.ir().model.faces.len()
    );
    assert!(reloaded.ir().model.features.is_empty());
    assert!(reloaded.ir().model.sketches.is_empty());
}

#[test]
fn one_damaged_history_record_gives_up_only_its_part() {
    let mut smbh = synthetic_geometry_with_history_smbh();
    let marker = b"\x0d\x0fhistory_payload";
    let at = smbh
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("history payload record");
    smbh[at] ^= 0x0d;
    let result = decode(f3d_with_smbh(&smbh));
    assert!(result.ir().scope.design);
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                    .note(String::new())
                    .code
        })
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0].message.contains("f3d.history_record"));
    assert!(result
        .source_fidelity()
        .retained_records()
        .keys()
        .any(|id| id.as_str().starts_with("f3d:source:part#")));
}

#[test]
fn one_damaged_protein_asset_gives_up_only_its_part() {
    let result = decode(f3d_with_damaged_protein_asset(&synthetic_geometry_smbh()));
    assert!(!result.ir().model.appearances.is_empty());
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                    .note(String::new())
                    .code
        })
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0].message.contains("f3d.protein_record"));
    assert!(result
        .source_fidelity()
        .retained_records()
        .keys()
        .any(|id| id.as_str().starts_with("f3d:source:part#")));
}

#[test]
fn one_damaged_primary_index_gives_up_only_its_stream() {
    let full = decode(design_archive());
    let damaged = decode(f3d_with_damaged_design_bulkstream(
        &synthetic_geometry_with_history_smbh(),
    ));
    let damaged_bodies = &damaged.ir().model.bodies;
    let full_bodies = &full.ir().model.bodies;
    assert_eq!(
        damaged_bodies.len(),
        full_bodies.len(),
        "damaged BulkStream keeps the body count"
    );
    for (damaged_body, full_body) in damaged_bodies.iter().zip(full_bodies.iter()) {
        assert_eq!(damaged_body.id, full_body.id);
        assert_eq!(damaged_body.kind, full_body.kind);
        assert_eq!(damaged_body.regions, full_body.regions);
    }
    assert!(damaged.ir().scope.design);
    assert!(!f3d_native(damaged.ir()).asm_histories.is_empty());
    assert!(damaged
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::DesignIntentUndecoded.kind()));
    let unreadable = damaged
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                    .note(String::new())
                    .code
        })
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0].message.contains("f3d.bulk_record"));
    assert!(unreadable[0].message.contains("Design1/BulkStream.dat"));
    let stub_ids = damaged
        .source_fidelity()
        .retained_records()
        .keys()
        .filter(|id| id.as_str().starts_with("f3d:source:part#"))
        .collect::<Vec<_>>();
    assert_eq!(stub_ids.len(), 1);
    let unknowns = damaged
        .ir()
        .native
        .namespace("f3d")
        .expect("f3d native namespace")
        .arenas()
        .get("unknowns")
        .expect("one unknowns home");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        cadmpeg_ir::native::NativeRecord::id(&unknowns[0]),
        stub_ids[0].as_str()
    );
    assert_valid(&damaged);
}
