// SPDX-License-Identifier: Apache-2.0
//! Unread source correspondence grants no semantic record-change permission.

use super::*;

type Snapshot = (CadIr, SourceFidelity, DecodeReport);

fn source() -> Vec<u8> {
    let mut bytes = payload();
    bytes.extend_from_slice(b"#2=CARTESIAN_POINT('second',(2,0,0));");
    bytes
}

fn damaged(bytes: &[u8]) -> Vec<u8> {
    let mut changed = bytes.to_vec();
    let text = std::str::from_utf8(bytes).unwrap();
    let coordinate = text.rfind("(2,0,0)").unwrap() + 1;
    changed[coordinate] = b'x';
    changed
}

fn decode(bytes: &[u8]) -> Snapshot {
    decode_records(bytes, false, true)
}

fn pair(prior: &[u8], next: &[u8], peer: &[u8]) -> (Vec<u8>, Vec<u8>, Snapshot, Snapshot) {
    let old = fixture(prior, peer, "target.step");
    let new = fixture(next, peer, "target.step");
    let before = decode(&old);
    let after = decode(&new);
    (old, new, before, after)
}

fn target_stream(snapshot: &Snapshot) -> String {
    snapshot.1.record_layout()["step:model:point#0:0"]
        .stream()
        .to_owned()
}

fn forge_span(snapshot: &mut Snapshot, offset: usize) {
    let stream = target_stream(snapshot);
    let mut wire = serde_json::to_value(&snapshot.1).unwrap();
    wire["b1_spans"][&stream]["spans"] = json!([
        {"offset": offset, "byte_len": 1, "classes": ["model", "design"]}
    ]);
    snapshot.1 = serde_json::from_value(wire).unwrap();
}

#[test]
fn both_genuine_b1_spans_prove_only_source_correspondence_in_the_failed_member() {
    let good = source();
    let mut bad = damaged(&good);
    bad[good.iter().position(|byte| *byte == b'a').unwrap()] = b'z';
    let (old, new, before, mut after) = pair(&good, &bad, b"#3=CARTESIAN_POINT('peer',(3,0,0));");
    let allowed = allowances(&old, &new, &before, &after).unwrap();
    assert_decodes_agree(
        &serde_json::to_value(&before.0).unwrap(),
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    );
    assert_eq!(after.0.model.points.len(), 2);
    after
        .0
        .model
        .points
        .iter_mut()
        .find(|point| point.id.as_str() == "step:model:point#0:0")
        .unwrap()
        .set_position(FinitePoint3::new(cadmpeg_ir::math::Point3::new(9.0, 0.0, 0.0)).unwrap());
    assert!(
        crate::comparison::agree(
            &serde_json::to_value(&before.0).unwrap(),
            &serde_json::to_value(&after.0).unwrap(),
            &allowed,
            Mode::Same,
        )
        .is_err(),
        "unread source grants no change to the independent held point"
    );
}

#[test]
fn one_sided_b1_and_wrong_actual_stream_lengths_prove_no_correspondence() {
    let good = source();
    let mut bad = damaged(&good);
    bad[good.iter().position(|byte| *byte == b'a').unwrap()] = b'z';
    let (old, new, before, after) = pair(&good, &bad, b"#3=CARTESIAN_POINT('',(3,0,0));");
    for remove_before in [false, true] {
        let mut before = before.clone();
        let mut after = after.clone();
        let snapshot = if remove_before {
            &mut before
        } else {
            &mut after
        };
        let stream = target_stream(snapshot);
        let mut wire = serde_json::to_value(&snapshot.1).unwrap();
        wire["b1_spans"].as_object_mut().unwrap().remove(&stream);
        snapshot.1 = serde_json::from_value(wire).unwrap();
        assert!(allowances(&old, &new, &before, &after).is_err());
    }
    let mut wrong_length = after.clone();
    let stream = target_stream(&wrong_length);
    let mut wire = serde_json::to_value(&wrong_length.1).unwrap();
    wire["b1_spans"][&stream]["byte_len"] = json!(bad.len() + 1);
    wrong_length.1 = serde_json::from_value(wire).unwrap();
    assert!(allowances(&old, &new, &before, &wrong_length).is_err());
}

#[test]
fn b1_forged_over_an_actual_record_read_or_part_framing_grants_no_correspondence() {
    let good = source();
    let coordinate = std::str::from_utf8(&good).unwrap().find("(1,0,0)").unwrap() + 1;
    for (offset, replacement) in [(coordinate, b'9'), (0, b'@')] {
        let mut bad = damaged(&good);
        bad[offset] = replacement;
        let (old, new, mut before, mut after) =
            pair(&good, &bad, b"#3=CARTESIAN_POINT('',(3,0,0));");
        forge_span(&mut before, offset);
        forge_span(&mut after, offset);
        assert!(allowances(&old, &new, &before, &after).is_err());
    }
}

#[test]
fn an_unrelated_members_b1_change_does_not_borrow_another_members_failure() {
    let good = source();
    let peer = b"#3=CARTESIAN_POINT('peer',(3,0,0));";
    let old = fixture(&good, peer, "target.step");
    let mut changed_peer = peer.to_vec();
    changed_peer[peer.iter().position(|byte| *byte == b'p').unwrap()] = b'z';
    let new = fixture(&damaged(&good), &changed_peer, "target.step");
    let before = decode(&old);
    let after = decode(&new);
    assert!(allowances(&old, &new, &before, &after).is_err());
}

#[test]
fn changed_member_method_is_not_a_b1_source_alias() {
    let good = source();
    let mut bad = damaged(&good);
    bad[good.iter().position(|byte| *byte == b'a').unwrap()] = b'z';
    let peer = b"#3=CARTESIAN_POINT('',(3,0,0));";
    let old = fixture(&good, peer, "target.step");
    let new = archive(&[
        RawEntry::stored("target.step", &bad),
        RawEntry::stored("peer.step", peer),
    ]);
    let before = decode(&old);
    let after = decode(&new);
    assert!(allowances(&old, &new, &before, &after).is_err());
}
