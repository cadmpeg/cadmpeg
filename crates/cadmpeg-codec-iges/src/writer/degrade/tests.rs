// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::{add_document_root, unit_cube};
use cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurve};
use cadmpeg_ir::ids::ProceduralCurveId;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::{empty_file, synthesize_dropping, Uses};
use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use crate::{IgesCodec, IgesVersion};
use cadmpeg_core::CodecError;

fn cube() -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    add_document_root(&mut ir);
    ir
}

fn charged<'a>(losses: &'a [cadmpeg_ir::report::loss::LossNote], code: &str) -> Vec<&'a str> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn an_edge_is_used_by_the_two_faces_that_bound_it() {
    let ir = cube();
    let edge = ir.geometry.edges[0].id.as_str();
    assert_eq!(Uses::new(&ir).faces[edge].len(), 2);
}

#[test]
fn the_faces_on_an_unwritable_edge_are_dropped_and_the_rest_is_written() {
    let mut ir = cube();
    ir.geometry.edges[0]
        .set_curve(None)
        .expect("an edge may state no curve");
    let edge = ir.geometry.edges[0].id.as_str().to_owned();
    let faces = Uses::new(&ir).faces[edge.as_str()]
        .iter()
        .map(|face| (*face).to_owned())
        .collect::<Vec<_>>();
    let synthesis = synthesize_dropping(&ir, IgesVersion::V5_3).expect("the rest is written");
    let omitted = charged(&synthesis.losses, "iges/writer.face-omitted");
    assert_eq!(omitted.len(), faces.len(), "{omitted:?}");
    for face in &faces {
        assert!(
            omitted
                .iter()
                .any(|message| message.contains(face.as_str())),
            "{omitted:?}"
        );
    }
    assert!(charged(&synthesis.losses, "export/output.empty").is_empty());
    assert_eq!(synthesis.counts.get("510_face"), Some(&4));
}

#[test]
fn a_body_the_writer_cannot_place_is_dropped_and_the_empty_file_is_written() {
    let mut ir = cube();
    ir.geometry.bodies[0].transform = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ]);
    let body = ir.geometry.bodies[0].id.to_string();
    let synthesis = synthesize_dropping(&ir, IgesVersion::V5_3).expect("the rest is written");
    let omitted = charged(&synthesis.losses, "iges/writer.body-omitted");
    assert_eq!(omitted.len(), 1, "{omitted:?}");
    assert!(omitted[0].contains(&body));
    assert!(omitted[0].contains("rigid body transform"), "{omitted:?}");
    assert_eq!(charged(&synthesis.losses, "export/output.empty").len(), 1);
    let text = String::from_utf8(synthesis.bytes).expect("IGES Fixed ASCII is UTF-8");
    assert!(text
        .lines()
        .any(|card| card.as_bytes().get(72) == Some(&b'T')));
    assert!(!text
        .lines()
        .any(|card| card.as_bytes().get(72) == Some(&b'D')));
}

#[test]
fn the_empty_file_charges_each_record_that_no_body_uses() {
    let mut ir = cube();
    let mut free = ir.geometry.points[0].clone();
    free.id = "test:model:point#free".try_into().expect("valid identity");
    ir.geometry.points.push(free);
    let error = CodecError::NotImplemented("no culprit".into());
    let synthesis =
        empty_file(&ir, IgesVersion::V5_3, &error, Vec::new()).expect("the empty file is written");
    let free = charged(&synthesis.losses, "iges/writer.free-geometry-omitted");
    assert_eq!(
        free,
        ["free record test:model:point#free was not written: not implemented yet: no culprit"]
    );
    assert_eq!(
        charged(&synthesis.losses, "iges/writer.body-omitted").len(),
        1
    );
}

/// The cube with the curve of its first edge stated as a subset of itself,
/// with no solved cache, and that curve's id.
fn cube_on_a_cyclic_subset() -> (CadIr, String) {
    let mut ir = cube();
    let curve = ir.geometry.edges[0]
        .curve()
        .expect("the cube's edges have curves")
        .clone();
    let construction =
        ProceduralCurveId::mint("test:model:construction#cyclic").expect("valid identity");
    ir.geometry
        .curves
        .iter_mut()
        .find(|carrier| carrier.id == curve)
        .expect("the edge's curve")
        .geometry = CurveGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    let subset = SubsetCurveConstruction::try_new(curve.clone(), [0.0, 1.0], true, None)
        .expect("a subset of the curve");
    ir.geometry
        .add_procedural_curve(
            &curve,
            ProceduralCurve::new(construction, ProceduralCurveDefinition::Subset(subset)),
        )
        .expect("the construction defines the curve");
    (ir, curve.to_string())
}

/// Leaves every identity as it is.
struct Unchanged;

impl cadmpeg_ir::document::EntityRewrite for Unchanged {
    type Error = std::convert::Infallible;
    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, Self::Error> {
        Ok(entity)
    }
}

#[test]
fn a_subset_curve_that_cannot_be_restated_drops_only_the_faces_on_it() {
    let (cube, curve) = cube_on_a_cyclic_subset();
    let edge = cube.geometry.edges[0].id.as_str().to_owned();
    let faces = Uses::new(&cube).faces[edge.as_str()]
        .iter()
        .map(|face| (*face).to_owned())
        .collect::<Vec<_>>();
    let mut ir = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("the tetrahedron decodes")
        .ir()
        .clone();
    ir.native = cadmpeg_ir::native::Native::default();
    let tetrahedron_faces = ir.geometry.faces.len();
    ir.geometry
        .extend_rewritten(cube.geometry, &mut Unchanged)
        .expect("the cube joins the document");
    ir.finalize();

    let synthesis = synthesize_dropping(&ir, IgesVersion::V5_3).expect("the rest is written");
    let omitted = charged(&synthesis.losses, "iges/writer.face-omitted");
    assert_eq!(omitted.len(), faces.len(), "{omitted:?}");
    for face in &faces {
        assert!(
            omitted
                .iter()
                .any(|message| message.contains(face.as_str())),
            "{omitted:?}"
        );
    }
    let free = charged(&synthesis.losses, "iges/writer.free-geometry-omitted");
    assert_eq!(free.len(), 1, "{free:?}");
    assert!(free[0].contains(&curve), "{free:?}");
    assert!(charged(&synthesis.losses, "iges/writer.body-omitted").is_empty());
    assert!(charged(&synthesis.losses, "export/output.empty").is_empty());

    let read = IgesCodec
        .decode(&mut Cursor::new(synthesis.bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    assert_eq!(
        read.ir().geometry.faces.len(),
        tetrahedron_faces + 6 - faces.len()
    );
}

#[test]
fn a_named_curve_no_face_or_body_uses_is_dropped_alone() {
    let mut ir = cube();
    let mut free = ir.geometry.curves[0].clone();
    free.id = "test:model:curve#free".try_into().expect("valid identity");
    ir.geometry.curves.push(free);
    let unwritten =
        super::naming("test:model:curve#free")(CodecError::NotImplemented("unwritable".into()));
    let (smaller, notes) =
        super::without_free_curves(&ir, &unwritten).expect("the named curve is dropped");
    assert_eq!(smaller.geometry.curves.len(), ir.geometry.curves.len() - 1);
    assert!(smaller
        .geometry
        .curves
        .iter()
        .all(|curve| curve.id.as_str() != "test:model:curve#free"));
    assert_eq!(
        notes
            .iter()
            .map(|note| note.message.as_str())
            .collect::<Vec<_>>(),
        ["free record test:model:curve#free was not written: not implemented yet: unwritable"]
    );
}
