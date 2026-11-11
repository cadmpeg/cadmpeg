// SPDX-License-Identifier: Apache-2.0

use crate::test_support::parasolid::sequence::{face_fields, topology_sequence_with_face, F};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn current_ownership_omissions_report_bodies_shells_and_faces() {
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
    assert!(out.faces.is_empty());
    assert_eq!(out.losses.len(), 1);
    assert_eq!(
        out.losses[0].code.local_code(),
        "topology.ownership-withheld"
    );
    assert_eq!(out.losses[0].message, "3 current typed ownership node(s) are withheld: 1 body node(s), 1 shell node(s), and 1 face node(s).");
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
