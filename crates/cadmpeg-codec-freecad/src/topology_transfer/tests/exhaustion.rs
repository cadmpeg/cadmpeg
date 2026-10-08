// SPDX-License-Identifier: Apache-2.0
//! Known-size visits and original-refusal reentry.

use crate::topology_transfer::{
    close_radial_rings, connected_components, edge_endpoint_uses, referenced_pcurve_ids,
    select_pcurve_representation, source_topology_indices, transfer, Builder, GeometryIndexes,
};
use crate::brep::{
    ShapePayload, ShapePayloadRecord, Tables, TextEdgeRepresentation, TextOrientation,
    TextShapeUse, TextTShapes,
};
use crate::native::element_map::ScopedData;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::transform::Transform;

fn with_work<T>(work: u64, call: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    call(&ctx)
}

fn tables(tshapes: &TextTShapes) -> Tables<'_> {
    Tables {
        locations: &[], curve2ds: &[], curves: &[], surfaces: &[],
        polygons3d: &[], polygons_on_triangulations: &[], triangulations: &[],
        tshapes, roots: &[],
    }
}

fn payload() -> ShapePayloadRecord {
    ShapePayloadRecord {
        id: "fcstd:native:entry#Exhaustion".into(),
        property: "fcstd:native:property#Exhaustion".into(),
        entry: "Shape.brp".into(),
        payload: ShapePayload::Empty,
    }
}

fn builder<'a, 'c, 'r, 'occ>(
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    tables: Tables<'a>,
) -> Builder<'a, 'c, 'r, 'occ> {
    Builder::new(
        ctx, payload, tables,
        ScopedData {
            data: cadmpeg_core::text::NonBlankString::try_from("Object").unwrap(),
            _storage: ctx.reserve_scoped(0, "test source object").unwrap(),
        },
        GeometryIndexes::new(ctx).unwrap(), None,
    ).unwrap()
}

fn assert_original<T>(result: Result<T, CodecError>, original: ResourceLimit) {
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn indexed_polygon_first_invalid_node_leaves_suffix_unvisited() {
    let tshapes = TextTShapes::default();
    let payload = payload();
    let polygons = [crate::brep::TextPolygonOnTriangulation {
        nodes: vec![0; 1025],
        parameters: None,
        deflection: cadmpeg_ir::scalar::NonNegativeReal::ZERO,
    }];
    let triangulations = [serde_json::from_value(serde_json::json!({
        "deflection": 0.0,
        "nodes": [{"x": 1.0, "y": 2.0, "z": 3.0}],
        "triangles": [], "uv_nodes": null, "normals": null,
    })).expect("finite triangulation")];
    let mut tables = tables(&tshapes);
    tables.polygons_on_triangulations = &polygons;
    tables.triangulations = &triangulations;
    with_work(1, |ctx| {
        let builder = builder(ctx, &payload, tables);
        assert!(matches!(builder.indexed_polygon(1, 1),
            Err(CodecError::Malformed(message))
                if message == "polygon-on-triangulation node is out of bounds"));
        assert_eq!(ctx.resource_refusal(), None);
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "after polygon prefix").unwrap_err()
            else { panic!("first node exhausts work") };
        assert_eq!(original.used, 1);
        assert_original(builder.indexed_polygon(1, 1), original);
    });
    with_work(0, |ctx| {
        let builder = builder(ctx, &payload, tables);
        let Err(CodecError::ResourceLimit(original)) = builder.indexed_polygon(1, 1)
            else { panic!("visit refuses before invalid lookup") };
        assert_eq!(original.operation, "FreeCAD indexed polygon node scan");
        assert_eq!((original.used, original.additional), (0, 1));
        assert_original(builder.indexed_polygon(1, 1), original);
    });
}

#[test]
fn indexed_polygon_valid_nodes_charge_only_actual_visits_and_keep_order() {
    let tshapes = TextTShapes::default();
    let payload = payload();
    let polygons = [crate::brep::TextPolygonOnTriangulation {
        nodes: vec![2, 1], parameters: None,
        deflection: cadmpeg_ir::scalar::NonNegativeReal::ZERO,
    }];
    let triangulations = [serde_json::from_value(serde_json::json!({
        "deflection": 0.0,
        "nodes": [{"x": 1.0, "y": 2.0, "z": 3.0}, {"x": 4.0, "y": 5.0, "z": 6.0}],
        "triangles": [], "uv_nodes": null, "normals": null,
    })).expect("finite triangulation")];
    let mut tables = tables(&tshapes);
    tables.polygons_on_triangulations = &polygons;
    tables.triangulations = &triangulations;
    with_work(2, |ctx| {
        let result = builder(ctx, &payload, tables).indexed_polygon(1, 1).unwrap();
        assert_eq!(result.samples.to_raw(),
            cadmpeg_ir::geometry::sampled::PolylineSamples::Unparameterized {
                points: vec![cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0),
                    cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)].try_into().unwrap(),
            });
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn empty_topology_sources_and_indexes_need_no_work() {
    with_work(0, |ctx| {
        let tshapes = TextTShapes::default();
        assert!(referenced_pcurve_ids(ctx, &[]).unwrap().is_empty());
        assert!(source_topology_indices(ctx, tables(&tshapes)).unwrap().is_empty());
        assert!(connected_components(ctx, &[]).unwrap().is_empty());
        let mut indexes = GeometryIndexes::new(ctx).unwrap();
        let ir = CadIr::empty();
        assert_eq!(indexes.curve_position(ctx, &ir, &CurveId::mint("fcstd:test:curve#1").unwrap()).unwrap(), None);
        assert_eq!(indexes.surface_position(ctx, &ir, &SurfaceId::mint("fcstd:test:surface#1").unwrap()).unwrap(), None);
        indexes.ensure_procedural(ctx, &ir).unwrap();
        indexes.ensure_procedural(ctx, &ir).unwrap();
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn empty_builder_scans_need_no_work() {
    with_work(0, |ctx| {
        let tshapes = TextTShapes::default();
        let payload = payload();
        let mut builder = builder(ctx, &payload, tables(&tshapes));
        builder.emit_pcurves().unwrap();
        builder.emit_unowned_triangulations(&mut CadIr::empty()).unwrap();
        assert!(builder.body_roots().unwrap().is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn topology_empty_and_cached_helpers_return_original_refusal() {
    with_work(0, |ctx| {
        let tshapes = TextTShapes::default();
        let payload = payload();
        let mut builder = builder(ctx, &payload, tables(&tshapes));
        let mut indexes = GeometryIndexes::new(ctx).unwrap();
        indexes.ensure_procedural(ctx, &CadIr::empty()).unwrap();
        ctx.charge_work(1, "test original fuse").unwrap_err();
        let original = ctx.resource_refusal().unwrap();
        assert_original(referenced_pcurve_ids(ctx, &[]), original);
        assert_original(source_topology_indices(ctx, tables(&tshapes)), original);
        assert_original(connected_components(ctx, &[]), original);
        assert_original(builder.emit_pcurves(), original);
        assert_original(builder.emit_unowned_triangulations(&mut CadIr::empty()), original);
        assert_original(builder.body_roots(), original);
        assert_original(indexes.ensure_procedural(ctx, &CadIr::empty()), original);
        assert_original(edge_endpoint_uses(ctx, 1, &[]), original);
        assert_original(select_pcurve_representation(ctx, &[], &tables(&tshapes),
            Transform::identity(), 1, Transform::identity()), original);
        assert_original(transfer(ctx, &mut CadIr::empty(), &[], &[], &mut Vec::new(), false), original);
        assert_original(close_radial_rings(ctx, &mut []), original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

fn shape_use(shape: usize, orientation: TextOrientation) -> TextShapeUse {
    TextShapeUse { shape, orientation, location: 0.into() }
}

#[test]
fn endpoint_exact_exhaustion_costs_only_actual_visits() {
    let children = [shape_use(1, TextOrientation::Forward), shape_use(2, TextOrientation::Reversed)];
    with_work(2, |ctx| {
        let (start, end) = edge_endpoint_uses(ctx, 9, &children).unwrap();
        assert_eq!(start.shape, 1);
        assert_eq!(end.shape, 2);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn endpoint_first_error_leaves_suffix_unvisited() {
    let mut children = vec![shape_use(1, TextOrientation::Forward); 259];
    children[1] = shape_use(2, TextOrientation::Reversed);
    with_work(3, |ctx| {
        assert!(matches!(edge_endpoint_uses(ctx, 9, &children),
            Err(CodecError::Malformed(message)) if message == "edge TShape 9 has multiple forward endpoint uses"));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn endpoint_refusal_precedes_duplicate_suffix_and_survives_empty_reentry() {
    let children = [shape_use(1, TextOrientation::Forward), shape_use(2, TextOrientation::Reversed),
        shape_use(3, TextOrientation::Forward)];
    with_work(2, |ctx| {
        let error = edge_endpoint_uses(ctx, 9, &children).unwrap_err();
        let CodecError::ResourceLimit(original) = error else { panic!("expected visit refusal") };
        assert_eq!(original.operation, "FreeCAD edge endpoint search");
        assert_eq!((original.used, original.additional), (2, 1));
        assert_original(edge_endpoint_uses(ctx, 9, &[]), original);
    });
}

#[test]
fn pcurve_nonmatching_and_empty_sources_cost_only_actual_visits() {
    let tshapes = TextTShapes::default();
    let representations: [TextEdgeRepresentation; 3] = std::array::from_fn(|_|
        TextEdgeRepresentation::Polygon3d { polygon: 1, location: 0 });
    with_work(0, |ctx| {
        assert!(select_pcurve_representation(ctx, &[], &tables(&tshapes),
            Transform::identity(), 1, Transform::identity()).unwrap().is_none());
    });
    with_work(3, |ctx| {
        assert!(select_pcurve_representation(ctx, &representations, &tables(&tshapes),
            Transform::identity(), 1, Transform::identity()).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn empty_endpoint_source_keeps_missing_endpoints_error_without_work() {
    with_work(0, |ctx| {
        assert!(matches!(edge_endpoint_uses(ctx, 9, &[]),
            Err(CodecError::Malformed(message))
                if message == "edge TShape 9 does not have both forward and reversed endpoint uses"));
        assert_eq!(ctx.resource_refusal(), None);
    });
}
