// SPDX-License-Identifier: Apache-2.0
//! Dictionary selectors follow the actual hexadecimal mapped-name grammar.

use super::*;

#[test]
fn mapped_name_dictionary_selectors_above_nine_are_hexadecimal_in_actual_parts() {
    let dictionary = (0..11)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let stream = format!("7 PostfixCount 11 {dictionary}\nMapCount 1\nElementMap 1 7 1\nFace ChildCount 0 NameCount 2\n0\n:a.2.0 0\nEndMap\n");
    for side in [false, true] {
        let bytes = if side {
            format!("BeginElementMap v1\n{stream}")
        } else {
            stream.clone()
        };
        let parsed = test_parse_element_map(bytes.as_bytes(), side).unwrap();
        let name = &parsed.maps.root().groups[0].as_ref().unwrap().names[1]
            .as_ref()
            .unwrap()[0];
        assert_eq!(name.encoded, ":a.2.0");
        assert_eq!(name.resolved.as_deref(), Some("p92"));
    }
}
