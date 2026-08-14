// SPDX-License-Identifier: Apache-2.0
//! Formula unit tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::{outer_container_in_scope, LegacyModelingScope};
use crate::native::CatiaOuterContainerBinding;

mod evaluate;
mod transfer;

fn binding(stream_name: &str) -> CatiaOuterContainerBinding {
    CatiaOuterContainerBinding {
        data_offset: 10,
        ordinal: 2,
        class_name: "CATPrtCont".to_string(),
        base_class: "CATProdCont".to_string(),
        stream_name: stream_name.to_string(),
    }
}

#[test]
fn legacy_parameter_scope_requires_the_exact_modeling_container() {
    let part = binding("part");
    let other_part = binding("other-part");

    assert!(outer_container_in_scope(
        Some(&part),
        LegacyModelingScope::Container(&part)
    ));
    assert!(!outer_container_in_scope(
        Some(&other_part),
        LegacyModelingScope::Container(&part)
    ));
    assert!(!outer_container_in_scope(
        None,
        LegacyModelingScope::Container(&part)
    ));
    assert!(!outer_container_in_scope(
        Some(&part),
        LegacyModelingScope::Unresolved
    ));
}

#[test]
fn legacy_parameter_scope_admits_unbound_fragment_runs() {
    assert!(outer_container_in_scope(
        None,
        LegacyModelingScope::Unbounded
    ));
}
