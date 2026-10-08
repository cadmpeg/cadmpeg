use crate::families::standard::decode::edge_geometry::split_bezier_half;
use crate::families::standard::decode::merge_standard_edge_vertex_references;
use cadmpeg_ir::math::Point3;
use std::collections::BTreeMap;

#[test]
fn bezier_half_split_preserves_midpoints() {
    let control = std::array::from_fn(|index| {
        Point3::new(
            f64::from(u32::try_from(index).expect("six controls")),
            0.0,
            0.0,
        )
    });
    let (left, right) = split_bezier_half(control);
    for index in 0..6 {
        let index = u32::try_from(index).expect("six controls");
        assert_eq!(
            left[usize::try_from(index).expect("control index")],
            Point3::new(f64::from(index) * 0.5, 0.0, 0.0)
        );
        assert_eq!(
            right[usize::try_from(index).expect("control index")],
            Point3::new(2.5 + f64::from(index) * 0.5, 0.0, 0.0)
        );
    }
}

#[test]
fn standard_edge_merge_propagates_source_work_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let mut target = BTreeMap::from([(70, [500, 300])]);
        let source = BTreeMap::from([(90, [100, 500])]);
        let error =
            merge_standard_edge_vertex_references(ctx, &mut target, &source, |vertices| *vertices)
                .expect_err("edge source must be admitted");
        let cadmpeg_core::CodecError::ResourceLimit(error) = error else {
            panic!("resource refusal required")
        };
        assert_eq!(error.operation, "catia_standard_e5_topology_edges");
        assert_eq!(ctx.resource_refusal(), Some(error));
        assert_eq!(target, BTreeMap::from([(70, [500, 300])]));
    });
}

#[test]
fn circle_selection_admits_caller_recursion_depth() {
    use crate::families::standard::decode::edge_geometry::circular_range_choices_have_simple_selection;
    let choices = vec![[[0.0, 1.0]]; 32];
    assert!(crate::test_support::with_service_context(|ctx| {
        circular_range_choices_have_simple_selection(ctx, &choices)
    })
    .expect("compatible coincident ranges"));
    crate::test_support::with_depth_limit(4, |ctx| {
        let error = circular_range_choices_have_simple_selection(ctx, &choices)
            .expect_err("fifth selection frame must refuse");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("depth refusal")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(limit.operation, "catia_standard_circle_range_selection");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn bezier_heap_admits_sifting_and_releases_search_storage() {
    use crate::families::standard::decode::edge_geometry::collect_bezier_point_parameters;
    let control = std::array::from_fn(|index| {
        Point3::new(
            f64::from(u32::try_from(index).expect("six poles")),
            0.0,
            0.0,
        )
    });
    for operation in ["catia_bezier_search_queue", "catia_bezier_search_work"] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            let mut parameters = Vec::new();
            collect_bezier_point_parameters(
                ctx,
                control,
                [0.0, 1.0],
                Point3::new(2.5, 0.0, 0.0),
                0.001,
                1.0,
                &mut parameters,
            )?;
            Ok(parameters)
        })
        .expect_err("heap work must be admitted");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
    crate::test_support::with_retained_limit(0, |ctx| {
        // The retained destination already owns two slots. All search storage
        // must use materialized bytes, and both admitted solutions stay intact.
        let mut parameters = Vec::with_capacity(2);
        collect_bezier_point_parameters(
            ctx,
            control,
            [0.0, 1.0],
            Point3::new(2.5, 0.0, 0.0),
            0.001,
            1.0,
            &mut parameters,
        )
        .expect("search storage is temporary");
        assert_eq!(parameters, vec![(0.5, 0.0), (0.5, 0.0)]);
    });
}

#[test]
fn native_point_index_filters_all_coordinates_on_a_constant_x_plane() {
    use crate::families::b5::graph::B5LogicalVertex;
    use crate::families::standard::decode::unique_native_identity_points;
    use cadmpeg_ir::ids::PointId;
    use cadmpeg_ir::topology::Point;
    let points = (0..1024_u32)
        .map(|index| {
            Point::new(
                PointId::mint(format!("catia:test:point#{index}")).expect("identity"),
                crate::test_support::test_b5::point([0.0, f64::from(index), 0.0]),
                None,
            )
        })
        .collect::<Vec<_>>();
    let vertices = points
        .iter()
        .enumerate()
        .map(|(index, point)| B5LogicalVertex {
            object_id: u32::try_from(index).expect("bounded index"),
            point: point.position(),
        })
        .collect::<Vec<_>>();
    let error =
        crate::test_support::with_work_refusal("catia_native_identity_point_match", |ctx| {
            unique_native_identity_points(ctx, &vertices, points.len(), &BTreeMap::new(), &points)
        })
        .expect_err("probe first query");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal")
    };
    // Admit the measured construction prefix, then 500,000 units for all
    // queries and output growth. A Cartesian query needs 1,048,576 visits.
    crate::test_support::with_work_limit(limit.used + 500_000, |ctx| {
        let matches =
            unique_native_identity_points(ctx, &vertices, points.len(), &BTreeMap::new(), &points)
                .expect("balanced spatial lookup");
        assert_eq!(matches.len(), points.len());
        for (index, vertex) in vertices.iter().enumerate() {
            assert_eq!(matches.get(&vertex.object_id), Some(&index));
        }
    });
}

#[test]
fn face_witness_spatial_dedup_preserves_first_order_and_limits_visits() {
    use super::super::distinct_face_witnesses;
    let points = (0..1024)
        .map(|index| Point3::new(0.0, f64::from(index), 0.0))
        .collect::<Vec<_>>();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| distinct_face_witnesses(ctx, &points);
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_a5_distinct_face_witnesses", run)
            .expect_err("first visit")
    else {
        panic!("work refusal");
    };
    assert_eq!(
        crate::test_support::with_work_limit(limit.used + 200_000, run).expect("spatial visits"),
        points
    );
    let tolerance = super::super::NURBS_SURFACE_MEMBERSHIP_TOLERANCE;
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(tolerance * 0.75, 0.0, 0.0),
        Point3::new(tolerance * 1.5, 0.0, 0.0),
    ];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| distinct_face_witnesses(ctx, &points))
            .expect("service"),
        vec![points[0], points[2]]
    );
}

#[test]
fn a5_bounds_binding_avoids_separated_owner_carrier_and_face_products() {
    use super::super::A5BindingIndex;
    use crate::families::a5a8::records::FreeformSurface;
    use crate::families::standard::records::StandardFaceBounds;
    use crate::native::owner_numeric_tail::CatiaOwnerNumericTail;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let packet_bytes = crate::test_support::test_b2::b2_all_compact_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&packet_bytes);
    let template = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::b2_owner_packets_from_records(ctx, &packet_bytes, &records)
            .map(|mut packets| packets.next().expect("one packet"))
    })
    .expect("fixture");
    let mut carriers = Vec::new();
    let mut owners = Vec::new();
    let mut faces = Vec::new();
    for item in 0..1024 {
        let x = f64::from(item) * 4.0;
        let geometry = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)],
                    vec![
                        Point3::new(x + 1.0, 0.0, 0.0),
                        Point3::new(x + 1.0, 1.0, 0.0),
                    ],
                ],
                None,
            ),
            false,
        )
        .expect("admitted fixture")
        .expect("square");
        carriers.push(FreeformSurface {
            pos: usize::try_from(item).expect("bounded"),
            identity: None,
            geometry,
        });
        let mut owner = template.clone();
        let lower = f32::from(u16::try_from(item * 4).expect("bounded"));
        owner.numeric_tail = CatiaOwnerNumericTail::new(
            [0x84, 0x41, 0, 0, 0x0d],
            [0.0; 2],
            [1.0; 2],
            [[lower, lower + 1.0], [0.0, 1.0], [-0.1, 0.1]],
        )
        .expect("tail");
        owners.push(owner);
        faces.push(StandardFaceBounds {
            aabb_center: [
                crate::test_support::test_b5::finite(x + 0.5),
                crate::test_support::test_b5::finite(0.5),
                crate::test_support::test_b5::finite(0.0),
            ],
            aabb_half_extents: [
                crate::test_support::test_b5::nonnegative_length(0.5),
                crate::test_support::test_b5::nonnegative_length(0.5),
                crate::test_support::test_b5::nonnegative_length(0.0),
            ],
            sphere_center: [crate::test_support::test_b5::finite(0.0); 3],
            sphere_radius: crate::test_support::test_b5::nonnegative_length(1.0),
        });
    }
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let index = A5BindingIndex::new(ctx, &carriers, &owners)?;
        let mut owner_carriers = Vec::new();
        for (item, owner) in owners.iter().enumerate() {
            let matched = index.matching_carriers(ctx, &owner.numeric_tail)?;
            assert_eq!(matched, vec![item]);
            owner_carriers.push(matched);
        }
        for (item, face) in faces.iter().enumerate() {
            assert_eq!(
                index.containing_owners(ctx, *face, &owner_carriers)?,
                vec![item]
            );
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    };
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_a5_owner_carriers", run)
            .expect_err("first query")
    else {
        panic!("work refusal");
    };
    crate::test_support::with_work_limit(limit.used + 900_000, run).expect("both indexed products");
}

#[test]
fn procedural_support_lookup_uses_the_stored_arena_index() {
    use super::super::standard_extrusion_support_id;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    let expected = SurfaceId::mint("catia:test:surface#existing").expect("id");
    let mut surfaces = (0..1024)
        .map(|index| Surface {
            id: SurfaceId::mint(format!("catia:test:surface#{index}")).expect("id"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        })
        .collect::<Vec<_>>();
    surfaces.push(Surface {
        id: expected.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    let mut supports = std::collections::HashMap::from([(7u32, 1024usize)]);
    crate::test_support::with_work_limit(1024, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let result = standard_extrusion_support_id(
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut surfaces,
            &mut supports,
            &mut ctx.reserve_scoped(0, "test support map").expect("scope"),
            7,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            &mut admission,
        )?;
        assert_eq!(result, expected);
        assert_eq!(surfaces.len(), 1025);
        assert_eq!(
            supports,
            std::collections::HashMap::from([(7u32, 1024usize)])
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("one lookup and identity copy");
}

#[test]
fn limit_curve_index_filters_distinct_curves_and_checks_collisions() {
    use crate::families::standard::decode::{
        edge_geometry::nurbs_curve_fingerprint, LimitCurveIndex,
    };
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    let curves = (0..1024_u32)
        .map(|row| {
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    Point3::new(f64::from(row), 0.0, 0.0),
                    Point3::new(f64::from(row), 1.0, 0.0),
                ],
                None,
                false,
            )
            .expect("construction admission")
            .expect("linear curve")
        })
        .collect::<Vec<_>>();
    // Index construction plus every distinct lookup stays below the 523,776
    // earlier-curve visits of an incremental linear scan.
    crate::test_support::with_work_limit(400_000, |ctx| {
        let mut index = LimitCurveIndex::new(ctx, &[]).expect("empty index");
        for (row, curve) in curves.iter().enumerate() {
            let key = nurbs_curve_fingerprint(ctx, curve).expect("fingerprint");
            assert!(!index.contains(ctx, key, &curves, curve).expect("query"));
            index.insert(ctx, key, row).expect("index insertion");
            assert!(index.contains(ctx, key, &curves, curve).expect("repeat"));
        }
        index.insert(ctx, 0, 0).expect("synthetic collision bucket");
        assert!(!index
            .contains(ctx, 0, &curves, &curves[1])
            .expect("exact collision check"));
    });
    let negative_zero = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![-0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(-0.0, -0.0, -0.0), Point3::new(0.0, 1.0, -0.0)],
        None,
        false,
    )
    .expect("construction admission")
    .expect("linear curve");
    assert_eq!(curves[0], negative_zero);
    crate::test_support::with_service_context(|ctx| {
        assert_eq!(
            nurbs_curve_fingerprint(ctx, &curves[0]).expect("key"),
            nurbs_curve_fingerprint(ctx, &negative_zero).expect("normalized key")
        );
    });
}

#[test]
fn native_surface_index_preserves_source_precedence_and_ambiguity() {
    use crate::families::b5::transfer::ResolvedPcurveSurface;
    use crate::families::standard::decode::edge_geometry::{
        ensure_native_edge_support_surface, NativeSurfaceIndex,
    };
    use cadmpeg_ir::{
        annotations::AnnotationBuilder,
        geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry},
        ids::SurfaceId,
        CadIr,
    };
    let plane = |row: u32| {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, f64::from(row)),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        ))
    };
    let surfaces = (0..1024_u32)
        .map(|row| Surface {
            id: SurfaceId::mint(format!("catia:test:surface#{row}")).expect("identity"),
            geometry: plane(row),
            source_object: None,
        })
        .collect::<Vec<_>>();
    crate::test_support::with_work_limit(500_000, |ctx| {
        let mut ir = CadIr::empty();
        ir.model.surfaces = surfaces.clone();
        let mut index = NativeSurfaceIndex::new(ctx).expect("index");
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let mut annotations = AnnotationBuilder::new();
        for row in 0..1024_u32 {
            let id = ensure_native_edge_support_surface(
                &mut ir,
                &mut annotations,
                row,
                &ResolvedPcurveSurface::Geometry(plane(row)),
                &mut index,
                &mut admission,
            )
            .expect("indexed exact geometry reuse");
            assert_eq!(id, surfaces[usize::try_from(row).expect("row")].id);
        }
        assert_eq!(ir.model.surfaces.len(), 1024);
        // The source identity wins even when the requested geometry differs.
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#source").expect("id"),
            geometry: plane(2000),
            source_object: Some(crate::assemble::cgm_source(ctx, "surface", 42).expect("source")),
        });
        let first = ensure_native_edge_support_surface(
            &mut ir,
            &mut annotations,
            42,
            &ResolvedPcurveSurface::Geometry(plane(0)),
            &mut index,
            &mut admission,
        )
        .expect("source wins");
        assert_eq!(first, ir.model.surfaces[1024].id);
        // A second distinct source identity leaves a tombstone. Geometry
        // fallback must not pick the otherwise unique plane zero.
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#other").expect("id"),
            geometry: plane(2001),
            source_object: Some(crate::assemble::cgm_source(ctx, "surface", 42).expect("source")),
        });
        let emitted = ensure_native_edge_support_surface(
            &mut ir,
            &mut annotations,
            42,
            &ResolvedPcurveSurface::Geometry(plane(0)),
            &mut index,
            &mut admission,
        )
        .expect("ambiguous source emits");
        assert_eq!(ir.model.surfaces.len(), 1027);
        assert_eq!(emitted, ir.model.surfaces[1026].id);
        // Plane zero now has two identities. An unrelated source also emits.
        ensure_native_edge_support_surface(
            &mut ir,
            &mut annotations,
            43,
            &ResolvedPcurveSurface::Geometry(plane(0)),
            &mut index,
            &mut admission,
        )
        .expect("ambiguous geometry emits");
        assert_eq!(ir.model.surfaces.len(), 1028);
    });
}

#[test]
fn circle_interval_selection_filters_disjoint_and_coincident_prefixes() {
    use crate::families::standard::decode::edge_geometry::circular_range_choices_have_simple_selection;
    let choices = (0..127_u32)
        .map(|row| {
            let start = f64::from(row) * std::f64::consts::TAU / 127.0;
            [[start, start + 0.02]]
        })
        .collect::<Vec<_>>();
    let error =
        crate::test_support::with_work_refusal("catia_standard_circle_range_selection", |ctx| {
            circular_range_choices_have_simple_selection(ctx, &choices)
        })
        .expect_err("first interval query");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal")
    };
    // The complete query/update pass fits below the 8001 old prefix visits.
    crate::test_support::with_work_limit(limit.used + 7500, |ctx| {
        assert!(circular_range_choices_have_simple_selection(ctx, &choices)
            .expect("indexed compatible arcs"));
    });
    let coincident = (0..127_u32)
        .map(|row| {
            let offset = f64::from(row)
                * crate::families::standard::decode::EPS_STANDARD_DECODE_GEOMETRY
                / 254.0;
            [[offset, 1.0 + offset]]
        })
        .collect::<Vec<_>>();
    crate::test_support::with_retained_limit(0, |ctx| {
        assert!(
            circular_range_choices_have_simple_selection(ctx, &coincident)
                .expect("coincident subtree and temporary state")
        );
    });
}

#[test]
fn indexed_circle_selection_matches_exhaustive_pair_predicate() {
    use crate::families::standard::decode::edge_geometry::{
        circular_range_choices_have_simple_selection,
        circular_ranges_are_nonoverlapping_or_coincident,
    };
    fn exhaustive(rows: &[Vec<[f64; 2]>], selected: &mut Vec<[f64; 2]>) -> bool {
        if selected.len() == rows.len() {
            return crate::test_support::with_service_context(|ctx| {
                circular_ranges_are_nonoverlapping_or_coincident(ctx, selected)
            })
            .expect("pair oracle");
        }
        for &range in &rows[selected.len()] {
            selected.push(range);
            if exhaustive(rows, selected) {
                return true;
            }
            selected.pop();
        }
        false
    }
    let eps = crate::families::standard::decode::EPS_STANDARD_DECODE_GEOMETRY;
    let ranges = [
        [0.0, 1.0],
        [0.75 * eps, 1.0 + 0.75 * eps],
        [1.5 * eps, 1.0 + 1.5 * eps],
        [1.0, 2.0],
        [5.5, 6.5],
        [0.0, 0.5],
        [-0.0, 1.0],
        [2.0, 1.0],
        [f64::NAN, 1.0],
        [0.0, f64::INFINITY],
    ];
    for &left in &ranges {
        for &right in &ranges {
            let rows = vec![vec![left], vec![right, [3.0, 4.0]], vec![[4.0, 5.0]]];
            let expected = exhaustive(&rows, &mut Vec::new());
            let actual = crate::test_support::with_service_context(|ctx| {
                circular_range_choices_have_simple_selection(ctx, &rows)
            })
            .expect("indexed selection");
            assert_eq!(actual, expected, "choices {rows:?}");
        }
    }
}

#[test]
fn line_segment_index_filters_separated_segments_and_exact_duplicates() {
    use crate::families::standard::decode::edge_geometry::StandardLinePairConstraint;
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::{ids::PointId, topology::Point};
    for separated in [true, false] {
        let points = (0..1024_u32)
            .flat_map(|row| {
                let y = if separated { f64::from(row) } else { 0.0 };
                [Point3::new(0.0, y, 0.0), Point3::new(1.0, y, 0.0)]
                    .into_iter()
                    .enumerate()
                    .map(move |(endpoint, position)| {
                        Point::new(
                            PointId::mint(format!("catia:test:point#{row}-{endpoint}"))
                                .expect("id"),
                            crate::test_support::test_b5::point(position.into()),
                            None,
                        )
                    })
            })
            .collect::<Vec<_>>();
        let supports = (0..1024_usize)
            .map(|row| StandardCurveSupport {
                pos: row,
                tag: 0,
                faces: [0, 0],
                geometry: StandardCurveGeometry::Line,
            })
            .collect::<Vec<_>>();
        let options = (0..1024_usize)
            .map(|row| vec![[2 * row, 2 * row + 1], [2 * row + 1, 2 * row]])
            .collect::<Vec<_>>();
        let pairs = options.iter().map(|row| Some(row[0])).collect::<Vec<_>>();
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let constraint = StandardLinePairConstraint::new(ctx, &points, &supports, &options)?;
            constraint.is_simple(ctx, &constraint.edge_pairs(&pairs).expect("matching roles"))
        };
        let error = crate::test_support::with_work_refusal("catia_standard_line_right_edges", run)
            .expect_err("first query");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal")
        };
        crate::test_support::with_work_limit(limit.used + 400_000, |ctx| {
            assert!(run(ctx).expect("indexed segments"));
        });
    }
}

#[test]
fn indexed_line_validation_matches_pair_predicate() {
    use crate::families::standard::decode::edge_geometry::{
        standard_line_pair_solution_is_simple, StandardLinePairConstraint,
    };
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::{ids::PointId, topology::Point};
    let spans = [[0.0, 1.0], [0.5, 1.5], [1.0, 2.0], [0.0, 1.0], [1.0, 0.0]];
    for &a in &spans {
        for &b in &spans {
            for y in [0.0, 0.001, 0.003] {
                let positions = [
                    Point3::new(a[0], 0.0, 0.0),
                    Point3::new(a[1], 0.0, 0.0),
                    Point3::new(b[0], y, 0.0),
                    Point3::new(b[1], y, 0.0),
                ];
                let points = positions
                    .into_iter()
                    .enumerate()
                    .map(|(row, p)| {
                        Point::new(
                            PointId::mint(format!("catia:test:point#{row}")).expect("id"),
                            crate::test_support::test_b5::point(p.into()),
                            None,
                        )
                    })
                    .collect::<Vec<_>>();
                let supports = (0..2)
                    .map(|row| StandardCurveSupport {
                        pos: row,
                        tag: 0,
                        faces: [0, 0],
                        geometry: StandardCurveGeometry::Line,
                    })
                    .collect::<Vec<_>>();
                let options = [vec![[0, 1], [1, 0]], vec![[2, 3], [3, 2]]];
                let pairs = [Some([0, 1]), Some([2, 3])];
                let expected =
                    standard_line_pair_solution_is_simple(&points, &supports, &options, &pairs);
                let actual = crate::test_support::with_service_context(|ctx| {
                    let constraint =
                        StandardLinePairConstraint::new(ctx, &points, &supports, &options)
                            .expect("constraint");
                    constraint
                        .is_simple(ctx, &constraint.edge_pairs(&pairs).expect("roles"))
                        .expect("validation")
                });
                assert_eq!(actual, expected, "spans {a:?} {b:?}, offset {y}");
            }
        }
    }
}

#[test]
fn limit_binding_bounds_remove_separated_point_and_support_products() {
    use super::super::edge_geometry::standard_limit_curve_bindings;
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{PointId, SurfaceId};
    use cadmpeg_ir::topology::Point;
    use std::collections::HashMap;

    let mut ir = CadIr::empty();
    let mut curves = Vec::new();
    let mut supports = Vec::new();
    let mut bindings = Vec::new();
    let mut surface_indices = HashMap::new();
    for item in 0..1024_u32 {
        let y = f64::from(item) * 10.0;
        for x in [0.0, 5.0] {
            let id = PointId::mint(format!("catia:test:point#{}", ir.model.points.len()))
                .expect("identity");
            ir.model.points.push(Point::new(
                id,
                crate::test_support::test_b5::point([x, y, 0.0]),
                None,
            ));
        }
        curves.push(
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                5,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                (0..6).map(|x| Point3::new(f64::from(x), y, 0.0)).collect(),
                None,
                false,
            )
            .expect("fixture admission")
            .expect("degree-five curve"),
        );
        let id = SurfaceId::mint(format!("catia:test:surface#{item}")).expect("identity");
        let geometry = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, y, 100.0), Point3::new(0.0, y + 1.0, 100.0)],
                    vec![Point3::new(5.0, y, 100.0), Point3::new(5.0, y + 1.0, 100.0)],
                ],
                None,
            ),
            false,
        )
        .expect("fixture admission")
        .expect("finite patch");
        let face = supports.len();
        surface_indices.insert(id.clone(), ir.model.surfaces.len());
        bindings.push((id.clone(), false, face));
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
            source_object: None,
        });
        supports.push(StandardCurveSupport {
            pos: face,
            tag: item,
            faces: [face, face],
            geometry: StandardCurveGeometry::Bspline,
        });
    }
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        standard_limit_curve_bindings(ctx, &ir, &bindings, &surface_indices, &supports, &curves)
    };
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_limit_curve_support_queries", run)
            .expect_err("first support query")
    else {
        panic!("resource refusal")
    };
    // The measured prefix includes every exact point match. The remaining
    // queries fit 500,000 units; the old support product alone visits 1,048,576 rows.
    let rows = crate::test_support::with_work_limit(limit.used + 500_000, run)
        .expect("bounded support queries");
    assert_eq!(rows, vec![Vec::new(); 1024]);

    // Each point is separated from every curve control hull. Admit the measured
    // index construction prefix, then 500,000 query units. The old point
    // product visits 2,097,152 rows before its exact parameter checks.
    for point in &mut ir.model.points {
        let position = point.position().get();
        point.set_position(crate::test_support::test_b5::point([
            position.x, position.y, 10.0,
        ]));
    }
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        standard_limit_curve_bindings(ctx, &ir, &bindings, &surface_indices, &supports, &curves)
    };
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_limit_curve_point_queries", run)
            .expect_err("first point query")
    else {
        panic!("resource refusal")
    };
    let rows = crate::test_support::with_work_limit(limit.used + 500_000, run)
        .expect("bounded point queries");
    assert_eq!(rows, vec![Vec::new(); 1024]);
}

#[test]
fn limit_curve_candidate_selection_stops_after_a_third_supported_point() {
    use super::super::edge_geometry::standard_limit_curve_bindings;
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{PointId, SurfaceId};
    use cadmpeg_ir::topology::Point;
    use std::collections::HashMap;
    let mut ir = CadIr::empty();
    for item in 0..1027_u32 {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#{item}")).expect("identity"),
            crate::test_support::test_b5::point([f64::from(item % 3), 0.0, 0.0]),
            None,
        ));
    }
    let id = SurfaceId::mint("catia:test:surface#plane").expect("identity");
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        )),
        source_object: None,
    });
    let curves = [NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        5,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        (0..6)
            .map(|x| Point3::new(f64::from(x), 0.0, 0.0))
            .collect(),
        None,
        false,
    )
    .expect("fixture admission")
    .expect("curve")];
    let bindings = [(id.clone(), false, 0)];
    let indices = HashMap::from([(id, 0)]);
    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    }];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        standard_limit_curve_bindings(ctx, &ir, &bindings, &indices, &supports, &curves)
    };
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_limit_curve_candidates", run)
            .expect_err("first candidate")
    else {
        panic!("resource refusal")
    };
    // Three candidate visits each perform two surface lookups. Each lookup
    // prices both the identity scan and comparison; 512 covers those fixed
    // keys and control steps and remains below the old 1,027-row bulk charge.
    assert_eq!(
        crate::test_support::with_work_limit(limit.used + 512, run)
            .expect("three visited candidates"),
        vec![Vec::new()]
    );
}

#[test]
fn standard_route_setup_retains_no_data_before_payload_emission() {
    let bytes = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, &bytes).expect("synthetic container")
    });
    let result = crate::test_support::with_retained_limit(0, |ctx| {
        super::super::try_decode_standard(ctx, &scan, &mut crate::nurbs::LaneRefusals::new())
    });
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("first emitted identity requires retained storage");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert_eq!(limit.operation, "catia_standard_payload_id");
    assert_eq!(limit.used, 0);
}

#[test]
fn unselected_e5_surface_carriers_use_only_temporary_storage() {
    let stream = crate::test_support::test_e5::e5_torus_stream();
    crate::test_support::with_retained_limit(0, |ctx| {
        assert!(super::super::associate_standard_freeform_e5_surfaces(
            ctx,
            &[],
            &stream,
            &std::collections::HashMap::new(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("unselected carriers are scratch")
        .is_empty());
    });
}

const LINE_COINCIDENCE_OFFSET_STEP: f64 = 1e-7;

#[test]
fn line_segment_projection_filters_overlapping_boxes_and_tolerance_coincidence() {
    use crate::families::standard::decode::edge_geometry::StandardLinePairConstraint;
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::{ids::PointId, topology::Point};
    for (separated, vertical) in [(true, false), (false, false), (false, true)] {
        let points = (0..1024_u32)
            .flat_map(|row| {
                let y = if separated {
                    f64::from(row)
                } else {
                    f64::from(row) * LINE_COINCIDENCE_OFFSET_STEP
                };
                {
                    let mut endpoints = if vertical {
                        let zero = if row % 2 == 0 { -0.0 } else { 0.0 };
                        [
                            Point3::new(zero, y, 0.0),
                            Point3::new(-zero, 10_000.0 + y, 0.0),
                        ]
                    } else {
                        [
                            Point3::new(0.0, y, 0.0),
                            Point3::new(10_000.0, 10_000.0 + y, 0.0),
                        ]
                    };
                    if row % 2 != 0 {
                        endpoints.swap(0, 1);
                    }
                    endpoints
                }
                .into_iter()
                .enumerate()
                .map(move |(endpoint, position)| {
                    Point::new(
                        PointId::mint(format!("catia:test:point#{row}-{endpoint}")).expect("id"),
                        crate::test_support::test_b5::point(position.into()),
                        None,
                    )
                })
            })
            .collect::<Vec<_>>();
        let supports = (0..1024_usize)
            .map(|row| StandardCurveSupport {
                pos: row,
                tag: 0,
                faces: [0, 0],
                geometry: StandardCurveGeometry::Line,
            })
            .collect::<Vec<_>>();
        let options = (0..1024_usize)
            .map(|row| vec![[2 * row, 2 * row + 1], [2 * row + 1, 2 * row]])
            .collect::<Vec<_>>();
        let pairs = options.iter().map(|row| Some(row[0])).collect::<Vec<_>>();
        assert!(
            crate::families::standard::decode::edge_geometry::standard_line_pair_solution_is_simple(
                &points, &supports, &options, &pairs,
            )
        );
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let constraint = StandardLinePairConstraint::new(ctx, &points, &supports, &options)?;
            constraint.is_simple(ctx, &constraint.edge_pairs(&pairs).expect("matching roles"))
        };
        let error = crate::test_support::with_work_refusal("catia_standard_line_right_edges", run)
            .expect_err("first query");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal")
        };
        crate::test_support::with_work_limit(limit.used + 400_000, |ctx| {
            assert!(run(ctx).expect("indexed segments"));
        });
    }
}

#[test]
fn projected_line_validation_matches_pair_predicate_at_tolerance_boundaries() {
    use crate::families::standard::decode::edge_geometry::{
        standard_line_pair_solution_is_simple, standard_line_pair_solution_is_simple_cached,
    };
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::{ids::PointId, topology::Point};
    for origin in [-100_000_000.0, 0.0, 100_000_000.0] {
        for length in [0.001, 0.002, 0.003, 1.0, 10_000.0] {
            for offset in [0.0, 0.0019, 0.002, 0.0021] {
                for angle in [0.0, 0.0002] {
                    let points = (0..8_u32)
                        .flat_map(|row| {
                            let shift = f64::from(row) * offset;
                            let turn = f64::from(row) * angle;
                            let mut ends = [
                                Point3::new(origin, origin + shift, 0.0),
                                Point3::new(
                                    origin + length,
                                    origin + shift + length * (1.0 + turn),
                                    0.0,
                                ),
                            ];
                            if row % 2 != 0 {
                                ends.swap(0, 1);
                            }
                            ends.into_iter().enumerate().map(move |(end, position)| {
                                Point::new(
                                    PointId::mint(format!("catia:test:point#{row}-{end}"))
                                        .expect("id"),
                                    crate::test_support::test_b5::point(position.into()),
                                    None,
                                )
                            })
                        })
                        .collect::<Vec<_>>();
                    let supports = (0..8_usize)
                        .map(|row| StandardCurveSupport {
                            pos: row,
                            tag: 0,
                            faces: [0, 0],
                            geometry: StandardCurveGeometry::Line,
                        })
                        .collect::<Vec<_>>();
                    let options = (0..8_usize)
                        .map(|row| vec![[2 * row, 2 * row + 1], [2 * row + 1, 2 * row]])
                        .collect::<Vec<_>>();
                    let pairs = options.iter().map(|row| Some(row[0])).collect::<Vec<_>>();
                    assert_eq!(
                        standard_line_pair_solution_is_simple_cached(
                            &points, &supports, &options, &pairs
                        ),
                        standard_line_pair_solution_is_simple(&points, &supports, &options, &pairs),
                        "origin={origin} length={length} offset={offset} angle={angle}"
                    );
                }
            }
        }
    }
}
