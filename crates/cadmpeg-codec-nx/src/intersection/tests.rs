// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::geometry::{PcurveGeometry, ProceduralCurveDefinition};
use cadmpeg_ir::math::Point2;
use std::collections::BTreeMap;

use crate::test_support::*;

#[test]
fn intersection_support_completion_requires_one_unique_incident_complement() {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    };
    use cadmpeg_ir::ids::{PcurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = ir.model.edges[0].clone();
    let incident = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .filter_map(|coedge| {
            let face = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?
                .face
                .clone();
            ir.model
                .faces
                .iter()
                .find(|candidate| candidate.id == face)
                .map(|face| face.surface.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(incident.len(), 2);
    let curve = edge.curve.expect("cube edge curve");
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:intersection#0".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(incident[0].clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    crate::decode::complete_intersection_supports_from_edge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].surface.as_ref(), Some(&incident[1]));

    let pcurve_id = PcurveId("nx:test:pcurve#0".into());
    let pcurve_geometry = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    ir.model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: pcurve_geometry.clone(),
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    let second_face = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == incident[1])
        .expect("second incident face")
        .id
        .clone();
    let second_loop = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.face == second_face)
        .expect("second incident loop")
        .id
        .clone();
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge.id && coedge.owner_loop == second_loop)
        .expect("second incident coedge")
        .pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
        parameter_range: None,
    }];

    crate::decode::complete_intersection_pcurves_from_coedge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].pcurve.as_ref(), Some(&pcurve_geometry));
}

#[test]
fn intersection_construction_recovers_one_missing_term_from_unique_edge_endpoints() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert_eq!(scan.constructions.len(), 1);
    assert_eq!(scan.curves.len(), 1);
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn intersection_construction_rejects_missing_term_without_topology_endpoint_match() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    put_f64(&mut stream, chart + 60, 0.005);

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert_eq!(scan.constructions.len(), 1);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
}

#[test]
fn intersection_auxiliaries_reject_duplicate_identities() {
    fn append_record(stream: &mut Vec<u8>, marker: &[u8], len: usize) {
        let start = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("auxiliary record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    let mut chart = charted_intersection_curve_topology_partition_stream();
    append_record(&mut chart, &[0, 40, 0, 0, 0, 2, 0, 20], 108);
    let scan = crate::intersection::scan(&chart, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &chart,
            &chart[..chart.len() - 108],
            &[&chart[chart.len() - 108..]],
        )
        .curves
        .len(),
        1
    );

    let base_term = charted_intersection_curve_topology_partition_stream();
    let mut term = base_term.clone();
    append_record(&mut term, &[0, 41, 0, 0, 0, 1, 0, 21], 34);
    assert_eq!(crate::intersection::term_use_records(&term).len(), 1);
    let scan = crate::intersection::scan(&term, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &term,
            &base_term,
            &[&term[base_term.len()..]],
        )
        .curves
        .len(),
        1
    );

    let mut uv = charted_intersection_curve_topology_partition_stream();
    append_record(&mut uv, &[0, 204, 0, 0, 0, 4, 0, 23], 41);
    assert!(crate::intersection::support_uv_records(&uv).is_empty());
    let [curve] = crate::intersection::scan(&uv, crate::intersection::ChartPointLayout::Xyz3)
        .curves
        .try_into()
        .unwrap();
    assert_eq!(curve.support_uv, [None, None]);

    let mut blend_bound = blend_bound_charted_intersection_curve_stream();
    append_record(&mut blend_bound, &[0, 59, 0, 14], 24);
    assert!(crate::intersection::blend_bounds(&blend_bound).is_empty());
}

#[test]
fn intersection_rejection_census_requires_resolved_supports() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 19, 998);
    put_ref(&mut stream, intersection + 21, 999);
    put_ref(&mut stream, intersection + 23, 997);

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.constructions.is_empty());
    assert!(scan.curves.is_empty());
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn intersection_chart_rejects_unresolved_support_relation() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 19, 998);

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.constructions.is_empty());
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_support, 1);
    assert_eq!(scan.rejected.total(), 1);
}

#[test]
fn intersection_rejects_cross_form_xmt_collision_atomically() {
    let construction = |delta_twin, pos| crate::topology::CompositeCurve {
        xmt: 12,
        header_references: [1; 5],
        sense: true,
        references: [6, 7, 20, 21, 22, 23],
        delta_twin,
        pos,
    };
    let scan = super::scan_with_auxiliaries(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &crate::topology::Graph::default(),
        vec![construction(false, 10), construction(true, 20)],
        super::CrossFormCollision::Reject,
    );

    assert!(scan.source_constructions.is_empty());
    assert!(scan.constructions.is_empty());
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.duplicate_identity, 2);
    assert_eq!(scan.rejected.total(), 2);
}

#[test]
fn paired_delta_intersection_replaces_the_partition_form_by_xmt() {
    let base = charted_intersection_curve_topology_partition_stream();
    let mut replacement = deltas_intersection_curve_stream();
    let delta_twin = replacement
        .iter()
        .rposition(|byte| *byte == 0x5a)
        .expect("single-byte intersection replacement");
    for (ordinal, reference) in [6u16, 7, 20, 21, 22, 23].into_iter().enumerate() {
        put_ref(&mut replacement, delta_twin + 18 + ordinal * 2, reference);
    }
    let mut semantic = base.clone();
    semantic.extend_from_slice(&crate::deltas::semantic_residual(&replacement));

    let scan =
        crate::intersection::scan_with_auxiliary_replacements(&semantic, &base, &[&replacement]);

    let [construction] = scan.source_constructions.as_slice() else {
        panic!("expected one current intersection construction");
    };
    assert_eq!(construction.xmt, 12);
    assert!(construction.delta_twin);
    let [curve] = scan.curves.as_slice() else {
        panic!("expected the replacement's charted carrier");
    };
    assert_eq!(curve.xmt, 12);
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn uncharted_intersection_requires_exact_topology_bounds() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    let [uncharted] = scan.uncharted.as_slice() else {
        panic!("one bounded uncharted intersection");
    };
    assert!(uncharted.supports.iter().all(|support| *support > 1));
    assert_ne!(uncharted.supports[0], uncharted.supports[1]);
    assert!(uncharted.tolerance.is_finite() && uncharted.tolerance > 0.0);

    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 10..edge + 18].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(
        crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3)
            .uncharted
            .is_empty()
    );
}

#[test]
fn intersection_chart_accepts_one_matching_parameter_complement() {
    let ext11 = ext11_charted_intersection_curve_stream();
    let ext11_start = ext11
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("ext11 chart");
    let complement = ext11[ext11_start..ext11_start + 236].to_vec();

    let base = charted_intersection_curve_topology_partition_stream();
    let mut stream = base.clone();
    stream.extend_from_slice(&complement);
    let [curve] =
        crate::intersection::scan_with_auxiliary_replacements(&stream, &base, &[&complement])
            .curves
            .try_into()
            .expect("complemented curve");
    assert_eq!(curve.parameters, [2.0, 5.0]);

    let base_chart = crate::intersection::chart_source_records(
        &base,
        crate::intersection::ChartPointLayout::Xyz3,
    )[0]
    .pos;
    let (_, base_chart_end) = crate::intersection::chart_source_record_at(
        &base,
        base_chart,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .expect("base chart bounds");
    let duplicate_chart = base[base_chart..base_chart_end].to_vec();
    let mut duplicate_stream = base.clone();
    duplicate_stream.extend_from_slice(&duplicate_chart);
    let scan = crate::intersection::scan(
        &duplicate_stream,
        crate::intersection::ChartPointLayout::Xyz3,
    );
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
}

#[test]
fn intersection_chart_accepts_encoded_count_without_arbitrary_ceiling() {
    let count = 1025usize;
    let mut chart = record(40, 60 + count * 24);
    chart[2..6].copy_from_slice(&(count as u32).to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28].copy_from_slice(&(count as u32).to_be_bytes());
    put_f64(&mut chart, 28, 0.00001);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    for index in 0..count {
        put_vec3(
            &mut chart,
            60 + index * 24,
            [index as f64 * 0.001, 0.0, 0.0],
        );
    }

    let [chart] = crate::intersection::chart_source_records(
        &chart,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .try_into()
    .expect("one wide chart");
    assert_eq!(chart.count, count as u32);
    assert_eq!(chart.chart_count, count as u32);
    assert_eq!(chart.points.len(), count);
}

#[test]
fn intersection_chart_scan_does_not_admit_nested_counted_candidates() {
    let mut nested = record(40, 108);
    nested[2..6].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut nested, 6, 20);
    put_f64(&mut nested, 8, 0.0);
    put_f64(&mut nested, 16, 1.0);
    nested[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_f64(&mut nested, 28, 0.000_01);
    put_f64(&mut nested, 36, 0.001);
    put_f64(&mut nested, 44, -31_415_800_000_000.0);
    put_f64(&mut nested, 52, -31_415_800_000_000.0);
    put_vec3(&mut nested, 60, [0.0, 0.0, 0.0]);
    put_vec3(&mut nested, 84, [0.01, 0.0, 0.0]);

    let count = 5;
    let mut outer = record(40, 60 + count * 24);
    outer[2..6].copy_from_slice(&(count as u32).to_be_bytes());
    put_ref(&mut outer, 6, 21);
    put_f64(&mut outer, 8, 0.0);
    put_f64(&mut outer, 16, 1.0);
    outer[24..28].copy_from_slice(&(count as u32).to_be_bytes());
    put_f64(&mut outer, 28, 0.000_01);
    put_f64(&mut outer, 36, 0.001);
    put_f64(&mut outer, 44, -31_415_800_000_000.0);
    put_f64(&mut outer, 52, -31_415_800_000_000.0);
    outer[60..60 + nested.len()].copy_from_slice(&nested);

    let records = crate::intersection::chart_source_records(
        &outer,
        crate::intersection::ChartPointLayout::Xyz3,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].xmt, 21);
    assert_eq!(records[0].points.len(), count);
}

#[test]
fn intersection_support_uv_scan_does_not_admit_nested_counted_candidates() {
    let mut nested = record(204, 25);
    nested[2..6].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut nested, 6, 24);
    nested[8] = 2;
    put_f64(&mut nested, 9, 0.0);
    put_f64(&mut nested, 17, 0.0);

    let mut outer = record(204, 41);
    outer[2..6].copy_from_slice(&4u32.to_be_bytes());
    put_ref(&mut outer, 6, 23);
    outer[8] = 2;
    outer[9..9 + nested.len()].copy_from_slice(&nested);

    let records = crate::intersection::support_uv_records(&outer);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].xmt, 23);
    assert_eq!(records[0].values.len(), 4);
}

#[test]
fn intersection_pcurve_attachment_requires_face_incidence() {
    let ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let surface = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .and_then(|coedge| {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support surface");
    let pcurve = |end| PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), end],
        weights: None,
        periodic: false,
    };

    assert!(crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 0.0)),
        None,
    ));
    assert!(!crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 5.0)),
        None,
    ));
}

#[test]
fn intersection_chart_rejects_nonfinite_millimeter_tolerance() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_f64(&mut stream, chart + 28, f64::MAX);
    assert!(
        crate::intersection::curves(&stream, crate::intersection::ChartPointLayout::Xyz3)
            .is_empty()
    );
}

#[test]
fn intersection_chart_layout_is_selected_by_stream_kind() {
    let ext11 = ext11_charted_intersection_curve_stream();
    assert!(crate::intersection::chart_source_records(
        &ext11,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .is_empty());
    let [chart] = crate::intersection::chart_source_records(
        &ext11,
        crate::intersection::ChartPointLayout::Ext11,
    )
    .try_into()
    .expect("one ext11 chart");
    assert_eq!(
        chart.point_layout,
        crate::intersection::ChartPointLayout::Ext11
    );
    assert_eq!(chart.native_parameters, Some(vec![2.0, 5.0]));
}

#[test]
fn intersection_chart_accepts_finite_model_coordinates_without_magnitude_bound() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_vec3(&mut stream, chart + 60, [1_000.0, 0.0, 0.0]);
    put_vec3(&mut stream, chart + 84, [1_000.01, 0.0, 0.0]);
    let [chart] = crate::intersection::chart_source_records(
        &stream,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .try_into()
    .expect("one large-coordinate chart");
    assert_eq!(chart.points[0].x, 1_000_000.0);
    assert_eq!(chart.points[1].x, 1_000_010.0);
}

#[test]
fn intersection_support_order_follows_type_38_values_marker() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 8, 0, 23])
        .expect("support UV record");
    stream[uv + 8] = 3;

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    let [curve] = scan.curves.as_slice() else {
        panic!("one charted intersection");
    };
    assert_eq!(curve.supports, [13, 6]);
}
