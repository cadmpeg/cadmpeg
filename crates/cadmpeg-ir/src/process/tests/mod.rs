// SPDX-License-Identifier: Apache-2.0
//! Wire forms, strict round trips, state chains, and schemas of the process
//! module.

mod fixtures;
mod nesting;
mod root;
mod root_fixtures;
mod round_trip;
mod state_chains;
mod unit_variants;
mod wire_forms;

use cadmpeg_core::serde_erase::json;
use std::fmt::Debug;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::design::read;

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
