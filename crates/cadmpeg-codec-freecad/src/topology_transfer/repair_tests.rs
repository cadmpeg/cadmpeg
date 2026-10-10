// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::{
    connected_components, pcurve_geometry, transfer, Builder, GeometryIndexes, PcurveGeometryError,
};
use crate::brep::triangulation::TextTriangulation;
use crate::brep::{
    NurbsCurve2d, ShapePayload, ShapePayloadRecord, Tables, TextCurve2d, TextEdgeRepresentation,
    TextOrientation, TextPolygon3d, TextShapeUse, TextSurface, TextTShape, TextTShapeGeometry,
    TextTShapes,
};
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::nurbs::NurbsError;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FinitePoint2;

fn with_limits<T>(work: u64, materialized: u64, call: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = materialized;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    call(&ctx)
}

fn payload() -> ShapePayloadRecord {
    ShapePayloadRecord {
        id: "fcstd:native:entry#Repair".into(),
        property: "fcstd:native:property#Repair".into(),
        entry: "Shape.brp".into(),
        payload: ShapePayload::Empty,
    }
}

fn tables(tshapes: &TextTShapes) -> Tables<'_> {
    Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        triangulations: &[],
        tshapes,
        roots: &[],
    }
}

fn builder<'a, 'c, 'r, 'occ>(
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    tables: Tables<'a>,
) -> Result<Builder<'a, 'c, 'r, 'occ>, CodecError> {
    Builder::new(
        ctx,
        payload,
        tables,
        super::ScopedData {
            data: cadmpeg_core::text::NonBlankString::try_from("Object".to_owned())
                .expect("source name"),
            _storage: ctx.reserve_scoped(0, "test topology source object")?,
        },
        GeometryIndexes::new(ctx)?,
        None,
    )
}

fn populated_ir() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("fcstd:model:curve#Repair:1").expect("curve identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                .expect("finite unit line"),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("fcstd:model:surface#Repair:1").expect("surface identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("finite orthonormal plane"),
        )),
        source_object: None,
    });
    ir
}

fn property() -> PropertyRecord {
    PropertyRecord {
        id: "fcstd:native:property#Repair".into(),
        owner: "fcstd:native:object#Object".into(),
        name: "Label".into(),
        type_name: "App::PropertyString".into(),
        family: PropertyFamily::String,
        status: None,
        body: PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_text("<Property/>".into(), 0).expect("retained XML"),
    }
}

#[test]
fn absent_topology_consumers_skip_populated_owner_and_geometry_indexes() {
    let empty_payload = payload();
    for payloads in [&[][..], std::slice::from_ref(&empty_payload)] {
        for operation in [
            "FreeCAD topology property owners",
            "FreeCAD curve index scan",
            "FreeCAD surface index scan",
        ] {
            with_limits(u64::MAX, u64::MAX, |ctx| {
                let mut ir = populated_ir();
                let original = ir.clone();
                let mut losses = Vec::new();
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
                let occurrences =
                    transfer(ctx, &mut ir, payloads, &[property()], &mut losses, false)
                        .expect("no topology index has a consumer");
                assert!(occurrences.records.is_empty());
                assert!(losses.is_empty());
                assert_eq!(ir, original);
                assert_eq!(ctx.resource_refusal(), None);
            });
        }
    }
}

#[test]
fn occurrence_lookup_storage_releases_before_occurrence_copies() {
    let shapes = TextTShapes::from(vec![TextTShape {
        geometry: TextTShapeGeometry::Edge {
            tolerance: FiniteReal::ZERO,
            same_parameter: false,
            same_range: false,
            degenerated: false,
            representations: Vec::new(),
        },
        flags: [false; 7],
        children: Vec::new(),
    }]);
    let roots = [TextShapeUse {
        shape: 1,
        orientation: TextOrientation::Forward,
        location: 0.into(),
    }];
    let mut payload = payload();
    payload.property = "p".into();
    with_limits(u64::MAX, u64::MAX, |ctx| {
        let mut occurrences = super::TopologyOccurrences {
            records: Vec::new(),
            _storage: ctx.reserve_scoped(0, "test occurrences").unwrap(),
        };
        let mut tables = tables(&shapes);
        tables.roots = &roots;
        let mut builder = Builder::new(
            ctx,
            &payload,
            tables,
            super::ScopedData {
                data: cadmpeg_core::text::NonBlankString::try_from("Object").unwrap(),
                _storage: ctx.reserve_scoped(0, "test source object").unwrap(),
            },
            GeometryIndexes::new(ctx).unwrap(),
            Some(&mut occurrences),
        ).unwrap();
        let _probe = RefusalProbe::arm(
            ResourceDimension::MaterializedBytes,
            "FreeCAD topology occurrence property",
            None,
        );
        builder.bind_topology(
            crate::brep::TextShapeKind::Edge, 1, Transform::identity(), "e",
        ).expect("lookup scratch expires before the short occurrence copies");
        drop(builder);
        assert_eq!(occurrences.records.len(), 1);
        assert_eq!(occurrences.records[0].property, "p");
        assert_eq!(occurrences.records[0].indexed_name, "Edge");
        assert_eq!(occurrences.records[0].source_index, 1);
        assert_eq!(occurrences.records[0].topology_id, "e");
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn procedural_indexes_wait_for_the_first_consumer_and_reuse_the_result() {
    let mut ir = populated_ir();
    let construction =
        cadmpeg_ir::ids::ProceduralSurfaceId::mint("fcstd:model:surface#Repair:construction")
            .expect("construction identity");
    let source = ir.model.surfaces[0].id.clone();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("fcstd:model:surface#Repair:procedural").expect("surface identity"),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Replica {
            source,
            transform: Transform::identity(),
        },
        None,
    ));
    for operation in [
        "FreeCAD procedural owner scan",
        "FreeCAD procedural surface index scan",
    ] {
        with_limits(u64::MAX, u64::MAX, |ctx| {
            let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let mut indexes = GeometryIndexes::new(ctx).expect("ordinary position indexes");
            assert!(indexes.procedural.is_none());
            drop(probe);
            indexes
                .ensure_procedural(ctx, &ir)
                .expect("first procedural consumer");
            let procedural = indexes.procedural.as_ref().expect("procedural indexes");
            assert!(procedural.procedural_surfaces.contains(&construction));
            assert_eq!(
                procedural.construction_owners.get(&construction),
                Some(&Some(1))
            );
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            indexes
                .ensure_procedural(ctx, &ir)
                .expect("existing procedural indexes");
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

fn placed_transform() -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite translation")
}

#[test]
fn successful_located_curve_hits_skip_base_identity_work() {
    with_limits(u64::MAX, u64::MAX, |ctx| {
        let payload = payload();
        let tshapes = TextTShapes::default();
        let mut ir = populated_ir();
        let mut builder = builder(ctx, &payload, tables(&tshapes)).expect("builder");
        let first = builder
            .located_curve(&mut ir, 1, placed_transform())
            .expect("first curve");
        assert_eq!(ir.model.curves.len(), 2);
        for operation in ["FreeCAD base curve key", "FreeCAD base curve identity"] {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            assert_eq!(
                builder
                    .located_curve(&mut ir, 1, placed_transform())
                    .expect("curve hit"),
                first
            );
            assert_eq!(ir.model.curves.len(), 2);
            assert_eq!(ctx.resource_refusal(), None);
        }
    });
}

#[test]
fn successful_located_surface_hits_skip_base_identity_work() {
    with_limits(u64::MAX, u64::MAX, |ctx| {
        let payload = payload();
        let tshapes = TextTShapes::default();
        let mut ir = populated_ir();
        let mut builder = builder(ctx, &payload, tables(&tshapes)).expect("builder");
        let first = builder
            .located_surface(&mut ir, 1, placed_transform())
            .expect("first surface");
        assert_eq!(ir.model.surfaces.len(), 2);
        for operation in ["FreeCAD base surface key", "FreeCAD base surface identity"] {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            assert_eq!(
                builder
                    .located_surface(&mut ir, 1, placed_transform())
                    .expect("surface hit"),
                first
            );
            assert_eq!(ir.model.surfaces.len(), 2);
            assert_eq!(ctx.resource_refusal(), None);
        }
    });
}

#[test]
fn parameterized_polygon_releases_input_storage_before_position_indexing() {
    const NODES: usize = 128;
    let polygon = TextPolygon3d {
        deflection: NonNegativeReal::ZERO,
        nodes: (0..NODES).map(|_| FinitePoint3::ZERO).collect(),
        parameters: Some(
            (0..NODES)
                .map(|index| {
                    FiniteReal::new(
                        cadmpeg_core::convert::f64_from_index(index).expect("exact fixture index"),
                    )
                    .expect("finite increasing parameter")
                })
                .collect(),
        ),
    };
    let edge = EdgeId::mint(format!("fcstd:model:edge#Repair:{}", "x".repeat(8192)))
        .expect("edge identity");
    let expected_id = CurveId::mint(format!("{}:polygon:1", edge.as_str())).unwrap();
    const OBSERVATION_CAP: u64 = 65_536;
    let index_bytes = with_limits(u64::MAX, OBSERVATION_CAP, |ctx| {
        let mut geometry = GeometryIndexes::new(ctx).unwrap();
        geometry.curves = Some(std::collections::BTreeMap::new());
        geometry.index_curve(ctx, &expected_id, 0).unwrap();
        let CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(OBSERVATION_CAP, "test curve index usage")
            .unwrap_err()
        else {
            panic!("live index must refuse a full-cap reservation");
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        limit.used
    });
    with_limits(u64::MAX, u64::MAX, |ctx| {
        let payload = payload();
        let tshapes = TextTShapes::default();
        let polygons = [polygon];
        let mut ir = CadIr::empty();
        let mut tables = tables(&tshapes);
        tables.polygons3d = &polygons;
        let mut builder = builder(ctx, &payload, tables).expect("builder");
        assert_eq!(builder.geometry.curve_position(ctx, &ir, &CurveId::mint("fcstd:model:curve#Repair:missing").unwrap()).unwrap(), None);
        // The index is live during polygon_curve. Consumed input lanes must
        // release their storage before the curve-position index is allocated.
        let _probe = RefusalProbe::arm(
            ResourceDimension::MaterializedBytes,
            "FreeCAD curve positions",
            None,
        );
        let baseline = ctx.reserve_scoped(index_bytes, "test curve index baseline").unwrap();
        drop(baseline);
        let id = builder
            .polygon_curve(
                &mut ir,
                &edge,
                0,
                &TextEdgeRepresentation::Polygon3d {
                    polygon: 1,
                    location: 0,
                },
                Transform::identity(),
            )
            .expect("input buffers have been consumed before indexing");
        assert_eq!(ir.model.curves.len(), 1);
        assert_eq!(id, expected_id);
        assert_eq!(
            builder.geometry.curve_position(ctx, &ir, &id).unwrap(),
            Some(0)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

fn native_nurbs(count: usize, rational: bool, first_weight: FiniteReal) -> TextCurve2d {
    let end = cadmpeg_core::convert::f64_from_index(count - 1).expect("exact fixture index");
    let parameters: Vec<_> = (0..count)
        .map(|index| {
            FiniteReal::new(
                cadmpeg_core::convert::f64_from_index(index).expect("exact fixture index") / end,
            )
            .expect("finite knot")
        })
        .collect();
    let mut knots = Vec::with_capacity(count + 2);
    knots.push(FiniteReal::ZERO);
    knots.extend_from_slice(&parameters);
    knots.push(FiniteReal::ONE);
    let points = parameters
        .iter()
        .map(|value| FinitePoint2::new(Point2::new(value.get(), 0.0)).expect("finite pole"))
        .collect();
    let weights = rational.then(|| {
        let mut weights: Vec<_> = (0..count).map(|_| FiniteReal::ONE).collect();
        weights[0] = first_weight;
        weights
    });
    TextCurve2d::Nurbs(NurbsCurve2d {
        degree: 1,
        knots,
        control_points: points,
        weights,
        periodic: false,
    })
}

#[test]
fn rational_pcurve_first_zero_weight_does_not_charge_its_untouched_suffix() {
    let curve = native_nurbs(128, true, FiniteReal::ZERO);
    with_limits(1, u64::MAX, |ctx| {
        let error = pcurve_geometry(ctx, &curve).expect_err("first zero weight");
        assert!(
            matches!(error, PcurveGeometryError::Nurbs(NurbsError::UnusableWeight {
            ref field, index: 0, weight,
        }) if field == "pcurve poles" && weight == 0.0)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    with_limits(0, u64::MAX, |ctx| {
        let error = pcurve_geometry(ctx, &curve).expect_err("first visited weight needs one unit");
        let PcurveGeometryError::Resource(CodecError::ResourceLimit(limit)) = error else {
            panic!("original work refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "FreeCAD pcurve rational pole scan");
        assert_eq!((limit.used, limit.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

fn triangulated_face() -> TextTShape {
    let shapes: TextTShapes = serde_json::from_value(serde_json::json!([{
        "index": 1, "kind": "face", "flags": [false, false, false, false, false, false, false], "children": [],
        "geometry": {"kind": "face", "natural_restriction": false,
            "tolerance": 0.0, "surface": 0, "location": 0, "triangulation": 1}
    }])).expect("checked native face");
    shapes[0].clone()
}

fn append_triangulated_face(
    ctx: &DecodeContext<'_>,
    triangulation: &TextTriangulation,
    transform: Transform,
) -> Result<(), CodecError> {
    let payload = payload();
    let tshapes = TextTShapes::from(vec![triangulated_face()]);
    let mut tables = tables(&tshapes);
    tables.triangulations = std::slice::from_ref(triangulation);
    let mut ir = CadIr::empty();
    let mut builder = builder(ctx, &payload, tables)?;
    builder
        .append_face(
            &mut ir,
            &ShellId::mint("fcstd:model:shell#Repair:1").expect("shell identity"),
            &TextShapeUse {
                shape: 1,
                orientation: TextOrientation::Forward,
                location: 0.into(),
            },
            transform,
            false,
        )
        .map(|_| ())
}

fn first_triangulation_failure_uses_one_visit(
    triangulation: &TextTriangulation,
    transform: Transform,
    operation: &str,
    message: &str,
) {
    // The probe/replay derives the work already required by face setup. Its
    // next_charged boundary needs exactly one unit. Adding that unit admits
    // the failing first element, so no allowance exists for a suffix scan.
    let error =
        crate::test_support::refusal_at(ResourceDimension::WorkUnits, &[], operation, |ctx| {
            append_triangulated_face(ctx, triangulation, transform)
        });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.additional, 1);
    let first_visit_need = limit.used + limit.additional;
    with_limits(first_visit_need, u64::MAX, |ctx| {
        let error = append_triangulated_face(ctx, triangulation, transform)
            .expect_err("first element is non-finite after placement");
        assert!(matches!(error, CodecError::Malformed(ref actual) if actual == message));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn placed_triangulation_first_overflow_preserves_malformed_before_long_tail() {
    let mut nodes = vec![Point3::new(0.0, 0.0, 0.0); 128];
    nodes[0] = Point3::new(1.0e308, 0.0, 0.0);
    let triangulation: TextTriangulation = serde_json::from_value(serde_json::json!({
        "deflection": 0.0, "nodes": nodes, "uv_nodes": null, "triangles": [[1, 2, 3]], "normals": null,
    })).expect("checked finite native nodes");
    let transform = Transform::affine([
        [1.0, 0.0, 0.0, 1.0e308],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite translation");
    first_triangulation_failure_uses_one_visit(
        &triangulation,
        transform,
        "FreeCAD placed triangulation node scan",
        "placed triangulation node for face 1 contains a non-finite coordinate",
    );
}

#[test]
fn placed_triangulation_first_zero_normal_preserves_malformed_before_long_tail() {
    let nodes = vec![Point3::new(0.0, 0.0, 0.0); 128];
    let mut normals = vec![Vector3::new(0.0, 0.0, 1.0); 128];
    normals[0] = Vector3::new(0.0, 0.0, 0.0);
    let triangulation: TextTriangulation = serde_json::from_value(serde_json::json!({
        "deflection": 0.0, "nodes": nodes, "uv_nodes": null, "triangles": [[1, 2, 3]], "normals": normals,
    })).expect("finite aligned native normals");
    first_triangulation_failure_uses_one_visit(
        &triangulation,
        Transform::identity(),
        "FreeCAD placed triangulation normal scan",
        "placed triangulation normal for face 1 contains a non-finite component",
    );
}

#[test]
fn connected_components_fanout_preserves_members_and_releases_scratch() {
    const FANOUT: usize = 127;
    const MATERIALIZED_CAP: u64 = 64 * 1024;
    let mut connectivity = vec![BTreeSet::from(["a".to_owned(), "b".to_owned()])];
    connectivity.extend((0..FANOUT).map(|_| BTreeSet::from(["a".to_owned()])));
    connectivity.extend((0..FANOUT).map(|_| BTreeSet::from(["b".to_owned()])));
    with_limits(u64::MAX, MATERIALIZED_CAP, |ctx| {
        assert_eq!(
            connected_components(ctx, &connectivity).expect("connected fanout"),
            vec![(0..connectivity.len()).collect::<Vec<_>>()]
        );
        let released = ctx.reserve_scoped(MATERIALIZED_CAP, "test released connectivity scratch")
            .expect("all traversal scratch is released");
        drop(released);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

fn pcurve_shapes(representations: Vec<TextEdgeRepresentation>) -> TextTShapes {
    let faces: TextTShapes = serde_json::from_value(serde_json::json!([{
        "index": 1, "kind": "face", "flags": [false, false, false, false, false, false, false], "children": [],
        "geometry": {"kind": "face", "natural_restriction": false,
            "tolerance": 0.0, "surface": 1, "location": 0, "triangulation": null}
    }])).expect("checked surface reference");
    TextTShapes::from(vec![
        TextTShape {
            geometry: TextTShapeGeometry::Edge {
                tolerance: FiniteReal::ZERO,
                same_parameter: true,
                same_range: true,
                degenerated: false,
                representations,
            },
            flags: [false; 7],
            children: Vec::new(),
        },
        faces[0].clone(),
    ])
}

fn support_surface() -> TextSurface {
    TextSurface::Plane {
        origin: FinitePoint3::ZERO,
        axis: FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("finite axis"),
        u_axis: FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).expect("finite axis"),
        v_reversed: false,
    }
}

fn pcurve_representation(curve: usize) -> TextEdgeRepresentation {
    TextEdgeRepresentation::Pcurve {
        curve,
        surface: 1,
        location: 0,
        parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
        uv_endpoints: None,
    }
}

fn face_pcurve(
    builder: &mut Builder<'_, '_, '_, '_>,
    reversed: bool,
) -> Result<Option<super::FacePcurve>, CodecError> {
    let TextTShapeGeometry::Face { surface, .. } = builder.tables.tshapes[1].geometry else {
        panic!("fixture face");
    };
    builder.face_pcurve(
        &TextShapeUse {
            shape: 1,
            orientation: if reversed {
                TextOrientation::Reversed
            } else {
                TextOrientation::Forward
            },
            location: 0.into(),
        },
        Transform::identity(),
        surface,
        Transform::identity(),
    )
}

#[test]
fn repeated_face_pcurves_reuse_checked_native_domains_without_lane_reconstruction() {
    for rational in [false, true] {
        with_limits(u64::MAX, u64::MAX, |ctx| {
            let payload = payload();
            let tshapes = pcurve_shapes(vec![pcurve_representation(1)]);
            let curves = [native_nurbs(128, rational, FiniteReal::ONE)];
            let surfaces = [support_surface()];
            let mut tables = tables(&tshapes);
            tables.curve2ds = &curves;
            tables.surfaces = &surfaces;
            let mut builder = builder(ctx, &payload, tables).expect("builder");
            builder.emit_pcurves().expect("initial checked geometry");
            assert_eq!(builder.pcurves.len(), 1);
            for operation in [
                if rational {
                    "FreeCAD pcurve rational pole scan"
                } else {
                    "FreeCAD pcurve polynomial poles"
                },
                "FreeCAD pcurve knot scan",
            ] {
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
                let first = face_pcurve(&mut builder, false)
                    .expect("first use")
                    .expect("pcurve");
                let second = face_pcurve(&mut builder, false)
                    .expect("second use")
                    .expect("pcurve");
                assert_eq!(first, second);
                assert_eq!(first.1, Some([FiniteReal::ZERO, FiniteReal::ONE]));
                assert_eq!(ctx.resource_refusal(), None);
            }
            assert!(builder.losses.is_empty());
        });
    }
}

#[test]
fn repeated_unavailable_face_pcurves_preserve_emission_and_use_loss_order() {
    with_limits(u64::MAX, u64::MAX, |ctx| {
        let payload = payload();
        let tshapes = pcurve_shapes(vec![pcurve_representation(1), pcurve_representation(2)]);
        let curves = [
            native_nurbs(128, true, FiniteReal::ZERO),
            TextCurve2d::Line {
                origin: FinitePoint2::ZERO,
                direction: FinitePoint2::ZERO,
            },
        ];
        let surfaces = [support_surface()];
        let mut tables = tables(&tshapes);
        tables.curve2ds = &curves;
        tables.surfaces = &surfaces;
        let mut builder = builder(ctx, &payload, tables).expect("builder");
        builder.emit_pcurves().expect("native failures are losses");
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "FreeCAD pcurve rational pole scan",
            None,
        );
        assert!(face_pcurve(&mut builder, false)
            .expect("first use")
            .is_none());
        assert!(face_pcurve(&mut builder, false)
            .expect("second use")
            .is_none());
        let invalid = "payload fcstd:native:entry#Repair curve2ds index 1 could not enter neutral geometry: pcurve poles: weight 0 at index 0 is not a usable weight";
        let unsupported =
            "payload fcstd:native:entry#Repair curve2ds index 2 could not enter neutral geometry";
        assert_eq!(
            builder
                .losses
                .iter()
                .map(|loss| loss.message.as_str())
                .collect::<Vec<_>>(),
            vec![invalid, unsupported, invalid, invalid]
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn skipped_pair_secondary_is_checked_once_on_its_first_reversed_face_use() {
    for first_weight in [FiniteReal::ZERO, FiniteReal::ONE] {
        with_limits(u64::MAX, u64::MAX, |ctx| {
            let payload = payload();
            let tshapes = pcurve_shapes(vec![TextEdgeRepresentation::PcurvePair {
                curves: [1, 2],
                continuity: "C0".into(),
                surface: 1,
                location: 0,
                parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
                uv_endpoints: None,
            }]);
            let curves = [
                TextCurve2d::Line {
                    origin: FinitePoint2::ZERO,
                    direction: FinitePoint2::ZERO,
                },
                native_nurbs(128, true, first_weight),
            ];
            let surfaces = [support_surface()];
            let mut tables = tables(&tshapes);
            tables.curve2ds = &curves;
            tables.surfaces = &surfaces;
            let mut builder = builder(ctx, &payload, tables).expect("builder");
            builder
                .emit_pcurves()
                .expect("failed primary skips the secondary");
            assert!(builder.pcurves.is_empty());
            assert_eq!(builder.losses.len(), 1);
            assert!(!builder.pcurve_availability.contains_key(&2));
            let first = face_pcurve(&mut builder, true).expect("first reversed use");
            assert!(builder.pcurve_availability.contains_key(&2));
            let _probe = RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "FreeCAD pcurve rational pole scan",
                None,
            );
            assert_eq!(
                face_pcurve(&mut builder, true).expect("repeated reversed use"),
                first
            );
            // Domain lookup does not create the candidate that primary failure skipped.
            assert!(builder.pcurves.is_empty());
            if first_weight == FiniteReal::ZERO {
                assert!(first.is_none());
                assert_eq!(builder.losses.iter().map(|loss| loss.message.as_str()).collect::<Vec<_>>(), vec![
                    "payload fcstd:native:entry#Repair curve2ds index 1 could not enter neutral geometry",
                    "payload fcstd:native:entry#Repair curve2ds index 2 could not enter neutral geometry: pcurve poles: weight 0 at index 0 is not a usable weight",
                    "payload fcstd:native:entry#Repair curve2ds index 2 could not enter neutral geometry: pcurve poles: weight 0 at index 0 is not a usable weight",
                ]);
            } else {
                let (id, range) =
                    first.expect("valid secondary keeps its existing reference behavior");
                assert!(id.as_str().ends_with("1%3A1%3A2"));
                assert_eq!(range, Some([FiniteReal::ZERO, FiniteReal::ONE]));
                assert_eq!(builder.losses.len(), 1);
            }
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}
