// SPDX-License-Identifier: Apache-2.0
//! Wire forms, strict round trips, canonical expression text, member
//! identity, depth bounds, and schemas of the design module.

mod bounds;
mod expressions;
mod fixtures;
mod identity_rewrite;
mod root;
mod root_fixtures;
mod round_trip;
#[cfg(feature = "schema")]
mod schema;
mod state_chains;
pub(in crate::design) mod unit_variants;
mod wire_forms;

use cadmpeg_core::serde_erase::json;
use std::fmt::Debug;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::feature::{FeatureOperation, OperationFamily};
use super::read;

/// Wraps each of `operations` in its `FeatureOperation` arm and requires the
/// arm to write `{"kind": family, "value": operation}`, to read back as the
/// same value and the same bytes, and to refuse its kind without a value.
/// Returns a line for each failure.
pub(in crate::design) fn arm_failures<T: Serialize>(
    family: OperationFamily,
    operations: Vec<T>,
    arm: fn(Box<T>) -> FeatureOperation,
) -> Vec<String> {
    assert!(!operations.is_empty(), "{family:?} has no sample");
    let kind = to_value(&family);
    let mut failures = Vec::new();
    if read_desktop::<FeatureOperation>(&serde_json::json!({ "kind": kind }).to_string()).is_ok() {
        failures.push(format!("{family:?}: the kind alone was read"));
    }
    for operation in operations {
        let expected = serde_json::json!({"kind": kind, "value": to_value(&operation)});
        let wrapped = arm(Box::new(operation));
        if to_value(&wrapped) != expected {
            failures.push(format!(
                "{family:?}: {} is not {expected}",
                to_value(&wrapped)
            ));
        }
        if let Err(error) = round_trip(&wrapped) {
            failures.push(format!("{family:?}: {error}"));
        } else if read_desktop::<FeatureOperation>(&expected.to_string()).ok() != Some(wrapped) {
            failures.push(format!("{family:?}: {expected} read back as another value"));
        }
    }
    failures
}

/// Writes `value`, reads it back strictly under the desktop ceilings, writes
/// it again, and requires the same bytes.
fn round_trip<T: Serialize + DeserializeOwned>(value: &T) -> Result<(), String> {
    let first = json::to_string(value).map_err(|error| error.to_string())?;
    let back: T = read::from_str(&first, &ResourceLimits::desktop())
        .map_err(|error| format!("{first}: {error}"))?;
    let second = json::to_string(&back).map_err(|error| error.to_string())?;
    if first == second {
        Ok(())
    } else {
        Err(format!("{first} wrote back as {second}"))
    }
}

fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
    json::to_value(value).unwrap()
}

fn read_desktop<T: DeserializeOwned>(text: &str) -> Result<T, CodecError> {
    read::from_str(text, &ResourceLimits::desktop())
}

/// Reads `text`, requires a refusal, and returns its message.
fn refuses<T: DeserializeOwned + Debug>(text: &str) -> String {
    match read_desktop::<T>(text) {
        Ok(value) => panic!("{text} was read as {value:?}"),
        Err(error) => error.to_string(),
    }
}
