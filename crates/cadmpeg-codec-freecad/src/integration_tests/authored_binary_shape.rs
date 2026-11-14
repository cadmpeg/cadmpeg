// SPDX-License-Identifier: Apache-2.0

use crate::brep::{
    ShapePayloadRecord, Tables, TextOrientation, TextShapeKind, TextSurface, TextTShapeGeometry,
};
use crate::FcstdCodec;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::test_support::round_trip::measured;
use cadmpeg_measure::value::Reading;
use std::io::Cursor;

const VOLUME_TOLERANCE: f64 = 1.0e-6;
const AREA_TOLERANCE: f64 = 1.0e-6;
const BOX_TOLERANCE: f64 = 1.0e-6;

#[test]
fn authored_binary_exact_shape_declared_reading_matches_source_dimensions() {
    let bytes =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    crate::test_support::test_archive::assert_valid_document(decoded.ir());
    let payloads = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let [payload] = payloads.as_slice() else {
        panic!("one shape payload")
    };
    let tables = Tables::from_payload(payload).unwrap();
    let [root] = tables.roots else {
        panic!("one shape root")
    };
    assert_eq!(root.orientation, TextOrientation::Forward);
    let compound = tables.tshapes.resolve(root.shape).unwrap();
    assert_eq!(compound.kind(), TextShapeKind::Compound);
    let [solid_use] = compound.children.as_slice() else {
        panic!("one solid use")
    };
    assert_eq!(solid_use.orientation, TextOrientation::Forward);
    let solid = tables.tshapes.resolve(solid_use.shape).unwrap();
    assert_eq!(solid.kind(), TextShapeKind::Solid);
    let [shell_use] = solid.children.as_slice() else {
        panic!("one shell use")
    };
    assert_eq!(shell_use.orientation, TextOrientation::Forward);
    let shell = tables.tshapes.resolve(shell_use.shape).unwrap();
    assert_eq!(shell.kind(), TextShapeKind::Shell);
    let sphere_use = shell
        .children
        .iter()
        .find(|child| {
            let face = tables.tshapes.resolve(child.shape).unwrap();
            matches!(&face.geometry, TextTShapeGeometry::Face { surface: Some(surface), .. }
            if matches!(surface.resolve(tables.surfaces).unwrap(), TextSurface::Sphere { .. }))
        })
        .unwrap();
    assert_eq!(sphere_use.orientation, TextOrientation::Forward);
    let sphere = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
            )
        })
        .unwrap();
    let face = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.surface == sphere.id)
        .unwrap();
    assert_eq!(face.sense, Sense::Forward);
    let report = measured("binary_exact_shape", decoded.ir()).unwrap();
    let [body] = report.bodies().as_slice() else {
        panic!("one fused body")
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("BinaryCarrier"))
        .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    ));
    let [output] = feature.evaluation.outputs().as_slice() else {
        panic!("one carrier output")
    };
    assert_eq!(body.body(), output.as_str());
    assert_eq!(*body.kind(), cadmpeg_ir::topology::BodyKind::Solid);
    let [shell] = body.shells().as_slice() else {
        panic!("one shell")
    };
    assert!(*shell.closed());
    let quantity = body.volume().as_ref().unwrap();
    let volume = if let Some(measured) = quantity.measured() {
        measured.value().value.get()
    } else {
        quantity
            .unavailable()
            .unwrap()
            .candidates()
            .iter()
            .find(|candidate| candidate.reading() == Reading::FaceSense)
            .unwrap()
            .value()
            .value
            .get()
    };
    let area = body
        .area()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    let intersection_height = 15.0_f64.sqrt();
    let expected_volume = std::f64::consts::PI * (2788.0 / 3.0 + 10.0 * intersection_height);
    let expected_area = std::f64::consts::PI * (345.0 + 2.0 * intersection_height);
    assert!((volume - expected_volume).abs() < VOLUME_TOLERANCE);
    assert!((area - expected_area).abs() < AREA_TOLERANCE);
    let bbox = body.bbox().measured().unwrap().value();
    for (actual, expected) in bbox
        .min
        .coordinates()
        .into_iter()
        .chain(bbox.max.coordinates())
        .zip([-8.0, -8.0, 0.0, 8.0, 8.0, 20.0])
    {
        assert!((actual.get() - expected).abs() < BOX_TOLERANCE);
    }
}

#[test]
fn authored_binary_exact_shape_has_confirmed_measures() {
    let bytes =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let report = measured("binary_exact_shape", decoded.ir()).unwrap();
    let [body] = report.bodies().as_slice() else {
        panic!("one fused body")
    };
    let volume = body.volume().as_ref().unwrap().measured().unwrap();
    let expected = std::f64::consts::PI * (2788.0 / 3.0 + 10.0 * 15.0_f64.sqrt());
    assert!((volume.value().value.get() - expected).abs() < VOLUME_TOLERANCE);
    assert!(body
        .area()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .reading()
        .is_none());
    assert!(body.bbox().measured().unwrap().reading().is_none());
}
