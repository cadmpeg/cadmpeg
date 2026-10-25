// SPDX-License-Identifier: Apache-2.0
use super::*;

fn source() -> (Vec<u8>, usize) {
    let bytes = archive(&[
        RawEntry::stored("first.step", b"#1=CARTESIAN_POINT('',(1,0,0));"),
        RawEntry::stored("second.step", b"#2=CARTESIAN_POINT('',(2,0,0));"),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let payload = entries[1].payload().start();
    let second = frames
        .iter()
        .find(|part| part.level().as_str() == "zip.local_header" && part.view().end() == payload)
        .unwrap()
        .view()
        .start();
    drop(ctx);
    (bytes, second)
}

#[test]
fn a_recovered_header_prefix_licenses_only_physically_missing_members() {
    let (old, second) = source();
    let new = &old[..second + 17];
    let before = decode(&old, false);
    let after = decode(new, false);
    assert_eq!(after.0.model.points.len(), 1);
    assert_eq!(after.0.model.points[0], before.0.model.points[0]);
    let allowed = allowances(&old, new, &before, &after).unwrap();
    assert!(!allowed.removed.contains("step:model:point#0"));
    assert!(allowed.removed.contains("step:model:point#1"));
    let prior = serde_json::to_value(&before.0).unwrap();
    assert_decodes_agree(
        &prior,
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    );
    let mut lost_prefix = after.0.clone();
    lost_prefix.model.points.clear();
    assert!(crate::comparison::agree(
        &prior,
        &serde_json::to_value(lost_prefix).unwrap(),
        &allowed,
        Mode::Same
    )
    .is_err());
}

#[test]
fn a_same_length_incomplete_header_grants_no_archive_suffix_permission() {
    let (old, second) = source();
    let before = decode(&old, false);
    let mut changed = old.clone();
    crate::bytes::put_u16(&mut changed, second + 26, u16::MAX);
    changed[second + 30..].fill(0);
    let after = decode(&changed, false);
    assert!(allowances(&old, &changed, &before, &after).is_err());
}

#[test]
fn an_altered_prefix_or_wrong_header_home_grants_no_missing_member_permission() {
    let (old, second) = source();
    let before = decode(&old, false);
    let prefix = &old[..second + 17];
    let mut changed = prefix.to_vec();
    let coordinate = changed
        .windows(5)
        .position(|bytes| bytes == b"(1,0,")
        .unwrap()
        + 1;
    changed[coordinate] = b'9';
    let payload_start = 30 + "first.step".len();
    let crc = crate::zip_fixture::crc32(&changed[payload_start..second]);
    crate::bytes::put_u32(&mut changed, 14, crc);
    let changed_after = decode(&changed, false);
    assert!(allowances(&old, &changed, &before, &changed_after).is_err());
    let mut after = decode(prefix, false);
    let mut wire = serde_json::to_value(&after.1).unwrap();
    let parts = wire["parts"].as_object_mut().unwrap();
    let failed = parts
        .values_mut()
        .find(|part| {
            part["level"] == "zip.local_header" && part["offset"] == u64_from_index(second)
        })
        .unwrap();
    failed["offset"] = json!(second + 1);
    failed["byte_len"] = json!(16);
    let spans = failed["framing_bytes"].as_array_mut().unwrap();
    for range in spans {
        let start = range["start"].as_u64().unwrap();
        let end = range["end"].as_u64().unwrap();
        range["start"] = json!(start + 1);
        range["end"] = json!((end + 1).min(u64_from_index(prefix.len())));
    }
    after.1 = serde_json::from_value(wire).unwrap();
    assert!(allowances(&old, prefix, &before, &after).is_err());
}
