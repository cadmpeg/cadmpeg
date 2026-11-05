// SPDX-License-Identifier: Apache-2.0
//! Surface content of the FTC-07 solid, compared with its AP242 model.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};

use crate::CatiaCodec;

#[test]
fn nist_ftc_07_transfers_every_solid_face_carrier() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    assert_eq!(model.faces.len(), 304);
    let mut nurbs = 0;
    for face in &model.faces {
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("face carrier exists");
        match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => nurbs += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
                panic!("unresolved face carrier")
            }
            SurfaceGeometry::Solved(_) => {}
            SurfaceGeometry::Procedural { .. } => panic!("unresolved face carrier construction"),
        }
    }
    assert_eq!(nurbs, 20);
}

#[test]
fn nist_ftc_07_circular_edge_agrees_with_both_face_curves() {
    const STEP_EDGE_LENGTH_MM: f64 = 2.367_294_783_918_816_3;
    const LENGTH_TOLERANCE_MM: f64 = 2e-3;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    let edge = model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "catia:standard:edge#259")
        .expect("edge");
    let curve = model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve())
        .expect("circle");
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &curve.geometry else {
        panic!("circular edge");
    };
    let [lower, upper] = edge.param_range().expect("trim interval").get();
    assert!(
        (circle.radius().get() * (upper - lower) - STEP_EDGE_LENGTH_MM).abs() < LENGTH_TOLERANCE_MM
    );
    let admission = cadmpeg_ir::eval::admission::EvaluationAdmission::Standard;
    let midpoint =
        cadmpeg_ir::eval::decode::curve_point(admission, &curve.geometry, lower.midpoint(upper))
            .expect("edge midpoint")
            .get();
    for (surface_id, pcurve_id) in [
        ("catia:standard:surf#10", "catia:standard:pcurve#10:0:0"),
        ("catia:standard:surf#139", "catia:standard:pcurve#139:0:5"),
    ] {
        let surface = model
            .surfaces
            .iter()
            .find(|item| item.id.as_str() == surface_id)
            .expect("face surface");
        let pcurve = model
            .pcurves
            .iter()
            .find(|item| item.id.as_str() == pcurve_id)
            .expect("face pcurve");
        let [first, last] = pcurve.parameter_range().expect("pcurve interval").get();
        let uv =
            cadmpeg_ir::eval::decode::pcurve_uv(admission, &pcurve.geometry, first.midpoint(last))
                .expect("pcurve midpoint");
        let lifted =
            cadmpeg_ir::eval::decode::surface_point(admission, &surface.geometry, uv.u, uv.v)
                .expect("lifted pcurve midpoint")
                .get();
        assert!(lifted.distance(midpoint) < LENGTH_TOLERANCE_MM);
    }
}
