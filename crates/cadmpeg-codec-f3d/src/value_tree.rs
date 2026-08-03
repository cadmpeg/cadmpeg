// SPDX-License-Identifier: Apache-2.0
//! Rebuild a typed value from a rewritten [`serde_value::Value`] tree.
//!
//! Rescoping identities and retaining a reachable subgraph both serialize a
//! typed value into an untyped tree, edit the tree, and rebuild the typed value
//! from it. [`serde_value::Value`] holds an `f64` directly, so a non-finite
//! coordinate survives the rebuild; JSON has no such number.
//!
//! [`from_value`] names [`serde_json::Error`] as the deserializer error type.
//! Serde buffers `tag`, `untagged`, and `flatten` members through its internal
//! `ContentDeserializer<E>`, which is monomorphized once for every distinct `E`
//! that reaches the whole IR type surface. Naming the error type the `.cadir`
//! JSON reader already uses keeps one such family in the binary instead of two.
//! The error type carries no encoding, so this does not put the rebuild through
//! JSON.

use serde::de::DeserializeOwned;
use serde_value::{Value, ValueDeserializer};

/// Rebuild `T` from an untyped value tree.
pub(crate) fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, serde_json::Error> {
    T::deserialize(ValueDeserializer::<serde_json::Error>::new(value))
}
