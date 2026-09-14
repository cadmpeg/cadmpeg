// SPDX-License-Identifier: Apache-2.0
//! Absent-only reads for optional wire keys.
//!
//! An `Option<T>` field written with `skip_serializing_if = "Option::is_none"`
//! writes nothing for `None`, so absence is the one spelling the writer
//! produces. Serde's own `Option` read also admits `null`, which gives the same
//! state a second spelling. [`present`] closes that: the key, when stated,
//! states a value.
//!
//! A field with no `skip_serializing_if` is the mirror case: the writer always
//! states the key, so `null` is its spelling of `None` and an absent key is no
//! spelling at all. [`nullable`] reads that key, and the reading declaration
//! states no `default`, so serde names the field when it is left out.

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

/// Read a required optional key whose writer spells `None` as `null`.
///
/// A field with no `skip_serializing_if` writes the key for every value, so
/// `null` is that key's own spelling of `None`. The reading declaration states
/// no `default`, so an absent key is a missing field named in the error and
/// `null` is the one spelling of `None`.
///
/// The helper exists so every optional key on a read type states which
/// spelling its writer produces: [`present`] where the writer omits the key,
/// this where the writer always states it. A field that states neither is an
/// undeclared key, and the wire-crate census names it.
///
/// # Errors
///
/// Returns the inner type's own error.
pub fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
