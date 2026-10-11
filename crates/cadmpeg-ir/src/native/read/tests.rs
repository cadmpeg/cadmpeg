// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::native::{NativeConvertError, NativeNamespace};

fn stored(record: &serde_json::Value) -> NativeNamespace {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut namespace = NativeNamespace::default();
    namespace
        .set_arena(&ctx, "records", std::slice::from_ref(record))
        .unwrap();
    namespace
}

/// Read the one stored record with `retained` bytes of retained storage.
fn read_with<T: DeserializeOwned>(
    namespace: &NativeNamespace,
    retained: u64,
) -> Result<Vec<T>, NativeConvertError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    namespace.arena_as_for_decode::<T>(&ctx, "records")
}

fn refused_retained<T: DeserializeOwned>(namespace: &NativeNamespace, retained: u64) -> bool {
    match read_with::<T>(namespace, retained) {
        Err(NativeConvertError::Resource(CodecError::ResourceLimit(limit))) => {
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "load typed native record");
            true
        }
        Err(error) => panic!("unexpected refusal: {error}"),
        Ok(_) => false,
    }
}

#[derive(Debug, PartialEq, Deserialize)]
struct Attributes {
    id: String,
    attributes: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct NamedAttribute {
    id: String,
    attributes: OneAttribute,
}

#[derive(Debug, PartialEq, Deserialize)]
struct OneAttribute {
    name: String,
}

#[test]
fn a_map_reader_pays_for_its_tree_nodes_where_a_struct_reader_does_not() {
    let namespace = stored(&serde_json::json!({
        "id": "test:native:record#map",
        "attributes": {"name": "x"},
    }));
    // A one-entry tree allocates a whole leaf; the struct keeps two strings.
    let leaf = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<serde_json::Value>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<serde_json::Value>();
    let ceiling = cadmpeg_core::decode::u64_from_index(leaf);
    assert!(!refused_retained::<NamedAttribute>(&namespace, ceiling));
    assert!(refused_retained::<Attributes>(&namespace, ceiling));
    let read = read_with::<Attributes>(&namespace, u64::MAX).unwrap();
    assert_eq!(read[0].attributes["name"], "x");
}

#[derive(Debug, PartialEq, Deserialize)]
struct Bytes {
    bytes: Vec<u8>,
}

#[test]
fn byte_elements_keep_their_own_width() {
    let namespace = stored(&serde_json::json!({
        "id": "test:native:record#bytes",
        "bytes": vec![7_u8; 256],
    }));
    // One byte per element, not one stored value per element.
    let ceiling: u64 = 1024;
    assert!(
        cadmpeg_core::decode::u64_from_index(256 * std::mem::size_of::<serde_json::Value>())
            > 4 * ceiling
    );
    assert!(!refused_retained::<Bytes>(&namespace, ceiling));
    assert!(refused_retained::<Bytes>(&namespace, 255));
}

#[derive(Debug, PartialEq, Deserialize)]
struct Small {
    id: String,
}

/// The least retained allowance under which the one stored record reads.
fn retained_need<T: DeserializeOwned>(namespace: &NativeNamespace) -> u64 {
    let (mut refused, mut admitted) = (0, 1 << 20);
    assert!(!refused_retained::<T>(namespace, admitted));
    while admitted - refused > 1 {
        let middle = refused + (admitted - refused) / 2;
        if refused_retained::<T>(namespace, middle) {
            refused = middle;
        } else {
            admitted = middle;
        }
    }
    admitted
}

#[test]
fn struct_keys_and_ignored_members_keep_nothing() {
    let plain = stored(&serde_json::json!({"id": "test:native:record#small"}));
    let padded = stored(&serde_json::json!({
        "id": "test:native:record#small",
        "ignored": "x".repeat(4096),
        "unmatched_key_with_a_long_name": null,
    }));
    assert_eq!(
        retained_need::<Small>(&padded),
        retained_need::<Small>(&plain)
    );
    assert!(retained_need::<serde_json::Value>(&padded) > 4096);
}

#[derive(Debug, PartialEq, Deserialize)]
enum Shape {
    Unit,
    Newtype(u32),
    Tuple(i8, String),
    Struct { left: f64, right: Option<bool> },
}

#[derive(Debug, Deserialize)]
struct Mixed {
    id: String,
    shapes: Vec<Shape>,
    numbered: BTreeMap<u32, Vec<Option<String>>>,
    pair: (u16, char),
    nested: Option<Box<Mixed>>,
    raw: Box<serde_json::value::RawValue>,
    #[serde(default)]
    absent: Vec<u64>,
}

impl PartialEq for Mixed {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.shapes == other.shapes
            && self.numbered == other.numbered
            && self.pair == other.pair
            && self.nested == other.nested
            && self.raw.get() == other.raw.get()
            && self.absent == other.absent
    }
}

#[test]
fn typed_reads_match_the_plain_value_reader() {
    let record = serde_json::json!({
        "id": "test:native:record#mixed",
        "shapes": ["Unit", {"Newtype": 7}, {"Tuple": [-3, "t"]},
                   {"Struct": {"left": 1.5, "right": null}}],
        "numbered": {"2": ["a", null], "10": []},
        "pair": [9, "c"],
        "nested": {
            "id": "inner", "shapes": [], "numbered": {}, "pair": [0, "z"],
            "nested": null, "raw": {"k": [1, 2]}
        },
        "raw": [true, {"deep": "text"}],
    });
    let namespace = stored(&record);
    let read = read_with::<Mixed>(&namespace, u64::MAX).unwrap();
    let plain: Mixed = serde_json::from_value(record.clone()).unwrap();
    assert_eq!(read, vec![plain]);
    let value = read_with::<serde_json::Value>(&namespace, u64::MAX).unwrap();
    assert_eq!(value, vec![record]);
}

#[derive(Debug, Deserialize)]
struct Numbered {
    attributes: BTreeMap<i32, String>,
}

#[test]
fn reader_shape_errors_stay_data_errors() {
    let namespace = stored(&serde_json::json!({
        "id": "test:native:record#bad",
        "bytes": "not a sequence",
    }));
    assert!(matches!(
        read_with::<Bytes>(&namespace, u64::MAX),
        Err(NativeConvertError::Arena { .. })
    ));
    let numbered = stored(&serde_json::json!({
        "id": "test:native:record#keys",
        "attributes": {"+1": "x"},
    }));
    assert!(matches!(
        read_with::<Numbered>(&numbered, u64::MAX).map(|read| read[0].attributes.len()),
        Err(NativeConvertError::Arena { .. })
    ));
}

#[derive(Debug, Deserialize)]
struct Links {
    links: Vec<String>,
}

#[test]
fn typed_null_diagnostic_matches_serde() {
    let record = serde_json::json!({
        "id": "test:native:record#null-links",
        "links": null,
    });
    let expected = serde_json::from_value::<Links>(record.clone())
        .expect_err("null cannot deserialize as a sequence")
        .to_string();
    let namespace = stored(&record);
    let error = read_with::<Links>(&namespace, u64::MAX).expect_err("null links are invalid");
    let NativeConvertError::Arena { source, .. } = error else {
        panic!("typed read error names its arena");
    };
    let NativeConvertError::ReadRecord { source, .. } = *source else {
        panic!("typed read error names its record");
    };
    assert_eq!(source.to_string(), expected);
    assert_eq!(
        source.to_string(),
        "invalid type: null, expected a sequence"
    );
    let empty: Links =
        serde_json::from_value(serde_json::json!({"links": []})).expect("empty links deserialize");
    assert!(empty.links.is_empty());
}
