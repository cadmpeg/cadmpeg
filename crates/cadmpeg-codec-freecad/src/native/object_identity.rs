// SPDX-License-Identifier: Apache-2.0
//! Native object identities coupled to their persisted names.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectIdentity {
    id: String,
    name: String,
}

impl ObjectIdentity {
    pub(crate) fn from_name(ctx: &DecodeContext<'_>, name: String) -> Result<Self, CodecError> {
        let id = super::native_id_charged(ctx, "object", &name)?;
        Ok(Self { id, name })
    }

    pub(crate) fn try_new(id: String, name: String) -> Result<Self, String> {
        if !id
            .strip_prefix("fcstd:native:object#")
            .is_some_and(|key| key.bytes().eq(super::encoded_segment_bytes(&name)))
        {
            return Err("object id disagrees with persisted name".to_owned());
        }
        Ok(Self { id, name })
    }

    pub(crate) fn id(&self) -> &String {
        &self.id
    }

    pub(crate) fn name(&self) -> &String {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectIdentity;

    #[test]
    fn object_identity_admission_checks_name_derivation_and_source_spellings() {
        for name in ["A", "", "A B#C", "A%20B", "A:B", "é"] {
            let id = crate::native::native_id("object", name);
            let identity =
                ObjectIdentity::try_new(id.clone(), name.to_owned()).expect("encoded name");
            assert_eq!(identity.id(), &id);
            assert_eq!(identity.name(), name);
            crate::test_support::with_service_context(&[], |ctx| {
                assert_eq!(
                    ObjectIdentity::from_name(ctx, name.to_owned()).expect("derived identity"),
                    identity
                );
            });
        }
        for id in ["", "fcstd:native:object#B", "fcstd:native:joint#A"] {
            assert!(ObjectIdentity::try_new(id.to_owned(), "A".to_owned()).is_err());
            let wire = serde_json::json!({"id": id, "name": "A", "type_name": "App::Feature", "persistent_id": null, "view_type": null, "attributes": {}, "dependencies": [], "dependency_allow_partial": null, "order": 0, "raw_xml": null, "byte_start": null, "byte_end": null});
            assert!(serde_json::from_value::<crate::native::ObjectRecord>(wire).is_err());
        }
    }
}
