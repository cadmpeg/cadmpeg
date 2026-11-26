// SPDX-License-Identifier: Apache-2.0
//! Every unit variant of an enum of `body.rs`, and of the shared enums these
//! operations use, tagged by `kind` has one spelling: `{"kind": name}`.

use std::collections::BTreeMap;
use std::path::Path;

use cadmpeg_core::json;

use crate::design::ops::body::{Heal, InstanceChange, PatternMethod, SplitMethod, TransformMode};
use crate::design::ops::shared::{
    Locus, PathAxisControl, PathPosition, PathReference, PathRollControl,
};
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};

/// §2.5: a unit variant of an adjacently tagged enum of `body.rs` is its
/// `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 3] = [
        ("Heal", reads::<Heal>),
        ("PatternMethod", reads::<PatternMethod>),
        ("SplitMethod", reads::<SplitMethod>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/body.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

/// A unit variant of an enum tagged by `kind` alone is `{"kind": name}` and
/// refuses every other key, `value` among them.
#[test]
fn every_internally_tagged_unit_variant_refuses_another_key() {
    let units: [(&str, Reader); 7] = [
        ("centroid", reads::<Locus>),
        ("plane_normal", reads::<PathReference>),
        ("skipped", reads::<InstanceChange>),
        ("move", reads::<TransformMode>),
        ("free", reads::<PathPosition>),
        ("free", reads::<PathAxisControl>),
        ("free", reads::<PathRollControl>),
    ];
    for (variant, read) in units {
        let bare = json!({"kind": variant});
        assert_eq!(read(&bare).as_ref(), Ok(&bare), "{variant}");
        for stated in [
            json!({"kind": variant, "value": null}),
            json!({"kind": variant, "value": {}}),
            json!({"kind": variant, "zz_bogus": 1}),
        ] {
            read(&stated).expect_err(&format!("{stated} is refused"));
        }
    }
}
