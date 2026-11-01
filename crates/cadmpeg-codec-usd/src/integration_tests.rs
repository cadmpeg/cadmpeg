// SPDX-License-Identifier: Apache-2.0
//! Codec-wide decode, encode, and round-trip checks through the facade.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeFailure, DecodeOptions};

use crate::test_support::{decode, encode, usdc_header};
use crate::UsdCodec;

fn fixtures() -> Vec<(String, Vec<u8>)> {
    cadmpeg_test_support::golden::Harness::new(env!("CARGO_MANIFEST_DIR"), "usda", "")
        .fixture_inputs()
}

#[test]
fn every_fixture_is_detected_with_high_confidence() {
    for (name, bytes) in fixtures() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        assert_eq!(
            UsdCodec.detect(&ctx, root).unwrap(),
            Confidence::High,
            "{name}"
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn a_binary_layer_of_an_undeclared_version_is_refused_with_its_identity() {
    let bytes = usdc_header([0, 3, 0]);
    let error = UsdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap_err();
    let DecodeFailure::Codec(CodecError::UnsupportedDialect { dialects, .. }) = error else {
        panic!("unexpected failure {error:?}");
    };
    assert_eq!(dialects.primary().dialect().as_str(), "usd:unknown");
    assert!(UsdCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .is_err());
}

/// Relative deviation that one binary32 rounding of a coordinate admits.
const EPS_BINARY32: f64 = 1e-7;

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() <= EPS_BINARY32 * left.abs().max(right.abs()).max(1.0)
}

/// Whether `result` holds a body with faces and no tessellation that has a
/// face with a loop or off a NURBS surface: the writer writes a mesh of such
/// a body.
fn meshed_on_write(result: &cadmpeg_ir::codec::DecodeResult) -> bool {
    let model = &result.ir().model;
    model.bodies.iter().any(|body| {
        let tessellated = model
            .tessellations
            .iter()
            .any(|tessellation| tessellation.body.as_ref() == Some(&body.id));
        !tessellated
            && model
                .shells
                .iter()
                .filter(|shell| body.regions.contains(&shell.region))
                .flat_map(|shell| shell.faces().iter())
                .filter_map(|face| model.faces.iter().find(|candidate| &candidate.id == face))
                .any(|face| {
                    !face.loops.is_empty()
                        || !model.surfaces.iter().any(|surface| {
                            surface.id == face.surface
                                && matches!(
                                    surface.geometry,
                                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                                        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(_)
                                    )
                                )
                        })
                })
    })
}

/// Every coordinate a model's tessellations and the poles of its curves that
/// no edge uses hold, in order.
fn coordinates(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<f64> {
    let model = &result.ir().model;
    let mut out = Vec::new();
    for tessellation in &model.tessellations {
        for vertex in tessellation.vertices() {
            let point = vertex.get();
            out.extend([point.x, point.y, point.z]);
        }
    }
    for curve in model.curves.iter().filter(|curve| {
        !model
            .edges
            .iter()
            .any(|edge| edge.carrier.curve() == Some(&curve.id))
    }) {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(nurbs),
        ) = &curve.geometry
        {
            for pole in nurbs.control_points() {
                let point = pole.get();
                out.extend([point.x, point.y, point.z]);
            }
        }
    }
    out
}

#[test]
fn every_fixture_round_trips_its_geometry() {
    for (name, bytes) in fixtures() {
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let first = decode(text);
        if meshed_on_write(&first) {
            continue;
        }
        let (written, _) = encode(first.ir());
        let second = decode(&written);
        let structure = |result: &cadmpeg_ir::codec::DecodeResult| {
            let model = &result.ir().model;
            (
                model
                    .tessellations
                    .iter()
                    .map(cadmpeg_ir::tessellation::Tessellation::triangles)
                    .collect::<Vec<_>>(),
                model
                    .curves
                    .iter()
                    .filter(|curve| {
                        !model
                            .edges
                            .iter()
                            .any(|edge| edge.carrier.curve() == Some(&curve.id))
                    })
                    .count(),
                model.surfaces.len(),
                model.occurrences.len(),
                model.appearances.len(),
                model.appearance_bindings.len(),
            )
        };
        assert_eq!(structure(&first), structure(&second), "{name}\n{written}");
        let (before, after) = (coordinates(&first), coordinates(&second));
        assert_eq!(before.len(), after.len(), "{name}");
        for (left, right) in before.iter().zip(&after) {
            assert!(close(*left, *right), "{name}: {left} != {right}");
        }
    }
}

/// Largest share of a cylinder's area that an inscribed mesh may miss.
const MESH_AREA_DEFICIT: f64 = 1e-2;

#[test]
fn a_cylinder_with_no_tessellation_is_written_as_an_inscribed_mesh() {
    let first = decode(
        "#usda 1.0\n(\n    upAxis = \"Z\"\n)\ndef Cylinder \"C\"\n{\n    double radius = 2\n    double height = 3\n    uniform token axis = \"Z\"\n}\n",
    );
    assert!(first.ir().model.tessellations.is_empty());
    let (written, _) = encode(first.ir());
    let second = decode(&written);
    let [mesh] = second.ir().model.tessellations.as_slice() else {
        panic!("one mesh\n{written}");
    };
    let points = mesh.vertices();
    let mut area = 0.0;
    let (mut radius, mut low, mut high) = (0.0_f64, f64::INFINITY, f64::NEG_INFINITY);
    for point in &points {
        let point = point.get();
        radius = radius.max(point.x.hypot(point.y));
        low = low.min(point.z);
        high = high.max(point.z);
    }
    for triangle in mesh.triangles() {
        let [a, b, c] = triangle.map(|at| points[usize::try_from(at).unwrap()].get());
        let (first, second) = (
            cadmpeg_ir::math::Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z),
            cadmpeg_ir::math::Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z),
        );
        let normal = cadmpeg_ir::math::Vector3::new(
            first.y * second.z - first.z * second.y,
            first.z * second.x - first.x * second.z,
            first.x * second.y - first.y * second.x,
        );
        area += (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt() / 2.0;
    }
    let exact = std::f64::consts::TAU * radius * (high - low + radius);
    assert!(area <= exact * (1.0 + EPS_BINARY32), "{area} > {exact}");
    assert!(
        area >= exact * (1.0 - MESH_AREA_DEFICIT),
        "{area} < {exact}"
    );
}

/// Significant digits within which a binary64 value survives the metre to
/// millimetre scaling of a round trip.
const ROUND_TRIP_DIGITS: usize = 12;

/// `text` with each real literal rounded to [`ROUND_TRIP_DIGITS`]
/// significant digits.
fn rounded(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        let real = token.contains(['.', 'e']);
        match token.parse::<f64>() {
            Ok(value) if real => {
                let value = if value == 0.0 { 0.0 } else { value };
                write!(
                    out,
                    "{value:.precision$e}",
                    precision = ROUND_TRIP_DIGITS - 1
                )
                .expect("write to a string");
            }
            _ => out.push_str(token),
        }
        token.clear();
    };
    for character in text.chars() {
        let continues = character.is_ascii_digit()
            || matches!(character, '.' | 'e')
            || (matches!(character, '-' | '+') && (token.is_empty() || token.ends_with('e')));
        if continues && !(character == 'e' && token.is_empty()) {
            token.push(character);
        } else {
            flush(&mut token, &mut out);
            out.push(character);
        }
    }
    flush(&mut token, &mut out);
    out
}

#[test]
fn the_written_layer_is_stable_under_a_second_round_trip() {
    for (name, bytes) in fixtures() {
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let (once, _) = encode(decode(text).ir());
        let (twice, _) = encode(decode(&once).ir());
        assert_eq!(rounded(&once), rounded(&twice), "{name}");
    }
}

#[test]
fn the_encoder_targets_each_usda_version_a_package_and_a_crate_layer() {
    let targets = UsdCodec.targets();
    let ids: Vec<_> = targets.iter().map(|target| target.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "usd:usda-1.0",
            "usd:usda-1.1",
            "usd:usda-1.2",
            "usd:usda-1.3",
            "usd:usdz",
            "usd:usdc-0.8.0"
        ]
    );
}

mod features;
mod intrinsics;
mod layers;
mod losses;
mod mates;
mod parameters;
mod physics;
mod sketches;
mod subsets;
mod textures;
mod usdc;
