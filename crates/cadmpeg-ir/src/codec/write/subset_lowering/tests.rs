// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::eval::admission::EvaluationAdmission;
use crate::eval::decode::curve_point;
use crate::eval::model_curve_point_by_id;
use crate::geometry::analytic::LineCurve;
use crate::geometry::curve_payloads::SubsetCurveConstruction;
use crate::geometry::{
    Curve, CurveGeometry, FitTolerance, LegacyCache, ProceduralCurve, ProceduralCurveDefinition,
    SolvedCurveGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::index::ModelIndex;
use crate::math::{Point3, Vector3};
use crate::topology::ParameterInterval;

const EPS_POINT: f64 = 1.0e-12;

fn line() -> SolvedCurveGeometry {
    SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(1.0, 2.0, 3.0), Vector3::new(0.0, 0.6, 0.8)).unwrap(),
    )
}

fn id(name: &str) -> CurveId {
    CurveId::mint(format!("test:model:curve#{name}")).unwrap()
}

/// Attach a subset of `source` as curve `name`, with its local cache when
/// `cached`.
fn add_subset(
    ir: &mut CadIr,
    name: &str,
    source: &CurveId,
    range: [f64; 2],
    sense: bool,
    cached: bool,
) {
    let subset = SubsetCurveConstruction::try_new(source.clone(), range, sense, None).unwrap();
    let cache = cached
        .then(|| {
            let source = ir.model.curves.iter().find(|curve| curve.id == *source)?;
            subset.local_cache(source.geometry.solved()?)
        })
        .flatten();
    let construction = ProceduralCurveId::mint(format!("test:model:construction#{name}")).unwrap();
    let mut definition = ProceduralCurveDefinition::Subset(subset);
    let geometry = match cache {
        Some(cache) => {
            definition
                .set_legacy_cache(LegacyCache::new(FitTolerance::ZERO))
                .unwrap();
            CurveGeometry::Solved(cache)
        }
        None => CurveGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
    };
    ir.model.curves.push(Curve {
        native_ref: None,
        id: id(name),
        geometry,
        parameter_range: None,
        source_object: None,
    });
    ir.model
        .add_procedural_curve(&id(name), ProceduralCurve::new(construction, definition))
        .unwrap();
}

/// The unit cube whose first edge runs on `outer`, a subset of `inner`, a
/// subset of a line, over the outer local range `[0, 1]`.
fn nested(inner_sense: bool, outer_sense: bool, inner_cached: bool) -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.curves.push(Curve {
        native_ref: None,
        id: id("root"),
        geometry: CurveGeometry::Solved(line()),
        parameter_range: None,
        source_object: None,
    });
    add_subset(
        &mut ir,
        "inner",
        &id("root"),
        [0.5, 3.0],
        inner_sense,
        inner_cached,
    );
    add_subset(
        &mut ir,
        "outer",
        &id("inner"),
        [0.2, 1.2],
        outer_sense,
        false,
    );
    let edge = &mut ir.model.edges[0];
    edge.set_curve(Some(id("outer"))).unwrap();
    edge.set_param_range(Some(ParameterInterval::new([0.0, 1.0]).unwrap()));
    ir
}

fn model_point(ir: &CadIr, curve: &CurveId, parameter: f64) -> Point3 {
    model_curve_point_by_id(
        EvaluationAdmission::Standard,
        &ModelIndex::build(ir),
        curve,
        parameter,
    )
    .unwrap()
    .get()
}

fn cache_point(ir: &CadIr, curve: &CurveId, parameter: f64) -> Point3 {
    let curve = ir
        .model
        .curves
        .iter()
        .find(|item| item.id == *curve)
        .unwrap();
    curve_point(
        EvaluationAdmission::Standard,
        &CurveGeometry::Solved(curve.geometry.solved().unwrap().clone()),
        parameter,
    )
    .unwrap()
    .get()
}

fn assert_close(actual: Point3, expected: Point3) {
    assert!(
        actual.distance(expected) < EPS_POINT,
        "{actual:?} != {expected:?}"
    );
}

/// The lowered first edge reaches the original edge end points on the lowered
/// cache, swaps its vertices and flips its coedges exactly when `forward` is
/// false.
fn assert_lowered_edge(ir: &CadIr, lowered: &CadIr, forward: bool) {
    let start = model_point(ir, &id("outer"), 0.0);
    let end = model_point(ir, &id("outer"), 1.0);
    let edge = &lowered.model.edges[0];
    let [a, b] = edge.param_range().unwrap().get();
    let (at_a, at_b) = if forward { (start, end) } else { (end, start) };
    assert_close(cache_point(lowered, &id("outer"), a), at_a);
    assert_close(cache_point(lowered, &id("outer"), b), at_b);
    let original = &ir.model.edges[0];
    let expected = if forward {
        (&original.start, &original.end)
    } else {
        (&original.end, &original.start)
    };
    assert_eq!((&edge.start, &edge.end), expected);
    assert!(ir
        .model
        .coedges
        .iter()
        .zip(&lowered.model.coedges)
        .filter(|(_, coedge)| coedge.edge == edge.id)
        .all(|(before, after)| (before.sense == after.sense) == forward));
}

#[test]
fn a_lowered_chain_states_its_edges_on_the_root_parameter_in_both_senses() {
    for inner_sense in [true, false] {
        for outer_sense in [true, false] {
            let ir = nested(inner_sense, outer_sense, false);
            let lowered = super::lower_subset_curves(&ir, |_| true).unwrap();
            let forward = inner_sense == outer_sense;
            assert_lowered_edge(&ir, &lowered, forward);
            let Some(ProceduralCurveDefinition::Subset(definition)) = lowered
                .model
                .procedural_curves
                .iter()
                .find(|procedural| {
                    lowered.model.procedural_curve_owner(&procedural.id) == Some(&id("outer"))
                })
                .map(ProceduralCurve::definition)
            else {
                panic!("outer subset definition");
            };
            assert_eq!(definition.source(), &id("root"));
            assert_eq!(*definition.sense(), forward);
            assert_eq!(
                definition.parameter_range().endpoints(),
                lowered.model.edges[0].param_range().unwrap().get()
            );
        }
    }
}

#[test]
fn a_kept_inner_subset_ends_the_chain_on_its_local_cache() {
    for inner_sense in [true, false] {
        for outer_sense in [true, false] {
            let ir = nested(inner_sense, outer_sense, true);
            let lowered = super::lower_subset_curves(&ir, |curve| curve.id == id("outer")).unwrap();
            assert_lowered_edge(&ir, &lowered, outer_sense);
        }
    }
}

#[test]
fn a_model_without_a_selected_subset_is_borrowed() {
    let ir = nested(true, true, false);
    assert!(matches!(
        super::lower_subset_curves(&ir, |_| false).unwrap(),
        std::borrow::Cow::Borrowed(_)
    ));
}

#[test]
fn a_spline_cache_moves_back_onto_the_source_parameter() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3};

    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.0),
        Point3::new(3.0, 2.0, 1.0),
        Point3::new(4.0, 0.0, 2.0),
    ]
    .into_iter()
    .map(|point| FinitePoint3::new(point).unwrap())
    .collect();
    let root = NurbsCurve::new(
        2,
        vec![1.0, 1.0, 1.0, 2.0, 3.0, 3.0, 3.0],
        NurbsPoles3::Polynomial { points },
        false,
    )
    .unwrap();
    for sense in [true, false] {
        let mut ir = crate::examples::unit_cube().unwrap();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: id("root"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(root.clone())),
            parameter_range: None,
            source_object: None,
        });
        add_subset(&mut ir, "outer", &id("root"), [1.25, 2.25], sense, true);
        let edge = &mut ir.model.edges[0];
        edge.set_curve(Some(id("outer"))).unwrap();
        edge.set_param_range(Some(ParameterInterval::new([0.0, 1.0]).unwrap()));
        let lowered = super::lower_subset_curves(&ir, |_| true).unwrap();
        assert_lowered_edge(&ir, &lowered, sense);
        let curve = lowered
            .model
            .curves
            .iter()
            .find(|curve| curve.id == id("outer"))
            .unwrap();
        let Some(SolvedCurveGeometry::Nurbs(cache)) = curve.geometry.solved() else {
            panic!("spline cache");
        };
        assert_eq!(cache.pole_rows(), root.pole_rows());
        for (actual, expected) in cache.knots().iter().zip(root.knots().iter()) {
            assert!(
                (actual - expected).abs() < EPS_POINT,
                "{actual} != {expected}"
            );
        }
    }
}
