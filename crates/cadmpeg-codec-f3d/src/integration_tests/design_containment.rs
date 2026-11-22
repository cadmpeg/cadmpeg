// SPDX-License-Identifier: Apache-2.0
//! A design-intent failure keeps the geometry and product document.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use super::{assert_valid, decode};
use crate::decode::design_intent::inject_failure;
use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{f3d_without_brep, XREF_ROLE};
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein;
use crate::F3dCodec;

fn design_archive() -> Vec<u8> {
    f3d_with_smbh_and_protein(&synthetic_geometry_with_history_smbh())
}

fn malformed_design() -> CodecError {
    CodecError::malformed("injected design-intent failure")
}

fn recursion_refusal() -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        scope: cadmpeg_core::decode::LimitScope::for_dimension(ResourceDimension::RecursionDepth),
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

    assert_eq!(contained.ir().geometry.bodies, full.ir().geometry.bodies);
    assert_eq!(contained.ir().geometry.faces, full.ir().geometry.faces);
    assert_eq!(
        contained.ir().geometry.surfaces,
        full.ir().geometry.surfaces
    );
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

fn moved_first_point(result: &EditableDecodeResult) -> cadmpeg_ir::document::CadIr {
    let mut edited = result.ir().clone();
    let moved = edited.geometry.points[0].position().get();
    edited.geometry.points[0].set_position(
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 1.0,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    edited
}

#[test]
fn a_source_whose_design_intent_was_contained_regenerates_instead_of_patching() {
    let full = decode(design_archive());
    let mut patched = Vec::new();
    crate::test_support::plan_inherited_write(
        &moved_first_point(&full),
        full.source_fidelity(),
        &mut patched,
    )
    .expect("the same edit patches when design intent decodes");

    inject_failure(malformed_design);
    let contained = decode(design_archive());
    let edited = moved_first_point(&contained);
    inject_failure(malformed_design);
    crate::test_support::assert_written_with_loss(
        &edited,
        Some(contained.source_fidelity()),
        &crate::loss::F3dLossCode::WriterPatchDeclined
            .kind()
            .to_string(),
        "design intent was not decoded",
    );
}

#[test]
fn an_unedited_contained_document_replays_its_source_bytes() {
    let source = design_archive();
    inject_failure(malformed_design);
    let contained = decode(source.clone());

    let mut replayed = Vec::new();
    crate::test_support::plan_inherited_write(
        contained.ir(),
        contained.source_fidelity(),
        &mut replayed,
    )
    .expect("an unedited contained document replays");

    assert_eq!(replayed, source);
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
/// owners that design intent binds. Its failure record tells native
/// validation that the bindings are absent rather than broken.
#[test]
fn native_validation_skips_design_intent_links_after_a_contained_failure() {
    let source =
        include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d").to_vec();
    inject_failure(malformed_design);
    let contained = decode(source.clone());

    let failures = f3d_native(contained.ir()).design_intent_failures;
    assert_eq!(failures.len(), 1);
    assert!(failures[0]
        .reason
        .contains("injected design-intent failure"));
    assert_eq!(native_findings(contained.ir()), Vec::<String>::new());

    let mut unmarked = contained.ir().clone();
    f3d_native_mut(&mut unmarked).design_intent_failures.clear();
    assert!(native_findings(&unmarked)
        .iter()
        .any(|message| message == "Fusion Design parameter scope has an invalid paired frame"));

    let full = decode(source);
    assert_eq!(native_findings(full.ir()), Vec::<String>::new());
    let mut marked = full.ir().clone();
    f3d_native_mut(&mut marked).design_intent_failures = failures;
    assert_eq!(
        native_findings(&marked),
        ["Fusion design-intent failure record accompanies decoded design records"]
    );
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

#[test]
fn a_consumed_history_failure_in_finalize_keeps_geometry_and_design() {
    let full = decode(design_archive());
    crate::history::history_bodies::inject_consumed_failure(malformed_design);
    let contained = decode(design_archive());
    assert_eq!(contained.ir().geometry, full.ir().geometry);
    assert_eq!(contained.ir().model.features, full.ir().model.features);
    assert_eq!(
        f3d_native(contained.ir()).asm_histories,
        f3d_native(full.ir()).asm_histories
    );
    let losses = contained
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::FeatureHistoryBodyUndecoded.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("injected design-intent failure"));
    assert_valid(&contained);
}

#[test]
fn a_consumed_history_resource_refusal_in_finalize_refuses_the_document() {
    crate::history::history_bodies::inject_consumed_failure(recursion_refusal);
    let failure = F3dCodec
        .decode(
            &mut Cursor::new(design_archive()),
            &DecodeOptions::default(),
        )
        .expect_err("a consumed history resource refusal propagates");
    assert!(matches!(
        failure,
        DecodeFailure::Codec(CodecError::ResourceLimit(_))
    ));
}
