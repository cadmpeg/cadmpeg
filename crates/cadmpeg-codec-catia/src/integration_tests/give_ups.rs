// SPDX-License-Identifier: Apache-2.0
//! Records a decode reads and leaves out of the document are given up by
//! the part or record they would have been: `record.unreadable` for a value
//! the carrier type forbids, `record.undecoded` for a record the decode does
//! not place.

use std::io::Cursor;

use cadmpeg_core::decode::ArenaClass;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{record_undecoded_kind, record_unreadable_kind, Part};

use super::decode;
use crate::test_support::test_container::{
    main_stream, standard_catpart_from_streams, surf_stream, zero_entity_nurbs_catpart,
};
use crate::test_support::test_e5::{append_e5_record, e5_catpart_appending};
use crate::test_support::test_zero_entity::{
    zero_entity_face_support_stream, zero_entity_support_stream,
};
use crate::CatiaCodec;
use cadmpeg_test_support::EditableDecodeResult;

const FBB_ROW: [u8; 8] = [0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2];
/// The offset of the first FBB row in [`main_stream`].
const FIRST_FBB_ROW: usize = 40;

fn unreadable(result: &EditableDecodeResult) -> Vec<&LossNote> {
    charged(result, &record_unreadable_kind())
}

fn undecoded(result: &EditableDecodeResult) -> Vec<&LossNote> {
    charged(result, &record_undecoded_kind())
}

fn charged<'r>(
    result: &'r EditableDecodeResult,
    code: &cadmpeg_ir::report::loss::LossKind,
) -> Vec<&'r LossNote> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| &loss.code == code)
        .collect()
}

fn names_part(loss: &LossNote, id: &UnknownId) -> bool {
    loss.message.contains(&format!("record={id}:"))
}

fn holds_stub(result: &EditableDecodeResult, id: &UnknownId) -> bool {
    stub_links(result, id).is_some()
}

/// The records the stub `id` links to, when the document holds the stub.
fn stub_links(result: &EditableDecodeResult, id: &UnknownId) -> Option<Vec<String>> {
    result
        .ir()
        .native_unknowns("catia")
        .expect("native unknowns")
        .into_iter()
        .find(|record| &record.id == id)
        .map(|record| {
            record
                .links
                .iter()
                .map(|link| link.as_str().to_owned())
                .collect()
        })
}

fn fbb_row_part(offset: usize) -> UnknownId {
    Part::at(
        "catia",
        "fbb-face-row",
        u64::try_from(offset).unwrap(),
        &FBB_ROW,
        [ArenaClass::Model],
    )
    .in_stream(cadmpeg_ir::stream_name!("catia:").with_suffix("MainDataStream+SurfacicReps"))
    .id()
    .unwrap()
}

/// The zero-entity record establishes its pole grid before it reads a pole,
/// so a pole that is not finite refuses the record: a permissive decode gives
/// up its frame and goes on, and a pinned decode refuses. The container has
/// no nested stream directory, so the kept payload record holds the whole
/// file image and the stub of the part links to it.
#[test]
fn a_non_finite_zero_entity_pole_gives_up_its_frame() {
    const RECORD: usize = 16;
    const FIRST_POLE: usize = RECORD + 167;
    let mut bytes = zero_entity_nurbs_catpart();
    bytes[FIRST_POLE..FIRST_POLE + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    let part = Part::at(
        "catia",
        "zero-entity-nurbs-surface",
        u64::try_from(RECORD).unwrap(),
        &bytes[RECORD..],
        [ArenaClass::Model],
    )
    .id()
    .unwrap();

    let permissive = decode(bytes.clone());
    let charges = unreadable(&permissive);
    assert_eq!(charges.len(), 1, "{charges:?}");
    assert!(names_part(charges[0], &part), "{}", charges[0].message);
    assert!(
        charges[0].message.ends_with("dropped=[]"),
        "{}",
        charges[0].message
    );
    let links = stub_links(&permissive, &part).expect("the stub of the part");
    let [payload] = links.as_slice() else {
        panic!("one containing record: {links:?}");
    };
    assert!(payload.starts_with("catia:payload:unknown#"), "{payload}");
    assert!(permissive
        .ir()
        .native_unknowns("catia")
        .expect("native unknowns")
        .iter()
        .any(|record| record.id.as_str() == payload));

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = CatiaCodec
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("a pinned decode refuses the given-up frame");
    let cadmpeg_ir::DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(rejection.loss().code, record_unreadable_kind());
    assert!(
        names_part(rejection.loss(), &part),
        "{}",
        rejection.loss().message
    );
}

/// The walk of support runs skips a refused zero-entity carrier, so the
/// support records after it form no run and the route mints no curve for
/// them. The part of the carrier lists the curve and construction each
/// support record would have held.
#[test]
fn a_refused_zero_entity_carrier_drops_the_curves_of_its_support_records() {
    let mut bytes = vec![0u8; 16];
    bytes[..8].copy_from_slice(crate::container::OUTER_MAGIC);
    let stream = zero_entity_support_stream();
    // The stream holds a plane carrier and its support record.
    let support = stream[0x6a + 12..].to_vec();
    bytes.extend(stream);
    let carrier = bytes.len();
    let mut nurbs = zero_entity_nurbs_catpart().split_off(16);
    nurbs[167..175].copy_from_slice(&f64::NAN.to_le_bytes());
    bytes.extend(nurbs);
    bytes.extend(support);
    let part = Part::at(
        "catia",
        "zero-entity-nurbs-surface",
        u64::try_from(carrier).unwrap(),
        &bytes[carrier..bytes.len() - (0x71 + 12)],
        [ArenaClass::Model],
    )
    .id()
    .unwrap();
    // The refused support record is the fourth framed record.
    let curve = crate::resource::compose_u32_id(
        &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "support-curve"),
        4,
        cadmpeg_ir::ids::CurveId::mint,
    )
    .unwrap();
    let construction = crate::resource::compose_u32_id(
        &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "support-curve-construction"),
        4,
        cadmpeg_ir::ids::ProceduralCurveId::mint,
    )
    .unwrap();

    let result = decode(bytes);
    let charges = unreadable(&result);
    assert_eq!(charges.len(), 1, "{charges:?}");
    assert!(names_part(charges[0], &part), "{}", charges[0].message);
    assert!(
        charges[0]
            .message
            .ends_with(&format!("dropped=[{curve}, {construction}]")),
        "{}",
        charges[0].message
    );
    let curves = &result.ir().geometry.curves;
    assert_eq!(curves.len(), 1, "the plane's support curve stays");
    assert_ne!(curves[0].id, curve);
}

/// A refused zero-entity carrier leaves the surface roster short of the face
/// roster, so no face binds a support run and the closed topology does not
/// transfer. The decode charges each face record by its record ordinal.
#[test]
fn a_refused_zero_entity_carrier_charges_the_faces_of_the_closed_topology() {
    let mut bytes = vec![0u8; 16];
    bytes[..8].copy_from_slice(crate::container::OUTER_MAGIC);
    let stream = zero_entity_face_support_stream();
    let support = stream[0x6a + 12..0x6a + 12 + 0x71 + 12].to_vec();
    bytes.extend(stream);
    let mut nurbs = zero_entity_nurbs_catpart().split_off(16);
    nurbs[167..175].copy_from_slice(&f64::NAN.to_le_bytes());
    bytes.extend(nurbs);
    bytes.extend(support);
    // The face record is the third framed record.
    let face = cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("catia", "zero-entity", "topology-face"),
        3,
    );

    let result = decode(bytes);
    let charges = undecoded(&result);
    assert!(
        charges.iter().any(|loss| loss
            .message
            .split_whitespace()
            .nth(1)
            .is_some_and(|id| id == face.as_str())),
        "{charges:?}"
    );
    assert!(result.ir().geometry.faces.is_empty());
}

/// A face record whose E5 reference graph does not parse builds no face; the
/// decode charges the face the record would have been.
#[test]
fn an_e5_face_record_outside_a_parsed_graph_is_given_up() {
    let mut face = Vec::new();
    // A face record that states no loop.
    append_e5_record(&mut face, 0x00, 41, &[0x81]);
    let result = decode(e5_catpart_appending(&face));
    let charges = undecoded(&result);
    assert_eq!(charges.len(), 1, "{charges:?}");
    assert!(
        charges[0]
            .message
            .starts_with("record catia:e5:face#41 given up: "),
        "{}",
        charges[0].message
    );
    assert!(
        charges[0].message.ends_with("dropped=[]"),
        "{}",
        charges[0].message
    );
    assert!(result.ir().geometry.faces.is_empty());
}

/// A face row outside the run the standard population takes becomes no face;
/// the decode gives up the row. The rows of the taken run stay faces, so no
/// row of that run is given up.
#[test]
fn an_fbb_face_row_outside_the_selected_run_is_given_up() {
    let mut main = main_stream();
    let lone_row = main.len();
    main.extend_from_slice(&FBB_ROW);
    let result = decode(standard_catpart_from_streams(&main, &surf_stream()));
    let charges = undecoded(&result);
    let lone = fbb_row_part(lone_row);
    assert_eq!(
        charges
            .iter()
            .filter(|loss| names_part(loss, &lone))
            .count(),
        1,
        "{charges:?}"
    );
    assert!(holds_stub(&result, &lone));
    for row in [FIRST_FBB_ROW, FIRST_FBB_ROW + 8] {
        let taken = fbb_row_part(row);
        assert!(
            !charges.iter().any(|loss| names_part(loss, &taken)),
            "{charges:?}"
        );
    }
}

/// Three face rows against two face-sense bindings build no face; the decode
/// gives up each row of the run.
#[test]
fn fbb_face_rows_without_matching_face_bindings_are_each_given_up() {
    let mut main = main_stream();
    let third_row = FIRST_FBB_ROW + 16;
    main.splice(third_row..third_row, FBB_ROW);
    let result = decode(standard_catpart_from_streams(&main, &surf_stream()));
    let charges = undecoded(&result);
    assert!(result.ir().geometry.faces.is_empty());
    for row in [FIRST_FBB_ROW, FIRST_FBB_ROW + 8, third_row] {
        let part = fbb_row_part(row);
        assert_eq!(
            charges
                .iter()
                .filter(|loss| names_part(loss, &part))
                .count(),
            1,
            "row at {row}: {charges:?}"
        );
        assert!(holds_stub(&result, &part));
    }
    assert!(
        !charges
            .iter()
            .any(|loss| loss.message.contains("catia:standard:face#")),
        "{charges:?}"
    );
}
