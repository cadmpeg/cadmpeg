// SPDX-License-Identifier: Apache-2.0

use crate::test_support::parasolid::sequence::{face_fields, topology_sequence_with_face, F};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn a_given_up_face_is_not_counted_again_by_the_ownership_note() {
    let mut fields = face_fields();
    fields[5] = F::P(1);
    let (payload, _) = topology_sequence_with_face(Some((&[0xff], &fields)));
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let out = super::decode_bodies(
        &ctx,
        &[(&payload, &header)],
        &cadmpeg_ir::stream_name!("ownership"),
    )
    .unwrap();
    let mut out = out;
    assert!(out.faces.is_empty());
    // The shell, region and body are dropped with the face, so the note
    // counts none of them.
    assert!(out.losses.is_empty(), "{:#?}", out.losses);
    let with: Vec<_> = out
        .dropped
        .iter()
        .filter_map(|dropped| match dropped {
            super::DroppedRecord::With { record, .. } => Some(record.as_str()),
            super::DroppedRecord::GivenUp { .. } => None,
        })
        .collect();
    assert_eq!(with.len(), 3, "{with:?}");
    let losses = out.take_losses().unwrap();
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
            .filter(|loss| loss.message.starts_with("record sldprt:brep:face#5 "))
            .count(),
        1
    );
}

#[test]
fn complete_current_ownership_has_no_omission_loss() {
    use crate::test_support::parasolid::sequence::sequence_of;
    use F::{C, I, P, R, V};
    let (mut payload, _) = topology_sequence_with_face(None);
    let prefix = vec![I(9), P(1), P(1), P(1), P(1), P(1), C(b'+')];
    let mut plane = prefix.clone();
    plane.extend([V([0.0, 0.0, 0.003]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]);
    let mut circle = prefix;
    circle.extend([
        V([0.0, 0.002, 0.003]),
        V([0.0, 0.0, 1.0]),
        V([1.0, 0.0, 0.0]),
        R(0.001),
    ]);
    let (geometry, offset) = sequence_of(&[(50, 20, plane), (31, 21, circle)]);
    let end = payload.len() - 4;
    payload.splice(
        end..end,
        geometry[offset..geometry.len() - 4].iter().copied(),
    );
    let records = super::topology::nodes::sequence(&payload).unwrap();
    let at = records
        .nodes()
        .find(|node| node.kind() == 17)
        .unwrap()
        .field_range("sense")
        .unwrap()
        .start;
    drop(records);
    payload[at] = b'+';
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let out = super::decode_bodies(
        &ctx,
        &[(&payload, &header)],
        &cadmpeg_ir::stream_name!("ownership"),
    )
    .unwrap();
    assert_eq!(out.faces.len(), 1);
    assert!(out
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "topology.ownership-withheld"));
}

#[test]
fn a_typed_face_that_names_no_loop_is_given_up_beside_a_kept_face() {
    use crate::test_support::parasolid::sequence::{node_with_program, sequence_of};
    use F::{C, I, P, R, V};
    let (mut payload, _) = topology_sequence_with_face(None);
    let prefix = vec![I(9), P(1), P(1), P(1), P(1), P(1), C(b'+')];
    let mut plane = prefix.clone();
    plane.extend([V([0.0, 0.0, 0.003]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]);
    let mut circle = prefix;
    circle.extend([
        V([0.0, 0.002, 0.003]),
        V([0.0, 0.0, 1.0]),
        V([1.0, 0.0, 0.0]),
        R(0.001),
    ]);
    let (geometry, offset) = sequence_of(&[(50, 20, plane), (31, 21, circle)]);
    let end = payload.len() - 4;
    payload.splice(
        end..end,
        geometry[offset..geometry.len() - 4].iter().copied(),
    );
    let mut extra_face = face_fields();
    extra_face[0] = I(32);
    extra_face[5] = P(1);
    extra_face[6] = P(31);
    let mut extra = Vec::new();
    for (kind, attr, fields) in [
        (19, 30, vec![I(30), P(1), P(2), P(1), P(1), P(31), C(b'S')]),
        (
            13,
            31,
            vec![I(31), P(1), P(2), P(1), P(32), P(1), P(1), P(30), P(1)],
        ),
        (14, 32, extra_face),
    ] {
        node_with_program(&mut extra, kind, &[], attr, &fields);
    }
    let end = payload.len() - 4;
    payload.splice(end..end, extra);
    let records = super::topology::nodes::sequence(&payload).unwrap();
    let at = records
        .nodes()
        .find(|node| node.kind() == 17)
        .unwrap()
        .field_range("sense")
        .unwrap()
        .start;
    drop(records);
    payload[at] = b'+';
    let facts = crate::brep::typed::scan(&payload);
    let attrs = facts.valid_ownership_face_attrs().unwrap();
    assert_eq!(attrs, std::collections::HashSet::from([5, 32]));
    assert!(facts
        .hierarchies(&std::collections::HashSet::from([5]))
        .is_some());
    assert!(facts.hierarchies(&attrs).is_none());
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut out = super::decode_bodies(
        &ctx,
        &[(&payload, &header)],
        &cadmpeg_ir::stream_name!("ownership"),
    )
    .unwrap();
    assert_eq!(out.faces.len(), 1);
    assert_eq!(out.faces[0].id, super::id_face(5));
    assert!(out
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "topology.ownership-withheld"));
    let losses = out.take_losses().unwrap();
    let charges = losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let [charge] = charges[..] else {
        panic!("one charge gives up the loopless face: {charges:?}");
    };
    assert!(
        charge.starts_with("record sldprt:brep:face#32 "),
        "{charge}"
    );
}
