// SPDX-License-Identifier: Apache-2.0

use super::{collect, restore, OffsetVisitor, SourceOffsets};
use crate::records::identity::Sourced;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
struct Inner {
    at: Option<u64>,
    run: Option<[u64; 2]>,
}

impl SourceOffsets for Inner {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset("at", &mut self.at);
        offsets.offsets("run", &mut self.run);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Outer {
    byte_offset: Option<u64>,
    inner: Option<Inner>,
    rows: Vec<Inner>,
    values: Vec<Sourced<u32>>,
}

impl SourceOffsets for Outer {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset("byte_offset", &mut self.byte_offset);
        offsets.optional("inner", &mut self.inner);
        offsets.each("rows", &mut self.rows);
        offsets.sourced_each("values", &mut self.values);
    }
}

fn decoded() -> Outer {
    Outer {
        byte_offset: Some(10),
        inner: Some(Inner {
            at: Some(20),
            run: Some([30, 31]),
        }),
        rows: vec![Inner {
            at: Some(40),
            run: None,
        }],
        values: vec![Sourced::at(7, 50), Sourced::at(8, 51)],
    }
}

fn unlocated() -> Outer {
    Outer {
        byte_offset: None,
        inner: Some(Inner {
            at: None,
            run: None,
        }),
        rows: vec![Inner {
            at: None,
            run: None,
        }],
        values: vec![
            Sourced {
                value: 7,
                offset: None,
            },
            Sourced {
                value: 8,
                offset: None,
            },
        ],
    }
}

#[test]
fn collected_pointers_name_every_offset_by_its_field_path() {
    let pointers = collect(&mut decoded());
    assert_eq!(
        pointers,
        BTreeMap::from([
            ("byte_offset".to_owned(), 10),
            ("inner.at".to_owned(), 20),
            ("inner.run.0".to_owned(), 30),
            ("inner.run.1".to_owned(), 31),
            ("rows.0.at".to_owned(), 40),
            ("values.0".to_owned(), 50),
            ("values.1".to_owned(), 51),
        ])
    );
}

#[test]
fn restored_offsets_equal_the_decoded_offsets() {
    let mut record = unlocated();
    restore(&mut record, &collect(&mut decoded()));
    assert_eq!(record, decoded());
}

#[test]
fn an_absent_pointer_restores_an_absent_offset() {
    let mut pointers = collect(&mut decoded());
    pointers.remove("inner.run.1");
    pointers.remove("byte_offset");
    let mut record = decoded();
    restore(&mut record, &pointers);
    assert_eq!(record.byte_offset, None);
    assert_eq!(record.inner.as_ref().and_then(|inner| inner.run), None);
    assert_eq!(record.inner.as_ref().and_then(|inner| inner.at), Some(20));
}

#[test]
fn a_sourced_value_wire_carries_the_value_only() {
    let wire = serde_json::to_value(Sourced::at(7_u32, 50)).unwrap();
    assert_eq!(wire, serde_json::json!({"value": 7}));
    let read: Sourced<u32> = serde_json::from_value(wire).unwrap();
    assert_eq!(
        read,
        Sourced {
            value: 7,
            offset: None
        }
    );
    assert!(
        serde_json::from_value::<Sourced<u32>>(serde_json::json!({"value": 7, "offset": 50}))
            .is_err()
    );
}

#[test]
fn a_restore_names_each_pointer_that_a_located_record_lacks() {
    let mut located = decoded();
    located.rows[0].run = Some([60, 61]);
    let mut pointers = collect(&mut located);
    assert!(restore(&mut unlocated(), &pointers).is_empty());
    pointers.remove("inner.at");
    pointers.remove("rows.0.run.1");
    pointers.remove("values.1");
    assert_eq!(
        restore(&mut unlocated(), &pointers),
        ["inner.at", "rows.0.run.1", "values.1"]
    );
}

#[test]
fn an_optional_offset_without_its_pointer_is_not_missing() {
    struct Optional(Option<u64>);
    impl SourceOffsets for Optional {
        fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
            offsets.optional_offset("at", &mut self.0);
        }
    }
    let mut record = Optional(Some(5));
    assert!(restore(&mut record, &BTreeMap::new()).is_empty());
    assert_eq!(record.0, None);
}
