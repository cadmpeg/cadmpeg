// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::brep::{
    intersection::IntersectionCarrier, intersection::IntersectionSupportData, CurveCarrier,
};
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

fn chart(tolerance: Option<f64>) -> IndexedCurve {
    IndexedCurve::Derived(IntersectionCarrier {
        carrier: CurveCarrier {
            attr: 9,
            offset: 0,
            end: 0,
            parameter_range: None,
            marker_reversed: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    None,
                    false,
                )
                .unwrap(),
            )),
        },
        support_data: IntersectionSupportData {
            supports: [2, 3],
            fit_tolerance_mm: tolerance,
            support_uv: None,
        },
    })
}

#[test]
fn approximation_losses_aggregate_counts_and_maxima_by_kind() {
    let mut counts = ApproximationCounts::default();
    for tolerance in [0.25, 0.5, 0.125] {
        counts.intersection(&chart(Some(tolerance)));
        counts.surface_curve(tolerance);
        counts.blend(tolerance);
        counts.inverse_boundary(tolerance);
    }
    counts.intersection(&chart(None));
    counts.intersection(&chart(None));
    let notes = counts.take_notes();
    assert_eq!(notes.len(), 5);
    assert!(notes
        .iter()
        .all(|note| note.code == SldprtLossCode::GeometryCarrierApproximated.kind()));
    assert_eq!(notes.iter().map(|note| note.message.as_str()).collect::<Vec<_>>(), [
        "3 intersection chart curve(s) use approximate carriers; largest fit tolerance 0.5 mm.",
        "3 fitted surface-parameter curve(s) use approximate carriers; largest sampled fit tolerance 0.5 mm.",
        "3 fitted blend surface(s) use approximate carriers; largest fit tolerance 0.5 mm.",
        "3 inverse NURBS boundary cache(s) use approximate carriers; largest sampled maximum distance 0.5 mm.",
        "2 intersection chart curve(s) use approximate carriers with absent source tolerance; largest fit tolerance unavailable.",
    ]);
    assert!(counts.take_notes().is_empty());
}

#[test]
fn approximation_losses_keep_stream_counts_separate_and_exact_carriers_add_nothing() {
    let IndexedCurve::Derived(carrier) = chart(Some(8.0)) else {
        unreachable!()
    };
    let exact = IndexedCurve::Exact(carrier.carrier);
    let mut first = ApproximationCounts::default();
    first.intersection(&exact);
    assert!(first.take_notes().is_empty());
    first.intersection(&chart(Some(0.125)));
    first.intersection(&chart(Some(0.5)));
    first.intersection(&exact);
    let mut second = ApproximationCounts::default();
    second.intersection(&chart(Some(0.25)));
    assert_eq!(
        first.take_notes()[0].message,
        "2 intersection chart curve(s) use approximate carriers; largest fit tolerance 0.5 mm."
    );
    assert_eq!(
        second.take_notes()[0].message,
        "1 intersection chart curve(s) use approximate carriers; largest fit tolerance 0.25 mm."
    );
}
