// SPDX-License-Identifier: Apache-2.0
use super::{key_work, project};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::cell::Cell;

#[derive(Serialize)]
struct Shape {
    text: String,
    values: Vec<f64>,
    optional: Option<Box<(bool, u64)>>,
}

#[test]
fn structural_projection_preserves_nonfinite_values_and_admits_each_dimension() {
    let shape = Shape {
        text: "source text".into(),
        values: vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0],
        optional: Some(Box::new((true, 17))),
    };
    let expected = serde_value::to_value(&shape).unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = project(&ctx, &shape, "project structural fixture").unwrap();
    assert_eq!(*projected, expected);
    drop(projected);
    ctx.finish_session().unwrap();
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("projection uses scoped dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project(&ctx, &shape, "project structural fixture").unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal must stay outside serde");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn structural_projection_holds_and_releases_its_storage_reservation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = project(&ctx, "abcde", "project first text").unwrap();
    assert_eq!(*first, serde_value::Value::String("abcde".into()));
    drop(first);
    let second = project(&ctx, "abcde", "project second text").unwrap();
    drop(second);
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = project(&ctx, "abcde", "project first text").unwrap();
    let error = project(&ctx, "x", "project second text").unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    drop(first);
}

#[test]
fn structural_projection_counts_both_variant_containers() {
    #[derive(Serialize)]
    enum Shape {
        Tuple(),
        Struct {},
    }
    for shape in [Shape::Tuple(), Shape::Struct {}] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project(&ctx, &shape, "project variant containers").unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.used == 1 && limit.additional == 1)
        );
    }
}

struct DisplayText;

impl std::fmt::Display for DisplayText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("abcde")
    }
}

impl Serialize for DisplayText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[test]
fn structural_display_text_uses_only_scoped_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = project(&ctx, &DisplayText, "project display text").unwrap();
    assert_eq!(*projected, serde_value::Value::String("abcde".into()));
    drop(projected);
    let storage = ctx.reserve_scoped(5, "display text released").unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn structural_display_text_preserves_formatting_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project(&ctx, &DisplayText, "project display text").unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("display formatting must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "project display text");
    assert_eq!(limit.additional, 5);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
    );
}

struct CountingIter<'a, T> {
    values: &'a [T],
    index: usize,
    yielded: &'a Cell<usize>,
}

impl<'a, T> Iterator for CountingIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.values.get(self.index)?;
        self.index += 1;
        self.yielded.set(self.yielded.get() + 1);
        Some(value)
    }
}

struct CountingSequence<'a> {
    values: &'a [u8],
    yielded: &'a Cell<usize>,
}

impl Serialize for CountingSequence<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.values.len()))?;
        for value in (CountingIter {
            values: self.values,
            index: 0,
            yielded: self.yielded,
        }) {
            sequence.serialize_element(value)?;
        }
        sequence.end()
    }
}

struct CountingMap<'a> {
    values: &'a [(&'a str, u8)],
    yielded: &'a Cell<usize>,
}

impl Serialize for CountingMap<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.values.len()))?;
        for (key, value) in (CountingIter {
            values: self.values,
            index: 0,
            yielded: self.yielded,
        }) {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

#[test]
fn structural_projection_precharges_declared_source_visits_before_next() {
    let values = [1, 2];
    let sequence_yielded = Cell::new(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project(
        &ctx,
        &CountingSequence {
            values: &values,
            yielded: &sequence_yielded,
        },
        "project counted sequence",
    )
    .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("source extent refusal must remain a resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 2);
    assert_eq!(sequence_yielded.get(), 0);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
    );

    let entries = [("a", 1), ("b", 2)];
    let map_yielded = Cell::new(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project(
        &ctx,
        &CountingMap {
            values: &entries,
            yielded: &map_yielded,
        },
        "project counted map",
    )
    .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("source extent refusal must remain a resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 2);
    assert_eq!(map_yielded.get(), 0);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
    );
}

#[test]
fn structural_projection_key_cost_admits_value_container_before_loop() {
    let sequence =
        serde_value::Value::Seq(vec![serde_value::Value::Unit, serde_value::Value::Unit]);
    let map = serde_value::Value::Map(std::collections::BTreeMap::from([
        (
            serde_value::Value::String("a".into()),
            serde_value::Value::Unit,
        ),
        (
            serde_value::Value::String("b".into()),
            serde_value::Value::Unit,
        ),
    ]));

    for key in [sequence, map] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(limit) = key_work(&ctx, &key, "project map key")
            .expect_err("container visits must be admitted before the scan")
        else {
            panic!("key traversal must refuse on the work limit");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.used, 1);
        assert_eq!(limit.additional, 2);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
        );
    }
}

struct UnknownLengthSequence;

impl Serialize for UnknownLengthSequence {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        sequence.serialize_element(&7u8)?;
        sequence.end()
    }
}

struct UnknownLengthMap;

impl Serialize for UnknownLengthMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("key", &7u8)?;
        map.end()
    }
}

#[test]
fn structural_projection_preserves_unknown_length_serializers() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let sequence = project(&ctx, &UnknownLengthSequence, "unknown length sequence").unwrap();
    assert_eq!(
        *sequence,
        serde_value::to_value(&UnknownLengthSequence).unwrap()
    );
    let map = project(&ctx, &UnknownLengthMap, "unknown length map").unwrap();
    assert_eq!(*map, serde_value::to_value(&UnknownLengthMap).unwrap());
    drop(sequence);
    drop(map);
    ctx.finish_session().unwrap();
}
