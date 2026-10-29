// SPDX-License-Identifier: Apache-2.0
//! Bounded byte processing primitives shared by cadmpeg codecs.

pub mod absent_key;
pub mod bytes;
pub mod container;
pub mod convert;
pub mod decode;
pub mod dialect;
pub mod distinct_keys;
pub mod encode;
pub mod error;
pub mod io;
mod operation_budget;
pub mod target;
pub mod text;

pub use container::ContainerEntry;
pub use error::CodecError;
pub use io::ReadSeek;
