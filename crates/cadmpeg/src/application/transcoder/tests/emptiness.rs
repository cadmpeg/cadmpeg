// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::subd::{
    SubdCage, SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
use cadmpeg_ir::{topology::Point, CadIr};
use cadmpeg_registry::Format;
use std::io::Cursor;

use super::{export_target, prepared, LossPolicy, TargetSelection};

fn triangle() -> [Point3; 3] {
    [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ]
}

#[test]
fn standalone_geometry_is_not_an_empty_result_without_brep() {
    let mut point = CadIr::empty();
    point.model.points.push(Point::new(
        "test:geometry:point#0".try_into().unwrap(),
        FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
        None,
    ));
    let mut curve = CadIr::empty();
    curve.model.curves.push(cadmpeg_ir::geometry::Curve::new(
        "test:geometry:curve#0".try_into().unwrap(),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                FinitePoint3::ZERO,
                cadmpeg_ir::units::UnitVector3::X_AXIS,
            ),
        )),
        Some(cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, 1.0]).unwrap()),
        None,
    ));
    let mut mesh = CadIr::empty();
    mesh.model.tessellations.push(
        Tessellation::new(
            TessellationId::mint("test:geometry:tessellation#0").unwrap(),
            TessellationMesh::List {
                vertices: triangle().to_vec(),
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap(),
    );
    let mut subd = CadIr::empty();
    subd.model.subds.push({
        let (id, scheme, source_object, cage) = (
            "test:geometry:subd#0".try_into().unwrap(),
            SubdScheme::CatmullClark,
            None,
            SubdCage::new(
                triangle()
                    .into_iter()
                    .map(|point| SubdVertex::new(point, SubdVertexTag::Smooth, None).unwrap())
                    .collect(),
                [[0, 1], [1, 2], [2, 0]]
                    .into_iter()
                    .map(|vertices| {
                        SubdEdge::new(vertices, [0.0, 0.0], SubdEdgeTag::Smooth, None, [0.0, 0.0])
                            .unwrap()
                    })
                    .collect(),
                vec![SubdFace::new(
                    (0..3)
                        .map(|edge| SubdEdgeUse {
                            edge,
                            reversed: false,
                        })
                        .collect(),
                )
                .unwrap()],
                Vec::new(),
            )
            .unwrap(),
        );
        SubdSurface::new(id, scheme, cage, source_object)
    });
    for (arena, ir) in [
        ("points", point),
        ("curves", curve),
        ("tessellations", mesh),
        ("subds", subd),
    ] {
        assert!(ir.model.bodies.is_empty(), "{arena} input contains no body");
        let mut conversion = prepared(
            ir,
            Format::Rhino,
            export_target(TargetSelection::new(Format::Rhino, None)).encoder,
            LossPolicy::Allow,
        );
        conversion.reject_errors = false;
        conversion.reject_empty = true;
        let planned = conversion.plan().expect("standalone geometry is writable");
        assert!(
            !planned.plan.report().losses.iter().any(|loss| {
                loss.code.namespace() == "export" && loss.code.local_code() == "output.empty"
            }),
            "{arena}"
        );
        let mut output = Vec::new();
        planned.plan.write_to(&mut output).unwrap();
        let decoded = cadmpeg_codec_rhino::RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap();
        assert!(
            decoded.ir().model.faces.is_empty(),
            "the control contains no B-rep faces"
        );
        let model = &decoded.ir().model;
        let count = match arena {
            "points" => model.points.len(),
            "curves" => model.curves.len(),
            "tessellations" => model.tessellations.len(),
            "subds" => model.subds.len(),
            _ => unreachable!(),
        };
        assert_eq!(count, 1, "{arena}");
    }
}
