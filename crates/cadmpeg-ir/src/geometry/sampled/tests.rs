// SPDX-License-Identifier: Apache-2.0
use crate::math::Point3;

#[test]
fn sampled_carriers_admit_finite_numeric_payloads_and_preserve_failed_edits() {
    use crate::geometry::sampled::{
        PolygonalSurface, PolylineCurve, PolylineSamples, PolylineVertex,
    };
    let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let parameterized = |parameters: [f64; 2]| PolylineSamples::Parameterized {
        vertices: points
            .iter()
            .copied()
            .zip(parameters)
            .map(|(point, parameter)| PolylineVertex { parameter, point })
            .collect::<Vec<_>>()
            .try_into()
            .expect("nonempty polyline fixture"),
    };
    assert!(PolylineCurve::new(parameterized([1.0, 1.0]), 0.0).is_err());
    assert!(PolylineCurve::new(parameterized([0.0, f64::INFINITY]), 0.0).is_err());
    assert!(PolylineCurve::new(
        PolylineSamples::Unparameterized {
            points: points
                .clone()
                .try_into()
                .expect("nonempty polyline fixture")
        },
        -1.0
    )
    .is_err());
    let mut polyline = PolylineCurve::new(parameterized([2.0, 1.0]), 0.0).unwrap();
    let original = polyline.clone();
    assert!(polyline
        .edit_samples(|samples| {
            samples.edit_points(|point| {
                point.x = f64::NAN;
                Ok(())
            })
        })
        .is_err());
    assert_eq!(polyline, original);
    assert!(polyline
        .edit_samples(|samples| {
            if let PolylineSamples::Parameterized { vertices } = samples {
                vertices[1].parameter = 2.0;
            }
            Ok(())
        })
        .is_err());
    assert_eq!(polyline, original);
    assert!(polyline.set_chordal_deflection(f64::INFINITY).is_err());
    assert_eq!(polyline, original);
    let mut wire = serde_json::to_value(&polyline).unwrap();
    assert_eq!(wire["samples"]["kind"], "parameterized");
    assert_eq!(
        wire["samples"]["vertices"][0]["parameter"],
        serde_json::json!(2.0)
    );
    assert_eq!(
        serde_json::from_value::<PolylineCurve>(wire.clone()).unwrap(),
        polyline
    );
    // A parameter travels in its own sample row, so a parameter list that does
    // not match the sample count has no spelling; a repeated parameter is still
    // refused by the monotonic mint.
    wire["samples"]["vertices"][1]["parameter"] = serde_json::json!(2.0);
    assert!(serde_json::from_value::<PolylineCurve>(wire.clone()).is_err());
    wire["samples"]["parameters"] = serde_json::json!([1.0, 1.0]);
    assert!(serde_json::from_value::<PolylineCurve>(wire).is_err());
    let mut surface = PolygonalSurface::new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2]],
        0.0,
    )
    .unwrap();
    let original = surface.clone();
    assert!(surface
        .edit_vertices(|vertices| {
            vertices[0].z = f64::INFINITY;
            Ok(())
        })
        .is_err());
    assert_eq!(surface, original);
    assert!(surface.set_chordal_deflection(-1.0).is_err());
    assert_eq!(surface, original);
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["chordal_deflection"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<PolygonalSurface>(wire).is_err());
}
#[test]
fn a_refused_sample_edit_keeps_the_prior_samples() {
    use crate::geometry::sampled::{GeometryLayoutError, PolylineCurve, PolylineSamples};

    let mut polyline = PolylineCurve::new(
        PolylineSamples::Unparameterized {
            points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
                .try_into()
                .unwrap(),
        },
        0.0,
    )
    .unwrap();
    let original = polyline.clone();
    let mut seen = 0;
    assert!(polyline
        .edit_samples(|samples| {
            samples.edit_points(|point| {
                seen += 1;
                if seen == 1 {
                    point.x = 9.0;
                    Ok(())
                } else {
                    Err(GeometryLayoutError::EditRefused(
                        "the caller refused this sample".to_string(),
                    ))
                }
            })
        })
        .is_err());
    assert_eq!(seen, 2);
    assert_eq!(polyline, original);
}

#[test]
fn a_refused_polygonal_vertex_edit_keeps_the_prior_vertices() {
    use crate::geometry::sampled::{GeometryLayoutError, PolygonalSurface};

    let mut surface = PolygonalSurface::new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2]],
        0.0,
    )
    .unwrap();
    let original = surface.clone();
    assert!(surface
        .edit_vertices(|vertices| {
            vertices[0].z = 5.0;
            Err(GeometryLayoutError::EditRefused(
                "the caller refused this vertex".to_string(),
            ))
        })
        .is_err());
    assert_eq!(surface, original);
}
