// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Integration contracts over synthesized `FCStd` archives and application graphs.

use super::*;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use std::io::Cursor;
use zip::write::SimpleFileOptions;

use crate::test_support::*;

use crate::annotation::tests::transfers_remaining_semantic_annotation_families_and_assets;
use crate::application_geometry::tests::transfers_application_mesh_and_transformed_point_cloud_payloads;
use crate::brep::tests::{
    transfers_binary_exact_curve_and_surface_carriers,
    transfers_recursive_exact_parameter_curve_geometry,
};
use crate::container::tests::rejects_unsafe_names;
use crate::design::tests::booleans_patterns::{
    transfers_partdesign_boolean_base_and_group_rules,
    transfers_uniform_irregular_and_two_axis_patterns,
};
use crate::design::tests::construction::transfers_part_construction_geometry_features;
use crate::design::tests::holes_extrude::{
    transfers_branch_complete_threaded_counterdrill_hole,
    transfers_non_default_extrusion_termination_branches,
};
use crate::design::tests::primitives::transfers_part_and_partdesign_analytic_primitives;
use crate::design::tests::sketches::{
    neutralizes_symmetric_locus_distance_and_point_on_object_constraints,
    transfers_bounded_rational_sketch_nurbs, transfers_full_and_bounded_sketch_conics,
    transfers_point_and_elliptical_sketch_geometry_without_fabricated_defaults,
};
use crate::drawing::tests::recovers_techdraw_page_template_and_view_graph;
use crate::gui::tests::retains_ordered_document_level_gui_state;
use crate::joint::tests::recovers_assembly_joint_operands_frames_and_state;
use crate::persistence::tests::{
    legacy_schema_dispatch_rejects_wrong_envelopes_and_inconsistent_counts,
    schema_three_uses_the_object_envelope_and_defaults_file_version,
    schema_two_uses_the_feature_envelope_and_common_property_grammar,
};
use crate::product::tests::recovers_product_prototypes_occurrences_and_placements;
use crate::topology_transfer::tests::{
    binds_both_seam_pcurves_and_closes_the_radial_pair,
    preserves_compound_ownership_and_composes_nested_mirrored_locations_once,
    transfers_connected_text_brep_topology,
    transfers_triangulation_only_face_and_indexed_edge_polygon,
};
use crate::writer::tests::{
    write_target_and_source_requirements_are_explicit,
    writer_rejects_unserialized_declaration_and_stale_payload_edits,
};

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized FCStd archive should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    assert_valid_document(result.ir());
    let findings = crate::validate_native(result.ir());
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn container_pipeline_handles_stored_deflated_streaming_and_zip64_layouts() {
    let document = "<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Objects Count=\"0\"></Objects><ObjectData Count=\"0\"></ObjectData></Document>";
    let fixtures = [
        archive(document),
        streaming_archive(document),
        streaming_archive_with_options(document, SimpleFileOptions::default().large_file(true)),
    ];
    for bytes in fixtures {
        assert_eq!(FcstdCodec.detect(&bytes), Confidence::High);
        let summary = FcstdCodec
            .inspect(
                &mut Cursor::new(&bytes),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .expect("FCStd inspection");
        assert_eq!(summary.format(), "fcstd");
        assert!(summary.notes.iter().any(|note| note == "SchemaVersion=4"));
        let result = decode(bytes);
        assert_valid(&result);
    }
}

#[test]
fn typed_graph_pipeline_builds_mutates_writes_and_reloads_side_entries() {
    let mut builder = crate::FcstdDocumentBuilder::new("generated integration document");
    builder
        .add_object("Box", "Part::Box")
        .unwrap()
        .add_property(
            "Box",
            "Label",
            "App::PropertyString",
            vec![crate::FcstdPropertyValue::attribute(
                "String", "value", "Original",
            )],
        )
        .unwrap()
        .add_property(
            "Box",
            "Length",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute(
                "Float", "value", "12.5",
            )],
        )
        .unwrap()
        .add_object("Part", "App::Part")
        .unwrap()
        .add_dependency("Part", "Box")
        .unwrap()
        .add_property(
            "Part",
            "Group",
            "App::PropertyLinkList",
            vec![crate::FcstdPropertyValue::empty("LinkList")
                .with_attribute("count", "1")
                .with_child(crate::FcstdPropertyValue::attribute("Link", "value", "Box"))],
        )
        .unwrap()
        .add_side_entry("Payload.bin", b"first payload".to_vec())
        .unwrap();
    let mut ir = builder.build().unwrap();
    FcstdCodec
        .set_property_value_attribute(
            &mut ir,
            crate::FcstdPropertyOwner::Object("Box"),
            "Label",
            0,
            "value",
            "Edited & encoded",
        )
        .unwrap();
    FcstdCodec
        .replace_side_entry(&mut ir, "Payload.bin", b"second payload".to_vec())
        .unwrap();

    let mut bytes = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    let round_trip = decode(bytes);
    let namespace = round_trip.ir().native.namespace("fcstd").unwrap();
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.name == "Payload.bin")
            .map(|entry| entry.data.as_slice()),
        Some(b"second payload".as_slice())
    );
    assert_valid(&round_trip);
}

#[test]
fn brep_pipeline_composes_exact_geometry_topology_pcurves_meshes_and_placements() {
    transfers_connected_text_brep_topology();
    transfers_binary_exact_curve_and_surface_carriers();
    transfers_triangulation_only_face_and_indexed_edge_polygon();
    binds_both_seam_pcurves_and_closes_the_radial_pair();
    preserves_compound_ownership_and_composes_nested_mirrored_locations_once();
}

#[test]
fn sketch_pipeline_composes_conics_nurbs_constraints_and_parameter_curves() {
    transfers_point_and_elliptical_sketch_geometry_without_fabricated_defaults();
    transfers_full_and_bounded_sketch_conics();
    transfers_bounded_rational_sketch_nurbs();
    neutralizes_symmetric_locus_distance_and_point_on_object_constraints();
    transfers_recursive_exact_parameter_curve_geometry();
}

#[test]
fn feature_pipeline_composes_primitives_booleans_patterns_and_terminal_branches() {
    transfers_part_and_partdesign_analytic_primitives();
    transfers_part_construction_geometry_features();
    transfers_partdesign_boolean_base_and_group_rules();
    transfers_uniform_irregular_and_two_axis_patterns();
    transfers_non_default_extrusion_termination_branches();
    transfers_branch_complete_threaded_counterdrill_hole();
}

#[test]
fn application_pipeline_composes_products_joints_payloads_gui_drawings_and_annotations() {
    recovers_product_prototypes_occurrences_and_placements();
    recovers_assembly_joint_operands_frames_and_state();
    transfers_application_mesh_and_transformed_point_cloud_payloads();
    retains_ordered_document_level_gui_state();
    recovers_techdraw_page_template_and_view_graph();
    transfers_remaining_semantic_annotation_families_and_assets();
}

#[test]
fn compatibility_and_refusal_pipeline_keeps_states_atomic() {
    rejects_unsafe_names();
    schema_three_uses_the_object_envelope_and_defaults_file_version();
    schema_two_uses_the_feature_envelope_and_common_property_grammar();
    legacy_schema_dispatch_rejects_wrong_envelopes_and_inconsistent_counts();
    write_target_and_source_requirements_are_explicit();
    writer_rejects_unserialized_declaration_and_stale_payload_edits();

    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(
                "<Document SchemaVersion=\"4\" FileVersion=\"1\"><Objects Count=\"0\"/></Document>",
            )),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("container-only FCStd decode");
    assert!(result.report().container_only);
    assert!(result.ir().model.features.is_empty());
    assert!(result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arenas
        .contains_key("physical_ledger"));
    assert_valid(&result);
}

#[test]
fn public_cc0_fixtures_decode_deterministically_without_blocking_loss() {
    let fixtures: [(&str, &[u8]); 12] = [
        (
            "external_component.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/external_component.FCStd"
            )),
        ),
        (
            "product_assembly.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd"
            )),
        ),
        (
            "core_operations.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
            )),
        ),
        (
            "sketch_constraints.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/sketch_constraints.FCStd"
            )),
        ),
        (
            "sketch_conics.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/sketch_conics.FCStd"
            )),
        ),
        (
            "gui_appearance.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
            )),
        ),
        (
            "design_history.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
            )),
        ),
        (
            "binary_exact_shape.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
            )),
        ),
        (
            "application_payloads.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/application_payloads.FCStd"
            )),
        ),
        (
            "geometry_topology.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd"
            )),
        ),
        (
            "core_design_product.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
            )),
        ),
        (
            "techdraw_annotations.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/techdraw_annotations.FCStd"
            )),
        ),
    ];
    for (name, bytes) in fixtures {
        let first = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let second = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            first.ir().to_canonical_json().expect("canonical fixture"),
            second.ir().to_canonical_json().expect("canonical fixture"),
            "{name} is nondeterministic"
        );
        assert!(
            first
                .report()
                .losses
                .iter()
                .all(|loss| loss.severity < cadmpeg_ir::Severity::Blocking),
            "{name}: {:#?}",
            first.report().losses
        );
        let native_findings = crate::validate_native(first.ir());
        assert!(native_findings.is_empty(), "{name}: {native_findings:#?}");
        assert_valid_document(first.ir());
    }
}

/// Every reported dialect path, end to end through the public codec surface.
///
/// The unit tests in `dialect/tests.rs` pin `classify` against the registry.
/// This one pins what a caller sees: the match on the container summary, the
/// match and the loss on the decode report, and the `SourceMeta` mirror, for a
/// declared schema and for one outside the declared rows.
#[test]
fn dialect_pipeline_reports_identity_admission_and_the_unverified_loss() {
    let unverified = crate::loss::FreecadLossCode::SourceDialectUnverified
        .note(String::new())
        .code;
    let body = "<Objects Count=\"0\"></Objects><ObjectData Count=\"0\"></ObjectData>";

    // A declared schema: its own row, admitted, no dialect loss.
    let bytes = archive(&format!(
        "<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\">{body}</Document>"
    ));
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("FCStd inspection");
    let matched = summary
        .dialects()
        .as_ref()
        .expect("FCStd inspection reports dialect layers")
        .primary();
    assert_eq!(matched.format(), "fcstd");
    assert_eq!(matched.dialect().as_str(), "fcstd:schema-4");
    assert_eq!(
        matched.admission(),
        cadmpeg_core::dialect::Admission::Admitted
    );

    let result = decode(bytes);
    assert_eq!(result.report().dialects(), summary.dialects());
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != unverified));
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.dialect(), Some(matched));
    assert_eq!(source.dialect().unwrap().declared()["schema_version"], "4");
    assert_eq!(source.dialect().unwrap().declared()["file_version"], "1");
    assert_eq!(
        source.dialect().unwrap().declared()["program_version"],
        "1.0"
    );
    // The pre-existing attribute keys keep their duties beside the mirror.
    assert_eq!(source.attributes["schema_version"], "4");
    assert_eq!(source.attributes["file_version"], "1");

    // A schema outside the declared rows: the totality row, admitted with the
    // schema-4 vocabulary substituted, and the loss naming the substitution.
    let bytes = archive(&format!(
        "<Document SchemaVersion=\"5\" FileVersion=\"1\">{body}</Document>"
    ));
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("FCStd inspection of an undeclared schema");
    assert_eq!(
        summary
            .dialects()
            .as_ref()
            .expect("FCStd inspection reports dialect layers")
            .primary()
            .dialect()
            .as_str(),
        "fcstd:unknown"
    );
    assert_eq!(
        summary
            .dialects()
            .as_ref()
            .expect("FCStd inspection reports dialect layers")
            .primary()
            .admission(),
        cadmpeg_core::dialect::Admission::AdmittedUnverified {
            using: Some(cadmpeg_core::dialect::DialectId::pinned("fcstd:schema-4")),
        }
    );

    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes.clone()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("container-only decode of an undeclared schema");
    assert_eq!(result.report().dialects(), summary.dialects());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == unverified)
            .count(),
        1
    );
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.dialect().unwrap().declared()["schema_version"], "5");
    assert!(!source
        .dialect()
        .unwrap()
        .declared()
        .contains_key("program_version"));

    // A full decode attempts the nearest declared strategy rather than
    // refusing on the discriminant, and reports the same match and loss.
    let result = decode(bytes);
    assert_eq!(result.report().dialects(), summary.dialects());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == unverified)
            .count(),
        1
    );
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.dialect().unwrap().declared()["schema_version"], "5");
}

/// A document that differs from a schema-4 document only in its declared
/// version decodes to the same model.
///
/// This is what separates an attempt from a label: the undeclared version runs
/// the schema-4 parse path, so the recovered content is byte-identical to the
/// schema-4 baseline while the identity, admission, and charged loss differ.
#[test]
fn an_undeclared_schema_version_alone_recovers_the_schema_four_content() {
    let baseline = decode(CORE_OPERATIONS.to_vec());
    let drifted = decode(rewrite_schema_version(CORE_OPERATIONS, "5"));

    let model = |result: &cadmpeg_ir::codec::DecodeResult| {
        serde_json::to_string(&result.ir().model).expect("serialize model")
    };
    assert_eq!(model(&drifted), model(&baseline));
    assert!(!drifted.ir().model.bodies.is_empty());

    assert_eq!(
        baseline
            .report()
            .dialects()
            .as_ref()
            .expect("FCStd decode reports dialect layers")
            .primary()
            .admission(),
        cadmpeg_core::dialect::Admission::Admitted
    );
    assert_eq!(
        drifted
            .report()
            .dialects()
            .as_ref()
            .expect("FCStd decode reports dialect layers")
            .primary()
            .admission(),
        cadmpeg_core::dialect::Admission::AdmittedUnverified {
            using: Some(cadmpeg_core::dialect::DialectId::pinned("fcstd:schema-4")),
        }
    );

    let unverified = crate::loss::FreecadLossCode::SourceDialectUnverified
        .note(String::new())
        .code;
    let charged = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == unverified)
            .count()
    };
    // The declared versions carry no dialect loss; only the drifted one does.
    assert_eq!(charged(&baseline), 0);
    assert_eq!(charged(&drifted), 1);
    // Schema 2 is excluded: it selects the `Features` vocabulary, which this
    // `Objects` document does not carry, so the version cannot be varied alone.
    // `schema_two_uses_the_feature_envelope_and_common_property_grammar` covers
    // that row on a document of its own.
    for version in ["3", "4"] {
        let declared = decode(rewrite_schema_version(CORE_OPERATIONS, version));
        assert_eq!(charged(&declared), 0, "schema {version} charged a loss");
        assert_eq!(
            declared
                .report()
                .dialects()
                .as_ref()
                .expect("FCStd decode reports dialect layers")
                .primary()
                .admission(),
            cadmpeg_core::dialect::Admission::Admitted
        );
    }
}

/// The attempt is self-limiting: an element vocabulary that does not fit still
/// fails, under an undeclared version exactly as under schema 4.
#[test]
fn a_structurally_alien_document_under_a_foreign_version_still_fails() {
    // The schema-2 `Features` vocabulary is alien to the `Objects` strategy the
    // undeclared version attempts, so the parse fails on the missing section.
    let alien = "<Features Count=\"0\"></Features><FeatureData Count=\"0\"></FeatureData>";
    for version in ["5", "4"] {
        let bytes = archive(&format!(
            "<Document SchemaVersion=\"{version}\" FileVersion=\"1\">{alien}</Document>"
        ));
        FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect_err("alien element vocabulary must fail");
    }
}
