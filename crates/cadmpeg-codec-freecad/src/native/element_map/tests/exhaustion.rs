// SPDX-License-Identifier: Apache-2.0
//! Generic binding upper bounds and refusal before adapter visits.

use crate::native::element_map::{ElementMapNode, ElementMapNodes};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::cell::Cell;

fn with_work<T>(work: u64, call: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    call(&ctx)
}

fn nodes() -> ElementMapNodes {
    ElementMapNodes::try_from(vec![ElementMapNode {
        map_id: 0,
        groups: Vec::new(),
    }])
    .unwrap()
}

#[test]
fn empty_binding_upper_bound_needs_no_work_and_does_not_advance_adapter() {
    with_work(0, |ctx| {
        let visits = Cell::new(0);
        let bindings = std::iter::empty::<(&str, usize, &str)>().inspect(|_| {
            visits.set(visits.get() + 1);
        });
        nodes().bind_root_topology(ctx, bindings).unwrap();
        assert_eq!(visits.get(), 0);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn chained_binding_exhaustion_costs_only_actual_visits() {
    with_work(2, |ctx| {
        let visits = Cell::new(0);
        let bindings = [("Edge", 1, "first")]
            .into_iter()
            .chain([("Face", 2, "second")])
            .inspect(|_| {
                visits.set(visits.get() + 1);
            });
        let mut input = nodes();
        // Two binding visits. An empty group tree has zero comparisons.
        input.bind_root_topology(ctx, bindings).unwrap();
        assert_eq!(visits.get(), 2);
        assert!(input.root().groups.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn unknown_binding_lower_bound_zero_does_not_prove_exhaustion() {
    with_work(2, |ctx| {
        let calls = Cell::new(0);
        let bindings = std::iter::from_fn(|| {
            calls.set(calls.get() + 1);
            (calls.get() == 1).then_some(("Edge", 1, "first"))
        });
        assert_eq!(bindings.size_hint(), (0, None));
        nodes().bind_root_topology(ctx, bindings).unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn binding_refusal_precedes_unvisited_adapter_suffix() {
    with_work(1, |ctx| {
        let visits = Cell::new(0);
        let bindings = [("Edge", 1, "first"), ("Face", 2, "second")]
            .into_iter()
            .inspect(|_| {
                visits.set(visits.get() + 1);
            });
        let mut input = nodes();
        let error = input.bind_root_topology(ctx, bindings).unwrap_err();
        let CodecError::ResourceLimit(original) = error else {
            panic!("expected visit refusal")
        };
        assert_eq!(original.operation, "FreeCAD element topology binding scan");
        assert_eq!((original.used, original.additional), (1, 1));
        assert_eq!(visits.get(), 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn child_string_id_exhaustion_admits_only_actual_bytes() {
    use crate::native::element_map::ElementMapGroup;
    let diagnostic = "element-map node 1 group Edge child 0 has an invalid child string-id list";
    let diagnostic_work = 2 * cadmpeg_core::decode::u64_from_index(diagnostic.len());
    for (ids, work) in [
        ("0", 40),
        ("0.12", 48),
        ("0.", 37 + diagnostic_work),
        ("1.unvisited", 46 + diagnostic_work),
    ] {
        with_work(work, |ctx| {
            let result = ElementMapNodes::from_nodes(
                vec![ElementMapNode {
                    map_id: 0,
                    groups: vec![ElementMapGroup {
                        indexed_name: "Edge".into(),
                        children: vec![format!("1 0 1 0 0 stable {ids}")],
                        names: Vec::new(),
                    }],
                }],
                ctx,
            )
            .unwrap();
            match ids {
                "0" | "0.12" => assert!(result.is_ok()),
                "0." | "1.unvisited" => assert_eq!(result.unwrap_err(), diagnostic),
                _ => unreachable!(),
            }
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}
