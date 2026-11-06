// SPDX-License-Identifier: Apache-2.0
//! Explicit edge parameter ranges.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{
    write::{target::TargetRequest, EncodeInput, Encoder},
    Codec, DecodeOptions,
};

const PARAMETER_TOLERANCE: f64 = 1.0e-9;
use std::io::Cursor;

#[test]
fn surface_curve_construction_survives_source_less_synthesis() {
    use crate::test_support::parasolid::{
        cylinder_carrier, edge_use, owned_triangle_on, planar_b_curve, sp_curve_records,
        triangle_body,
    };
    use cadmpeg_ir::geometry::{
        pcurve::PcurveGeometry, CurveGeometry, ProceduralCurveDefinition, SurfaceCurveFamily,
    };
    const UV_TOLERANCE: f64 = 1.0e-9;

    let mut cylinder = cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0);
    cylinder.extend(owned_triangle_on(
        0,
        900,
        [[1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
        100,
        1,
    ));
    for (template, start, end, points) in [
        (
            triangle_body(),
            [0.0, 0.0],
            [1.0, 0.0],
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        ),
        (
            cylinder,
            [0.0, 0.0],
            [0.0, 1.0],
            [[1.0, 0.0, 0.0], [1.0, 0.0, 1.0]],
        ),
    ] {
        for rational in [false, true] {
            let mut body = template.clone();
            let old = edge_use(40, 0);
            let at = body
                .windows(old.len())
                .position(|window| window == old)
                .unwrap();
            body.splice(at..at + old.len(), edge_use(40, 70));
            body.extend(planar_b_curve(start, end, rational));
            body.extend(sp_curve_records(100, points));
            let source = crate::test_support::container::sldprt_with_body(&body);
            let input = crate::SldprtCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            let expected = input
                .ir()
                .model
                .procedural_curves
                .iter()
                .find_map(|curve| {
                    let ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric { context, .. },
                    } = curve.definition()
                    else {
                        return None;
                    };
                    context.sides()[0]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| pcurve.geometry.clone())
                })
                .expect("native surface curve");
            let id = input
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| matches!(curve.geometry, CurveGeometry::Procedural { .. }))
                .unwrap()
                .id
                .clone();
            let plan = crate::SldprtCodec
                .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
                .unwrap();
            assert!(plan
                .report()
                .losses
                .iter()
                .any(
                    |loss| loss.code.local_code() == "record.residual-unavailable"
                        && loss.message.contains(id.as_str())
                ));
            assert!(!plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "writer.procedural-reduced"
                    && loss.message.contains(id.as_str())));
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes).unwrap();
            let output = crate::SldprtCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(output
                .ir()
                .model
                .curves
                .iter()
                .any(|curve| matches!(curve.geometry, CurveGeometry::Procedural { .. })));
            let actual = output
                .ir()
                .model
                .procedural_curves
                .iter()
                .find_map(|curve| {
                    let ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric { context, .. },
                    } = curve.definition()
                    else {
                        return None;
                    };
                    context.sides()[0]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry)
                })
                .expect("written surface curve");
            let (PcurveGeometry::Nurbs { nurbs: before }, PcurveGeometry::Nurbs { nurbs: after }) =
                (&expected, actual)
            else {
                panic!("surface curve UV spline");
            };
            assert_eq!(before.degree(), after.degree());
            assert_eq!(before.periodic(), after.periodic());
            assert_eq!(before.knots().len(), after.knots().len());
            for (left, right) in before.knots().iter().zip(after.knots().iter()) {
                assert!((left - right).abs() < UV_TOLERANCE);
            }
            let before_points = before.control_points();
            let after_points = after.control_points();
            assert_eq!(before_points.len(), after_points.len());
            for (left, right) in before_points.iter().zip(after_points.iter()) {
                assert!((left.get().u - right.get().u).abs() < UV_TOLERANCE);
                assert!((left.get().v - right.get().v).abs() < UV_TOLERANCE);
            }
            match (before.weights(), after.weights()) {
                (Some(left), Some(right)) => {
                    assert_eq!(left.len(), right.len());
                    for (left, right) in left.iter().zip(right.iter()) {
                        assert!((left.get() - right.get()).abs() < UV_TOLERANCE);
                    }
                }
                (None, None) => {}
                _ => panic!("surface curve weight form changed"),
            }
        }
    }
}

#[test]
fn surface_curve_without_cache_preserves_its_construction() {
    use crate::test_support::parasolid::{
        edge_use, planar_b_curve, sp_curve_records, triangle_body,
    };
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceCurveFamily};

    let mut body = triangle_body();
    let old = edge_use(40, 0);
    let at = body
        .windows(old.len())
        .position(|window| window == old)
        .unwrap();
    body.splice(at..at + old.len(), edge_use(40, 70));
    body.extend(planar_b_curve([0.0, 0.0], [1.0, 0.0], false));
    body.extend(sp_curve_records(100, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]));
    let input = crate::SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = input.ir().clone();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| matches!(curve.geometry, CurveGeometry::Procedural { .. }))
        .unwrap();
    let id = curve.id.clone();
    let CurveGeometry::Procedural {
        construction,
        cache,
    } = &mut curve.geometry
    else {
        unreachable!()
    };
    *cache = None;
    let construction = construction.clone();
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|curve| curve.id == construction)
        .unwrap()
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric { tail, .. },
            } = definition
            else {
                panic!("parametric surface curve")
            };
            *tail = None;
        });
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str())
            && loss.code.local_code() == "writer.procedural-reduced"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition(),
            ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric { .. }
            }
        )));
}

#[test]
fn bounded_surface_curve_preserves_its_interval_and_construction() {
    use crate::test_support::parasolid::{
        edge_use, planar_b_curve, sp_curve_records, triangle_body,
    };
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceCurveFamily};

    let mut body = triangle_body();
    let old = edge_use(40, 0);
    let at = body
        .windows(old.len())
        .position(|window| window == old)
        .unwrap();
    body.splice(at..at + old.len(), edge_use(40, 75));
    body.extend(planar_b_curve([-1.0, 0.0], [3.0, 0.0], false));
    let mut curves = sp_curve_records(100, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    let end = curves.len();
    curves[end - 16..end - 8].copy_from_slice(&0.25_f64.to_be_bytes());
    curves[end - 8..].copy_from_slice(&0.5_f64.to_be_bytes());
    body.extend(curves);
    let input = crate::SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let interval = |ir: &cadmpeg_ir::CadIr| {
        ir.model.procedural_curves.iter().find_map(|curve| {
            let ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric { context, .. },
            } = curve.definition()
            else {
                return None;
            };
            Some(context.parameter_range().endpoints())
        })
    };
    let expected = interval(input.ir()).expect("input surface curve");
    assert!((expected[0] - 0.25).abs() < PARAMETER_TOLERANCE);
    assert!((expected[1] - 0.5).abs() < PARAMETER_TOLERANCE);
    for without_cache in [false, true] {
        let mut ir = input.ir().clone();
        if without_cache {
            let curve = ir
                .model
                .curves
                .iter_mut()
                .find(|curve| matches!(curve.geometry, CurveGeometry::Procedural { .. }))
                .unwrap();
            let CurveGeometry::Procedural {
                construction,
                cache,
            } = &mut curve.geometry
            else {
                unreachable!()
            };
            *cache = None;
            let construction = construction.clone();
            ir.model
                .procedural_curves
                .iter_mut()
                .find(|curve| curve.id == construction)
                .unwrap()
                .edit_definition(|definition| {
                    let ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric { tail, .. },
                    } = definition
                    else {
                        panic!("parametric surface curve")
                    };
                    *tail = None;
                });
        }
        let plan = crate::SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let actual = interval(output.ir()).expect("written surface curve");
        assert!((actual[0] - expected[0]).abs() < PARAMETER_TOLERANCE);
        assert!((actual[1] - expected[1]).abs() < PARAMETER_TOLERANCE);
    }
}

#[test]
fn linear_edge_parameter_range_is_authored_as_a_bounded_curve() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for edge in &mut ir.model.edges[1..] {
        edge.set_param_range(None);
    }
    let id = ir.model.edges[0].id.clone();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("param_range")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let partition = crate::container::select_active_parasolid_site(&scan)
        .unwrap()
        .section
        .payload();
    assert!(partition.windows(2).any(|pair| pair == [0x00, 0x85]));
    let written = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.edges.iter().any(|edge| edge
        .param_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        == Some([0.0, 10.0])));
}

#[test]
fn circular_edge_parameter_range_survives_synthesis() {
    let source = include_bytes!("../../../tests/golden/fixtures/authored_complete_cylinder.sldprt");
    let input = crate::SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (edge, range) = input
        .ir()
        .model
        .edges
        .iter()
        .find_map(|edge| {
            let curve = input
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == edge.curve())?;
            if matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedCurveGeometry::Circle(_)
                )
            ) {
                Some((edge.id.clone(), edge.param_range()?.get()))
            } else {
                None
            }
        })
        .unwrap();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(edge.as_str()) && loss.message.contains("param_range")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(written
        .ir()
        .model
        .edges
        .iter()
        .filter_map(cadmpeg_ir::topology::Edge::param_range)
        .any(|actual| {
            actual
                .get()
                .into_iter()
                .zip(range)
                .all(|(actual, expected)| (actual - expected).abs() < PARAMETER_TOLERANCE)
        }));
}

#[test]
fn nurbs_edge_parameter_range_survives_synthesis() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsCurve, NurbsPoles3},
        CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::math::Point3;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for edge in &mut ir.model.edges[1..] {
        edge.set_param_range(None);
    }
    let poles = NurbsPoles3::from_lanes(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        None,
    )
    .unwrap();
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(1, vec![0.0, 0.0, 10.0, 10.0], poles, false).unwrap(),
    ));
    let id = ir.model.edges[0].id.clone();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("param_range")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.edges.iter().any(|edge| edge
        .param_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        == Some([0.0, 10.0])
        && edge
            .curve()
            .is_some_and(
                |id| written.ir().model.curves.iter().any(|curve| &curve.id == id
                    && matches!(
                        curve.geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
                    ))
            )));
}
