// SPDX-License-Identifier: Apache-2.0
//! Body partition ownership.
#![allow(clippy::unwrap_used)]

#[test]
fn empty_body_survives_beside_a_body_with_faces() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut body = ir.model.bodies[0].clone();
    let mut region = ir.model.regions[0].clone();
    body.id = cadmpeg_ir::ids::BodyId::mint("synthetic:test:body#empty").unwrap();
    region.id = cadmpeg_ir::ids::RegionId::mint("synthetic:test:region#empty").unwrap();
    region.body = body.id.clone();
    region.shells.clear();
    body.regions = vec![region.id.clone()];
    ir.model.bodies.push(body);
    ir.model.regions.push(region);
    ir.model.finalize();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = crate::SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert_eq!(decoded.ir().model.regions.len(), 2);
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
}

#[test]
fn configuration_subset_keeps_only_owned_and_detached_curves() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let source = serde_json::to_string(&ir)
        .unwrap()
        .replace("synthetic:cube:", "synthetic:other:");
    let mut other: cadmpeg_ir::CadIr = serde_json::from_str(&source).unwrap();
    let first_body = ir.model.bodies[0].id.clone();
    let other_curves = other
        .model
        .curves
        .iter()
        .map(|curve| curve.id.clone())
        .collect::<Vec<_>>();
    let mut detached = ir.model.curves[0].clone();
    detached.id = cadmpeg_ir::ids::CurveId::mint("synthetic:test:curve#detached").unwrap();
    let detached_id = detached.id.clone();
    macro_rules! append { ($($arena:ident),* $(,)?) => { $(ir.model.$arena.append(&mut other.model.$arena);)* }; }
    append!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves
    );
    ir.model.curves.push(detached);
    let subset = super::super::body_subset(&ir, &[first_body]).unwrap();
    assert!(subset
        .model
        .curves
        .iter()
        .any(|curve| curve.id == detached_id));
    assert!(subset
        .model
        .curves
        .iter()
        .all(|curve| !other_curves.contains(&curve.id)));
}

#[test]
fn configuration_subset_keeps_detached_supported_surfaces() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.clone();
    let mut surface = ir.model.surfaces[0].clone();
    surface.id = cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:surface#detached").unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 100.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    ir.model.surfaces.push(surface.clone());
    let subset = super::super::body_subset(&ir, &[body]).unwrap();
    assert!(subset
        .model
        .surfaces
        .iter()
        .any(|candidate| candidate.id == surface.id));
    let body = super::super::brep_body(&subset, 0.001, false).unwrap();
    assert!((0..body.len()).any(|offset| matches!(crate::brep::parse_carrier(&body, offset), Some(crate::brep::Carrier::Surface(carrier)) if carrier.geometry == surface.geometry)));
}

#[test]
fn detached_carriers_survive_when_a_wire_body_precedes_the_solid() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let solid = ir.model.bodies[0].id.clone();
    let mut detached = ir.model.curves[0].clone();
    detached.id = cadmpeg_ir::ids::CurveId::mint("synthetic:test:curve#unowned").unwrap();
    let detached_id = detached.id.clone();
    ir.model.curves.push(detached);
    let mut wire = ir.model.bodies[0].clone();
    wire.id = cadmpeg_ir::ids::BodyId::mint("synthetic:test:body#wire").unwrap();
    wire.kind = cadmpeg_ir::topology::BodyKind::Wire;
    wire.regions.clear();
    ir.model.bodies.insert(0, wire);
    let subset = super::super::body_subset(&ir, &[solid]).unwrap();
    assert!(subset
        .model
        .curves
        .iter()
        .any(|curve| curve.id == detached_id));
}

#[test]
fn detached_plane_is_authored_in_the_partition() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut surface = ir.model.surfaces[0].clone();
    surface.id = cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:surface#detached").unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 100.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    ir.model.surfaces.push(surface.clone());
    ir.model.finalize();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let partition = crate::container::select_active_parasolid_site(&scan)
        .unwrap()
        .section
        .payload();
    assert!((0..partition.len()).any(|offset| matches!(crate::brep::parse_carrier(partition, offset), Some(crate::brep::Carrier::Surface(carrier)) if carrier.geometry == surface.geometry)));
}

#[test]
fn detached_carriers_are_authored_without_a_body() {
    const EPS_STANDALONE_POINT_MM: f64 = 1e-9;

    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let surface = ir.model.surfaces[0].clone();
    let curve = ir.model.curves[0].clone();
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();
    ir.model.faces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();
    ir.model.points.clear();
    let point = Point::new(
        cadmpeg_ir::ids::PointId::mint("synthetic:test:point#standalone").unwrap(),
        FinitePoint3::new(Point3::new(1000.0, 2000.0, 3000.0)).unwrap(),
        None,
    );
    ir.model.points.push(point.clone());
    ir.model.surfaces = vec![surface.clone()];
    ir.model.curves = vec![curve.clone()];
    ir.model.finalize();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let partition = crate::container::select_active_parasolid_site(&scan)
        .unwrap()
        .section
        .payload();
    assert!((0..partition.len()).any(|offset| matches!(crate::brep::parse_carrier(partition, offset), Some(crate::brep::Carrier::Surface(carrier)) if carrier.geometry == surface.geometry)));
    assert!((0..partition.len()).any(|offset| matches!(crate::brep::parse_carrier(partition, offset), Some(crate::brep::Carrier::Curve(carrier)) if carrier.geometry == curve.geometry)));
    let decoded = crate::SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "config.inferred-without-native"));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|item| item.geometry == surface.geometry));
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|item| item.geometry == curve.geometry));
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .any(|item| {
            item.position().get().distance(point.position().get()) < EPS_STANDALONE_POINT_MM
        }));
}

#[test]
fn model_only_geometry_has_a_native_default_configuration() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut scope = ir.scope;
    scope.design = false;
    ir.set_scope(scope).unwrap();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = crate::SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "config.inferred-without-native"));
}

#[test]
fn detached_point_is_written_in_the_selected_partition() {
    use cadmpeg_core::decode::View;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.clone();
    let point = Point::new(
        cadmpeg_ir::ids::PointId::mint("synthetic:test:point#detached").unwrap(),
        FinitePoint3::new(Point3::new(1000.0, 2000.0, 3000.0)).unwrap(),
        None,
    );
    ir.model.points.push(point.clone());
    let subset = super::super::body_subset(&ir, &[body]).unwrap();
    assert!(subset
        .model
        .points
        .iter()
        .any(|candidate| candidate.id == point.id));
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let partition = crate::container::select_active_parasolid_site(&scan)
        .unwrap()
        .section
        .payload();
    let points = crate::brep::topology::scan_plain(partition);
    assert!(points.points().values().any(|candidate| {
        [1.0, 2.0, 3.0].iter().enumerate().all(|(index, value)| {
            View::f64_be_at(partition, candidate.xyz_offset + index * 8) == Some(*value)
        })
    }));
}

#[test]
fn omitted_wire_free_point_survives_with_solid() {
    const EPS_WIRE_POINT_MM: f64 = 1e-9;
    use crate::SldprtCodec;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::FinitePoint3;
    use std::io::Cursor;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let body = cadmpeg_ir::ids::BodyId::mint("synthetic:test:body#wire").unwrap();
    let region = cadmpeg_ir::ids::RegionId::mint("synthetic:test:region#wire").unwrap();
    let shell = cadmpeg_ir::ids::ShellId::mint("synthetic:test:shell#wire").unwrap();
    let vertex = cadmpeg_ir::ids::VertexId::mint("synthetic:test:vertex#wire").unwrap();
    let point = cadmpeg_ir::ids::PointId::mint("synthetic:test:point#wire").unwrap();
    let position = Point3::new(120.0, 140.0, 160.0);
    ir.model.points.push(Point::new(
        point.clone(),
        FinitePoint3::new(position).unwrap(),
        None,
    ));
    ir.model.vertices.push(Vertex {
        native_ref: None,
        id: vertex.clone(),
        point,
        tolerance: None,
    });
    ir.model
        .shells
        .push(Shell::with_free_vertices(shell.clone(), region.clone(), vec![vertex]).unwrap());
    ir.model.regions.push(Region {
        native_ref: None,
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell],
    });
    ir.model.bodies.push(Body {
        native_ref: None,
        id: body,
        kind: BodyKind::Wire,
        regions: vec![region],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.points.iter().any(|point| {
        let actual = point.position().get();
        (actual.x - position.x).abs() < EPS_WIRE_POINT_MM
            && (actual.y - position.y).abs() < EPS_WIRE_POINT_MM
            && (actual.z - position.z).abs() < EPS_WIRE_POINT_MM
    }));
}

