// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized SLDPRT compound-document images.

use crate::test_support::*;
use std::io::Cursor;

use crate::writer::tests::{
    semantic_writer_rejects_nonfinite_analytic_carriers, semantic_writer_rejects_subds,
};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

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
