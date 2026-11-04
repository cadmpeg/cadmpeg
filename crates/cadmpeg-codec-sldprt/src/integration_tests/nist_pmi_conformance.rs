// SPDX-License-Identifier: Apache-2.0
//! NIST FTC specimen PMI against the definition workbook and STEP source.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::pmi::{
    DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
    PmiTarget,
};
use cadmpeg_ir::CadIr;

use crate::SldprtCodec;

const MM_PER_INCH: f64 = 25.4;
const VALUE_TOLERANCE: f64 = 1.0e-8;

fn specimen(name: &str) -> DecodeResult {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/sldprt")
        .join(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
}

fn assert_dimension(ir: &CadIr, kind: &DimensionKind, nominal: f64, deviation: f64) {
    assert!(
        ir.model.pmi.iter().any(|annotation| {
            let PmiDefinition::Dimension(dimension) = &annotation.definition else {
                return false;
            };
            let Some(nominal_value) = dimension.nominal() else {
                return false;
            };
            let Some(DimensionTolerance::PlusMinus { lower, upper }) = dimension.tolerance() else {
                return false;
            };
            dimension.kind() == kind
                && (nominal_value.value.get() - nominal * MM_PER_INCH).abs() < VALUE_TOLERANCE
                && (lower.value.get() + deviation * MM_PER_INCH).abs() < VALUE_TOLERANCE
                && (upper.value.get() - deviation * MM_PER_INCH).abs() < VALUE_TOLERANCE
                && annotation
                    .targets
                    .iter()
                    .any(|target| matches!(target, PmiTarget::Face { .. }))
        }),
        "missing {kind:?} {nominal} ± {deviation} inches with a face target"
    );
}

fn datum_names(ir: &CadIr, system: &cadmpeg_ir::ids::PmiId) -> Vec<String> {
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|pmi| &pmi.id == system)
        .expect("datum system");
    let PmiDefinition::DatumSystem { references } = &annotation.definition else {
        panic!("datum system")
    };
    references
        .as_slice()
        .iter()
        .map(|reference| {
            let datum = ir
                .model
                .pmi
                .iter()
                .find(|pmi| pmi.id == reference.datum)
                .expect("referenced datum");
            let PmiDefinition::Datum { identification } = &datum.definition else {
                panic!("datum")
            };
            identification.clone()
        })
        .collect()
}

fn assert_counterbore_targets(ir: &CadIr) {
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| {
            let PmiDefinition::Dimension(dimension) = &annotation.definition else {
                return false;
            };
            dimension.kind() == &DimensionKind::Diameter
                && dimension.nominal().is_some_and(|value| {
                    (value.value.get() - 0.625 * MM_PER_INCH).abs() < VALUE_TOLERANCE
                })
        })
        .expect("counterbore dimension");
    let cylinders = annotation
        .targets
        .iter()
        .filter_map(|target| {
            let PmiTarget::Face { face } = target else {
                return None;
            };
            let face = ir
                .model
                .faces
                .iter()
                .find(|candidate| &candidate.id == face)?;
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|candidate| candidate.id == face.surface)?;
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface.geometry.solved() else {
                return None;
            };
            Some(cylinder)
        })
        .collect::<Vec<_>>();
    for (x, z) in [(5.25, -2.25), (-5.25, -2.25), (-5.25, -7.25), (5.25, -7.25)] {
        assert!(
            cylinders.iter().any(|cylinder| {
                let origin = cylinder.origin().get();
                (origin.x - x * MM_PER_INCH).abs() < VALUE_TOLERANCE
                    && (origin.y - 1.25 * MM_PER_INCH).abs() < VALUE_TOLERANCE
                    && (origin.z - z * MM_PER_INCH).abs() < VALUE_TOLERANCE
                    && (cylinder.radius().get() - 0.625 * MM_PER_INCH / 2.0).abs() < VALUE_TOLERANCE
            }),
            "missing counterbore face at ({x}, 1.25, {z}) inches"
        );
    }
}

fn assert_tolerance<'a>(
    ir: &'a CadIr,
    kind: &GeometricToleranceKind,
    magnitude: f64,
    datums: &[&str],
    modifiers: &[&str],
) -> &'a PmiAnnotation {
    ir.model.pmi.iter().find(|annotation| {
        let PmiDefinition::GeometricTolerance { tolerance, magnitude: value, datum_system, modifiers: actual_modifiers, .. } = &annotation.definition else { return false };
        tolerance == kind
            && (value.get().value.get() - magnitude * MM_PER_INCH).abs() < VALUE_TOLERANCE
            && datum_system.as_ref().map(|id| datum_names(ir, id)).unwrap_or_default() == datums
            && actual_modifiers == modifiers
            && annotation.targets.iter().any(|target| matches!(target, PmiTarget::Face { .. }))
    }).unwrap_or_else(|| panic!("missing {kind:?} {magnitude} inches, datums {datums:?}, modifiers {modifiers:?}, and a face target"))
}

fn assert_cylinder_targets(ir: &CadIr, annotation: &PmiAnnotation, diameter: f64, count: usize) {
    let faces = annotation
        .targets
        .iter()
        .filter_map(|target| {
            let PmiTarget::Face { face } = target else {
                return None;
            };
            let face = ir
                .model
                .faces
                .iter()
                .find(|candidate| &candidate.id == face)?;
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|candidate| candidate.id == face.surface)?;
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface.geometry.solved() else {
                return None;
            };
            ((cylinder.radius().get() * 2.0 - diameter * MM_PER_INCH).abs() < VALUE_TOLERANCE)
                .then_some(&face.id)
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        faces.len() >= count,
        "expected {count} cylinder faces of diameter {diameter} inches, found {}",
        faces.len()
    );
}

fn assert_spherical_position(ir: &CadIr) {
    let annotation = assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.025,
        &["D", "B", "C"],
        &[],
    );
    let PmiDefinition::GeometricTolerance { constraints, .. } = &annotation.definition else {
        panic!("spherical position definition")
    };
    let [zone] = constraints.zones.as_slice() else {
        panic!("one spherical tolerance zone")
    };
    assert_eq!(zone.form.as_str(), "spherical");
}

#[test]
fn ftc_06_0812_nominal_and_counterbore_parameter_remain_distinct_source_values() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    let diameter = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter49"))
        .expect("Diameter49 PMI");
    let PmiDefinition::Dimension(dimension) = &diameter.definition else {
        panic!("Diameter49 dimension")
    };
    let nominal = dimension.nominal().expect("Diameter49 nominal");
    assert!((nominal.value.get() - 0.812 * MM_PER_INCH).abs() < VALUE_TOLERANCE);

    assert!(ir.model.parameters.iter().any(|parameter| {
        parameter.name == "C'Bore Dia."
            && parameter.value.as_ref().is_some_and(|value| {
                let cadmpeg_ir::features::ParameterValue::Length(value) = value else {
                    return false;
                };
                (value.get() - 0.8125 * MM_PER_INCH).abs() < VALUE_TOLERANCE
            })
    }));
}

#[test]
fn ftc_06_hole_dimensions_match_nist_and_step() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    assert_dimension(ir, &DimensionKind::Diameter, 0.415, 0.008);
    assert_dimension(ir, &DimensionKind::Diameter, 0.625, 0.020);
    assert_counterbore_targets(ir);
    // The STEP source gives 0.5625 rather than the workbook's 0.563.
    assert_dimension(ir, &DimensionKind::Diameter, 0.5625, 0.008);
    assert_dimension(ir, &DimensionKind::Diameter, 0.281, 0.008);
    assert_dimension(ir, &DimensionKind::Diameter, 0.40, 0.01);
    // The STEP source gives 0.125 rather than the workbook's 0.12.
    assert_dimension(ir, &DimensionKind::Radius, 0.125, 0.02);
    assert_dimension(ir, &DimensionKind::Size, 0.50, 0.02);
    assert_dimension(ir, &DimensionKind::Size, 0.5625, 0.02);
    assert_dimension(ir, &DimensionKind::Size, 0.31, 0.02);
    assert_spherical_position(ir);
    let hole_and_counterbore = assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.025,
        &["A", "B", "C"],
        &["maximum_material_requirement"],
    );
    assert_cylinder_targets(ir, hole_and_counterbore, 0.415, 2);
    assert_cylinder_targets(ir, hole_and_counterbore, 0.625, 2);
    let hole = assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.015,
        &["C", "A", "B"],
        &[],
    );
    assert_cylinder_targets(ir, hole, 0.5625, 2);
    let counterbore = assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.025,
        &["C", "A", "B"],
        &["maximum_material_requirement"],
    );
    // The STEP source gives 0.8125 rather than the workbook's 0.813.
    assert_cylinder_targets(ir, counterbore, 0.8125, 2);
    let hole = assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.015,
        &["E", "A", "B"],
        &[],
    );
    assert_cylinder_targets(ir, hole, 0.281, 4);
    let counterbore = assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.020,
        &["F"],
        &["maximum_material_requirement"],
    );
    assert_cylinder_targets(ir, counterbore, 0.40, 4);
}

#[test]
fn ftc_08_tangent_and_free_state_tolerances_match_nist() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    assert_tolerance(
        ir,
        &GeometricToleranceKind::Parallelism,
        0.015,
        &["D"],
        &["tangent_plane"],
    );
    assert_tolerance(
        ir,
        &GeometricToleranceKind::Position,
        0.020,
        &["A", "B"],
        &["maximum_material_requirement", "free_state"],
    );
}

#[test]
fn ftc_08_common_datum_compartments_match_nist() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    for (magnitude, expected, modifier) in [
        (0.04, ["D", "E", "F"], None),
        (0.015, ["D", "H", "J"], Some("maximum_material_requirement")),
    ] {
        assert!(
            ir.model.pmi.iter().any(|annotation| {
                let PmiDefinition::GeometricTolerance {
                    tolerance: GeometricToleranceKind::SurfaceProfile,
                    magnitude: value,
                    datum_system: Some(system),
                    ..
                } = &annotation.definition
                else {
                    return false;
                };
                if (value.get().value.get() - magnitude * MM_PER_INCH).abs() >= VALUE_TOLERANCE
                    || datum_names(ir, system) != expected
                    || !annotation
                        .targets
                        .iter()
                        .any(|target| matches!(target, PmiTarget::Face { .. }))
                {
                    return false;
                }
                let Some(datum_system) = ir.model.pmi.iter().find(|pmi| &pmi.id == system) else {
                    return false;
                };
                let PmiDefinition::DatumSystem { references } = &datum_system.definition else {
                    return false;
                };
                let [primary, second, third] = references.as_slice() else {
                    return false;
                };
                primary.precedence < second.precedence
                    && second.precedence == third.precedence
                    && primary.modifiers.is_empty()
                    && second.modifiers.iter().map(String::as_str).eq(modifier)
                    && third.modifiers.iter().map(String::as_str).eq(modifier)
            }),
            "missing surface profile {magnitude} inches with common datum references {expected:?}"
        );
    }
}
