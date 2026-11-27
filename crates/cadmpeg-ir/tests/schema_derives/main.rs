// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! JSON Schema derive and committed-shape checks for `cadmpeg-ir`.

use schemars::{JsonSchema, SchemaGenerator};

mod design;
mod features;
mod json_schema;
mod ops;
mod process;
mod root;

pub(crate) fn generates<T: JsonSchema>(generator: &mut SchemaGenerator) {
    let schema = T::json_schema(generator);
    assert!(schema.as_object().is_some(), "{}", T::schema_name());
}
