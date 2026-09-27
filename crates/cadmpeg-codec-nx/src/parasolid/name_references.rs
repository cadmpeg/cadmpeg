// SPDX-License-Identifier: Apache-2.0
//! Nonempty type-99 field-name references.

use crate::framing::xmt_reference::NonNullXmt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<NonNullXmt>")]
pub(crate) struct NameReferences(Vec<NonNullXmt>);

impl Serialize for NameReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl TryFrom<Vec<NonNullXmt>> for NameReferences {
    type Error = &'static str;
    fn try_from(values: Vec<NonNullXmt>) -> Result<Self, Self::Error> {
        if values.is_empty() {
            return Err("name_xmts: must contain at least one reference");
        }
        Ok(Self(values))
    }
}

#[cfg(test)]
std::thread_local! {
    static NAME_REFERENCES_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<NameReferences> for Vec<NonNullXmt> {
    fn from(values: NameReferences) -> Self {
        NAME_REFERENCES_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        values.0
    }
}

impl NameReferences {
    pub(crate) fn as_slice(&self) -> &[NonNullXmt] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{NameReferences, NAME_REFERENCES_INTO_WIRE_COUNT};
    #[test]
    fn field_names_preserve_numeric_wire_and_reject_empty_or_null_references() {
        let wire = "[2,4294967295]";
        let names: NameReferences = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&names).unwrap(), wire);
        assert_eq!(
            serde_json::to_vec(&names).unwrap(),
            serde_json::to_vec(&Vec::<crate::framing::xmt_reference::NonNullXmt>::from(
                names.clone()
            ))
            .unwrap()
        );
        for wire in ["[]", "[0]", "[1]", "[2,1]"] {
            assert!(serde_json::from_str::<NameReferences>(wire).is_err());
        }
    }

    #[test]
    fn name_references_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            name_xmts: &'a NameReferences,
        }
        let names: NameReferences = serde_json::from_str("[2,4294967295]").unwrap();
        let record = Record {
            id: "nx:parasolid:name-references#0",
            name_xmts: &names,
        };
        NAME_REFERENCES_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id":"nx:parasolid:name-references#0", "name_xmts":[2,4_294_967_295_u32]}),
        );
        NAME_REFERENCES_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
