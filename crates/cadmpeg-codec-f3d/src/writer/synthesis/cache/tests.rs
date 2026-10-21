// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::native_test::TestEncode;
use cadmpeg_ir::codec::{write::ArenaDisposition, Codec, DecodeOptions};
use std::io::Cursor;

fn decode(smbh: &[u8]) -> CadIr {
    crate::F3dCodec
        .decode(
            &mut Cursor::new(crate::test_support::zip_test::f3d_with_smbh(smbh)),
            &DecodeOptions::default(),
        )
        .unwrap()
        .into_parts()
        .0
}

#[test]
fn generated_cache_is_reported_without_reducing_the_construction() {
    let decoded = crate::F3dCodec
        .decode(
            &mut Cursor::new(crate::test_support::zip_test::f3d_with_smbh(
                &crate::test_support::smbh_surfaces_test::synthetic_cyl_spl_sur_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("decode the unchanged extrusion with its rejected source cache");
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == F3dLossCode::SurfaceCacheRejected.kind()
            && loss.message.contains("fit tolerance Some(0.02)")
    }));
    let source = decoded.into_parts().0;
    let expected_fit = source.tolerances.linear.get();
    let source_surface = source
        .model
        .surfaces
        .iter()
        .find(|row| row.geometry.procedural_construction().is_some())
        .unwrap();
    assert!(source_surface.geometry.solved_cache().is_none());
    assert_eq!(
        source.model.procedural_surfaces[0].cache_fit_tolerance(),
        None
    );
    let input_construction = source.model.procedural_surfaces[0].clone();
    let before = cadmpeg_ir::validate_neutral(&source, vec![]).unwrap();
    assert!(before.is_ok(), "{before:#?}");
    let mut prepared = source.clone();
    let mut losses = vec![];
    prepare(&mut prepared, &mut losses).unwrap();
    assert_eq!(
        prepared.model.procedural_surfaces.len(),
        source.model.procedural_surfaces.len()
    );
    assert_eq!(
        prepared.model.procedural_surfaces[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(expected_fit)
    );
    assert_eq!(source.model.procedural_surfaces[0], input_construction);
    assert!(source_surface.geometry.solved_cache().is_none());
    assert_eq!(prepared.model.curves, source.model.curves);
    let surface = prepared
        .model
        .surfaces
        .iter()
        .find(|row| row.id == source_surface.id)
        .unwrap();
    let cache = surface.geometry.solved_cache().unwrap();
    let point = cadmpeg_ir::eval::decode::surface_point_solved(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        cache,
        0.5,
        0.25,
    )
    .unwrap()
    .get();
    // Unchanged quadratic C(.5)=(10,10,0) mm plus .25*(0,0,20).
    assert_eq!(point, cadmpeg_ir::math::Point3::new(10.0, 10.0, 5.0));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::WriterSurfaceCacheGenerated.kind()
    );
    assert!(losses[0].message.contains(source_surface.id.as_str()));
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Info);
    assert!(losses[0].message.contains(
        "a new target fit contract is derived from the current document linear tolerance"
    ));
    assert!(losses[0]
        .message
        .contains(&format!("fit {:?} mm", Some(expected_fit))));
    let mut retained_construction = prepared.model.procedural_surfaces[0].clone();
    retained_construction
        .clear_fitted_cache_contract()
        .expect("remove only the new target cache contract for intrinsic comparison");
    assert_eq!(retained_construction, input_construction);
    let after = cadmpeg_ir::validate_neutral(&prepared, vec![]).unwrap();
    assert!(after.is_ok(), "{after:#?}");

    let mut encoded = Vec::new();
    let written = crate::F3dCodec.encode(&source, &mut encoded).unwrap();
    assert_eq!(
        written
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::Surface),
        ArenaDisposition::Reported
    );
    assert_eq!(
        written
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::ProceduralSurface),
        ArenaDisposition::Reported
    );
    assert_eq!(
        written
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::Curve),
        ArenaDisposition::Written
    );
    assert!(written
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::WriterSurfaceCacheGenerated.kind()));
    let round_trip = crate::F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let validity = cadmpeg_ir::validate_neutral(round_trip.ir(), vec![]).unwrap();
    assert!(validity.is_ok(), "{validity:#?}");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(expected_fit)
    );
    let mut current = round_trip.ir().model.procedural_surfaces[0].clone();
    current
        .clear_fitted_cache_contract()
        .expect("remove only the derived output cache contract");
    assert_eq!(
        crate::test_support::smbh_revision_test::scrubbed_definition(current.definition()),
        crate::test_support::smbh_revision_test::scrubbed_definition(
            input_construction.definition()
        )
    );
    assert_eq!(source.model.procedural_surfaces[0], input_construction);
    // The cache and its new fit contract are Reported; unchanged directrix
    // geometry and all other Written arenas retain full conservation.
    cadmpeg_test_support::contract::assert_written_arenas_agree(&written, &source, round_trip.ir());
}

#[test]
fn valid_existing_cache_is_unchanged_and_remains_written() {
    let source = decode(&crate::test_support::procedural_test::exact_nurbs_edit_control(false));
    let before = cadmpeg_ir::validate_neutral(&source, vec![]).unwrap();
    assert!(before.is_ok(), "{before:#?}");
    assert!(source
        .model
        .surfaces
        .iter()
        .any(|surface| surface.geometry.solved_cache().is_some()));
    let mut prepared = source.clone();
    let mut losses = vec![];
    prepare(&mut prepared, &mut losses).unwrap();
    assert_eq!(prepared.model, source.model);
    assert!(losses.is_empty());
    let mut encoded = Vec::new();
    let written = crate::F3dCodec.encode(&source, &mut encoded).unwrap();
    assert_eq!(
        written
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::Surface),
        ArenaDisposition::Written
    );
    assert_eq!(
        written
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::ProceduralSurface),
        ArenaDisposition::Written
    );
    assert!(!written
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::WriterSurfaceCacheGenerated.kind()));
    let round_trip = crate::F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let validity = cadmpeg_ir::validate_neutral(round_trip.ir(), vec![]).unwrap();
    assert!(validity.is_ok(), "{validity:#?}");
    cadmpeg_test_support::contract::assert_written_arenas_agree(&written, &source, round_trip.ir());
}
