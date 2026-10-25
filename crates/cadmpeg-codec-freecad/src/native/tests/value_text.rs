// SPDX-License-Identifier: Apache-2.0
//! Appendix B keeps direct native value text in its exact base wire form.

use super::ValueRecord;
use std::collections::BTreeMap;

#[test]
fn direct_text_survives_every_value_tag_without_serialization_filtering() {
    for tag in [
        "Python",
        "StringHasher",
        "StringHasher2",
        "ElementMap",
        "ElementMap2",
        "String",
    ] {
        for text in [None, Some(""), Some("  authored\ntext Ω  ")] {
            let value = ValueRecord {
                item_index: None,
                link_index: None,
                tag: tag.into(),
                order: 0,
                parent: None,
                attributes: BTreeMap::new(),
                decoded_json: None,
                text: text.map(str::to_owned),
            };
            let wire = serde_json::to_value(&value).unwrap();
            assert_eq!(wire["text"], serde_json::json!(text), "{tag}");
            let restored: ValueRecord = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(restored.text.as_deref(), text, "{tag}");
            assert_eq!(serde_json::to_value(restored).unwrap(), wire, "{tag}");
        }
    }
}
