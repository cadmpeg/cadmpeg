// SPDX-License-Identifier: Apache-2.0
//! The reader of a unit variant in an enum tagged by `kind` alone.
//!
//! A unit variant of such an enum is the object `{"kind": name}`. serde reads
//! it by its tag and skips every other key of the object, even under
//! `deny_unknown_fields`, so `{"kind": name, "extra": 1}` would read as the
//! same value. Each such variant reads through [`read`], which refuses every
//! key beside the tag by name.

use serde::de::{Deserialize, Deserializer};

/// Reads the keys beside a unit variant's tag, refusing each one.
///
/// # Errors
///
/// Names the first key the object states beside its tag.
pub fn read<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TagOnly {}

    TagOnly::deserialize(deserializer).map(|TagOnly {}| ())
}

#[cfg(test)]
mod tests {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    enum Probe {
        #[serde(deserialize_with = "super::read")]
        Empty,
        Full {
            count: u32,
        },
    }

    #[test]
    fn a_unit_variant_reads_its_tag_and_refuses_every_other_key() {
        let empty = serde_json::json!({"kind": "empty"});
        assert_eq!(
            serde_json::to_value(Probe::Empty).expect("a unit variant serializes"),
            empty
        );
        assert_eq!(
            serde_json::from_value::<Probe>(empty).expect("the bare tag reads"),
            Probe::Empty
        );
        let error = serde_json::from_value::<Probe>(serde_json::json!({
            "kind": "empty", "zz_bogus": 1
        }))
        .expect_err("a key beside the tag is refused")
        .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
        assert_eq!(
            serde_json::from_value::<Probe>(serde_json::json!({"kind": "full", "count": 2}))
                .expect("a struct variant reads"),
            Probe::Full { count: 2 }
        );
    }
}
