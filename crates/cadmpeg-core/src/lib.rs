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
pub mod io;
pub mod iter_wire;
pub mod printable;
pub mod scan;
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
