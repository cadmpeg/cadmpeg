use super::*;

const EPS_CENTER_POSITION: f64 = 1.0e-8;
const SMALL_COORDINATE_SCALE: f64 = 1.0e-10;

// Linear oracle: admit centers by equal radii and a nonzero sweep, then select
// one quantized cell. Preserve the source order for the cell representative.
fn linear_center(
    start: Point2,
    end: Point2,
    candidates: &[(Option<&str>, Point2)],
    tolerance: f64,
    excluded: [Option<&str>; 2],
) -> Option<Point2> {
    if start == end { return None; }
    let mut centers = Vec::new();
    for &(reference, center) in candidates {
        if reference.is_some() && excluded.contains(&reference) { continue; }
        let radius = (start.u - center.u).hypot(start.v - center.v);
        let end_radius = (end.u - center.u).hypot(end.v - center.v);
        if radius <= tolerance || (radius - end_radius).abs()
            > tolerance * radius.abs().max(end_radius.abs()).max(1.0) { continue; }
        let sweep = ((end.v - center.v).atan2(end.u - center.u)
            - (start.v - center.v).atan2(start.u - center.u)).rem_euclid(std::f64::consts::TAU);
        if sweep <= SKETCH_ANGLE_TOLERANCE || std::f64::consts::TAU - sweep <= SKETCH_ANGLE_TOLERANCE { continue; }
        centers.push((quantize(center, tolerance), center));
    }
    centers.sort_unstable_by_key(|value| value.0);
    centers.dedup_by_key(|value| value.0);
    match centers.as_slice() { [(_, point)] => Some(*point), _ => None }
}

fn bits(point: Option<Point2>) -> Option<[u64; 2]> {
    point.map(|point| [point.u.to_bits(), point.v.to_bits()])
}

#[test]
fn position_index_preserves_radius_admission_and_cell_representative() {
    for scale in [1.0e-250, SMALL_COORDINATE_SCALE, 1.0, 1.0e8, 1.0e150] {
        for tolerance in [0.0, EPS_CENTER_POSITION, scale * EPS_CENTER_POSITION] {
            let ctx = cadmpeg_test_support::service_decode_context();
            let start = Point2::new(-scale, 0.0);
            let end = Point2::new(scale, 0.0);
            // Include both sides of the radius tolerance and different points in one cell.
            let mut points = vec![(Some("center"), Point2::new(0.0, 0.0))];
            for index in 0..128 {
                let u = (f64::from(index) - 64.0) * tolerance * 0.015625;
                let v = scale * (f64::from(index % 7) - 3.0);
                points.push((None, Point2::new(u, v)));
                points.push((None, Point2::new((f64::from(index) + 2.0) * scale, v)));
            }
            for shift in [0.0, scale * 1024.0] {
                let moved = points.iter().map(|&(id, point)| (id, Point2::new(point.u + shift, point.v + shift))).collect::<Vec<_>>();
                let start = Point2::new(start.u + shift, start.v + shift);
                let end = Point2::new(end.u + shift, end.v + shift);
                let index = ArcCenterIndex::from_points(&ctx, &moved, tolerance).unwrap();
                for excluded in [[None, None], [Some("center"), None]] {
                    let actual = unique_arc_center_marker(&ctx, start, end, &index, tolerance, excluded).unwrap();
                    assert_eq!(bits(actual), bits(linear_center(start, end, &moved, tolerance, excluded)),
                        "scale={scale}, tolerance={tolerance}, shift={shift}");
                }
            }
        }
    }
}

#[test]
fn position_index_keeps_points_in_one_cell_with_different_radius_results() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let tolerance = 0.125;
    let start = Point2::new(-1.0, 0.0);
    let end = Point2::new(1.0, 0.0);
    // The first point fails the radius check; the second shares its cell and passes.
    let points = [(None, Point2::new(0.074, 0.0)), (None, Point2::new(0.064, 0.0))];
    let index = ArcCenterIndex::from_points(&ctx, &points, tolerance).unwrap();
    let actual = unique_arc_center_marker(&ctx, start, end, &index, tolerance, [None, None]).unwrap();
    assert_eq!(actual, linear_center(start, end, &points, tolerance, [None, None]));
    assert_eq!(actual, Some(Point2::new(0.064, 0.0)));
}

#[test]
fn position_index_prunes_distant_cells_without_rebuilding_for_queries() {
    const MEASURE: &str = "measure SLDPRT arc index query work";
    let mut points = vec![(None, Point2::new(0.0, 0.0))];
    for index in 1..4096 {
        points.push((None, Point2::new(f64::from(index) * 4.0, f64::from(index % 13))));
    }
    let measure = |query: bool| {
        crate::test_support::work_refusal_at(MEASURE, |ctx| {
            let index = ArcCenterIndex::from_points(ctx, &points, EPS_CENTER_POSITION)?;
            if query {
                let center = unique_arc_center_marker(ctx, Point2::new(-1.0, 0.0), Point2::new(1.0, 0.0),
                    &index, EPS_CENTER_POSITION, [None, None])?;
                assert_eq!(center, Some(Point2::new(0.0, 0.0)));
            }
            ctx.charge_work(1, MEASURE)
        })
    };
    let CodecError::ResourceLimit(build) = measure(false) else { panic!("work boundary"); };
    let CodecError::ResourceLimit(query) = measure(true) else { panic!("work boundary"); };
    assert!(query.used - build.used < cadmpeg_core::decode::u64_from_index(points.len()),
        "one query must visit fewer cells than a complete point scan");
}

#[test]
fn position_index_releases_scratch_storage_after_each_sketch() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1 << 20;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = (0..4096).map(|index| (None, Point2::new(f64::from(index), 0.0))).collect::<Vec<_>>();
    for _ in 0..20 {
        let index = ArcCenterIndex::from_points(&ctx, &points, EPS_CENTER_POSITION).unwrap();
        assert_eq!(index.records.len(), points.len());
    }
}

#[test]
fn indexed_point_queries_preserve_relative_predicates_and_exclusions() {
    use crate::resolved_features::relation_loci::same_dimension_length;
    let ordered_bits = |mut points: Vec<[f64; 2]>| {
        points.sort_unstable_by(|left, right| left[0].total_cmp(&right[0]).then_with(|| left[1].total_cmp(&right[1])));
        points.into_iter().map(|point| point.map(f64::to_bits)).collect::<Vec<_>>()
    };
    for scale in [1.0e-250, SMALL_COORDINATE_SCALE, 1.0, 1.0e9, 1.0e150, 1.0e308] {
        let ctx = cadmpeg_test_support::service_decode_context();
        let start = Point2::new(-scale, 0.0);
        let end = Point2::new(scale, 0.0);
        let mut points = vec![(Some("excluded"), Point2::new(0.0, 0.0))];
        for index in 0..128 {
            let u = (f64::from(index) - 64.0) * scale * EPS_RELATIVE_POINT_MATCH;
            let v = scale * (f64::from(index % 7) - 3.0);
            points.push((None, Point2::new(u, v)));
            points.push((None, Point2::new((f64::from(index) + 2.0) * scale, v)));
        }
        let index = ArcCenterIndex::from_points(&ctx, &points, 1.0).unwrap();
        for query in [
            PointPositionQuery::EqualRadii { start, end },
            PointPositionQuery::Coordinates { point: Point2::new(scale, scale) },
            PointPositionQuery::Coordinates { point: Point2::new(f64::INFINITY, f64::INFINITY) },
        ] {
            let expected = points.iter().filter_map(|&(reference, center)| {
                if reference == Some("excluded") { return None; }
                let matches = match query {
                    PointPositionQuery::EqualRadii { start, end } => {
                        let first_radius = (start.u - center.u).hypot(start.v - center.v);
                        let second_radius = (end.u - center.u).hypot(end.v - center.v);
                        first_radius > 0.0 && same_dimension_length(first_radius, second_radius)
                    }
                    PointPositionQuery::Coordinates { point } =>
                        same_dimension_length(center.u, point.u) && same_dimension_length(center.v, point.v),
                };
                matches.then_some([center.u, center.v])
            }).collect::<Vec<_>>();
            let (actual, _storage) = index.point_candidates(&ctx, query, [Some("excluded"), None]).unwrap();
            assert_eq!(ordered_bits(actual), ordered_bits(expected), "scale={scale}");
        }
    }
}

#[test]
fn indexed_point_queries_prune_distant_positions_and_release_candidates() {
    const MEASURE: &str = "measure SLDPRT point index query work";
    let mut points = vec![(None, Point2::new(0.0, 0.0))];
    for index in 1..4096 {
        points.push((None, Point2::new(f64::from(index) * 4.0, f64::from(index % 13))));
    }
    for query in [
        PointPositionQuery::EqualRadii { start: Point2::new(-1.0, 0.0), end: Point2::new(1.0, 0.0) },
        PointPositionQuery::Coordinates { point: Point2::new(0.0, 0.0) },
    ] {
        let measure = |run_query: bool| crate::test_support::work_refusal_at(MEASURE, |ctx| {
            let index = ArcCenterIndex::from_points(ctx, &points, 1.0)?;
            if run_query {
                let (coordinates, _storage) = index.point_candidates(ctx, query, [None, None])?;
                assert_eq!(coordinates, [[0.0, 0.0]]);
            }
            ctx.charge_work(1, MEASURE)
        });
        let CodecError::ResourceLimit(build) = measure(false) else { panic!("work boundary"); };
        let CodecError::ResourceLimit(query) = measure(true) else { panic!("work boundary"); };
        assert!(query.used - build.used < cadmpeg_core::decode::u64_from_index(points.len()));
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1 << 20;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = ArcCenterIndex::from_points(&ctx, &points, 1.0).unwrap();
    for _ in 0..64 {
        let (coordinates, _storage) = index.point_candidates(&ctx,
            PointPositionQuery::Coordinates { point: Point2::new(f64::INFINITY, f64::INFINITY) }, [None, None],
        ).unwrap();
        assert_eq!(coordinates.len(), points.len());
    }
}
