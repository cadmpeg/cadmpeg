// SPDX-License-Identifier: Apache-2.0
//! The readers of a unit variant in an enum tagged by `kind`.
//!
//! A unit variant of such an enum is the object `{"kind": name}`. In an enum
//! tagged by `kind` alone, serde reads it by its tag and skips every other key
//! of the object, even under `deny_unknown_fields`, so `{"kind": name,
//! "extra": 1}` would read as the same value. Each such variant reads through
//! [`read`], which refuses every key beside the tag by name.
//!
//! In an enum tagged by `kind` with content under `value`, serde reads a unit
//! variant's `value` as a unit, so `{"kind": name, "value": null}` would read
//! as the same value. Each such variant reads through [`refuse_value`].

use serde::de::{Deserialize, Deserializer, Error};

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

/// Refuses the `value` key of a unit variant in an enum tagged by `kind` with
/// content under `value`. serde calls it only when the key is stated.
///
/// # Errors
///
/// Always.
pub fn refuse_value<'de, D>(_deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    Err(D::Error::custom(
        "a unit variant states its kind alone; it does not state value",
    ))
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

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(
        tag = "kind",
        content = "value",
        rename_all = "snake_case",
        deny_unknown_fields
    )]
    enum AdjacentProbe {
        #[serde(deserialize_with = "super::refuse_value")]
        Empty,
        Count(u32),
    }

    #[test]
    fn a_unit_variant_reads_its_tag_and_refuses_every_other_key() {
        let empty = crate::json!({"kind": "empty"});
        assert_eq!(
            crate::serde_erase::json::to_value(Probe::Empty).expect("a unit variant serializes"),
            empty
        );
        assert_eq!(
            crate::serde_erase::json::from_value::<Probe>(empty).expect("the bare tag reads"),
            Probe::Empty
        );
        let error = crate::serde_erase::json::from_value::<Probe>(crate::json!({
            "kind": "empty", "zz_bogus": 1
        }))
        .expect_err("a key beside the tag is refused")
        .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
        assert_eq!(
            crate::serde_erase::json::from_value::<Probe>(
                crate::json!({"kind": "full", "count": 2})
            )
            .expect("a struct variant reads"),
            Probe::Full { count: 2 }
        );
    }

    #[test]
    fn an_adjacent_unit_variant_reads_its_tag_and_refuses_a_stated_value() {
        let empty = crate::json!({"kind": "empty"});
        assert_eq!(
            crate::serde_erase::json::to_value(AdjacentProbe::Empty)
                .expect("a unit variant serializes"),
            empty
        );
        assert_eq!(
            crate::serde_erase::json::from_value::<AdjacentProbe>(empty)
                .expect("the bare tag reads"),
            AdjacentProbe::Empty
        );
        for value in [serde_json::Value::Null, crate::json!({}), crate::json!([])] {
            for text in [
                format!(r#"{{"kind": "empty", "value": {value}}}"#),
                format!(r#"{{"value": {value}, "kind": "empty"}}"#),
            ] {
                let error = serde_json::from_str::<AdjacentProbe>(&text)
                    .expect_err("a stated value is refused")
                    .to_string();
                assert!(error.contains("does not state value"), "{text}: {error}");
            }
        }
        assert_eq!(
            crate::serde_erase::json::from_value::<AdjacentProbe>(crate::json!({
                "kind": "count", "value": 2
            }))
            .expect("a newtype variant reads"),
            AdjacentProbe::Count(2)
        );
    }
}
