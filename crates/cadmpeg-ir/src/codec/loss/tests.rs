// SPDX-License-Identifier: Apache-2.0
//! Stale sidecars have a shared decode warning, separate from missing depth.

use crate::codec::DecodeLossCode;
use crate::report::{
    loss::{LossNote, LossTaxonomy},
    Severity,
};

#[test]
fn stale_sidecar_factory_preserves_fixed_code_and_warning() {
    let note = DecodeLossCode::SidecarStale.note("sidecar binding does not match document");
    assert_eq!(note.code.namespace(), "decode");
    assert_eq!(note.code.local_code(), "sidecar.stale");
    assert_eq!(
        note.code.taxonomy(),
        LossTaxonomy::PreservedSourceUnavailable
    );
    assert_eq!(note.severity, Severity::Warning);
    let wire = serde_json::to_string(&note).expect("loss note serializes");
    assert_eq!(
        serde_json::from_str::<LossNote>(&wire).expect("loss note round trips"),
        note
    );
    let missing_depth = DecodeLossCode::DepthNotDelivered {
        delivered: crate::ContentScope::container(),
    }
    .note("missing geometry");
    assert_eq!(missing_depth.code.local_code(), "depth.not-delivered");
    assert_eq!(missing_depth.severity, Severity::Error);
    assert_eq!(missing_depth.code.strict_floor(), Some(Severity::Warning));
}
