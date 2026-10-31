// SPDX-License-Identifier: Apache-2.0
//! Sketches decode as planar sketch records and survive a write.

use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometryDefinition;

use crate::test_support::{decode, encode, with_context};

/// Agreement of a sketch coordinate after a decode or a write, in
/// millimetres.
const COORDINATE_TOLERANCE: f64 = 1e-9;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/{name}.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn evaluate(curve: &PcurveNurbs, t: f64) -> Point2 {
    let points: Vec<Point2> = curve
        .control_points()
        .iter()
        .map(|point| point.get())
        .collect();
    with_context(b"", |ctx| {
        cadmpeg_ir::eval::nurbs_pcurve_uv(
            ctx,
            curve.degree(),
            curve.knots().as_slice(),
            &points,
            None,
            t,
        )
        .expect("a point")
        .get()
    })
}

#[test]
fn a_closed_spline_passes_through_its_fit_points_at_its_knots() {
    let result = decode(&fixture("sketch_spline_closed"));
    let model = &result.ir().model;
    let curve = model
        .sketch_entities
        .iter()
        .find_map(|entity| match entity.geometry.definition() {
            SketchGeometryDefinition::Nurbs { curve } => Some(curve.clone()),
            _ => None,
        })
        .expect("a spline");
    assert!(curve.periodic());
    // Fit points in centimetres, at knots 0 to 3.
    for (knot, [u, v]) in [[0.0, 0.0], [20.0, 4.0], [26.0, 22.0], [6.0, 26.0]]
        .into_iter()
        .enumerate()
    {
        let point = evaluate(&curve, f64::from(u8::try_from(knot).unwrap()));
        assert!(
            (point.u - u * 10.0).abs() <= COORDINATE_TOLERANCE,
            "{knot}: {point:?}"
        );
        assert!(
            (point.v - v * 10.0).abs() <= COORDINATE_TOLERANCE,
            "{knot}: {point:?}"
        );
    }
}

#[test]
fn a_sketch_keeps_its_entities_and_constraints_through_a_write() {
    for name in ["sketch_curved", "sketch_spline_closed"] {
        let first = decode(&fixture(name));
        let (text, _) = encode(first.ir());
        let second = decode(&text);
        let (before, after) = (&first.ir().model, &second.ir().model);
        assert_eq!(before.sketches.len(), after.sketches.len(), "{name}");
        assert_eq!(
            before.sketch_entities.len(),
            after.sketch_entities.len(),
            "{name}\n{text}"
        );
        assert_eq!(
            before.sketch_constraints.len(),
            after.sketch_constraints.len(),
            "{name}\n{text}"
        );
        let profile = |model: &cadmpeg_ir::document::Model| {
            model
                .sketches
                .iter()
                .map(|sketch| sketch.profiles.iter().map(Vec::len).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        assert_eq!(profile(before), profile(after), "{name}");
        let placement = |model: &cadmpeg_ir::document::Model| {
            model.sketches[0]
                .placement
                .resolved()
                .map(|(origin, _, _)| origin.get())
        };
        let (one, two) = (placement(before).unwrap(), placement(after).unwrap());
        assert!((one.x - two.x).abs() <= COORDINATE_TOLERANCE, "{name}");
    }
}
