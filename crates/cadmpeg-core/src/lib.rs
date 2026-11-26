// SPDX-License-Identifier: Apache-2.0
//! Bounded byte decoding primitives shared by cadmpeg codecs.

pub mod absent_key;
pub mod bytes;
pub mod container;
pub mod convert;
pub mod decode;
pub mod dialect;
pub mod distinct_keys;
pub mod error;
pub mod finite_json;
pub mod io;
pub mod iter_wire;
mod json_macro;
pub mod parallelism;
pub mod printable;
pub mod read_session;
pub mod scan;
pub mod serde_erase;
pub mod tag_only;
pub mod tagged;
pub mod target;
pub mod text;

// `#[derive(cadmpeg_derive::DeserializeTagged)]` names its reader by this path.
#[cfg(test)]
extern crate self as cadmpeg_core;

pub use container::ContainerEntry;
pub use error::CodecError;
pub use io::ReadSeek;
pub use parallelism::parallelism;
#[doc(hidden)]
pub use serde_json as __serde_json;
