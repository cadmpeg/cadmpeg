use std::collections::BTreeSet;

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::pmi::{DimensionKind, DimensionTolerance, PmiDefinition, PmiTarget};

use super::{datum_names, specimen, MM_PER_INCH, VALUE_TOLERANCE};

fn named_position<'a>(ir: &'a cadmpeg_ir::CadIr, name: &str) -> &'a cadmpeg_ir::pmi::PmiAnnotation {
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name} PMI"));
    let PmiDefinition::GeometricTolerance {
        tolerance,
        magnitude,
        datum_system,
        modifiers,
        ..
    } = &annotation.definition
    else {
        panic!("{name} geometric tolerance")
    };
    assert_eq!(
        tolerance,
        &cadmpeg_ir::pmi::GeometricToleranceKind::Position
    );
    assert!((magnitude.get().value.get() - 0.025 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    assert_eq!(
        datum_system
            .as_ref()
            .map(|system| datum_names(ir, system))
            .unwrap_or_default(),
        vec![String::from("A"), String::from("B"), String::from("C")]
    );
    assert_eq!(
        modifiers.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["maximum_material_requirement"]
    );
    annotation
}

fn cylinder_geometry(
    ir: &cadmpeg_ir::CadIr,
    annotation: &cadmpeg_ir::pmi::PmiAnnotation,
) -> Vec<(f64, [f64; 3], [f64; 3])> {
    annotation
        .targets
        .iter()
        .map(|target| {
            let PmiTarget::Face { face } = target else {
                panic!(
                    "{} target is a face",
                    annotation.name.as_deref().unwrap_or("PMI")
                );
            };
            let face = ir
                .model
                .faces
                .iter()
                .find(|candidate| &candidate.id == face)
                .expect("target face");
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|candidate| candidate.id == face.surface)
                .expect("target surface");
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface.geometry.solved() else {
                panic!(
                    "{} target is a cylinder",
                    annotation.name.as_deref().unwrap_or("PMI")
                );
            };
            let origin = cylinder.origin().get();
            let axis = cylinder.frame().axis().as_raw();
            (
                cylinder.radius().get(),
                [origin.x, origin.y, origin.z],
                [axis.x, axis.y, axis.z],
            )
        })
        .collect()
}

fn axis_parallel(actual: [f64; 3], expected: [f64; 3]) -> bool {
    let dot = actual
        .into_iter()
        .zip(expected)
        .map(|(actual, expected)| actual * expected)
        .sum::<f64>();
    (dot.abs() - 1.0).abs() < VALUE_TOLERANCE
}

fn assert_radius_count(geometries: &[(f64, [f64; 3], [f64; 3])], radius: f64, count: usize) {
    assert_eq!(
        geometries
            .iter()
            .filter(|(actual, _, _)| (actual - radius).abs() < VALUE_TOLERANCE)
            .count(),
        count,
        "radius {radius} mm"
    );
}

#[test]
fn ftc_06_emits_base_datums_with_face_targets() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    let expected = [
        ("G", 1, "cylinder"),
        ("H", 1, "cylinder"),
        ("J", 2, "plane"),
        ("K", 2, "plane"),
    ];

    for (identification, target_count, surface_kind) in expected {
        let annotations = ir
            .model
            .pmi
            .iter()
            .filter(|annotation| {
                matches!(
                    &annotation.definition,
                    PmiDefinition::Datum { identification: actual }
                        if actual == identification
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            annotations.len(),
            1,
            "expected one datum {identification} annotation"
        );
        let annotation = annotations.first().expect("datum annotation");
        let faces = annotation
            .targets
            .iter()
            .filter_map(|target| match target {
                PmiTarget::Face { face } => Some(face),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(annotation.targets.len(), target_count);
        assert_eq!(faces.len(), target_count, "datum {identification} faces");

        for face_id in faces {
            let face = ir
                .model
                .faces
                .iter()
                .find(|face| &face.id == face_id)
                .expect("datum target face");
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .expect("datum target surface");
            let actual_kind = match surface.geometry.solved() {
                Some(SolvedSurfaceGeometry::Cylinder(_)) => "cylinder",
                Some(SolvedSurfaceGeometry::Plane(_)) => "plane",
                _ => "other",
            };
            assert_eq!(
                actual_kind, surface_kind,
                "datum {identification} target surface"
            );
        }
    }
}

#[test]
fn ftc_06_positions_target_typed_cylinders_only() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    let position = named_position(ir, "Position12");
    assert_eq!(position.targets.len(), 8);
    let geometry = cylinder_geometry(ir, position);
    assert_radius_count(&geometry, 7.9375, 4);
    assert_radius_count(&geometry, 5.2705, 4);
    for radius in [7.9375, 5.2705] {
        let expected = [
            (-133.35, -57.15),
            (-133.35, -184.15),
            (133.35, -57.15),
            (133.35, -184.15),
        ];
        let actual = geometry
            .iter()
            .filter(|(actual_radius, _, _)| (actual_radius - radius).abs() < VALUE_TOLERANCE)
            .collect::<Vec<_>>();
        for (x, z) in expected {
            assert!(
                actual.iter().any(|(_, origin, axis)| {
                    (origin[0] - x).abs() < VALUE_TOLERANCE
                        && (origin[2] - z).abs() < VALUE_TOLERANCE
                        && axis_parallel(*axis, [0.0, 1.0, 0.0])
                }),
                "missing {radius} mm cylinder axis at X={x} mm, Z={z} mm"
            );
        }
    }

    for (name, x, expected_datums) in [
        ("Position15", -76.2, ["D", "H"]),
        ("Position16", 76.2, ["D", "G"]),
    ] {
        let annotation = ir
            .model
            .pmi
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name} PMI"));
        let PmiDefinition::GeometricTolerance {
            tolerance,
            magnitude,
            datum_system,
            ..
        } = &annotation.definition
        else {
            panic!("{name} geometric tolerance")
        };
        assert_eq!(
            tolerance,
            &cadmpeg_ir::pmi::GeometricToleranceKind::Position
        );
        assert!((magnitude.get().value.get() - 0.015 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
        let datums = datum_system
            .as_ref()
            .map(|system| datum_names(ir, system))
            .unwrap_or_default();
        assert_eq!(
            datums,
            expected_datums
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            "{name} ordered datum references"
        );
        assert_eq!(annotation.targets.len(), 1);
        let geometry = cylinder_geometry(ir, annotation);
        let [(radius, origin, axis)] = geometry.as_slice() else {
            panic!("{name} has one cylinder target")
        };
        assert!((origin[0] - x).abs() < VALUE_TOLERANCE);
        assert!((origin[2] + 158.75).abs() < VALUE_TOLERANCE);
        assert!((radius - 3.175).abs() < VALUE_TOLERANCE);
        assert!(
            axis_parallel(*axis, [0.0, 1.0, 0.0]),
            "{name} cylinder axis direction"
        );
    }
}

#[test]
fn ftc_06_diameter8_targets_both_counted_spheres() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter8"))
        .expect("Diameter8 PMI");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("Diameter8 dimension")
    };
    assert_eq!(dimension.kind(), &DimensionKind::SphericalDiameter);
    let nominal = dimension.nominal().expect("Diameter8 nominal");
    assert!((nominal.value.get() - 1.250 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    let Some(DimensionTolerance::PlusMinus { lower, upper }) = dimension.tolerance() else {
        panic!("Diameter8 plus-minus tolerance")
    };
    assert!((lower.value.get() + 0.008 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    assert!((upper.value.get() - 0.008 * MM_PER_INCH).abs() < VALUE_TOLERANCE);

    for x in [-114.3, 114.3] {
        assert!(
            annotation.targets.iter().any(|target| {
                let PmiTarget::Face { face: face_id } = target else {
                    return false;
                };
                let Some(face) = ir.model.faces.iter().find(|face| &face.id == face_id) else {
                    return false;
                };
                let Some(surface) = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)
                else {
                    return false;
                };
                let Some(SolvedSurfaceGeometry::Sphere(sphere)) = surface.geometry.solved() else {
                    return false;
                };
                let center = sphere.center().get();
                (sphere.radius().get() - 15.875).abs() < VALUE_TOLERANCE
                    && (center.x - x).abs() < VALUE_TOLERANCE
                    && (center.y - 63.5).abs() < VALUE_TOLERANCE
                    && (center.z + 107.95).abs() < VALUE_TOLERANCE
            }),
            "Diameter8 must target the sphere centered at ({x}, 63.5, -107.95)"
        );
    }
}

#[test]
fn ftc_06_diameter8_reports_unresolved_counted_target_association() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "pmi.swift-spherical-diameter-target-unresolved"
    }));
    let annotation = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter8"))
        .expect("Diameter8 PMI");
    assert_eq!(annotation.targets.len(), 1);
}

#[test]
fn ftc_06_diameter7_maps_to_its_spherical_radius_owner() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    let annotation = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter7"))
        .expect("Diameter7 PMI");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("Diameter7 dimension")
    };
    assert_eq!(dimension.kind(), &DimensionKind::SphericalRadius);
    let nominal = dimension.nominal().expect("Diameter7 nominal");
    assert!((nominal.value.get() - 0.5 * MM_PER_INCH).abs() < VALUE_TOLERANCE);
    let Some(DimensionTolerance::PlusMinus { lower, upper }) = dimension.tolerance() else {
        panic!("Diameter7 plus-minus tolerance")
    };
    assert!((lower.value.get() + 0.508).abs() < VALUE_TOLERANCE);
    assert!((upper.value.get() - 0.508).abs() < VALUE_TOLERANCE);

    assert!(
        annotation.targets.iter().any(|target| {
            let PmiTarget::Face { face: face_id } = target else {
                return false;
            };
            let Some(face) = ir.model.faces.iter().find(|face| &face.id == face_id) else {
                return false;
            };
            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
            else {
                return false;
            };
            let Some(SolvedSurfaceGeometry::Sphere(sphere)) = surface.geometry.solved() else {
                return false;
            };
            let center = sphere.center().get();
            (sphere.radius().get() - 12.7).abs() < VALUE_TOLERANCE
                && center.x.abs() < VALUE_TOLERANCE
                && (center.y - 97.79).abs() < VALUE_TOLERANCE
                && (center.z + 15.875).abs() < VALUE_TOLERANCE
        }),
        "Diameter7 must retain the supported spherical-radius target"
    );
}
