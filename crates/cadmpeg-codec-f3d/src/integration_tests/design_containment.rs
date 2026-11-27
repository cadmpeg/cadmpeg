// SPDX-License-Identifier: Apache-2.0
//! A design-intent failure keeps the geometry and product document.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeDepth, ResourceDimension, ResourceFailure, ResourceLimit};
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

/// A design-intent failure gives up the history partition from where the
/// history decode starts, the history preamble, as a failed history decode
/// gives it up.
#[test]
fn a_design_intent_failure_gives_up_the_history_partition_from_its_preamble() {
    let smbh = synthetic_geometry_with_history_smbh();
    let start = crate::history::partition_start(&smbh).expect("the stream stores a history");
    assert_ne!(
        Some(start),
        cadmpeg_asm::asm_header::solved_record_limit(&smbh)
    );
    inject_failure(malformed_design);
    let contained = decode(design_archive());
    let unreadable = cadmpeg_ir::unknown::part::record_unreadable_kind();
    let parts = contained
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == unreadable && loss.message.starts_with("part asm-history "))
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 1, "{parts:#?}");
    assert!(
        parts[0]
            .message
            .contains(&format!(" offset={start} length={} ", smbh.len() - start)),
        "{}",
        parts[0].message
    );
}

#[test]
fn strict_mode_refuses_a_contained_design_intent_failure() {
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
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
    assert!(rejection
        .loss()
        .message
        .contains("injected design-intent failure"));
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
    crate::test_support::assert_patch_declined(
        &edited,
        Some(contained.source_fidelity()),
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

#[test]
fn an_unread_configuration_keeps_timeline_sketches_and_geometry() {
    use std::io::Write;
    let source =
        include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d").to_vec();
    let full = decode(source.clone());
    assert!(!full.ir().model.features.is_empty());
    assert!(!full.ir().model.sketches.is_empty());
    let mut zip = zip::ZipWriter::new_append(Cursor::new(source)).unwrap();
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    zip.start_file("FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.unread.dsgcfg", stored).unwrap();
    zip.write_all(b"[").unwrap();
    let source = zip.finish().unwrap().into_inner();
    let recovered = decode(source.clone());
    assert_eq!(recovered.ir().model.features, full.ir().model.features);
    assert_eq!(recovered.ir().model.sketches, full.ir().model.sketches);
    assert_eq!(recovered.ir().geometry, full.ir().geometry);
    assert!(f3d_native(recovered.ir()).design_intent_failures.is_empty());
    assert_eq!(
        recovered
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::ConfigurationDocumentUndecoded.kind())
            .count(),
        1
    );
    assert!(recovered
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::DesignIntentUndecoded.kind()));
    assert_valid(&recovered);
    let mut replayed = Vec::new();
    crate::test_support::plan_inherited_write(
        recovered.ir(),
        recovered.source_fidelity(),
        &mut replayed,
    )
    .unwrap();
    assert_eq!(replayed, source);
}

const SKETCH_CONSTRAINTS: &[u8] =
    include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d");

/// The sketch-constraints archive plus a Design stream whose first primary
/// record has an unreadable header and whose second is intact.
fn with_unread_primary_header() -> Vec<u8> {
    use std::io::Write;
    let source = SKETCH_CONSTRAINTS.to_vec();
    let mut bulk = Vec::new();
    crate::test_support::lp_ascii(&mut bulk, "bad");
    bulk.extend_from_slice(&41u32.to_le_bytes());
    let sibling_start = bulk.len();
    crate::test_support::lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&42u32.to_le_bytes());
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "00000000-0000-0000-0000-000000000001",
            "",
            0,
            "Test",
            &[41, 42],
        )],
        &[
            (41, 0),
            (42, cadmpeg_core::decode::u64_from_index(sibling_start)),
        ],
    );
    let mut zip = zip::ZipWriter::new_append(Cursor::new(source)).unwrap();
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    zip.start_file(
        "FusionAssetName[Active]/FusionDesignSegmentType2/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/FusionDesignSegmentType2/MetaStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&meta).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn an_unread_primary_header_keeps_the_other_design_stream() {
    let source = SKETCH_CONSTRAINTS.to_vec();
    let full = decode(source);
    let recovered = decode(with_unread_primary_header());
    assert_eq!(recovered.ir().model.features, full.ir().model.features);
    assert_eq!(recovered.ir().model.sketches, full.ir().model.sketches);
    assert_eq!(recovered.ir().geometry, full.ir().geometry);
    assert!(f3d_native(recovered.ir()).design_intent_failures.is_empty());
    assert_eq!(
        recovered
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::PrimaryRecordUndecoded.kind())
            .count(),
        1
    );
    assert!(recovered
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::DesignIntentUndecoded.kind()));
    assert_valid(&recovered);
}

const RELATION_OWNER_FINDING: &str = "Fusion sketch relation has an invalid owner or byte frame";
const SHARED_MEMBER_FINDING: &str = "Fusion sketch member belongs to multiple sketch owners";

fn sketch_ownership_findings(ir: &cadmpeg_ir::document::CadIr) -> Vec<String> {
    native_findings(ir)
        .into_iter()
        .filter(|message| message == RELATION_OWNER_FINDING || message == SHARED_MEMBER_FINDING)
        .collect()
}

#[test]
fn native_validation_accepts_a_relation_whose_owner_names_no_sketch() {
    let full = decode(
        include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d").to_vec(),
    );
    assert_eq!(native_findings(full.ir()), Vec::<String>::new());

    let mut unresolved = full.ir().clone();
    {
        let mut native = f3d_native_mut(&mut unresolved);
        let relation = &mut native.sketch_relations[0];
        relation.owner_reference = u32::MAX;
        relation.owner_entity_id = None;
    }
    assert_eq!(sketch_ownership_findings(&unresolved), Vec::<String>::new());

    let mut dropped = full.ir().clone();
    f3d_native_mut(&mut dropped).sketch_relations[0].owner_entity_id = None;
    assert_eq!(
        sketch_ownership_findings(&dropped),
        [RELATION_OWNER_FINDING]
    );
}

#[test]
fn native_validation_accepts_shared_sketch_members_only_without_an_owner() {
    let full = decode(
        include_bytes!("../../tests/golden/fixtures/design_sketch_constraints.f3d").to_vec(),
    );
    let mut shared = full.ir().clone();
    let members = {
        let mut native = f3d_native_mut(&mut shared);
        let owned = native
            .sketch_points
            .iter()
            .map(|point| (point.record_index, point.owner_reference))
            .chain(
                native
                    .sketch_curve_identities
                    .iter()
                    .map(|curve| (curve.record_index, curve.owner_reference)),
            )
            .filter(|(_, owner)| owner.is_some())
            .map(|(record_index, _)| record_index)
            .collect::<std::collections::HashSet<_>>();
        let mut claimant = native
            .design_entity_headers
            .iter()
            .find(|header| header.in_sketch_module())
            .cloned()
            .expect("a sketch header");
        let (prefix, _) = claimant
            .entity_id
            .as_str()
            .rsplit_once('_')
            .expect("entity id suffix");
        let suffix = native
            .design_entity_headers
            .iter()
            .map(|header| header.entity_id.suffix())
            .max()
            .expect("entity headers")
            + 1;
        claimant.entity_id = crate::records::identity::DesignEntityId::from_parts(prefix, suffix);
        claimant.id = format!("{}-shared", claimant.id);
        let owner_entity_id =
            cadmpeg_core::text::NonBlankString::new(claimant.entity_id.as_str().to_owned());
        let mut members = claimant.member_values().copied().collect::<Vec<_>>();
        native.design_entity_headers.push(claimant);
        let relation = native
            .sketch_relations
            .iter_mut()
            .find(|relation| {
                relation
                    .all_member_indices()
                    .any(|member| owned.contains(&member))
            })
            .expect("a relation names an owned record");
        relation.owner_reference = u32::try_from(suffix).expect("u32 suffix");
        relation.owner_entity_id = owner_entity_id;
        members.extend(relation.all_member_indices());
        members
    };
    assert!(sketch_ownership_findings(&shared)
        .iter()
        .any(|message| message == SHARED_MEMBER_FINDING));

    {
        let mut native = f3d_native_mut(&mut shared);
        for point in &mut native.sketch_points {
            if members.contains(&point.record_index) {
                point.owner_reference = None;
            }
        }
        for curve in &mut native.sketch_curve_identities {
            if members.contains(&curve.record_index) {
                curve.owner_reference = None;
            }
        }
        for surface in &mut native.sketch_surfaces {
            if members.contains(&surface.record_index) {
                surface.owner_reference = None;
            }
        }
    }
    assert_eq!(sketch_ownership_findings(&shared), Vec::<String>::new());
}

#[test]
fn an_unread_primary_header_is_a_loss_at_geometry_and_design_depth() {
    let bytes = with_unread_primary_header();
    for depth in [DecodeDepth::Geometry, DecodeDepth::Design] {
        let mut options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        options.strictness.pinned = true;
        let failure = F3dCodec
            .decode(&mut Cursor::new(&bytes), &options)
            .expect_err("strict decode refuses the unread header");
        let DecodeFailure::StrictRejected { rejection } = &failure else {
            panic!("expected a strict rejection at {depth:?}, got {failure:?}");
        };
        assert_eq!(
            rejection.loss().code,
            cadmpeg_ir::unknown::part::record_unreadable_kind()
        );
        assert!(
            rejection.loss().message.starts_with(
                "part design-record stream=\"FusionAssetName[Active]/FusionDesignSegmentType2/BulkStream.dat\" offset="
            ),
            "{}",
            rejection.loss().message
        );
    }
}

fn unreadable_messages(result: &EditableDecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn a_design_intent_failure_gives_up_the_design_streams() {
    assert!(unreadable_messages(&decode(design_archive())).is_empty());

    inject_failure(malformed_design);
    let contained = decode(design_archive());

    let messages = unreadable_messages(&contained);
    let design = messages
        .iter()
        .filter(|message| {
            message.starts_with(
                "part archive-entry \"FusionAssetName[Active]/Design1/BulkStream.dat\" stream=\"\" offset=",
            )
        })
        .collect::<Vec<_>>();
    let history = messages
        .iter()
        .filter(|message| {
            message.starts_with(
                "part asm-history stream=\"FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh\"",
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(design.len(), 1, "{messages:#?}");
    assert_eq!(history.len(), 1, "{messages:#?}");
    for message in design.iter().chain(&history) {
        assert!(
            message.contains("design intent was not decoded: "),
            "{message}"
        );
        assert!(
            message.contains("injected design-intent failure"),
            "{message}"
        );
        assert!(message.contains("; dropped=[]"), "{message}");
    }
    assert_valid(&contained);
}

#[test]
fn generated_feature_designs_give_up_no_record() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::{
        f3d_with_smbh_and_protein_with_generated_copy_paste,
        f3d_with_smbh_and_protein_with_generated_copy_paste_bodies,
        f3d_with_smbh_and_protein_with_generated_form,
        f3d_with_smbh_and_protein_with_generated_remove_body,
        f3d_with_smbh_and_protein_with_generated_surface_stitch,
    };
    let smbh = synthetic_geometry_smbh();
    for (name, bytes) in [
        (
            "remove body",
            f3d_with_smbh_and_protein_with_generated_remove_body(&smbh),
        ),
        (
            "surface stitch",
            f3d_with_smbh_and_protein_with_generated_surface_stitch(&smbh),
        ),
        (
            "copy paste",
            f3d_with_smbh_and_protein_with_generated_copy_paste(&smbh),
        ),
        (
            "copy paste bodies",
            f3d_with_smbh_and_protein_with_generated_copy_paste_bodies(&smbh),
        ),
        ("form", f3d_with_smbh_and_protein_with_generated_form(&smbh)),
    ] {
        let result = decode(bytes);
        assert_eq!(unreadable_messages(&result), Vec::<String>::new(), "{name}");
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != F3dLossCode::SketchCurveRecordUndecoded.kind()),
            "{name}"
        );
    }
}
