// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

use std::io::Cursor;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::test_surface_fixtures::{
    pointer_defined_surface_file, pointer_defined_surface_with_reference,
};
use crate::IgesCodec;
use super::{AnalyticDirectionError, DirectionError};

#[test]
fn analytic_direction_refusals_render_without_intermediate_strings() {
    let reasons = [
        (DirectionError::MissingEntry(7), "points to missing Directory entry D7"),
        (DirectionError::WrongTypeForm { sequence: 7, entity_type: 124, form: 1 }, "points to type 124 form 1 at D7, not type 123 form 0"),
        (DirectionError::NotDependent(7), "points to D7, which is not physically dependent"),
        (DirectionError::Transformed(7), "points to D7, which has a prohibited transformation"),
        (DirectionError::MissingParameters(7), "points to D7, whose Parameter Data record is missing"),
        (DirectionError::NonNumeric(7), "points to D7, whose direction components are not numeric"),
        (DirectionError::ZeroOrNonFinite(7), "points to D7, whose direction is zero or non-finite"),
    ];
    for (reason, expected) in reasons {
        assert_eq!(reason.to_string(), expected);
        assert_eq!(
            AnalyticDirectionError::Pointed { role: "plane axis", reason }.to_string(),
            format!("plane axis {expected}")
        );
    }
    assert_eq!(AnalyticDirectionError::MissingPointer("plane axis").to_string(), "plane axis pointer is missing, even, or non-integer");
    assert_eq!(AnalyticDirectionError::Collapse("plane axis").to_string(), "plane axis collapses under the surface transformation");
    assert_eq!(AnalyticDirectionError::SphereAxisCollapse.to_string(), "sphere axis collapses under its transformation");
}

fn assert_analytic_refusal(bytes: &[u8], operation: &str, retained: bool) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = cap;
        } else {
            policy.limits.max_collection_items = cap;
        }
        let result = IgesCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions { policy, ..DecodeOptions::default() },
        );
        match result {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                let dimension = if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems };
                assert_eq!(limit.dimension, dimension);
                if limit.operation == operation {
                    return;
                }
                let next = limit.used.checked_add(limit.additional).unwrap();
                assert!(next > cap, "limit did not advance from {cap}: {limit:?}");
                cap = next;
            }
            other => panic!("did not reach {operation} at cap {cap}: {other:?}"),
        }
    }
    panic!("did not reach {operation} within 4096 admission boundaries");
}

#[test]
fn analytic_surface_indexes_and_losses_refuse_limits() {
    let valid = pointer_defined_surface_file(190, 0);
    for operation in [
        "iges analytic-surface parameter index",
        "iges analytic-surface directory index",
        "iges analytic-surface slots",
        "iges analytic-surface decoded sequences",
    ] {
        assert_analytic_refusal(&valid, operation, false);
    }

    let invalid = owned_test_file(&[OwnedTestEntity {
        entity_type: 190,
        form: 0,
        label: "PLANE".into(),
        status: "00010000",
        parameters: "190,999,0;".into(),
    }]);
    assert_analytic_refusal(&invalid, "iges entity loss slots", false);
    assert_analytic_refusal(&invalid, "iges entity loss message", true);
}

#[test]
fn decode_projects_all_pointer_defined_analytic_surface_forms() {
    for entity_type in [190, 192, 194, 196, 198] {
        for form in [0, 1] {
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(pointer_defined_surface_file(entity_type, form)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let surface_id = format!(
                "iges:model:surface#D{}",
                if form == 1 {
                    7
                } else if entity_type == 196 {
                    3
                } else {
                    5
                }
            );
            let surface = result
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id.as_str() == surface_id)
                .unwrap();
            match (entity_type, &surface.geometry) {
                (
                    190,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                        plane_surface,
                    )),
                ) => {
                    let origin = plane_surface.origin();
                    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
                }
                (
                    192,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                        cylinder_surface,
                    )),
                ) if {
                    let radius = cylinder_surface.radius().get();
                    radius == 2.0
                } => {}
                (
                    194,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                        cone_surface,
                    )),
                ) if {
                    let radius = cone_surface.radius().get();
                    let half_angle = cone_surface.half_angle().get();
                    radius == 2.0 && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-15
                } => {}
                (
                    196,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                        sphere_surface,
                    )),
                ) if {
                    let radius = sphere_surface.radius().get();
                    radius == 2.0
                } => {}
                (
                    198,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                        torus_surface,
                    )),
                ) if {
                    let major_radius = torus_surface.major_radius().get();
                    let minor_radius = torus_surface.minor_radius().get();
                    major_radius == 4.0 && minor_radius == 1.0
                } => {}
                _ => panic!(
                    "unexpected type {entity_type} form {form} projection: {:?}",
                    surface.geometry
                ),
            }
            assert!(cadmpeg_ir::eval::surface_point(&surface.geometry, 0.25, 0.5).is_ok());
            assert!(
                result.report().losses.is_empty(),
                "{:#?}",
                result.report().losses
            );
            let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("resource allocation did not fail");
            assert!(validation.is_ok(), "{:#?}", validation.findings);
        }
    }
}

#[test]
fn decode_rejects_unresolved_form_one_analytic_surface_references() {
    for entity_type in [190, 192, 194, 196, 198] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(pointer_defined_surface_with_reference(
                    entity_type,
                    "",
                    123,
                    "00010000",
                    "123,1,0,0;",
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .all(|surface| surface.id.as_str() != "iges:model:surface#D7"));
        assert!(result.report().losses.iter().any(|loss| {
            loss.message
                .contains(&format!("IGES entity type {entity_type} form 1"))
                && loss
                    .message
                    .contains("reference direction pointer is missing")
        }));
    }

    for (pointer, reference_type, status, parameters, expected) in [
        (
            "9",
            123,
            "00010000",
            "123,1,0,0;",
            "missing Directory entry D9",
        ),
        (
            "5",
            110,
            "00010000",
            "110,0,0,0,1,0,0;",
            "not type 123 form 0",
        ),
        (
            "5",
            123,
            "00010000",
            "123,1HX,0,0;",
            "components are not numeric",
        ),
        (
            "5",
            123,
            "00000000",
            "123,1,0,0;",
            "not physically dependent",
        ),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(pointer_defined_surface_with_reference(
                    192,
                    pointer,
                    reference_type,
                    status,
                    parameters,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .all(|surface| surface.id.as_str() != "iges:model:surface#D7"));
        assert!(result.report().losses.iter().any(|loss| {
            loss.message.contains("IGES entity type 192 form 1") && loss.message.contains(expected)
        }));
    }

    let mut transformed_reference =
        pointer_defined_surface_with_reference(192, "5", 123, "00010000", "123,1,0,0;");
    let reference_marker = transformed_reference
        .windows(8)
        .position(|window| window == b"D      5")
        .unwrap();
    transformed_reference[reference_marker - 24..reference_marker - 16]
        .copy_from_slice(b"       9");
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_reference),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "iges:model:surface#D7"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("IGES entity type 192 form 1")
            && loss.message.contains("prohibited transformation")
    }));
}

#[test]
fn iges_sphere_radius_remains_positive_when_the_ir_carrier_accepts_signed_radii() {
    let source = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CENTER".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 196,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "196,1,-2;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("sphere radius is not positive and finite")));
}
