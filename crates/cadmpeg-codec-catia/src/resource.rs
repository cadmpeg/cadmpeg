// SPDX-License-Identifier: Apache-2.0
//! Identity, source-attribute, and support-context helpers for CATIA decode.

use std::collections::BTreeMap;

use cadmpeg_core::decode::index_from_u32;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

pub(crate) fn source_attribute(
    attributes: &mut BTreeMap<NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let key = NonBlankString::new(key.to_string())
        .ok_or_else(|| CodecError::malformed("CATIA source attribute key is blank"))?;
    attributes.insert(key, value.to_string());
    Ok(())
}

pub(crate) fn compose_index_id<T>(
    namespace: &cadmpeg_ir::ids::IdentityNamespace,
    index: usize,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
) -> Result<T, CodecError> {
    let id = format!(
        "{}:{}:{}#{index}",
        namespace.format(),
        namespace.scope(),
        namespace.kind()
    );
    construct(id).map_err(CodecError::malformed)
}

pub(crate) fn compose_u32_id<T>(
    namespace: &cadmpeg_ir::ids::IdentityNamespace,
    value: u32,
    construct: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
) -> Result<T, CodecError> {
    compose_index_id(namespace, index_from_u32(value), construct)
}

#[cfg(test)]
mod id_format_tests {
    #[test]
    fn b5_numeric_identity_preserves_spelling() {
        use cadmpeg_ir::ids::CurveId;

        let namespace = cadmpeg_ir::identity_namespace!("catia", "b5", "profile");
        let id = super::compose_u32_id(&namespace, 42, CurveId::mint)
            .expect("identity spelling is valid");
        assert_eq!(id.as_str(), "catia:b5:profile#42");
    }
}
