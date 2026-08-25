// SPDX-License-Identifier: Apache-2.0
//! Lossless write round-trip invariant over the committed IGES fixtures.
//!
//! An export whose report carries no losses must decode back to an IR that
//! [`cadmpeg_ir::diff`] reports as empty against the pre-write document.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::SourceFidelity;
use cadmpeg_test_support::golden::Harness;

use crate::test_support::{hyperbola_surface_of_revolution_file, tabulated_hyperbola_file};
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

/// Extension of the committed fixture inputs (matches `golden_tests`).
const FIXTURE_EXTENSION: &str = "igs";

/// Crate-relative regeneration hint used by the shared harness constructor.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

fn try_lossless_round_trip(
    stem: &str,
    original: &CadIr,
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
) -> bool {
    let Ok(plan) = Encoder::plan(&IgesEncoder::default(), EncodeInput { ir, fidelity }) else {
        return false;
    };
    let mut produced = Vec::new();
    let Ok(report) = plan.write_to(&mut produced) else {
        return false;
    };
    if !report.losses.is_empty() {
        return false;
    }
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(produced), &DecodeOptions::default())
        .unwrap_or_else(|e| panic!("{stem}: written file failed to decode: {e}"));
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{stem}: {:#?}", validation.findings);
    let d = cadmpeg_ir::diff::diff(original, round_trip.ir());
    assert!(d.is_empty(), "{stem}: no-loss export drifted: {d:#?}");
    true
}

#[test]
fn lossless_exports_round_trip_to_identical_ir() {
    let harness = harness();
    let mut written_any = false;
    for (stem, bytes) in harness.fixture_inputs() {
        let Ok(decoded) =
            IgesCodec.decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        else {
            continue;
        };
        if try_lossless_round_trip(&stem, decoded.ir(), decoded.ir(), None)
            || try_lossless_round_trip(
                &stem,
                decoded.ir(),
                decoded.ir(),
                Some(decoded.source_fidelity()),
            )
        {
            written_any = true;
        }
    }
    assert!(
        written_any,
        "no fixture took the lossless write path — test is vacuous"
    );
}

#[test]
fn semantic_writer_emits_type120_for_cacheless_hyperbola_revolution() {
    for version in [IgesVersion::V4_0, IgesVersion::V5_0, IgesVersion::V5_3] {
        assert_type120_round_trip(version);
    }
}

#[test]
fn semantic_writer_emits_type122_for_cacheless_hyperbola_extrusion() {
    const EPS_EXTRUSION_ROUND_TRIP: f64 = 1.0e-10;

    for version in [IgesVersion::V4_0, IgesVersion::V5_0, IgesVersion::V5_3] {
        let original = IgesCodec
            .decode(
                &mut Cursor::new(tabulated_hyperbola_file()),
                &DecodeOptions::default(),
            )
            .expect("hyperbola tabulated fixture decodes");
        let plan = Encoder::plan(
            &IgesEncoder::new(IgesWriteOptions { version }),
            EncodeInput {
                ir: original.ir(),
                fidelity: None,
            },
        )
        .expect("cache-less extrusion has an exact Type 122 writer path");
        let mut produced = Vec::new();
        let report = plan
            .write_to(&mut produced)
            .expect("Type 122 output writes");
        assert!(
            report.losses.iter().all(|loss| {
                loss.code != crate::loss::IgesLossCode::ProceduralReduced.kind()
                    && loss.code.taxonomy() != cadmpeg_ir::LossTaxonomy::GeometryNotTransferred
            }),
            "{version:?}: {:#?}",
            report.losses
        );
        let round_trip = IgesCodec
            .decode(&mut Cursor::new(produced), &DecodeOptions::default())
            .expect("Type 122 output decodes");
        assert!(
            round_trip
                .ir()
                .native
                .namespace("iges")
                .and_then(|namespace| namespace.arenas.get("entities"))
                .is_some_and(|entities| {
                    entities.iter().any(|entity| {
                        entity.field("entity_type").and_then(|value| value.as_i64()) == Some(122)
                    })
                }),
            "{version:?}: output has no Type 122 entity"
        );
        let source_surface = original
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                original
                    .ir()
                    .model
                    .procedural_surfaces
                    .iter()
                    .any(|procedural| {
                        procedural.surface == surface.id
                            && matches!(
                                &procedural.definition,
                                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { .. }
                            )
                    })
            })
            .expect("source extrusion surface");
        let round_surface = round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                round_trip
                    .ir()
                    .model
                    .procedural_surfaces
                    .iter()
                    .any(|procedural| {
                        procedural.surface == surface.id
                            && matches!(
                                &procedural.definition,
                                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { .. }
                            )
                    })
            })
            .expect("round-trip extrusion surface");
        let source_range = original
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.surface == source_surface.id)
            .and_then(|procedural| match &procedural.definition {
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
                    parameter_interval: Some(range),
                    ..
                } => Some(*range),
                _ => None,
            })
            .expect("source extrusion interval");
        let round_range = round_trip
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.surface == round_surface.id)
            .and_then(|procedural| match &procedural.definition {
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
                    parameter_interval: Some(range),
                    ..
                } => Some(*range),
                _ => None,
            })
            .expect("round-trip extrusion interval");
        let source_index = cadmpeg_ir::index::ModelIndex::new(original.ir());
        let round_index = cadmpeg_ir::index::ModelIndex::new(round_trip.ir());
        for fraction in [0.25, 0.75] {
            let source_parameter = source_range[0] + fraction * (source_range[1] - source_range[0]);
            let round_parameter = round_range[0] + fraction * (round_range[1] - round_range[0]);
            let source_point = cadmpeg_ir::eval::model_surface_point_by_id(
                &source_index,
                &source_surface.id,
                source_parameter,
                1.0,
            )
            .expect("source extrusion evaluates");
            let round_point = cadmpeg_ir::eval::model_surface_point_by_id(
                &round_index,
                &round_surface.id,
                round_parameter,
                1.0,
            )
            .expect("round-trip extrusion evaluates");
            assert!(
                source_point.distance(round_point) < EPS_EXTRUSION_ROUND_TRIP,
                "{version:?}: source={source_point:?} round_trip={round_point:?}"
            );
        }
        assert!(
            round_trip
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != crate::loss::IgesLossCode::EntityNotProjected.kind()),
            "{version:?}: {:#?}",
            round_trip.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
        assert!(
            validation.is_ok(),
            "{version:?}: {:#?}",
            validation.findings
        );
    }
}

fn assert_type120_round_trip(version: IgesVersion) {
    const EPS_REVOLUTION_ROUND_TRIP: f64 = 1.0e-10;

    let original = IgesCodec
        .decode(
            &mut Cursor::new(hyperbola_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .expect("hyperbola revolution fixture decodes");
    let plan = Encoder::plan(
        &IgesEncoder::new(IgesWriteOptions { version }),
        EncodeInput {
            ir: original.ir(),
            fidelity: None,
        },
    )
    .expect("cache-less revolution has an exact Type 120 writer path");
    let mut produced = Vec::new();
    let report = plan
        .write_to(&mut produced)
        .expect("Type 120 output writes");
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.code != crate::loss::IgesLossCode::ProceduralReduced.kind()),
        "{version:?}: {:#?}",
        report.losses
    );
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(produced), &DecodeOptions::default())
        .expect("Type 120 output decodes");
    assert_eq!(
        round_trip
            .ir()
            .source
            .as_ref()
            .expect("Type 120 output has source metadata")
            .attributes["iges_version"],
        version.name()
    );
    let source_surface = original
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            original
                .ir()
                .model
                .procedural_surfaces
                .iter()
                .any(|procedural| {
                    procedural.surface == surface.id
                        && matches!(
                            &procedural.definition,
                            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution { .. }
                        )
                })
        })
        .expect("source revolution surface");
    let Some(round_surface) = round_trip.ir().model.surfaces.iter().find(|surface| {
        round_trip
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .any(|procedural| {
                procedural.surface == surface.id
                    && matches!(
                        &procedural.definition,
                        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution { .. }
                    )
            })
    }) else {
        panic!(
            "{version:?}: round-trip revolution surface missing; losses={:#?}",
            round_trip.report().losses
        );
    };
    let source_index = cadmpeg_ir::index::ModelIndex::new(original.ir());
    let round_index = cadmpeg_ir::index::ModelIndex::new(round_trip.ir());
    let source_range = original
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == source_surface.id)
        .and_then(|procedural| match &procedural.definition {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
                parameter_interval: Some(range),
                ..
            } => Some(*range),
            _ => None,
        })
        .expect("source revolution interval");
    let round_range = round_trip
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == round_surface.id)
        .and_then(|procedural| match &procedural.definition {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
                parameter_interval: Some(range),
                ..
            } => Some(*range),
            _ => None,
        })
        .expect("round-trip revolution interval");
    for (fraction, angle) in [(0.25, 0.0), (0.75, 0.7)] {
        let source_parameter = source_range[0] + fraction * (source_range[1] - source_range[0]);
        let round_parameter = round_range[0] + fraction * (round_range[1] - round_range[0]);
        let source_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &source_index,
            &source_surface.id,
            source_parameter,
            angle,
        )
        .expect("source revolution evaluates");
        let round_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &round_index,
            &round_surface.id,
            round_parameter,
            angle,
        )
        .expect("round-trip revolution evaluates");
        assert!(
            source_point.distance(round_point) < EPS_REVOLUTION_ROUND_TRIP,
            "source={source_point:?} round_trip={round_point:?}"
        );
    }
    assert!(
        round_trip
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != crate::loss::IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        round_trip.report().losses
    );
}
