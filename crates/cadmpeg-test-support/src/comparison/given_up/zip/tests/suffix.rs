// SPDX-License-Identifier: Apache-2.0
use super::*;

fn records() -> [&'static [u8]; 3] {
    [
        b"#1=CARTESIAN_POINT('',(1,0,0));",
        b"#2=CARTESIAN_POINT('',(2,0,0));",
        b"#3=CARTESIAN_POINT('',(3,0,0));",
    ]
}

#[test]
fn a_true_eof_cut_witnesses_only_the_physically_removed_suffix_and_its_failed_record() {
    let records = records();
    let target = records.concat();
    let cut = records[0].len() + records[1].len() / 2;
    let old = fixture(&target, b"#4=CARTESIAN_POINT('',(4,0,0));", "target.step");
    let new = fixture(
        &target[..cut],
        b"#4=CARTESIAN_POINT('',(4,0,0));",
        "target.step",
    );
    let before = decode_records(&old, false, true);
    let after = decode_records(&new, false, true);
    assert_eq!(before.0.model.points.len(), 4);
    assert_eq!(
        after
            .0
            .model
            .points
            .iter()
            .map(|point| point.id.as_str())
            .collect::<Vec<_>>(),
        ["step:model:point#0:0", "step:model:point#1:0"]
    );
    let allowed = allowances(&old, &new, &before, &after).unwrap();
    assert!(allowed.removed.contains("step:model:point#0:1"));
    assert!(allowed.removed.contains("step:model:point#0:2"));
    assert!(!allowed.removed.contains("step:model:point#0:0"));
    assert!(!allowed.removed.contains("step:model:point#1:0"));
    let prior = serde_json::to_value(&before.0).unwrap();
    let current = serde_json::to_value(&after.0).unwrap();
    assert_decodes_agree(&prior, &current, &allowed, Mode::Same);
    let mut lost_prefix = after.0.clone();
    lost_prefix
        .model
        .points
        .retain(|point| point.id.as_str() != "step:model:point#0:0");
    assert!(crate::comparison::agree(
        &prior,
        &serde_json::to_value(lost_prefix).unwrap(),
        &allowed,
        Mode::Same
    )
    .is_err());
}

#[test]
fn equal_length_content_failure_cannot_license_a_later_healthy_record() {
    let records = records();
    let target = records.concat();
    let mut damaged = target.clone();
    damaged[records[0].len() + 3..records[0].len() + records[1].len() - 1].fill(0xff);
    let old = fixture(&target, b"#4=CARTESIAN_POINT('',(4,0,0));", "target.step");
    let new = fixture(&damaged, b"#4=CARTESIAN_POINT('',(4,0,0));", "target.step");
    let before = decode_records(&old, false, true);
    let after = decode_records(&new, false, true);
    let allowed = allowances(&old, &new, &before, &after).unwrap();
    assert!(allowed.removed.contains("step:model:point#0:1"));
    assert!(!allowed.removed.contains("step:model:point#0:2"));
    let prior = serde_json::to_value(&before.0).unwrap();
    assert_decodes_agree(
        &prior,
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    );
    let mut lost_peer = after.0.clone();
    lost_peer
        .model
        .points
        .retain(|point| point.id.as_str() != "step:model:point#0:2");
    assert!(crate::comparison::agree(
        &prior,
        &serde_json::to_value(lost_peer).unwrap(),
        &allowed,
        Mode::Same
    )
    .is_err());
}

#[test]
fn shortening_with_an_unrelated_prefix_mutation_proves_no_removed_suffix() {
    let records = records();
    let target = records.concat();
    let cut = records[0].len() + records[1].len() / 2;
    let mut damaged = target[..cut].to_vec();
    let coordinate = damaged
        .windows(5)
        .position(|bytes| bytes == b"(1,0,")
        .unwrap()
        + 1;
    damaged[coordinate] = b'9';
    let old = fixture(&target, b"#4=CARTESIAN_POINT('',(4,0,0));", "target.step");
    let new = fixture(&damaged, b"#4=CARTESIAN_POINT('',(4,0,0));", "target.step");
    let before = decode_records(&old, false, true);
    let after = decode_records(&new, false, true);
    assert!(allowances(&old, &new, &before, &after)
        .unwrap_err()
        .contains("not the original source prefix"));
}
