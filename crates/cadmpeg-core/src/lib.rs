// SPDX-License-Identifier: Apache-2.0
//! Bounded byte decoding primitives shared by cadmpeg codecs.

pub mod bytes;
pub mod container;
pub mod decode;
pub mod dialect;
pub mod error;
pub mod io;
pub mod target;

pub use container::{ContainerEntry, ContainerSummary};
pub use error::CodecError;
pub use io::ReadSeek;
