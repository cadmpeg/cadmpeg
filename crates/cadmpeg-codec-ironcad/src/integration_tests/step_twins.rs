// SPDX-License-Identifier: Apache-2.0
//! The committed scenes against the STEP files that `IronCAD` exported from
//! them: the same bodies, placed at the same transforms.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::compare::{compare, CompareOptions, Verdict};
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::value::Quantity;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::test_support::corpus;
use crate::IronCadCodec;

/// Agreement of a decoded translation with the STEP translation, which the
/// export rounds to 15 significant digits.
const EPS_TRANSLATION_MM: f64 = 1.0e-9;
/// Agreement of a decoded rotation coefficient with the STEP coefficient.
const EPS_ROTATION: f64 = 1.0e-12;
/// Allowance of a measured volume beyond its stated numeric errors, relative
/// to the volume.
const VOLUME_RELATIVE_FLOOR: f64 = 1.0e-9;

fn scene(name: &str) -> CadIr {
    IronCadCodec
        .decode(
            &mut Cursor::new(corpus(&format!("{name}.ics"))),
            &DecodeOptions::default(),
        )
        .unwrap()
        .ir()
        .clone()
}

fn twin(name: &str) -> CadIr {
    cadmpeg_codec_step::StepCodec::default()
        .decode(
            &mut Cursor::new(corpus(&format!("{name}.stp"))),
            &DecodeOptions::default(),
        )
        .unwrap()
        .ir()
        .clone()
}

fn measured(ir: &CadIr) -> MeasureReport {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .unwrap();
    ctx.finish_session().unwrap();
    report
}

#[test]
fn assembly_scenes_agree_with_their_step_exports() {
    for name in ["Assembly_moved", "Assembly_nested", "Assembly_nested_moved"] {
        let (native, step) = (scene(name), twin(name));
        let comparison = compare(
            &native,
            &measured(&native),
            &step,
            &measured(&step),
            &CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(comparison.verdict(), Verdict::Agree, "{name}");
    }
}

/// The scene holds two equal boxes and two equal cylinders, so bodies pair
/// by the name of the occurrence that places them.
#[test]
fn each_part_of_the_mixed_scene_is_placed_where_its_step_export_places_it() {
    let name = "Ironcad_2025_simple_test_parts";
    let (native, step) = (scene(name), twin(name));
    let (native_measures, step_measures) = (measured(&native), measured(&step));
    let volume = |ir: &CadIr, report: &MeasureReport, prototype| {
        let definition = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| &definition.id == prototype)
            .unwrap();
        let [body] = definition.bodies.as_slice() else {
            return None;
        };
        let row = report
            .bodies()
            .iter()
            .find(|row| row.body() == body.as_str())
            .unwrap();
        let measured = row.volume().as_ref().and_then(Quantity::measured).unwrap();
        Some((measured.value().value.get(), measured.numeric_error().get()))
    };
    let mut parts = 0;
    for occurrence in &native.model.occurrences {
        let partner = step
            .model
            .occurrences
            .iter()
            .find(|candidate| candidate.name == occurrence.name)
            .unwrap_or_else(|| panic!("{:?} has no STEP occurrence", occurrence.name));
        let rows = occurrence.transform.unwrap().affine_rows();
        let expected = partner.transform.unwrap().affine_rows();
        for (row, expected_row) in rows.iter().zip(&expected) {
            for column in 0..3 {
                assert!(
                    (row[column] - expected_row[column]).abs() <= EPS_ROTATION,
                    "{:?}",
                    occurrence.name
                );
            }
            assert!(
                (row[3] - expected_row[3]).abs() <= EPS_TRANSLATION_MM,
                "{:?}",
                occurrence.name
            );
        }
        let native_volume = volume(&native, &native_measures, &occurrence.prototype);
        let step_volume = volume(&step, &step_measures, &partner.prototype);
        if let (Some((a, a_error)), Some((b, b_error))) = (native_volume, step_volume) {
            let allowed = a_error + b_error + VOLUME_RELATIVE_FLOOR * b.abs();
            assert!(
                (a - b).abs() <= allowed,
                "{:?}: {a} != {b}",
                occurrence.name
            );
            parts += 1;
        }
    }
    assert_eq!(parts, 5);
}
