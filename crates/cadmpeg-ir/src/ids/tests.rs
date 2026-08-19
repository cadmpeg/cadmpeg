// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{format_identity, is_valid_identity, IdentityError};
use crate::examples::unit_cube;
use crate::report::Check;
use crate::validate::validate_neutral;

#[test]
fn three_component_ids_are_valid() {
    assert!(is_valid_identity("step:file:signature#0"));
    assert!(format_identity("step", "file", "signature", 0u8).is_ok());
}

#[test]
fn two_component_ids_are_rejected() {
    assert!(!is_valid_identity("step:signature#0"));
    assert!(matches!(
        format_identity("step", "", "signature", 0u8),
        Err(IdentityError::InvalidComponent { label: "scope", .. })
    ));
}

#[test]
fn typed_ids_keep_their_canonical_json_string_shape() {
    let id = crate::ids::BodyId("test:model:body#1".into());
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"test:model:body#1\"");
    assert_eq!(
        serde_json::from_str::<crate::ids::BodyId>("\"test:model:body#1\"").unwrap(),
        id
    );
}

#[test]
fn entity_ids_follow_canonical_grammar() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = "synthetic:scope:point".into();
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Identity
            && finding.entity.as_deref() == Some("synthetic:scope:point")
            && finding.message.contains("<format>:<scope>:<kind>#<key>")
    }));
}
