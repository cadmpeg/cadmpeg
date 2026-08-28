// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized SLDPRT compound-document images.

use crate::test_support::*;
use std::io::Cursor;

use crate::writer::tests::{
    semantic_writer_rejects_nonfinite_analytic_carriers, semantic_writer_rejects_subds,
};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};

use crate::container::role;
use crate::SldprtCodec;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized SLDPRT should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    let native = crate::validate_native(result.ir());
    assert!(native.is_empty(), "{native:#?}");
}

#[test]
fn compound_pipeline_aligns_detection_inspection_blocks_cache_directory_and_metadata() {
    let bytes = synthetic_sldprt();
    assert_eq!(SldprtCodec.detect(&bytes), Confidence::High);
    let summary = SldprtCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("SLDPRT inspection");
    assert_eq!(summary.format, "sldprt");
    assert_eq!(
        summary
            .entries
            .iter()
            .filter(|entry| entry.role == role::BLOCK)
            .count(),
        2
    );
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == role::CACHE_CELL));
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == role::DIRECTORY_ENTRY));
    let result = decode(bytes);
    assert!(!result.source_fidelity().retained_records.is_empty());
    assert_valid(&result);
}

#[test]
fn parasolid_pipeline_composes_closed_open_analytic_freeform_and_degenerate_topology() {
    let fixtures = [
        triangle_body(),
        closed_cylinder_body(),
        sphere_patch_body(),
        triangle_body_with_overlapping_point(),
        prefixed_edge_triangle_body(),
    ];
    let mut saw_solid = false;
    let mut saw_pcurve = false;
    for body in fixtures {
        let result = decode(sldprt_with_body(&body));
        saw_solid |= result
            .ir()
            .model
            .bodies
            .iter()
            .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid);
        saw_pcurve |= !result.ir().model.pcurves.is_empty();
        assert!(result.report().geometry_transferred);
        assert_valid(&result);
    }
    assert!(saw_solid && saw_pcurve);
}

#[test]
fn configuration_pipeline_merges_partition_deltas_colliding_sites_and_membership() {
    let fixtures = [
        sldprt_with_partition_and_deltas(&triangle_body(), &tripled_triangle_body()),
        sldprt_with_colliding_sites(),
        sldprt_with_body_and_envelope(&owned_triangle(0, 700, 0.0)),
    ];
    for bytes in fixtures {
        let result = decode(bytes);
        assert!(result.report().geometry_transferred);
        assert!(!result.ir().model.bodies.is_empty());
        assert_valid(&result);
    }
}

#[test]
fn design_pipeline_correlates_feature_history_sketch_profiles_constraints_and_dimensions() {
    let fixtures = [
        sldprt_with_body_and_history(&triangle_body()),
        sldprt_with_nested_sketch_profiles(&triangle_body(), 2),
        sldprt_with_nested_circular_sketch(&triangle_body()),
        sldprt_with_nested_arc_sketch(&triangle_body()),
        sldprt_with_nested_nurbs_sketches(&triangle_body()),
        sldprt_with_tagged_compact_relation_scalar(
            &triangle_body(),
            "sgPntPntDist",
            [[0xd6, 0x80]; 2],
            25.0,
        ),
    ];
    let mut saw_feature = false;
    let mut saw_sketch = false;
    let mut saw_parameter = false;
    for bytes in fixtures {
        let result = decode(bytes);
        saw_feature |= !result.ir().model.features.is_empty();
        saw_sketch |= !result.ir().model.sketches.is_empty();
        saw_parameter |= !result.ir().model.parameters.is_empty();
        assert_valid(&result);
    }
    assert!(saw_feature && saw_sketch && saw_parameter);
}

#[test]
fn presentation_pipeline_binds_materials_face_colors_tessellation_and_pmi() {
    let material = decode(sldprt_with_body_and_material(
        &triangle_body(),
        "Generated Steel",
        [32, 64, 96],
    ));
    assert!(!material.ir().model.appearances.is_empty());
    assert_valid(&material);

    let display = decode(sldprt_with_body_and_display_list(&triangle_body()));
    assert!(!display.ir().model.tessellations.is_empty());
    assert_eq!(
        display.ir().model.tessellations[0].faces,
        [display.ir().model.faces[0].id.clone()]
    );
    assert_eq!(
        display.ir().model.tessellations[0].body.as_ref(),
        Some(&display.ir().model.bodies[0].id)
    );
    let tessellation_exactness =
        &display.source_fidelity().annotations.exactness[&display.ir().model.tessellations[0].id];
    assert_eq!(
        tessellation_exactness.fields["body"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        tessellation_exactness.fields["faces"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_valid(&display);

    let mut bytes = sldprt_with_body(&triangle_body());
    bytes.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    bytes.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload(),
    ));
    let pmi = decode(bytes);
    assert!(!sldprt_native(pmi.ir()).pmi_dimensions.is_empty());
    assert!(!pmi.ir().model.parameters.is_empty());
    assert_valid(&pmi);
}

#[test]
fn tessellation_geometry_does_not_choose_between_coincident_faces() {
    let mut decoded = decode(sldprt_with_body_and_display_list(&triangle_body()));
    decoded.ir_mut().model.tessellations[0].body = None;
    decoded.ir_mut().model.tessellations[0].faces.clear();
    let mut coincident = decoded.ir().model.faces[0].clone();
    coincident.id = cadmpeg_ir::ids::FaceId("sldprt:brep:face#coincident".into());
    decoded.ir_mut().model.shells[0]
        .faces
        .push(coincident.id.clone());
    decoded.ir_mut().model.faces.push(coincident);

    let _ = crate::tessellation::assign_unique_surface_owners(&mut decoded.ir_mut().model);

    assert!(decoded.ir().model.tessellations[0].body.is_none());
    assert!(decoded.ir().model.tessellations[0].faces.is_empty());
}

#[test]
fn retained_writer_pipeline_regenerates_geometry_and_preserves_unedited_sections() {
    let decoded = decode(sldprt_with_body_and_history(&triangle_body()));
    let mut edited = decoded.ir().clone();
    translate_model_x(&mut edited, 3.0);
    let mut bytes = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut bytes)
        .expect("semantic SLDPRT write");
    let round_trip = decode(bytes);
    assert_eq!(
        round_trip.ir().model.points[0].position.x,
        edited.model.points[0].position.x
    );
    assert!(!round_trip.ir().model.features.is_empty());
    assert_valid(&round_trip);
}

#[test]
fn source_less_writer_pipeline_round_trips_a_cube_and_rejects_unrepresentable_ir() {
    let first = encode_decode_result(&source_less_cube());
    assert_eq!(first.ir().model.faces.len(), 6);
    assert_eq!(first.ir().model.edges.len(), 12);
    assert_valid(&first);

    let mut bytes = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(first.ir(), first.source_fidelity(), &mut bytes)
        .unwrap();
    let second = decode(bytes);
    assert_eq!(
        crate::decode::document_local_sha256(first.ir()),
        crate::decode::document_local_sha256(second.ir())
    );
    semantic_writer_rejects_subds();
    semantic_writer_rejects_nonfinite_analytic_carriers();
}

// --------------------------------------------------------------------------
// Resolution of a write request against the source (design §8.2), and the
// §8.3 honesty invariant on every write path.
// --------------------------------------------------------------------------

/// A part declaring `swVersion`, so its dialect is a versioned row this writer
/// cannot synthesize and can only preserve.
///
/// The envelope carries a `swModel` as well as the version, so the semantic
/// writer can run over this part: without one it refuses before resolution is
/// reached, and the resolution is what these tests are about.
fn versioned_part() -> Vec<u8> {
    let mut bytes = sldprt_with_body_and_history(&triangle_body());
    bytes.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    bytes
}

fn plan<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    fidelity: bool,
    request: cadmpeg_ir::codec::TargetRequest<'a>,
) -> Result<cadmpeg_ir::codec::ExportPlan<'a>, cadmpeg_core::CodecError> {
    SldprtCodec.plan(
        cadmpeg_ir::codec::EncodeInput::new(
            result.ir(),
            fidelity.then(|| result.source_fidelity()),
        ),
        request,
    )
}

fn named_target(plan: &cadmpeg_ir::codec::ExportPlan<'_>) -> String {
    plan.report()
        .target
        .as_ref()
        .expect("a SLDPRT write always names its dialect")
        .to_string()
}

fn classify(bytes: Vec<u8>) -> String {
    let redecoded = decode(bytes);
    cadmpeg_core::dialect::primary_layer(&redecoded.report().dialects, &redecoded.report().format)
        .and_then(|entry| entry.dialect.clone())
        .expect("the written part classifies a host dialect")
        .to_string()
}

/// The flagship case: `convert in.sldprt -o out.sldprt` on a part whose version
/// is not the catalog row keeps the part it was handed, and says so.
#[test]
fn inherit_replays_a_versioned_part_and_names_its_dialect() {
    let source = versioned_part();
    let result = decode(source.clone());
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|meta| meta.dialect.as_ref())
            .map(cadmpeg_core::dialect::DialectId::as_str),
        Some("sldprt:sw-version-12000-plus")
    );

    let plan = plan(&result, true, cadmpeg_ir::codec::TargetRequest::Inherit)
        .expect("the source's own dialect is preserved");
    assert_eq!(plan.write_path(), cadmpeg_ir::WritePath::VerbatimReplay);
    assert_eq!(named_target(&plan), "sldprt:sw-version-12000-plus");

    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(written, source);
    assert_eq!(classify(written), "sldprt:sw-version-12000-plus");
}

/// A source dialect outside the one-row catalog refuses under `Inherit` once
/// there is nothing to preserve. There is no fall-through to the catalog row: a
/// same-format conversion never silently changes what the part is, and the
/// refusal names both the source's dialect and the escape.
#[test]
fn inherit_refuses_an_off_catalog_source_dialect_with_nothing_retained() {
    let result = decode(versioned_part());
    let error = plan(&result, false, cadmpeg_ir::codec::TargetRequest::Inherit)
        .err()
        .expect("a versioned row is not a synthesis target");
    let cadmpeg_core::CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "sldprt");
    assert_eq!(requested.as_deref(), Some("sldprt:sw-version-12000-plus"));
    assert!(available.contains("sldprt:unknown"), "{available}");
    let cadmpeg_core::CodecError::UnsupportedTarget { reason, .. } = &error else {
        unreachable!()
    };
    assert!(
        reason.contains("sldprt:unknown"),
        "the refusal must name what the write would have been: {reason}"
    );
}

/// The replay law's compare, not the presence of a retained image: an explicit
/// catalog row over a retained part of a different dialect never replays that
/// part while claiming the row. This writer passes a retained `swSolidWorks`
/// envelope through unchanged and regenerates none over one it kept, so it
/// cannot deliver the version-less row from this input and refuses by name.
#[test]
fn an_explicit_catalog_row_does_not_replay_a_different_dialect() {
    let result = decode(versioned_part());
    let error = plan(
        &result,
        true,
        cadmpeg_ir::codec::TargetRequest::Explicit("sldprt:unknown"),
    )
    .err()
    .expect("the version-less row is not deliverable from a versioned part");
    let cadmpeg_core::CodecError::UnsupportedTarget {
        requested, reason, ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(requested.as_deref(), Some("sldprt:unknown"));
    assert!(
        reason.contains("sldprt:sw-version-12000-plus"),
        "the refusal must name what the write would have been: {reason}"
    );
}

/// An explicit id outside the catalog is refused with the catalog, so the
/// caller can correct the request from the message alone.
#[test]
fn an_unknown_explicit_target_is_refused_with_the_catalog() {
    let result = decode(synthetic_sldprt());
    let error = plan(
        &result,
        true,
        cadmpeg_ir::codec::TargetRequest::Explicit("step:ap242-e3"),
    )
    .err()
    .expect("a STEP schema is not a SLDPRT target");
    let cadmpeg_core::CodecError::UnsupportedTarget {
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(requested.as_deref(), Some("step:ap242-e3"));
    assert!(available.contains("sldprt:unknown"), "{available}");
}

/// The §8.3 honesty invariant on the patch path: an edited part still writes
/// the source's own dialect, because the retained `swSolidWorks` envelope goes
/// through unchanged, and the report names it.
#[test]
fn the_patch_path_names_the_preserved_dialect() {
    let result = decode(versioned_part());
    assert!(
        !result.ir().model.points.is_empty(),
        "the patch lane needs an editable point"
    );
    let mut edited = result.ir().clone();
    edited.model.points[0].position.x += 1.0;

    let plan = SldprtCodec
        .plan(
            cadmpeg_ir::codec::EncodeInput::new(&edited, Some(result.source_fidelity())),
            cadmpeg_ir::codec::TargetRequest::Inherit,
        )
        .expect("an edited part still preserves its dialect");
    assert_eq!(plan.write_path(), cadmpeg_ir::WritePath::Patched);
    let cadmpeg_ir::FidelityResolution::Degraded { reason } = plan.fidelity_resolution() else {
        panic!("digest mismatch must report degraded fidelity");
    };
    assert!(reason.contains("digest"), "{reason}");
    assert!(plan.report().losses.iter().all(|loss| {
        loss.code != crate::loss::SldprtLossCode::SourcePreservedImageUnavailable.kind()
    }));
    assert!(plan
        .report()
        .notes
        .iter()
        .any(|note| note == "preserved source container replayed with semantic patches"));
    let claimed = named_target(&plan);
    assert_eq!(claimed, "sldprt:sw-version-12000-plus");

    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(classify(written), claimed);
}

#[test]
fn a_retained_source_record_without_data_reports_degraded_fidelity() {
    let result = decode(versioned_part());
    let (ir, _, mut fidelity) = result.into_parts();
    fidelity
        .retained_records
        .iter_mut()
        .find(|record| record.id.as_str() == crate::SOURCE_IMAGE_ID)
        .expect("decode retains the source image")
        .data = None;

    let plan = SldprtCodec
        .plan(
            cadmpeg_ir::codec::EncodeInput::new(&ir, Some(&fidelity)),
            cadmpeg_ir::codec::TargetRequest::Inherit,
        )
        .expect("missing retained bytes fall back to semantic writing");
    assert_ne!(plan.write_path(), cadmpeg_ir::WritePath::VerbatimReplay);
    assert_eq!(
        plan.fidelity_resolution(),
        &cadmpeg_ir::FidelityResolution::Degraded {
            reason: "preserved SLDPRT source image is unavailable".into(),
        }
    );
    let unavailable = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::SldprtLossCode::SourcePreservedImageUnavailable.kind()
        })
        .expect("missing retained bytes charge image unavailability");
    assert!(unavailable.message.contains("retained source records"));
    assert!(!unavailable.message.contains("regenerated from IR"));
}

/// The §8.3 honesty invariant on the generation path: a part built with nothing
/// retained lands on the totality row, which is the whole catalog, and the
/// report names it.
#[test]
fn the_generation_path_names_the_catalog_row() {
    let ir = source_less_cube();
    let plan = SldprtCodec
        .plan(
            cadmpeg_ir::codec::EncodeInput::new(&ir, None),
            cadmpeg_ir::codec::TargetRequest::Inherit,
        )
        .expect("nothing to inherit, so the catalog default stands in");
    assert_eq!(plan.write_path(), cadmpeg_ir::WritePath::Synthesized);
    let claimed = named_target(&plan);
    assert_eq!(claimed, "sldprt:unknown");

    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(classify(written), claimed);
}
