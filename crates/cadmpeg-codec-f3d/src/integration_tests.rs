// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized F3D and F3Z archives.

use cadmpeg_ir::codec::EncodeInput;
use cadmpeg_ir::codec::TargetRequest;
use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};

use crate::container::role;
use crate::test_support::*;
use crate::F3dCodec;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized Fusion archive should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert_f3d_native_parity(result.ir());
}

#[test]
fn f3d_pipeline_aligns_detection_inspection_container_roles_and_decode() {
    let bytes = f3d_with_smbh(&synthetic_geometry_smbh());
    assert_eq!(F3dCodec.detect(&bytes), Confidence::High);
    let summary = F3dCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("F3D inspection");
    assert_eq!(summary.format, "f3d");
    assert_eq!(summary.container_kind, "zip");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == role::BREP_SMBH));

    let result = decode(bytes);
    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result.source_fidelity().retained_records.is_empty());
    assert_valid(&result);
}

#[test]
fn geometry_pipeline_composes_topology_pcurves_freeform_and_procedural_families() {
    let fixtures = [
        (synthetic_geometry_smbh(), true),
        (synthetic_geometry_with_rational_pcurve_smbh(), true),
        (synthetic_geometry_with_helix_curve_smbh(), false),
        (synthetic_cyl_spl_sur_smbh(), true),
        (synthetic_profile_first_sweep_smbh(), true),
        (synthetic_compound_loft_smbh(), true),
    ];
    let mut saw_pcurve = false;
    let mut saw_nurbs = false;
    let mut saw_procedural_curve = false;
    let mut saw_procedural_surface = false;
    for (smbh, geometrically_consistent) in fixtures {
        let result = decode(f3d_with_smbh(&smbh));
        saw_pcurve |= !result.ir().model.pcurves.is_empty();
        saw_nurbs |= result.ir().model.curves.iter().any(|curve| {
            matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
            )
        }) || result.ir().model.surfaces.iter().any(|surface| {
            matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(_)
            )
        });
        saw_procedural_curve |= !result.ir().model.procedural_curves.is_empty();
        saw_procedural_surface |= !result.ir().model.procedural_surfaces.is_empty();
        if geometrically_consistent {
            assert_valid(&result);
        } else {
            assert!(result.ir().native.namespace("f3d").is_some());
        }
    }
    assert!(saw_pcurve && saw_nurbs && saw_procedural_curve && saw_procedural_surface);
}

#[test]
fn design_pipeline_correlates_protein_properties_history_sketches_and_configuration() {
    let protein = decode(f3d_with_smbh_and_protein(
        &synthetic_geometry_with_history_smbh(),
    ));
    let native = f3d_native(protein.ir());
    assert!(!native.design_record_headers.is_empty());
    assert!(!native.asm_histories.is_empty());
    assert!(!protein.ir().model.appearances.is_empty());
    assert_valid(&protein);

    let name = "FusionAssetName[Active]/DesignConfigurationTable.integration.dsgcfg";
    let payload = br#"{"configurations":{"wide":{"parameters":{"width":"25 mm"},"suppressed":["slot"]}},"active":"wide"}"#;
    let configured = decode(f3d_with_configuration(
        &synthetic_geometry_smbh(),
        name,
        payload,
    ));
    assert_eq!(configured.ir().model.configurations.len(), 1);
    assert!(configured.ir().model.configurations[0].active.is_active());
    assert_valid(&configured);
}

#[test]
fn preserved_source_pipeline_applies_semantic_geometry_edits_without_losing_archive_entries() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = decode(source);
    let mut edited = decoded.ir().clone();
    edited.model.points[0].position.x = 2.5;
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    let mut bytes = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut bytes)
        .expect("preserved F3D write");
    let round_trip = decode(bytes);
    assert_eq!(round_trip.ir().model.points[0].position.x, 2.5);
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_valid(&round_trip);
}

#[test]
fn source_less_writer_pipeline_emits_a_fresh_valid_archive() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.source = None;
    drop(f3d_native_mut(&mut ir));
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("source-less F3D encode");
    assert_eq!(F3dCodec.detect(&bytes), Confidence::High);
    let round_trip = decode(bytes);
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(round_trip.ir().model.faces.len(), 6);
    assert_valid(&round_trip);
}

#[test]
fn f3z_pipeline_recursively_merges_occurrences_and_reports_reference_cycles() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", CHILD_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let merged = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    ));
    assert!(merged.ir().model.bodies[0].id.0.contains(&format!(
        "xref/{XREF_ROLE}/occurrence-0/xref/{CHILD_ROLE}/occurrence-0/"
    )));
    assert_valid(&merged);

    let cyclic_middle =
        f3d_without_brep("assembly-design", "middle.f3d", &[("root.f3d", CHILD_ROLE)]);
    let cyclic = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", cyclic_middle.as_slice()),
        ],
    ));
    assert!(cyclic
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("reference cycle")));
}

#[test]
fn container_only_pipeline_retains_native_sections_without_semantic_projection() {
    let bytes = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let result = F3dCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("container-only F3D decode");
    assert!(result.report().container_only);
    assert!(!result.report().geometry_transferred);
    assert!(result.ir().model.bodies.is_empty());
    assert!(!result.source_fidelity().retained_records.is_empty());
    assert!(result.ir().native.namespace("f3d").is_some());
}

#[test]
fn a_version_only_manifest_drift_decodes_as_unverified_and_charges_the_recovery() {
    // The archive differs from the known-version archive in the manifest
    // version field alone, so the whole decode runs the same code the known
    // version runs. What changes is the identity claim: the row is the
    // recovery row, the admission names the strategy applied, and the report
    // charges the dialect-unverified loss.
    let known = decode(f3d_with_smbh(&synthetic_geometry_smbh()));
    let drifted = decode(f3d_with_smbh_and_manifest_version(
        &synthetic_geometry_smbh(),
        "3-3-0-0",
    ));

    assert!(drifted.report().geometry_transferred);
    assert_eq!(
        drifted.ir().model.bodies.len(),
        known.ir().model.bodies.len()
    );

    let matched = drifted
        .report()
        .dialects
        .first()
        .expect("the primary layer is classified");
    assert_eq!(
        matched
            .dialect
            .as_ref()
            .map(cadmpeg_core::dialect::DialectId::as_str),
        Some("f3d:unknown")
    );
    assert_eq!(
        matched.admission,
        cadmpeg_core::dialect::Admission::AdmittedUnverified {
            nearest: cadmpeg_core::dialect::DialectId::pinned("f3d:manifest-3-2-0-0"),
        }
    );
    assert_eq!(matched.declared["top_level_manifest_version"], "3-3-0-0");

    let expected = crate::loss::F3dLossCode::SourceDialectUnverified.kind();
    assert!(
        drifted
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == expected),
        "the recovery must be charged"
    );
    assert!(
        !known
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == expected),
        "a known version charges no recovery"
    );
}
