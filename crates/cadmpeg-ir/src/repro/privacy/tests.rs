// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn constrained_references_are_rewritten_and_enum_tokens_survive_name_collisions() {
    let original = "synthetic:private-member.f3d:record#1";
    let anonymous = "synthetic:repro0:record#1";
    let mut replacements = BTreeMap::from([(original.into(), anonymous.into())]);
    let fields = BTreeSet::from(["/kind".into(), "/parent".into()]);
    let mut row =
        serde_json::json!({"id":original, "parent":original, "kind":"line", "name":"line"});
    sanitize_native(&mut row, "", "", false, &fields, &mut replacements);
    assert_eq!(row["id"], anonymous);
    assert_eq!(row["parent"], anonymous);
    assert_eq!(row["kind"], "line");
    assert_ne!(row["name"], "line");
    let mut next = serde_json::json!({"kind":"line"});
    sanitize_native(&mut next, "", "", false, &fields, &mut replacements);
    assert_eq!(next["kind"], "line");
    assert!(!row.to_string().contains("private-member.f3d"));
}
