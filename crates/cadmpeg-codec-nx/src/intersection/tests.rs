// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::decode::pcurves::{
    complete_intersection_pcurves_from_coedge_incidence,
    complete_intersection_supports_from_edge_incidence, pcurve_matches_edge,
};
use crate::test_support::test_bytes::put_f64;
use crate::test_support::test_bytes::put_ref;
use crate::test_support::test_bytes::put_vec3;
use crate::test_support::test_bytes::record;
use crate::test_support::test_streams::blend_bound_charted_intersection_curve_stream;
use crate::test_support::test_streams::charted_intersection_curve_topology_partition_stream;
use crate::test_support::test_streams::charted_intersection_with_edge_endpoint_witnesses_stream;
use crate::test_support::test_streams::deltas_intersection_curve_stream;
use crate::test_support::test_streams::ext11_charted_intersection_curve_stream;
use crate::test_support::test_streams::two_support_charted_intersection_curve_stream;

use cadmpeg_ir::geometry::{
    pcurve::{PcurveGeometry, PcurveNurbs},
    ProceduralCurveDefinition,
};
use cadmpeg_ir::math::Point2;
use std::collections::BTreeMap;

fn blend_bound_limit_error(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) -> cadmpeg_core::CodecError {
    let stream = blend_bound_charted_intersection_curve_stream();
    crate::test_support::resource_refusal_at(&stream, dimension, operation, |ctx| {
        crate::intersection::blend_bounds(ctx, &stream)
    })
}

#[test]
fn intersection_blend_bound_route_refuses_collection_limit() {
    let error = blend_bound_limit_error(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "NX blend-bound records",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems && limit.operation == "NX blend-bound records")
    );
}

#[test]
fn intersection_blend_bound_route_refuses_retained_limit() {
    let error = blend_bound_limit_error(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "NX blend-bound records",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation == "NX blend-bound records")
    );
}

#[test]
fn intersection_blend_bound_route_refuses_scoped_limit() {
    let error = blend_bound_limit_error(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX blend-bound identity index",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "NX blend-bound identity index")
    );
}

#[test]
fn intersection_blend_bound_route_refuses_work_limit() {
    let error = blend_bound_limit_error(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX blend-bound identity index",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits && limit.operation == "NX blend-bound identity index")
    );
}

#[test]
fn intersection_chart_route_refuses_scoped_limit() {
    let stream = ext11_charted_intersection_curve_stream();
    let error = crate::test_support::resource_refusal_at(
        &stream,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX chart sample pairs",
        |ctx| {
            crate::intersection::chart_source_records(
                ctx,
                &stream,
                crate::intersection::ChartPointLayout::Ext11,
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "NX chart sample pairs")
    );
}

#[test]
fn intersection_solved_route_refuses_retained_limit() {
    let stream = charted_intersection_curve_topology_partition_stream();
    let error = crate::test_support::resource_refusal_at(
        &stream,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "NX solved chart sample copy",
        |ctx| {
            crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
                .map(|scan| scan.curves)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation == "NX solved chart sample copy")
    );
}

#[test]
fn intersection_support_completion_requires_one_unique_incident_complement() {
    use cadmpeg_ir::geometry::{
        pcurve::Pcurve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
    };
    use cadmpeg_ir::ids::{PcurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
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
    let curve = edge.curve().cloned().expect("cube edge curve");
    let _attached = ir.model.add_procedural_curve(
        &cadmpeg_ir::document::admission::StandardAdmission,
        &curve,
        ProceduralCurve::new(
            ProceduralCurveId::mint("nx:test:intersection#0").expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(incident[0].clone()),
                            pcurve: None,
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
                cache: None,
            },
        ),
    );

    complete_intersection_supports_from_edge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides()[1].surface.as_ref(), Some(&incident[1]));

    let pcurve_id = PcurveId::mint("nx:test:pcurve#0").expect("identity grammar");
    let pcurve_geometry = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .unwrap(),
    );
    ir.model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: pcurve_geometry.clone(),
        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
            None,
            Some(cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]).expect("finite fixture range")),
            None,
        ),
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

    complete_intersection_pcurves_from_coedge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("intersection");
    };
    assert_eq!(
        context.sides()[1]
            .pcurve
            .as_ref()
            .map(|binding| &binding.geometry),
        Some(&pcurve_geometry)
    );
}

#[test]
fn intersection_construction_recovers_one_missing_term_from_unique_edge_endpoints() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
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

    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
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
    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &chart, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::intersection::scan_with_auxiliary_replacements(
                ctx,
                &chart,
                &chart[..chart.len() - 108],
                &[&chart[chart.len() - 108..]],
            )
        })
        .unwrap()
        .curves
        .len(),
        1
    );

    let base_term = charted_intersection_curve_topology_partition_stream();
    let mut term = base_term.clone();
    append_record(&mut term, &[0, 41, 0, 0, 0, 1, 0, 21], 34);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::intersection::term_use_records(
            ctx, &term
        ))
        .unwrap()
        .len(),
        1
    );
    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &term, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::intersection::scan_with_auxiliary_replacements(
                ctx,
                &term,
                &base_term,
                &[&term[base_term.len()..]],
            )
        })
        .unwrap()
        .curves
        .len(),
        1
    );

    let mut uv = charted_intersection_curve_topology_partition_stream();
    append_record(&mut uv, &[0, 204, 0, 0, 0, 4, 0, 23], 41);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::intersection::support_uv_records(ctx, &uv)
    })
    .unwrap()
    .is_empty());
    let [curve] = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &uv, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap()
    .curves
    .try_into()
    .unwrap();
    assert_eq!(curve.support_uv, [None, None]);

    let mut blend_bound = blend_bound_charted_intersection_curve_stream();
    append_record(&mut blend_bound, &[0, 59, 0, 14], 24);
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::intersection::blend_bounds(
            ctx,
            &blend_bound
        ))
        .unwrap()
        .is_empty()
    );
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

    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
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

    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
    assert!(scan.constructions.is_empty());
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_support, 1);
    assert_eq!(scan.rejected.total(), 1);
}

#[test]
fn intersection_rejects_cross_form_xmt_collision_atomically() {
    let construction = |delta_twin, pos| crate::topology::CompositeCurve {
        xmt: 12,
        header_references: [None; 5],
        sense: true,
        references: [6, 7, 20, 21, 22, 23].map(crate::framing::xmt_reference::XmtTarget::from_wire),
        delta_twin,
        pos,
    };
    let scan = crate::test_support::with_decode_context(|ctx| {
        super::scan_with_auxiliaries(
            ctx,
            super::AuxiliaryMaps {
                charts: &BTreeMap::new(),
                terms: &BTreeMap::new(),
                uv: &BTreeMap::new(),
                bridges: &BTreeMap::new(),
            },
            &crate::topology::Graph::default(),
            vec![construction(false, 10), construction(true, 20)],
            super::CrossFormCollision::Reject,
        )
    })
    .unwrap();

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
    semantic.extend_from_slice(
        &crate::test_support::with_decode_context(|ctx| {
            crate::deltas::semantic_residual(ctx, &replacement)
        })
        .unwrap(),
    );

    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan_with_auxiliary_replacements(
            ctx,
            &semantic,
            &base,
            &[&replacement],
        )
    })
    .unwrap();

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

    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
    let [uncharted] = scan.uncharted.as_slice() else {
        panic!("one bounded uncharted intersection");
    };
    assert!(uncharted
        .supports
        .references()
        .iter()
        .all(|support| u32::from(*support) > 1));
    assert_ne!(
        uncharted.supports.references()[0],
        uncharted.supports.references()[1]
    );
    assert!(uncharted.tolerance.get().is_finite() && uncharted.tolerance.get() > 0.0);

    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 10..edge + 18].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::intersection::scan(
            ctx,
            &stream,
            crate::intersection::ChartPointLayout::Xyz3
        ))
        .unwrap()
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
    let [curve] = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan_with_auxiliary_replacements(ctx, &stream, &base, &[&complement])
    })
    .unwrap()
    .curves
    .try_into()
    .expect("complemented curve");
    assert_eq!(curve.samples.parameters(), [2.0, 5.0]);

    let base_chart = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_records(
            ctx,
            &base,
            crate::intersection::ChartPointLayout::Xyz3,
        )
    })
    .unwrap()[0]
        .pos;
    let (_, base_chart_end) = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_record_at(
            ctx,
            &base,
            base_chart,
            crate::intersection::ChartPointLayout::Xyz3,
        )
    })
    .unwrap()
    .expect("base chart bounds");
    let duplicate_chart = base[base_chart..base_chart_end].to_vec();
    let mut duplicate_stream = base.clone();
    duplicate_stream.extend_from_slice(&duplicate_chart);
    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(
            ctx,
            &duplicate_stream,
            crate::intersection::ChartPointLayout::Xyz3,
        )
    })
    .unwrap();
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
}

#[test]
fn intersection_chart_accepts_encoded_count_without_arbitrary_ceiling() {
    let count = 1025usize;
    let mut chart = record(40, 60 + count * 24);
    chart[2..6]
        .copy_from_slice(&(u32::try_from(count).expect("fixture value fits u32")).to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28]
        .copy_from_slice(&(u32::try_from(count).expect("fixture value fits u32")).to_be_bytes());
    put_f64(&mut chart, 28, 0.00001);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    for index in 0..count {
        put_vec3(
            &mut chart,
            60 + index * 24,
            [
                cadmpeg_core::convert::f64_from_index(index)
                    .expect("fixture integer is exactly representable")
                    * 0.001,
                0.0,
                0.0,
            ],
        );
    }

    let [chart] = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_records(
            ctx,
            &chart,
            crate::intersection::ChartPointLayout::Xyz3,
        )
    })
    .unwrap()
    .try_into()
    .expect("one wide chart");
    assert_eq!(
        chart.data.count(),
        u32::try_from(count).expect("fixture value fits u32")
    );
    assert_eq!(chart.data.points().len(), count);
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
    outer[2..6]
        .copy_from_slice(&(u32::try_from(count).expect("fixture value fits u32")).to_be_bytes());
    put_ref(&mut outer, 6, 21);
    put_f64(&mut outer, 8, 0.0);
    put_f64(&mut outer, 16, 1.0);
    outer[24..28]
        .copy_from_slice(&(u32::try_from(count).expect("fixture value fits u32")).to_be_bytes());
    put_f64(&mut outer, 28, 0.000_01);
    put_f64(&mut outer, 36, 0.001);
    put_f64(&mut outer, 44, -31_415_800_000_000.0);
    put_f64(&mut outer, 52, -31_415_800_000_000.0);
    outer[60..60 + nested.len()].copy_from_slice(&nested);

    let records = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_records(
            ctx,
            &outer,
            crate::intersection::ChartPointLayout::Xyz3,
        )
    })
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(u32::from(records[0].xmt), 21);
    assert_eq!(records[0].data.points().len(), count);
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

    let records = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::support_uv_records(ctx, &outer)
    })
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(u32::from(records[0].xmt), 23);
    assert_eq!(records[0].values.values().len(), 4);
}

#[test]
fn intersection_pcurve_attachment_requires_face_incidence() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        let edge =
            cadmpeg_ir::ids::EdgeId::mint("synthetic:cube:edge#0").expect("identity grammar");
        let surface = ir
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.edge == edge && coedge.id.as_str().contains("bottom"))
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
            nurbs: PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), end],
                None,
                false,
            )
            .expect("fixture pcurve construction admission")
            .expect("valid intersection pcurve"),
        };

        assert!(pcurve_matches_edge(
            geometry_ctx,
            &ir,
            &edge,
            &surface,
            &pcurve(Point2::new(10.0, 0.0)),
            None,
        ));
        assert!(!pcurve_matches_edge(
            geometry_ctx,
            &ir,
            &edge,
            &surface,
            &pcurve(Point2::new(10.0, 5.0)),
            None,
        ));
    });
}

#[test]
fn intersection_chart_rejects_nonfinite_millimeter_tolerance() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_f64(&mut stream, chart + 28, f64::MAX);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap()
    .curves
    .is_empty());
}

#[test]
fn intersection_chart_layout_is_selected_by_stream_kind() {
    let ext11 = ext11_charted_intersection_curve_stream();

    crate::test_support::with_decode_context(|ctx| {
        let [chart] = crate::intersection::chart_source_records(
            ctx,
            &ext11,
            crate::intersection::ChartPointLayout::Xyz3,
        )
        .unwrap()
        .try_into()
        .expect("one physical xyz3 reading");
        assert_eq!(
            chart.data.point_layout(),
            crate::intersection::ChartPointLayout::Xyz3
        );
        assert_eq!(
            chart.data.points(),
            vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 2]
        );
        assert_eq!(chart.data.native_parameters(), None);
        assert!(chart
            .data
            .into_samples_charged(ctx, chart.preamble)
            .unwrap()
            .is_none());
    });
    let [chart] = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_records(
            ctx,
            &ext11,
            crate::intersection::ChartPointLayout::Ext11,
        )
    })
    .unwrap()
    .try_into()
    .expect("one ext11 chart");
    assert_eq!(
        chart.data.point_layout(),
        crate::intersection::ChartPointLayout::Ext11
    );
    assert_eq!(chart.data.native_parameters(), Some(vec![2.0, 5.0]));
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
    let [chart] = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::chart_source_records(
            ctx,
            &stream,
            crate::intersection::ChartPointLayout::Xyz3,
        )
    })
    .unwrap()
    .try_into()
    .expect("one large-coordinate chart");
    assert_eq!(chart.data.points()[0].x, 1_000_000.0);
    assert_eq!(chart.data.points()[1].x, 1_000_010.0);
}

#[test]
fn intersection_support_order_follows_type_38_values_marker() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 8, 0, 23])
        .expect("support UV record");
    stream[uv + 8] = 3;

    let scan = crate::test_support::with_decode_context(|ctx| {
        crate::intersection::scan(ctx, &stream, crate::intersection::ChartPointLayout::Xyz3)
    })
    .unwrap();
    let [curve] = scan.curves.as_slice() else {
        panic!("one charted intersection");
    };
    assert_eq!(u32::from(curve.primary_support), 13);
    assert_eq!(curve.secondary_support.map(u32::from), Some(6));
}

#[test]
fn physical_chart_parser_retains_single_and_coincident_point_lanes() {
    for count in [1_usize, 2] {
        let mut bytes = record(40, 60 + count * 24);
        bytes[2..6].copy_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
        put_ref(&mut bytes, 6, 20);
        put_f64(&mut bytes, 8, 0.0);
        put_f64(&mut bytes, 16, 1.0);
        bytes[24..28].copy_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
        put_f64(&mut bytes, 28, 0.01);
        put_f64(&mut bytes, 36, 0.0);
        put_f64(&mut bytes, 44, super::MISSING_PARAMETER);
        put_f64(&mut bytes, 52, super::MISSING_PARAMETER);
        for point in 0..count {
            put_vec3(&mut bytes, 60 + point * 24, [1.0, 2.0, 3.0]);
        }
        crate::test_support::with_decode_context(|ctx| {
            let records =
                super::chart_source_records(ctx, &bytes, super::ChartPointLayout::Xyz3).unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].data.count(), u32::try_from(count).unwrap());
            assert_eq!(
                records[0].data.points(),
                vec![cadmpeg_ir::math::Point3::new(1000.0, 2000.0, 3000.0); count]
            );
            assert!(
                super::chart_records(ctx, &bytes, super::ChartPointLayout::Xyz3)
                    .unwrap()
                    .is_empty()
            );
        });
    }
}

#[test]
fn physical_support_uv_parser_retains_single_complete_tuples() {
    for marker in [2_u8, 3, 4] {
        let count = if marker == 4 { 4_usize } else { 2 };
        let mut bytes = record(204, 9 + count * 8);
        bytes[2..6].copy_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
        put_ref(&mut bytes, 6, 20);
        bytes[8] = marker;
        crate::test_support::with_decode_context(|ctx| {
            let records = super::support_uv_records(ctx, &bytes).unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].values.count(), u32::try_from(count).unwrap());
            assert_eq!(records[0].values.marker(), marker);
        });
    }
}

#[test]
fn auxiliary_source_parsers_reject_reserved_record_identities() {
    for identity in [0_u16, 1, 2] {
        let mut chart = record(40, 108);
        chart[2..6].copy_from_slice(&2_u32.to_be_bytes());
        put_ref(&mut chart, 6, identity);
        put_f64(&mut chart, 16, 1.0);
        chart[24..28].copy_from_slice(&2_u32.to_be_bytes());
        put_f64(&mut chart, 28, 0.01);
        put_f64(&mut chart, 44, super::MISSING_PARAMETER);
        put_f64(&mut chart, 52, super::MISSING_PARAMETER);
        put_vec3(&mut chart, 84, [1.0, 0.0, 0.0]);
        let mut term = record(41, 34);
        term[2..6].copy_from_slice(&2_u32.to_be_bytes());
        put_ref(&mut term, 6, identity);
        term[8..10].copy_from_slice(b"TF");
        let mut uv = record(204, 41);
        uv[2..6].copy_from_slice(&4_u32.to_be_bytes());
        put_ref(&mut uv, 6, identity);
        uv[8] = 2;
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                super::chart_source_records(ctx, &chart, super::ChartPointLayout::Xyz3)
                    .unwrap()
                    .len(),
                usize::from(identity > 1)
            );
            assert_eq!(
                super::term_use_records(ctx, &term).unwrap().len(),
                usize::from(identity > 1)
            );
            assert_eq!(
                super::support_uv_records(ctx, &uv).unwrap().len(),
                usize::from(identity > 1)
            );
            assert_eq!(
                super::term_at(&term, 2, super::TermUseFraming::DescriptorInline, 0).is_some(),
                identity > 1
            );
            assert_eq!(
                super::uv_at(ctx, &uv, 2, super::SupportUvFraming::DescriptorInline, 0)
                    .unwrap()
                    .is_some(),
                identity > 1
            );
        });
    }
}

#[test]
fn duplicate_uv_payloads_are_scratch_until_an_identity_survives() {
    let mut bytes = record(204, 41);
    bytes[2..6].copy_from_slice(&4_u32.to_be_bytes());
    put_ref(&mut bytes, 6, 20);
    bytes[8] = 2;
    let duplicate = bytes.clone();
    bytes.extend(duplicate);
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            assert!(super::support_uv_records(ctx, &bytes).unwrap().is_empty());
        },
    );
}

#[test]
fn intersection_candidates_refuse_scoped_storage_before_the_named_construction() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let mut construction = crate::test_support::with_decode_context(|ctx| {
        let graph = crate::topology::Graph::parse(ctx, &stream).unwrap();
        let node = &graph.of_kind(crate::framing::node_kind::NodeKind::Intersection)[0];
        stream[node.pos()..node.end()].to_vec()
    });
    // Keep the complete chart setup. The larger construction lane must raise
    // the scratch peak after the chart parser's temporary data is released.
    for identity in 100..228 {
        put_ref(&mut construction, 2, identity);
        stream.extend_from_slice(&construction);
    }
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    assert_eq!(
        graph
            .of_kind(crate::framing::node_kind::NodeKind::Intersection)
            .len(),
        129
    );
    crate::test_support::resource_refusal_at(
        &stream,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX composite curves",
        |ctx| super::scan_with_graph(ctx, &stream, &graph, super::ChartPointLayout::Xyz3),
    );
}

#[test]
fn intersection_twins_refuse_scoped_storage_before_the_named_construction() {
    let stream = deltas_intersection_curve_stream();
    crate::test_support::resource_refusal_at(
        &stream,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX intersection data curves",
        |ctx| {
            super::scan_with_graph(
                ctx,
                &stream,
                &crate::topology::Graph::default(),
                super::ChartPointLayout::Ext11,
            )
        },
    );
}

#[test]
fn intersection_selected_source_constructions_refuse_retention_at_the_named_boundary() {
    let stream = deltas_intersection_curve_stream();
    crate::test_support::resource_refusal_at(
        &stream,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "NX selected source constructions",
        |ctx| {
            super::scan_with_graph(
                ctx,
                &stream,
                &crate::topology::Graph::default(),
                super::ChartPointLayout::Ext11,
            )
        },
    );
}

#[test]
fn intersection_fixed_replacements_release_each_stream_map() {
    let mut term = record(41, 34);
    term[2..6].copy_from_slice(&2_u32.to_be_bytes());
    put_ref(&mut term, 6, 2);
    term[8..10].copy_from_slice(b"TF");
    let mut bridge = record(59, 24);
    put_ref(&mut bridge, 2, 14);
    bridge[4..8].copy_from_slice(&9_u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut bridge, at, 1);
    }
    bridge[18] = b'+';
    put_ref(&mut bridge, 19, 0);
    put_ref(&mut bridge, 21, 13);
    term.extend(bridge);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(super::term_records(ctx, &term).unwrap().len(), 1);
        assert_eq!(super::blend_bound_records(ctx, &term).unwrap().len(), 1);
    });
    let replacements = vec![term.as_slice(); 1024];
    crate::test_support::with_decode_context_over(
        &term,
        |policy| {
            policy.limits.max_materialized_bytes = 65_536;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let result = super::scan_with_auxiliary_replacements_and_graph(
                ctx,
                &[],
                &term,
                &replacements,
                &crate::topology::Graph::default(),
            )
            .unwrap();
            assert!(result.source_constructions.is_empty());
            assert!(result.curves.is_empty());
            assert_eq!(result.rejected.total(), 0);
        },
    );
}

#[test]
fn intersection_owned_replacements_release_superseded_payloads() {
    let base = charted_intersection_curve_topology_partition_stream();
    let stream = ext11_charted_intersection_curve_stream();
    let graph =
        crate::test_support::with_decode_context(|ctx| crate::topology::Graph::parse(ctx, &stream))
            .unwrap();
    let expected = crate::test_support::with_decode_context(|ctx| {
        super::scan_with_graph(ctx, &stream, &graph, super::ChartPointLayout::Ext11)
    })
    .unwrap();
    assert_eq!(expected.curves.len(), 1);
    assert_eq!(expected.source_constructions.len(), 1);
    let replacements = vec![stream.as_slice(); 256];
    crate::test_support::with_decode_context_over(
        &stream,
        |policy| {
            policy.limits.max_materialized_bytes = 65_536;
        },
        |ctx| {
            let actual = super::scan_with_auxiliary_replacements_and_graph(
                ctx,
                &stream,
                &base,
                &replacements,
                &graph,
            )
            .unwrap();
            assert_eq!(actual.curves.len(), expected.curves.len());
            assert_eq!(
                actual.source_constructions.len(),
                expected.source_constructions.len()
            );
            assert_eq!(actual.rejected, expected.rejected);
            let actual = &actual.curves[0];
            let expected = &expected.curves[0];
            assert_eq!(actual.xmt, expected.xmt);
            assert_eq!(actual.references, expected.references);
            assert_eq!(actual.samples, expected.samples);
            assert_eq!(actual.fit_tolerance, expected.fit_tolerance);
            assert_eq!(actual.support_uv, expected.support_uv);
            assert_eq!(actual.ext_support_uv, expected.ext_support_uv);
        },
    );
}

#[test]
fn intersection_replacement_payload_copies_refuse_at_named_work_boundaries() {
    let stream = ext11_charted_intersection_curve_stream();
    for operation in [
        "NX replacement chart payload copy",
        "NX replacement UV payload copy",
    ] {
        crate::test_support::resource_refusal_at(
            &stream,
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            |ctx| {
                super::scan_with_auxiliary_replacements_and_graph(
                    ctx,
                    &[],
                    &[],
                    &[&stream],
                    &crate::topology::Graph::default(),
                )
            },
        );
    }
}
