// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::examples::{add_document_root, unit_cube};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::{isolate, synthesize_dropping, Uses};
use crate::IgesVersion;

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
fn halving_isolates_the_one_candidate_the_failure_needs() {
    let candidates = ["a", "b", "c", "d", "e", "f", "g"];
    let found = isolate(&candidates, |set: &[&str]| -> Result<bool, CodecError> {
        Ok(set.contains(&"f"))
    })
    .expect("isolation runs");
    assert_eq!(found, Some(vec!["f"]));
}

#[test]
fn candidates_whose_removal_changes_nothing_isolate_nothing() {
    let found = isolate(&["a", "b"], |_: &[&str]| -> Result<bool, CodecError> {
        Ok(false)
    })
    .expect("isolation runs");
    assert_eq!(found, None);
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
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let body = ir.geometry.bodies[0].id.to_string();
    let synthesis = synthesize_dropping(&ir, IgesVersion::V5_3).expect("the rest is written");
    let omitted = charged(&synthesis.losses, "iges/writer.body-omitted");
    assert_eq!(omitted.len(), 1, "{omitted:?}");
    assert!(omitted[0].contains(&body));
    assert_eq!(charged(&synthesis.losses, "export/output.empty").len(), 1);
    let text = String::from_utf8(synthesis.bytes).expect("IGES Fixed ASCII is UTF-8");
    assert!(text
        .lines()
        .any(|card| card.as_bytes().get(72) == Some(&b'T')));
    assert!(!text
        .lines()
        .any(|card| card.as_bytes().get(72) == Some(&b'D')));
}
