// SPDX-License-Identifier: Apache-2.0
//! Layer hierarchy serialization preserves the context-free wire fields.

use crate::presentation::LayerHierarchySlot;
use crate::settings::LayerHierarchy;
use crate::wire::Uuid;

#[test]
fn absent_layer_hierarchy_preserves_both_null_fields() {
    assert_eq!(serde_json::to_string(&LayerHierarchySlot(None)).unwrap(),
        r#"{"parent_uuid":null,"expanded":null}"#);
}

#[test]
fn nil_layer_parent_preserves_null_identity_and_expansion() {
    for (expanded, expected) in [(false, r#"{"parent_uuid":null,"expanded":false}"#),
        (true, r#"{"parent_uuid":null,"expanded":true}"#)] {
        let hierarchy = LayerHierarchySlot(Some(LayerHierarchy {
            parent_id: Uuid::nil(), expanded,
        }));
        assert_eq!(serde_json::to_string(&hierarchy).unwrap(), expected);
    }
}

#[test]
fn layer_parent_streams_the_canonical_identity_without_decode_context() {
    let hierarchy = LayerHierarchySlot(Some(LayerHierarchy {
        parent_id: Uuid::from_canonical([0x55; 16]), expanded: true,
    }));
    assert_eq!(serde_json::to_string(&hierarchy).unwrap(),
        r#"{"parent_uuid":"55555555-5555-5555-5555-555555555555","expanded":true}"#);
}
