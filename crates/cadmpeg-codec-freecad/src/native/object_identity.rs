// SPDX-License-Identifier: Apache-2.0
//! Opaque native object identities and their independently persisted names.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectIdentity {
    id: String,
    name: String,
}

impl ObjectIdentity {
    pub(crate) fn from_name(name: String) -> Self {
        let id = super::native_id("object", &name);
        Self { id, name }
    }

    pub(crate) fn try_new(id: String, name: String) -> Result<Self, String> {
        if !cadmpeg_ir::ids::is_valid_identity(&id) {
            return Err("object id is not a valid identity".to_owned());
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
            assert_eq!(ObjectIdentity::from_name(name.to_owned()), identity);
        }
        // §209 identities are opaque; only source birth derives a spelling.
        for id in [
            "fcstd:native:object#B",
            "fcstd:native:joint#A",
            "authored:graph:node#owner",
        ] {
            let identity = ObjectIdentity::try_new(id.to_owned(), "A".to_owned()).unwrap();
            assert_eq!(identity.id(), id);
            assert_eq!(identity.name(), "A");
            let wire = serde_json::json!({"id": id, "name": "A", "type_name": "App::Feature", "persistent_id": null, "view_type": null, "attributes": {}, "dependencies": [], "dependency_allow_partial": null, "order": 0});
            let record = serde_json::from_value::<crate::native::ObjectRecord>(wire).unwrap();
            assert_eq!(record.id(), id);
            assert_eq!(record.name(), "A");
            // #212 keeps original source evidence in fidelity, never native wire.
            for field in ["raw_xml", "byte_start", "byte_end"] {
                let mut retired = serde_json::to_value(&record).unwrap();
                retired[field] = serde_json::Value::Null;
                assert!(
                    serde_json::from_value::<crate::native::ObjectRecord>(retired)
                        .unwrap_err()
                        .to_string()
                        .contains(&format!("unknown field `{field}`"))
                );
            }
        }
        for id in ["", "invalid identity", "unqualified"] {
            assert!(ObjectIdentity::try_new(id.to_owned(), "A".to_owned()).is_err());
            let wire = serde_json::json!({"id": id, "name": "A", "type_name": "App::Feature", "persistent_id": null, "view_type": null, "attributes": {}, "dependencies": [], "dependency_allow_partial": null, "order": 0});
            assert!(serde_json::from_value::<crate::native::ObjectRecord>(wire).is_err());
        }
    }
}
