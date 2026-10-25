// SPDX-License-Identifier: Apache-2.0
//! Unread native tables cannot be serialized as authored empty tables.
use super::*;

#[test]
fn native_only_unread_tables_are_omitted_with_loss_and_dropped_disposition() {
    let owner = "fcstd:native:shape-payload#partial";
    let payload: ShapePayloadRecord = serde_json::from_value(serde_json::json!({
        "id": owner, "property": "fcstd:native:property#Shape",
        "entry": "fcstd:native:entry#Shape.brp", "form":"binary", "text":null,
        "binary": { "topology_version":4, "locations":[] },
    }))
    .unwrap();
    assert!(payload.has_unread_tables());
    let original = serde_json::to_value(&payload).unwrap();
    let mut bytes = b"current seed must not turn unread tables into zero counts".to_vec();
    let mut outcomes =
        NativeDispositions::for_namespace("fcstd", ["shape_payloads"], NativeDisposition::Written);
    let mut losses = Vec::new();
    let mut has_geometry = false;
    let meshes = merge(
        &CadIr::empty(),
        &payload,
        &[],
        &mut bytes,
        &HashMap::new(),
        true,
        &mut has_geometry,
        &mut outcomes,
        &mut losses,
    )
    .unwrap();
    assert!(bytes.is_empty());
    assert!(meshes.is_empty());
    assert!(!has_geometry);
    assert_eq!(
        outcomes.record_disposition("fcstd", "shape_payloads", owner),
        NativeDisposition::Dropped
    );
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(owner));
    assert!(losses[0].message.contains("unread tables or root uses"));
    assert_eq!(serde_json::to_value(&payload).unwrap(), original);
}
