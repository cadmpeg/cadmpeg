// SPDX-License-Identifier: Apache-2.0
//! JSON Schema checks for `cadmpeg-core` public types.

use std::collections::BTreeMap;

use cadmpeg_core::serde_erase::json;
use cadmpeg_core::text::NonBlankString;

#[test]
fn nonblank_schema_constrains_property_names_without_trimming_source_text() {
    let schema = json::to_value(schemars::schema_for!(NonBlankString)).unwrap();
    assert_eq!(schema["type"], "string");
    let pattern = schema["pattern"].as_str().unwrap();
    let map = json::to_value(schemars::schema_for!(BTreeMap<NonBlankString, String>)).unwrap();
    assert_eq!(map["additionalProperties"], false);
    assert!(map["patternProperties"].get(pattern).is_some());
    for text in [" \tname\n", "\u{feff}", "\0"] {
        let value = NonBlankString::new(text).unwrap();
        assert_eq!(json::to_value(value).unwrap(), text);
    }
}
