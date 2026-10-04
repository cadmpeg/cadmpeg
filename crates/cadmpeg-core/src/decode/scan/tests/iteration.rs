// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::decode::iter_source::IncrementalSource;
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;

fn with_work_limit(limit: u64, run: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit;
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    run(&context);
}

#[derive(Debug)]
struct HintedIterator<I> {
    source: I,
    next_calls: Rc<Cell<usize>>,
    hint_calls: Rc<Cell<usize>>,
}

impl<I: Iterator> Iterator for HintedIterator<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_calls.set(self.next_calls.get() + 1);
        self.source.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.hint_calls.set(self.hint_calls.get() + 1);
        (usize::MAX, Some(usize::MAX))
    }
}

#[test]
fn owned_vec_map_option_and_array_admit_their_complete_bounds_once() {
    with_work_limit(7, |context| {
        assert_eq!(
            context
                .admit_iter(vec![1, 2], "owned Vec")
                .expect("admission")
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(
            context
                .admit_iter(BTreeMap::from([(2, "b"), (1, "a")]), "owned BTreeMap")
                .expect("admission")
                .collect::<Vec<_>>(),
            [(1, "a"), (2, "b")]
        );
        assert_eq!(
            context
                .admit_iter(Some(7), "Some")
                .expect("admission")
                .collect::<Vec<_>>(),
            [7]
        );
        assert_eq!(
            context
                .admit_iter(None::<u8>, "None")
                .expect("admission")
                .collect::<Vec<_>>(),
            Vec::<u8>::new()
        );
        assert_eq!(
            context
                .admit_iter([3, 4], "array")
                .expect("admission")
                .collect::<Vec<_>>(),
            [3, 4]
        );

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("seven precharged visits use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 7);
    });
}

#[test]
fn serde_json_map_supports_owned_and_borrowed_sources() {
    with_work_limit(4, |context| {
        let mut map = serde_json::Map::new();
        map.insert("a".to_owned(), serde_json::Value::from(1));
        map.insert("b".to_owned(), serde_json::Value::from(2));

        let owned = context
            .admit_iter(map.clone(), "owned JSON map")
            .expect("owned map admission")
            .map(|(key, value)| (key, value.as_i64()))
            .collect::<Vec<_>>();
        assert_eq!(owned, [("a".to_owned(), Some(1)), ("b".to_owned(), Some(2))]);

        let borrowed = context
            .admit_iter(&map, "borrowed JSON map")
            .expect("borrowed map admission")
            .map(|(key, value)| (key.as_str(), value.as_i64()))
            .collect::<Vec<_>>();
        assert_eq!(borrowed, [("a", Some(1)), ("b", Some(2))]);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("both map traversals each admit two entries")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 4);
    });
}

#[test]
fn mutable_vec_and_slice_admission_precedes_mutation() {
    with_work_limit(4, |context| {
        let mut values = vec![1, 2];
        for value in context
            .admit_iter(&mut values, "mutable Vec")
            .expect("admission")
        {
            *value += 10;
        }
        assert_eq!(values, [11, 12]);

        let mut slice = [3, 4];
        for value in context
            .admit_iter(&mut slice[..], "mutable slice")
            .expect("admission")
        {
            *value *= 2;
        }
        assert_eq!(slice, [6, 8]);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("four precharged visits use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 4);
    });
}

#[test]
fn mutable_array_admission_and_refusal_precede_direct_array_traversal() {
    with_work_limit(2, |context| {
        let mut values = [1, 2];
        for value in context
            .admit_iter(&mut values, "mutable array")
            .expect("direct array admission")
        {
            *value += 10;
        }
        assert_eq!(values, [11, 12]);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("both array visits use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 2);
    });

    with_work_limit(1, |context| {
        let mut values = [3, 4];
        let refusal = context
            .admit_iter(&mut values, "mutable array")
            .expect_err("the complete array bound exceeds the work limit");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 2);
        assert_eq!(values, [3, 4]);
    });
}

#[test]
fn mutable_source_refusal_precedes_the_first_mutable_item() {
    with_work_limit(1, |context| {
        let mut values = vec![1, 2];
        let refusal = context
            .admit_iter(&mut values, "mutable Vec")
            .expect_err("the complete bound exceeds the work limit");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 2);
        assert_eq!(values, [1, 2]);
    });
}

#[test]
fn unknown_iterator_ignores_size_hint_and_charges_each_attempted_step() {
    with_work_limit(3, |context| {
        let next_calls = Rc::new(Cell::new(0));
        let hint_calls = Rc::new(Cell::new(0));
        let source = HintedIterator {
            source: [5, 6].into_iter(),
            next_calls: Rc::clone(&next_calls),
            hint_calls: Rc::clone(&hint_calls),
        };
        let mut admitted = context
            .admit_iter(IncrementalSource::new(source), "unknown")
            .expect("unknown source has no upfront refusal");

        assert_eq!(admitted.size_hint(), (0, None));
        assert_eq!(hint_calls.get(), 0);
        assert_eq!(next_calls.get(), 0);
        assert!(matches!(admitted.next(), Some(Ok(5))));
        assert!(matches!(admitted.next(), Some(Ok(6))));
        assert!(admitted.next().is_none());
        assert_eq!(next_calls.get(), 3);
        assert_eq!(hint_calls.get(), 0);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("two values and the terminal probe cost three steps")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 3);
    });
}

#[test]
fn unknown_iterator_refusal_precedes_next_and_is_emitted_once() {
    with_work_limit(1, |context| {
        let next_calls = Rc::new(Cell::new(0));
        let hint_calls = Rc::new(Cell::new(0));
        let source = HintedIterator {
            source: [5, 6].into_iter(),
            next_calls: Rc::clone(&next_calls),
            hint_calls: Rc::clone(&hint_calls),
        };
        let mut admitted = context
            .admit_iter(IncrementalSource::new(source), "unknown")
            .expect("unknown source has no upfront refusal");

        assert!(matches!(admitted.next(), Some(Ok(5))));
        let Some(Err(CodecError::ResourceLimit(first))) = admitted.next() else {
            panic!("the second step must refuse before advancing the source");
        };
        assert_eq!(next_calls.get(), 1);
        assert_eq!(hint_calls.get(), 0);
        assert_eq!(context.resource_refusal(), Some(first));
        assert!(admitted.next().is_none());
        assert_eq!(next_calls.get(), 1);

        let CodecError::ResourceLimit(repeated) = context
            .charge_work(0, "later work")
            .expect_err("the resource refusal remains fused")
        else {
            panic!("resource refusal");
        };
        assert_eq!(repeated, first);
    });
}

#[test]
fn dialect_layers_iterator_charges_borrowed_matches_and_terminal_probe() {
    use crate::dialect::{DialectLayers, DialectMatch};

    let layers = DialectLayers::of(DialectMatch::residual(crate::dialect_id!(
        "rhino:archive-80"
    )))
    .with(DialectMatch::residual(crate::dialect_id!("acis:other")))
    .expect("distinct dialect layers");

    with_work_limit(3, |context| {
        let mut admitted = context
            .admit_iter(IncrementalSource::new(layers.iter()), "dialect layers")
            .expect("unknown source has no upfront refusal");
        assert_eq!(
            admitted
                .next()
                .expect("primary layer step")
                .expect("primary layer admission")
                .dialect()
                .as_str(),
            "rhino:archive-80"
        );
        assert_eq!(
            admitted
                .next()
                .expect("extra layer step")
                .expect("extra layer admission")
                .dialect()
                .as_str(),
            "acis:other"
        );
        assert!(admitted.next().is_none());

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("two matches and the terminal probe use all three steps")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 3);
    });

    with_work_limit(2, |context| {
        let mut admitted = context
            .admit_iter(IncrementalSource::new(layers.iter()), "dialect layers")
            .expect("unknown source has no upfront refusal");
        for expected in ["rhino:archive-80", "acis:other"] {
            assert_eq!(
                admitted
                    .next()
                    .expect("layer step")
                    .expect("layer admission")
                    .dialect()
                    .as_str(),
                expected
            );
        }
        let Some(Err(CodecError::ResourceLimit(first))) = admitted.next() else {
            panic!("the terminal probe must refuse after both layer visits");
        };
        assert_eq!(first.used, 2);
        assert_eq!(first.additional, 1);
        assert_eq!(context.resource_refusal(), Some(first));
        assert!(admitted.next().is_none());

        let CodecError::ResourceLimit(repeated) = context
            .charge_work(0, "later work")
            .expect_err("the terminal-probe refusal remains fused")
        else {
            panic!("resource refusal");
        };
        assert_eq!(repeated, first);
    });
}

#[test]
fn roxmltree_children_use_per_step_admission() {
    with_work_limit(3, |context| {
        let document =
            roxmltree::Document::parse("<root><first/><second/></root>").expect("valid XML");
        let mut children = context
            .admit_iter(document.root_element().children(), "XML children")
            .expect("unknown source has no upfront refusal");
        assert_eq!(children.size_hint(), (0, None));
        assert_eq!(
            children
                .next()
                .expect("first child step")
                .expect("first child admission")
                .tag_name()
                .name(),
            "first"
        );
        assert_eq!(
            children
                .next()
                .expect("second child step")
                .expect("second child admission")
                .tag_name()
                .name(),
            "second"
        );
        assert!(children.next().is_none());

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("two children and the terminal probe use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 3);
    });
}

