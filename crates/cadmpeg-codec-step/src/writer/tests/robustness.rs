// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for STEP (#233 item 3, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error, capacity-helper and `max_entities` runners from
//! `cadmpeg-test-support`; the residual checker with the STEP residual
//! positions; and the `REDUCED_FIELDS` row check.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    capacity_findings, injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::StepCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    let bytes = include_bytes!("../../../tests/fixtures/ap214_sheet.p21");
    StepCodec::default()
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .expect("decode the robustness seed")
        .ir()
        .clone()
}

fn request() -> TargetRequest<'static> {
    TargetRequest::Inherit
}

#[test]
fn injected_errors_succeed_and_name_the_record() {
    let seed = seed_golden();
    let findings = injected_error_findings(
        &seed,
        &StepCodec::default(),
        request(),
        &standard_injections(),
    );
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    // STEP has no fixed-width field; every count chains or splits records, so
    // there is no capacity row and no helper to probe (#233).
    assert!(crate::writer::CAPACITY_FIELDS.is_empty());
    let findings = capacity_findings(&[]);
    assert!(
        findings.is_empty(),
        "capacity findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = u64::try_from(seed.model.entity_count()).expect("entity count fits");
    let findings = max_entities_findings(&seed, &StepCodec::default(), request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn empty_document_charges_the_records_step_writes() {
    let ir = cadmpeg_ir::CadIr::empty();
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = 0;
    let error = StepCodec::default()
        .plan(input, request())
        .expect_err("STEP writes records for an empty document");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::write::EncodeError::ResourceLimit(_)
    ));
}

#[test]
fn reduced_cone_chart_row_matches_the_written_surface_and_names_it() {
    use cadmpeg_ir::codec::write::ReducedStorage;
    use cadmpeg_ir::geometry::analytic::ConeSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut seed = seed_golden();
    let surface = &mut seed.model.surfaces[0];
    let id = surface.id.to_string();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            -0.5,
        )
        .expect("cone"),
    ));
    let original = serde_json::to_value(&surface.geometry).expect("geometry JSON");
    let row = &crate::writer::REDUCED_FIELDS[0];
    assert_eq!(crate::writer::REDUCED_FIELDS.len(), 2);
    assert_eq!(row.path, "model.surfaces[].geometry");
    let ReducedStorage::Normalized { loss } = row.storage else {
        panic!("cone normalization")
    };
    let expected = cadmpeg_test_support::reduced::step_surface_chart(&original);
    assert_ne!(original, expected);

    let codec = StepCodec::default();
    let encoder: &dyn Encoder = &codec;
    let plan = encoder
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan cone");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write cone");
    assert!(report
        .losses
        .iter()
        .any(|note| note.code.to_string() == loss && note.message.contains(&id)));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode cone");
    let written = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Cone(_))
            )
        })
        .expect("written cone");
    assert_eq!(
        serde_json::to_value(&written.geometry).expect("written geometry JSON"),
        expected
    );
}

#[test]
fn signed_sphere_and_torus_radii_are_normalized_and_named() {
    use cadmpeg_ir::geometry::analytic::{SphereSurface, TorusSurface};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let center = Point3::new(1.0, 2.0, 3.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let cases = [
        SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(center, axis, reference, -2.0).expect("sphere"),
        ),
        SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(center, axis, reference, 5.0, -2.0).expect("torus"),
        ),
    ];
    for (index, geometry) in cases.into_iter().enumerate() {
        let mut seed = seed_golden();
        let surface = &mut seed.model.surfaces[0];
        let id = surface.id.to_string();
        surface.geometry = SurfaceGeometry::Solved(geometry);
        let codec = StepCodec::default();
        let plan = codec
            .plan(EncodeInput::new(&seed, None), request())
            .expect("plan surface");
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).expect("write surface");
        assert!(report.losses.iter().any(|loss| {
            loss.code == crate::loss::StepLossCode::AnalyticSurfaceNormalized.kind()
                && loss.message.contains(&id)
        }));
        let decoded = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode surface");
        assert!(decoded.ir().model.surfaces.iter().any(|surface| {
            match (index, surface.geometry.solved()) {
                (0, Some(SolvedSurfaceGeometry::Sphere(sphere))) => {
                    (sphere.radius().get() - 2.0).abs() < 1e-12
                }
                (1, Some(SolvedSurfaceGeometry::Torus(torus))) => {
                    (torus.major_radius().get() - 5.0).abs() < 1e-12
                        && (torus.minor_radius().get() - 2.0).abs() < 1e-12
                }
                _ => false,
            }
        }));
    }
}

#[test]
fn synthesized_records_carry_their_specified_residuals() {
    let seed = seed_golden();
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the residual output");
    let after = serde_json::to_value(decoded.ir()).expect("serialize the re-decode");
    let specs = residual_specs(&seed);
    let findings = residual_findings(&after, &report.losses, &specs);
    assert!(
        findings.is_empty(),
        "residual findings:\n{}",
        findings.join("\n")
    );
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.residual-unavailable"),
        "the writer charges no residual-unavailable loss on this seed: {:?}",
        report.losses
    );
}

/// Residual positions the STEP writer synthesizes, from `docs/formats/`.
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}
#[test]
fn max_entities_counts_each_written_step_record() {
    let ir = cadmpeg_ir::CadIr::empty();
    let codec = StepCodec::default();
    let mut bytes = Vec::new();
    codec
        .plan(EncodeInput::new(&ir, None), request())
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the empty document writes");
    let text = std::str::from_utf8(&bytes).expect("STEP is UTF-8");
    let records = u64::try_from(text.lines().filter(|line| line.starts_with('#')).count())
        .expect("record count fits");
    assert!(records > 0);
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = records;
    codec
        .plan(input, request())
        .expect("the exact limit writes");
    input.limits.max_entities = records - 1;
    let error = codec
        .plan(input, request())
        .expect_err("one fewer record is refused");
    let cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit) = error else {
        panic!("the entity ceiling returns a resource limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(
        (limit.limit, limit.used, limit.additional),
        (records - 1, records - 1, 1)
    );
}
