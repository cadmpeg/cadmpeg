// SPDX-License-Identifier: Apache-2.0
//! Admission of native identities for neutral history references.

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{Identity, IdentityComponent};

pub(crate) fn neutral_history_id(
    native_id: &str,
    kind: &IdentityComponent,
) -> Result<Identity, CodecError> {
    Identity::new(native_id)
        .map(|identity| identity.with_kind(kind))
        .map_err(CodecError::malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_kind_replacement_preserves_colon_keys() {
        let result = neutral_history_id(
            "catia:graph:object#owner:child",
            &cadmpeg_ir::identity_component!("feature"),
        );
        assert_eq!(
            result.expect("valid native identity").as_str(),
            "catia:graph:feature#owner:child"
        );
    }

    #[test]
    fn malformed_native_history_identity_is_refused() {
        for value in ["short", "catia:graph:object#", "catia:graph:object#a#b"] {
            assert!(matches!(
                neutral_history_id(value, &cadmpeg_ir::identity_component!("feature")),
                Err(CodecError::Malformed(_))
            ));
        }
    }
}
