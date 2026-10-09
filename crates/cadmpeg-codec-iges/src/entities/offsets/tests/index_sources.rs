// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::OffsetSourceIndex;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::topology::EdgeCarrier;

#[derive(Clone, Copy)]
enum Source { Curve, Point, Vertex, Edge }

impl Source {
    fn operation(self) -> &'static str {
        match self {
            Self::Curve => "iges offset source curve scan",
            Self::Point => "iges offset source point scan",
            Self::Vertex => "iges offset source vertex scan",
            Self::Edge => "iges offset source edge scan",
        }
    }

    fn key(self, position: usize) -> String {
        let kind = match self {
            Self::Curve | Self::Edge => "curve",
            Self::Point => "point",
            Self::Vertex => "vertex",
        };
        format!("iges:model:{kind}#D{}", 2 * position + 1)
    }

    fn append(self, ir: &mut CadIr, positions: std::ops::Range<usize>) {
        for position in positions {
            let key = self.key(position);
            match self {
                Self::Curve => ir.model.curves.push(Curve {
                    id: CurveId::mint(key).unwrap(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
                        ).unwrap(),
                    )), source_object: None,
                }),
                Self::Point => ir.model.points.push(Point::new(
                    PointId::mint(key).unwrap(),
                    FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(), None,
                )),
                Self::Vertex => ir.model.vertices.push(Vertex {
                    id: VertexId::mint(key).unwrap(),
                    point: PointId::mint("iges:model:point#D1").unwrap(), tolerance: None,
                }),
                Self::Edge => ir.model.edges.push(Edge {
                    id: EdgeId::mint(format!("iges:model:edge#D{}", 2 * position + 1)).unwrap(),
                    carrier: EdgeCarrier::unbounded(Some(CurveId::mint(key).unwrap())),
                    start: VertexId::mint("iges:model:vertex#D1").unwrap(),
                    end: VertexId::mint("iges:model:vertex#D1").unwrap(), tolerance: None,
                }),
            }
        }
    }

    fn prefix_work(self, count: usize) -> u64 {
        let value_bytes = match self {
            Self::Edge => std::mem::size_of::<Vec<usize>>(),
            _ => std::mem::size_of::<usize>(),
        };
        // Each typed identity contains one String, with the same alignment.
        let alignment = std::mem::align_of::<CurveId>().max(std::mem::align_of::<usize>());
        let node = u64_from_index(11 * (std::mem::size_of::<CurveId>() + value_bytes)
            + 16 * std::mem::size_of::<usize>() + 2 * alignment);
        (0..count).map(|position| {
            let comparisons = if position == 0 { 0 } else {
                let height = position.div_ceil(2).ilog(6) + 1;
                u64_from_index(position).min(11 * u64::from(height))
            };
            let key_bytes = u64_from_index(self.key(position).len());
            // One source next, one identity copy, three key queries, and the
            // existing node shift/split bound. A new one-slot edge Vec moves
            // no old backing and adds no Work units.
            1 + key_bytes + 3 * key_bytes * comparisons
                + node * (1 + 2 * u64::from(position.is_multiple_of(5)))
        }).sum()
    }

    fn assert_complete(self, index: &OffsetSourceIndex, count: usize) {
        let expected = match self {
            Self::Curve => [count, 0, 0, 0],
            Self::Point => [0, count, 0, 0],
            Self::Vertex => [0, 0, count, 0],
            Self::Edge => [0, 0, 0, count],
        };
        assert_eq!([index.curve_count, index.point_count, index.vertex_count, index.edge_count], expected);
        assert_eq!([index.curves.len(), index.points.len(), index.vertices.len(), index.edges.len()], expected);
        for position in 0..count {
            let key = self.key(position);
            match self {
                Self::Curve => assert_eq!(index.curves.get(&CurveId::mint(key).unwrap()), Some(&position)),
                Self::Point => assert_eq!(index.points.get(&PointId::mint(key).unwrap()), Some(&position)),
                Self::Vertex => assert_eq!(index.vertices.get(&VertexId::mint(key).unwrap()), Some(&position)),
                Self::Edge => assert_eq!(index.edges.get(&CurveId::mint(key).unwrap()).unwrap(), &[position]),
            }
        }
    }
}

fn boundaries(source: Source) {
    for count in [1, 64] {
        let mut ir = CadIr::empty();
        source.append(&mut ir, 0..count);
        let expected_model = ir.model.clone();
        for visited in [0, count - 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            let cap = source.prefix_work(visited);
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test actual offset source index").unwrap();
            let mut index = OffsetSourceIndex::default();
            let first = match index.update(&ir, &ctx, &mut storage) {
                Err(CodecError::ResourceLimit(first)) => first,
                _ => panic!("expected actual offset source visit refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, source.operation());
            assert_eq!((first.used, first.additional, first.limit), (cap, 1, cap));
            let curves = index.curves.clone();
            let points = index.points.clone();
            let vertices = index.vertices.clone();
            let edges = index.edges.clone();
            let counts = [index.curve_count, index.point_count, index.vertex_count, index.edge_count];
            for replay in [&ir, &CadIr::empty()] {
                assert!(matches!(index.update(replay, &ctx, &mut storage),
                    Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(index.curves, curves);
                assert_eq!(index.points, points);
                assert_eq!(index.vertices, vertices);
                assert_eq!(index.edges, edges);
                assert_eq!([index.curve_count, index.point_count, index.vertex_count, index.edge_count], counts);
            }
            assert_eq!(ir.model, expected_model);
            drop(index);
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = source.prefix_work(count);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test actual offset source index").unwrap();
        let mut index = OffsetSourceIndex::default();
        index.update(&ir, &ctx, &mut storage).unwrap();
        source.assert_complete(&index, count);
        // A second update over identical backing executes no source visits.
        index.update(&ir, &ctx, &mut storage).unwrap();
        source.assert_complete(&index, count);
        assert_eq!(ir.model, expected_model);
        drop(index);
        drop(storage);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn offset_curve_index_first_last_and_exact_completion() { boundaries(Source::Curve); }
#[test]
fn offset_point_index_first_last_and_exact_completion() { boundaries(Source::Point); }
#[test]
fn offset_vertex_index_first_last_and_exact_completion() { boundaries(Source::Vertex); }
#[test]
fn offset_edge_index_first_last_and_exact_completion() { boundaries(Source::Edge); }

#[test]
fn offset_source_index_incremental_suffix_keeps_all_original_positions() {
    for source in [Source::Curve, Source::Point, Source::Vertex, Source::Edge] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = source.prefix_work(64);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test actual offset source index").unwrap();
        let mut index = OffsetSourceIndex::default();
        let mut ir = CadIr::empty();
        for position in 0..64 {
            source.append(&mut ir, position..position + 1);
            let expected = ir.model.clone();
            index.update(&ir, &ctx, &mut storage).unwrap();
            source.assert_complete(&index, position + 1);
            assert_eq!(ir.model, expected);
        }
        drop(index);
        drop(storage);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_offset_source_index_accepts_fresh_zero_budgets() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test empty offset source index").unwrap();
    let mut index = OffsetSourceIndex::default();
    index.update(&CadIr::empty(), &ctx, &mut storage).unwrap();
    Source::Curve.assert_complete(&index, 0);
    drop(index);
    drop(storage);
    ctx.finish_session().unwrap();
}
