// SPDX-License-Identifier: Apache-2.0
//! Exact inverse coverage for cyclic OCCT knot and pole representations.

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use std::fmt::Write as _;

use crate::brep::{Tables, TextCurve, TextSurface};
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord};

struct Carriers {
    curves: Vec<TextCurve>,
    surfaces: Vec<TextSurface>,
}

fn read(curves: &str, curve_count: usize, surfaces: &str, surface_count: usize) -> Carriers {
    let text = format!("CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves {curve_count}\n{curves}\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces {surface_count}\n{surfaces}\nTriangulations 0\nTShapes 0\n*");
    let property = crate::test_support::source_fidelity::PropertyFixture::new(
        PropertyRecord {
            dynamic: None,
            id: crate::native::native_id("property", "Shape:Shape"),
            owner: crate::native::native_id("object", "Shape"),
            name: "Shape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: PropertyFamily::Geometry,
            status: None,
            body: Some(PropertyBody::Persisted {
                values: Vec::new().into(),
                links: Vec::new(),
                side_entries: vec!["Shape.brp".into()],
            }),
            order: 0,
        },
        r#"<Property><Part file="Shape.brp"/></Property>"#,
    );
    let entry = crate::test_support::entry_fixture(
        crate::native::native_id("entry", "Shape.brp"),
        "Shape.brp".into(),
        cadmpeg_core::container::ContainerRole::Brep,
        vec![property.id.clone()],
        text.into_bytes(),
    );
    let payloads = crate::test_support::source_fidelity::with_decode_sources(
        &[],
        &[&property],
        &[&entry],
        &DecodePolicy::default(),
        |ctx, sources| {
            crate::brep::source::parse_payloads(
                ctx,
                &[property.property.clone()],
                &[entry.record.clone()],
                sources,
            )
        },
    )
    .expect("OCCT shape tables");
    let tables = Tables::from_payload(&payloads[0]).expect("text tables");
    Carriers {
        curves: tables.curves.to_vec(),
        surfaces: tables.surfaces.to_vec(),
    }
}

#[test]
fn periodic_curve_preserves_knots_poles_weights_and_flag() {
    for (rational, degree, multiplicity, count, knot_values) in [
        (false, 3, 1, 4, vec![0.0, 1.0, 2.0, 3.0, 4.0]),
        (true, 3, 3, 6, vec![0.0, 0.5, 1.0]),
        (true, 6, 6, 6, vec![0.0, std::f64::consts::TAU]),
    ] {
        let mut source = format!(
            "7 {} 1 {degree} {count} {}",
            u8::from(rational),
            knot_values.len()
        );
        for index in 0..count {
            write!(source, " {index} {} 0", index % 2).expect("writing to String cannot fail");
            if rational {
                write!(source, " {}", index + 1).expect("writing to String cannot fail");
            }
        }
        for knot in knot_values {
            write!(source, " {knot} {multiplicity}").expect("writing to String cannot fail");
        }
        let before = read(&source, 1, "", 0);
        let TextCurve::Nurbs(curve) = &before.curves[0] else {
            panic!("NURBS curve")
        };
        let output = super::curve(curve).expect("exact periodic carrier");
        assert_eq!(output.split_whitespace().nth(2), Some("1"));
        assert_eq!(
            output.split_whitespace().nth(4),
            Some(count.to_string().as_str())
        );
        let after = read(&output, 1, "", 0);
        assert_eq!(before.curves, after.curves);
    }
}

#[test]
fn doubly_periodic_rational_surface_preserves_both_axes() {
    let mut source = String::from("9 1 1 1 1 3 2 4 3 5 4");
    for u in 0..4 {
        for v in 0..3 {
            write!(source, " {u} {v} {} {}", u * v, u + v + 1)
                .expect("writing to String cannot fail");
        }
    }
    for knot in 0..5 {
        write!(source, " {knot} 1").expect("writing to String cannot fail");
    }
    for knot in 0..4 {
        write!(source, " {knot} 1").expect("writing to String cannot fail");
    }
    let before = read("", 0, &source, 1);
    let TextSurface::Nurbs(surface) = &before.surfaces[0] else {
        panic!("NURBS surface")
    };
    let output = super::surface(surface).expect("exact doubly periodic carrier");
    let header: Vec<_> = output.split_whitespace().take(11).collect();
    assert_eq!(
        header,
        ["9", "1", "1", "1", "1", "3", "2", "4", "3", "5", "4"]
    );
    let after = read("", 0, &output, 1);
    assert_eq!(before.surfaces, after.surfaces);
}

#[test]
fn periodic_flag_does_not_allow_discarding_a_noncyclic_pole_or_weight() {
    let source = "7 1 1 3 4 5 0 0 0 1 1 0 0 2 1 1 0 3 0 1 0 4 0 1 1 1 2 1 3 1 4 1";
    let facts = read(source, 1, "", 0);
    let TextCurve::Nurbs(curve) = &facts.curves[0] else {
        panic!("NURBS curve")
    };
    let knots = curve
        .knots()
        .as_slice()
        .iter()
        .map(|knot| FiniteReal::new(*knot).expect("finite knot"))
        .collect::<Vec<_>>();
    let points = curve.control_points();
    let weights = curve
        .weights()
        .expect("rational")
        .iter()
        .map(|weight| FiniteReal::new(weight.get()).expect("finite weight"))
        .collect::<Vec<_>>();
    let mut changed = points.clone();
    *changed.last_mut().expect("periodic tail") =
        FinitePoint3::new(Point3::new(9.0, 8.0, 7.0)).expect("finite point");
    let changed =
        NurbsCurve::from_finite_lanes(3, knots.clone(), changed, Some(weights.clone()), true)
            .expect("neutral admitted periodic curve");
    assert!(super::curve(&changed).is_none());
    let mut changed = weights.clone();
    *changed.last_mut().expect("periodic weight") = FiniteReal::new(17.0).expect("finite");
    let changed =
        NurbsCurve::from_finite_lanes(3, knots.clone(), points.clone(), Some(changed), true)
            .expect("neutral admitted weights");
    assert!(super::curve(&changed).is_none());
    let changed = NurbsCurve::from_finite_lanes(
        3,
        knots,
        points,
        Some(
            weights
                .into_iter()
                .map(|weight| FiniteReal::new(-weight.get()).expect("finite negative weight"))
                .collect(),
        ),
        true,
    )
    .expect("neutral allows signed nonzero weights");
    assert!(super::curve(&changed).is_none());
}
