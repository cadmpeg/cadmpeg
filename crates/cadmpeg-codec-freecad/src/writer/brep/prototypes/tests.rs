// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::collections::HashMap;
use std::io::Cursor;

fn decode(brep: &[u8]) -> CadIr {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Shape'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", brep),
    ]);
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

fn source() -> CadIr {
    decode(b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 1\n1\n1 0 0 5\n0 1 0 0\n0 0 1 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 4\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +4 0 -3 0 *\nCo 1001000 +2 0 +2 1 *\n+1 0\n")
}

fn write(ir: &CadIr) -> CadIr {
    let bodies = ir.model.bodies.iter().collect::<Vec<_>>();
    let serialized = super::super::serialize(ir, &bodies).unwrap().unwrap();
    decode(&serialized.bytes)
}

fn positions(ir: &CadIr) -> Vec<[f64; 3]> {
    let mut positions = ir
        .model
        .points
        .iter()
        .map(|point| {
            let p = point.position().get();
            [p.x, p.y, p.z]
        })
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| left.partial_cmp(right).unwrap());
    positions
}

#[test]
fn placed_uses_share_a_current_prototype_without_baking_their_location() {
    let ir = source();
    assert_eq!(ir.model.edges.len(), 2);
    assert_eq!(
        positions(&ir),
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [6.0, 0.0, 0.0]
        ]
    );
    let output = write(&ir);
    assert_eq!(positions(&output), positions(&ir));
    assert_eq!(output.model.edges.len(), 2);
    assert_eq!(
        output.model.edges[0].native_ref,
        output.model.edges[1].native_ref
    );
    assert!(output.model.edges[0].native_ref.is_some());
}

#[test]
fn a_divergent_current_instance_splits_its_prototype_and_retains_the_edit() {
    use cadmpeg_ir::geometry::{
        analytic::LineCurve, CurveGeometry, PlacedCurve, SolvedCurveGeometry,
    };
    let mut ir = source();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| {
            matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Transformed(_))
            )
        })
        .unwrap();
    let Some(SolvedCurveGeometry::Transformed(placed)) = curve.geometry.solved() else {
        unreachable!()
    };
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            *placed.transform(),
        )
        .unwrap(),
    ));
    let output = write(&ir);
    assert_eq!(positions(&output), positions(&ir));
    assert_eq!(output.model.edges.len(), 2);
    let payloads =
        crate::brep::records::payloads_untracked(output.native.namespace("fcstd").unwrap())
            .unwrap();
    assert_eq!(payloads.len(), 1);
    let tables = crate::brep::Tables::from_payload(&payloads[0]).unwrap();
    let edges = tables
        .tshapes
        .iter()
        .filter_map(|shape| match &shape.geometry {
            crate::brep::TextTShapeGeometry::Edge {
                representations, ..
            } => Some(representations),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    let curves = edges
        .iter()
        .map(|representations| {
            let actual = representations
                .iter()
                .filter_map(|representation| match representation {
                    crate::brep::TextEdgeRepresentation::Curve3d {
                        curve, location, ..
                    } => Some((*curve, *location)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(actual.len(), 1);
            actual[0]
        })
        .collect::<Vec<_>>();
    assert_ne!(curves[0].0, curves[1].0);
    assert!(curves.iter().any(|(_, location)| {
        crate::brep::LocationRef::from(*location)
            .resolve(tables.locations)
            .unwrap()
            .rows()
            == [
                [1.0, 0.0, 0.0, 5.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
    }));
    assert!(output
        .model
        .curves
        .iter()
        .any(
            |curve| super::super::super::geometry::curve_record(&output, curve)
                .is_some_and(|row| row.starts_with("1 0 2 0 "))
        ));
}

#[test]
fn neutral_only_topology_does_not_require_a_native_prototype_candidate() {
    let mut ir = source();
    ir.native = Default::default();
    for point in &mut ir.model.points {
        point.native_ref = None;
        point.source_object = None;
    }
    for vertex in &mut ir.model.vertices {
        vertex.native_ref = None;
    }
    for edge in &mut ir.model.edges {
        edge.native_ref = None;
    }
    for curve in &mut ir.model.curves {
        curve.native_ref = None;
        curve.source_object = None;
    }
    for region in &mut ir.model.regions {
        region.native_ref = None;
    }
    for shell in &mut ir.model.shells {
        shell.native_ref = None;
    }
    for body in &mut ir.model.bodies {
        body.native_ref = None;
    }
    let output = write(&ir);
    assert_eq!(positions(&output), positions(&ir));
    assert_eq!(output.model.edges.len(), 2);
}

#[test]
fn placed_prototypes_use_forward_native_homes_after_arbitrary_neutral_id_rewrites() {
    let mut ir = source();
    let mut ids = HashMap::new();
    macro_rules! collect {
        ($($arena:ident),*) => {$(for value in &ir.model.$arena {
            ids.insert(value.id.as_str().to_owned(), format!("authored:model:topology#{}", ids.len()));
        })*};
    }
    collect!(points, vertices, curves, edges, shells, regions, bodies);
    let ctx = cadmpeg_test_support::service_decode_context();
    macro_rules! rewrite {
        ($($arena:ident),*) => {$(ir.model.$arena = cadmpeg_ir::schema::rewrite::identities(
            &ctx, ir.model.$arena, |id| Ok(ids.get(id).cloned().unwrap_or_else(|| id.to_owned()))
        ).unwrap();)*};
    }
    rewrite!(points, vertices, curves, edges, shells, regions, bodies);
    let output = write(&ir);
    assert_eq!(positions(&output), positions(&ir));
    assert_eq!(output.model.edges.len(), 2);
    assert_eq!(
        output.model.edges[0].native_ref,
        output.model.edges[1].native_ref
    );
}
