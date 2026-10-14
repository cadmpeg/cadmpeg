// SPDX-License-Identifier: Apache-2.0
//! Admission of native identities for neutral history references.

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{Identity, IdentityComponent};

pub(crate) fn neutral_history_id(
    native_id: &str,
    kind: &IdentityComponent,
) -> Result<Identity, CodecError> {
    let native = Identity::new(native_id.to_owned()).map_err(CodecError::malformed)?;
    let (namespace, key) = native
        .as_str()
        .split_once('#')
        .ok_or_else(|| CodecError::malformed("CATIA history identity has no key"))?;
    let (format, rest) = namespace
        .split_once(':')
        .ok_or_else(|| CodecError::malformed("CATIA history identity has no format"))?;
    let (scope, _) = rest
        .split_once(':')
        .ok_or_else(|| CodecError::malformed("CATIA history identity has no scope"))?;
    let derived = format!("{format}:{scope}:{}#{key}", kind.as_str());
    Identity::new(derived).map_err(CodecError::malformed)
}

#[cfg(test)]
mod tests {
    use super::neutral_history_id;
    use cadmpeg_core::CodecError;

    #[test]
    fn history_kind_replacement_preserves_colon_keys() {
        let result = crate::test_support::with_service_context(|ctx| {
            neutral_history_id(
                "catia:graph:object#owner:child",
                &cadmpeg_ir::identity_component!("feature"),
            )
        });
        assert_eq!(
            result.expect("valid native identity").as_str(),
            "catia:graph:feature#owner:child"
        );
    }

    #[test]
    fn malformed_native_history_identity_is_refused() {
        for value in ["short", "catia:graph:object#", "catia:graph:object#a#b"] {
            assert!(matches!(
                crate::test_support::with_service_context(|ctx| neutral_history_id(
                    value,
                    &cadmpeg_ir::identity_component!("feature")
                )),
                Err(CodecError::Malformed(_))
            ));
        }
    }
}
