// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

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
                .find(|surface| surface.id.0 == surface_id)
                .unwrap();
            match (entity_type, &surface.geometry) {
                (190, cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. }) => {
                    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
                }
                (192, cadmpeg_ir::geometry::SurfaceGeometry::Cylinder { radius, .. })
                    if *radius == 2.0 => {}
                (
                    194,
                    cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                        radius, half_angle, ..
                    },
                ) if *radius == 2.0
                    && (*half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-15 => {}
                (196, cadmpeg_ir::geometry::SurfaceGeometry::Sphere { radius, .. })
                    if *radius == 2.0 => {}
                (
                    198,
                    cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                        major_radius,
                        minor_radius,
                        ..
                    },
                ) if *major_radius == 4.0 && *minor_radius == 1.0 => {}
                _ => panic!(
                    "unexpected type {entity_type} form {form} projection: {:?}",
                    surface.geometry
                ),
            }
            assert!(cadmpeg_ir::eval::surface_point(&surface.geometry, 0.25, 0.5).is_some());
            assert!(
                result.report().losses.is_empty(),
                "{:#?}",
                result.report().losses
            );
            let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
            .all(|surface| surface.id.0 != "iges:model:surface#D7"));
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
            .all(|surface| surface.id.0 != "iges:model:surface#D7"));
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
        .all(|surface| surface.id.0 != "iges:model:surface#D7"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("IGES entity type 192 form 1")
            && loss.message.contains("prohibited transformation")
    }));
}
