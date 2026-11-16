// SPDX-License-Identifier: Apache-2.0
//! The one spelling of the shared operation types' unit variants.

use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};
use crate::scalar::Length;

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 3] = [
        ("Interpolation", reads::<Interpolation>),
        ("Limit", reads::<Limit<Length>>),
        ("OffsetJoin", reads::<OffsetJoin>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/shared.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}
