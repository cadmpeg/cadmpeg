// SPDX-License-Identifier: Apache-2.0
//! Absent-only reads for optional wire keys.
//!
//! An `Option<T>` field written with `skip_serializing_if = "Option::is_none"`
//! writes nothing for `None`, so absence is the one spelling the writer
//! produces. Serde's own `Option` read also admits `null`, which gives the same
//! state a second spelling. [`present`] closes that: the key, when stated,
//! states a value.

use serde::Deserialize;

/// Read a stated optional key. `null` is refused; absence is `None` through the
/// field's `default`.
///
/// # Errors
///
/// Returns the inner type's own error, and refuses `null` by name.
pub fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a stated value, or the key left out")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Err(E::custom(
                "this key states a value or is left out; it does not state null",
            ))
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Err(E::custom(
                "this key states a value or is left out; it does not state null",
            ))
        }

        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            T::deserialize(deserializer).map(Some)
        }
    }

    deserializer.deserialize_option(Visitor(std::marker::PhantomData))
}
