// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::Point3;

pub(super) fn historical_edge_samples(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Vec<crate::history_records::AsmHistoricalEdgeSample>, cadmpeg_core::decode::ResourceLimit>
{
    let curves = brep
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<HashMap<_, _>>();
    let mut samples = Vec::new();
    for edge in &brep.edges {
        let Some([start, end]) = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let Some(curve) = edge.curve().and_then(|curve| curves.get(curve.as_str())) else {
            continue;
        };
        let Some(edge_ref) = super::stable_ref(edge.id.as_str()) else {
            continue;
        };
        let Some(geometry) = curve.geometry.solved() else {
            if let Some(sample) = helix_sample(brep, curve, edge_ref, [start, end])? {
                samples.push(sample);
            }
            continue;
        };
        let parameter = 0.5 * start + 0.5 * end;
        let Some(interior) = finite_or_refusal(cadmpeg_ir::eval::decode::curve_point_solved(
            EvaluationAdmission::Standard,
            geometry,
            parameter,
        ))?
        else {
            continue;
        };
        let tangent = finite_or_refusal(cadmpeg_ir::eval::curve_tangent_solved(
            EvaluationAdmission::Standard,
            geometry,
            parameter,
        ))?
        .and_then(|tangent| cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(tangent.get()))
        .map(|tangent| *tangent.as_raw());
        let bounds = sampled_bounds(geometry, start, end)?;
        samples.push(crate::history_records::AsmHistoricalEdgeSample {
            edge: edge_ref,
            interior: interior.get(),
            tangent,
            bounds,
        });
    }
    samples.sort_by_key(|sample| sample.edge);
    Ok(samples)
}

fn helix_sample(
    brep: &cadmpeg_asm::brep::AsmBrep,
    curve: &cadmpeg_ir::geometry::Curve,
    edge: i64,
    [start, end]: [f64; 2],
) -> Result<
    Option<crate::history_records::AsmHistoricalEdgeSample>,
    cadmpeg_core::decode::ResourceLimit,
> {
    let Some(construction) = curve.geometry.procedural_construction() else {
        return Ok(None);
    };
    let mut definitions = brep
        .procedural_curves
        .iter()
        .filter(|(owner, definition)| *owner == curve.id && definition.id == *construction);
    let Some((_, definition)) = definitions.next().filter(|_| definitions.next().is_none()) else {
        return Ok(None);
    };
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix) = definition.definition()
    else {
        return Ok(None);
    };
    let [low, high] = helix
        .angle_range()
        .finite_components()
        .map(cadmpeg_ir::scalar::FiniteReal::get);
    if start.min(end) < low || start.max(end) > high {
        return Ok(None);
    }
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(curve.clone());
    ir.model.procedural_curves.push(definition.clone());
    let index = cadmpeg_ir::index::ModelIndex::build(&ir);
    let Some(interior) = finite_or_refusal(cadmpeg_ir::eval::model_curve_point_by_id(
        EvaluationAdmission::Standard,
        &index,
        &curve.id,
        0.5 * start + 0.5 * end,
    ))?
    else {
        return Ok(None);
    };
    Ok(Some(crate::history_records::AsmHistoricalEdgeSample {
        edge,
        interior: interior.get(),
        tangent: None,
        bounds: None,
    }))
}

fn sampled_bounds(
    geometry: &SolvedCurveGeometry,
    start: f64,
    end: f64,
) -> Result<Option<[Point3; 2]>, cadmpeg_core::decode::ResourceLimit> {
    const SAMPLES_PER_SPAN: u32 = 16;
    let mut breaks = vec![start, end];
    if let SolvedCurveGeometry::Nurbs(curve) = geometry {
        breaks.extend(
            curve
                .knots()
                .iter()
                .copied()
                .filter(|&knot| knot > start.min(end) && knot < start.max(end)),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    if breaks.len() == 1 {
        breaks.push(end);
    }
    let mut bounds: Option<[Point3; 2]> = None;
    for span in breaks.windows(2) {
        for index in 0..=SAMPLES_PER_SPAN {
            let fraction = f64::from(index) / f64::from(SAMPLES_PER_SPAN);
            let parameter = span[0] * (1.0 - fraction) + span[1] * fraction;
            let Some(point) = finite_or_refusal(cadmpeg_ir::eval::decode::curve_point_solved(
                EvaluationAdmission::Standard,
                geometry,
                parameter,
            ))?
            else {
                return Ok(None);
            };
            let point = point.get();
            let [low, high] = bounds.get_or_insert([point, point]);
            *low = Point3::new(low.x.min(point.x), low.y.min(point.y), low.z.min(point.z));
            *high = Point3::new(
                high.x.max(point.x),
                high.y.max(point.y),
                high.z.max(point.z),
            );
        }
    }
    Ok(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;

    fn peaked_curve() -> SolvedCurveGeometry {
        SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0., 0., 0.25, 0.5, 1., 1.],
                vec![
                    Point3::new(0., 0., 0.),
                    Point3::new(1., 8., 0.),
                    Point3::new(2., 0., 0.),
                    Point3::new(3., 0., 0.),
                ],
                None,
                false,
            )
            .unwrap(),
        )
    }

    #[test]
    fn knot_spans_capture_a_peak_away_from_the_interval_midpoint() {
        assert_eq!(
            sampled_bounds(&peaked_curve(), 0., 1.).unwrap(),
            Some([Point3::new(0., 0., 0.), Point3::new(3., 8., 0.)])
        );
        assert_eq!(
            sampled_bounds(&peaked_curve(), 0.5, 1.).unwrap(),
            Some([Point3::new(2., 0., 0.), Point3::new(3., 0., 0.)])
        );
    }

    #[test]
    fn an_interval_outside_the_carrier_domain_has_no_sample_box() {
        assert!(sampled_bounds(&peaked_curve(), -0.25, 1.25)
            .unwrap()
            .is_none());
    }

    #[test]
    fn an_unresolved_interval_box_preserves_the_evaluated_midpoint() {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry};
        use cadmpeg_ir::ids::{CurveId, EdgeId, VertexId};
        use cadmpeg_ir::topology::{Edge, EdgeCarrier};

        let curve = CurveId::mint("f3d:test:curve#1").unwrap();
        let brep = cadmpeg_asm::brep::AsmBrep {
            curves: vec![Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Solved(peaked_curve()),
                parameter_range: None,
                source_object: None,
            }],
            edges: vec![Edge {
                id: EdgeId::mint("f3d:test:edge#1").unwrap(),
                carrier: EdgeCarrier::new(Some(curve), Some([-0.25, 1.25])).unwrap(),
                start: VertexId::mint("f3d:test:vertex#1").unwrap(),
                end: VertexId::mint("f3d:test:vertex#2").unwrap(),
                tolerance: None,
            }],
            ..Default::default()
        };
        let samples = historical_edge_samples(&brep).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].interior, Point3::new(2., 0., 0.));
        assert!(samples[0].bounds.is_none());
    }

    #[test]
    fn cacheless_helix_samples_use_the_stored_edge_interval() {
        use cadmpeg_ir::geometry::{
            Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve,
            ProceduralCurveDefinition,
        };
        use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, VertexId};
        use cadmpeg_ir::math::Vector3;
        use cadmpeg_ir::topology::{Edge, EdgeCarrier};

        let curve = CurveId::mint("f3d:test:curve#1").unwrap();
        let construction = ProceduralCurveId::mint("f3d:test:helix#1").unwrap();
        let helix = HelixCurveConstruction::try_new(
            [0.0, std::f64::consts::TAU],
            HelixFrame {
                center: Point3::new(1.0, 2.0, 3.0),
                major: Vector3::new(2.0, 0.0, 0.0),
                minor: Vector3::new(0.0, 2.0, 0.0),
                pitch: Vector3::new(0.0, 0.0, 4.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            0.0,
            None,
        )
        .unwrap();
        let mut brep = cadmpeg_asm::brep::AsmBrep {
            curves: vec![Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Procedural {
                    construction: construction.clone(),
                    cache: None,
                },
                parameter_range: None,
                source_object: None,
            }],
            procedural_curves: vec![(
                curve.clone(),
                ProceduralCurve::new(construction, ProceduralCurveDefinition::Helix(helix)),
            )],
            edges: vec![Edge {
                id: EdgeId::mint("f3d:test:edge#1").unwrap(),
                carrier: EdgeCarrier::new(Some(curve), None).unwrap(),
                start: VertexId::mint("f3d:test:vertex#1").unwrap(),
                end: VertexId::mint("f3d:test:vertex#2").unwrap(),
                tolerance: None,
            }],
            ..Default::default()
        };
        for range in [
            None,
            Some([-0.25, std::f64::consts::TAU]),
            Some([0.0, std::f64::consts::TAU + 0.25]),
        ] {
            brep.edges[0].set_param_range(
                range.map(|range| cadmpeg_ir::topology::ParameterInterval::new(range).unwrap()),
            );
            assert!(historical_edge_samples(&brep).unwrap().is_empty());
        }
        for (range, expected) in [
            ([0.0, std::f64::consts::TAU], Point3::new(-1.0, 2.0, 5.0)),
            ([0.0, std::f64::consts::PI], Point3::new(1.0, 4.0, 4.0)),
        ] {
            brep.edges[0].set_param_range(Some(
                cadmpeg_ir::topology::ParameterInterval::new(range).unwrap(),
            ));
            let samples = historical_edge_samples(&brep).unwrap();
            assert_eq!(samples.len(), 1);
            assert_eq!(samples[0].edge, 1);
            assert!((samples[0].interior.x - expected.x).abs() < 1.0e-12);
            assert!((samples[0].interior.y - expected.y).abs() < 1.0e-12);
            assert!((samples[0].interior.z - expected.z).abs() < 1.0e-12);
            assert!(samples[0].bounds.is_none());
        }
    }
}
