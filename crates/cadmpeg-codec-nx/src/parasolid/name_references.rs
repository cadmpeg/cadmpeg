// SPDX-License-Identifier: Apache-2.0
//! Nonempty type-99 field-name references.

use serde::{Deserialize, Serialize};
use crate::framing::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<NonNullXmt>", into = "Vec<NonNullXmt>")]
pub(crate) struct NameReferences(Vec<NonNullXmt>);

impl TryFrom<Vec<NonNullXmt>> for NameReferences {
    type Error = &'static str;
    fn try_from(values: Vec<NonNullXmt>) -> Result<Self, Self::Error> {
        if values.is_empty() { return Err("name_xmts: must contain at least one reference"); }
        Ok(Self(values))
    }
}

impl From<NameReferences> for Vec<NonNullXmt> {
    fn from(values: NameReferences) -> Self { values.0 }
}

impl NameReferences {
    pub(crate) fn as_slice(&self) -> &[NonNullXmt] { &self.0 }
}

#[cfg(test)]
mod tests {
    use super::NameReferences;
    #[test]
    fn field_names_preserve_numeric_wire_and_reject_empty_or_null_references() {
        let wire = "[2,4294967295]";
        let names: NameReferences = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&names).unwrap(), wire);
        for wire in ["[]", "[0]", "[1]", "[2,1]"] {
            assert!(serde_json::from_str::<NameReferences>(wire).is_err());
        }
    }
}
