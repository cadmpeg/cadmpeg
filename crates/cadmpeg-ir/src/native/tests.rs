// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::disallowed_methods)]

use crate::diff;
use crate::examples::unit_cube;
use crate::native::NativeRecord;
use crate::validate::validate_neutral;

#[test]
fn native_records_use_own_ids_for_counts_diff_and_validation() {
    let left = unit_cube();
    let mut right = left.clone();
    right.native.namespace_mut("f3d").arenas.insert(
        "act_guids".into(),
        vec![NativeRecord::new(
            "f3d:test:act-guid#0",
            serde_json::Map::new(),
        )],
    );
    right.native.namespace_mut("sldprt").arenas.insert(
        "configurations".into(),
        vec![NativeRecord::new(
            "sldprt:test:configuration#0",
            serde_json::Map::new(),
        )],
    );
    right.native.finalize();

    let result = diff(&left, &right);
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.f3d.act_guids")
            .unwrap()
            .added,
        ["f3d:test:act-guid#0"]
    );
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.sldprt.configurations")
            .unwrap()
            .added,
        ["sldprt:test:configuration#0"]
    );
    let report = validate_neutral(&right, Vec::new());
    assert_eq!(report.entity_counts["native.f3d.act_guids"], 1);
    assert_eq!(report.entity_counts["native.sldprt.configurations"], 1);
    assert!(report.is_ok(), "{:?}", report.findings);

    right
        .native
        .namespace_mut("sldprt")
        .arenas
        .get_mut("configurations")
        .unwrap()[0] = NativeRecord::new("f3d:test:act-guid#0", serde_json::Map::new());
    right.native.finalize();
    assert!(validate_neutral(&right, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "entity id is not globally unique"));
}
