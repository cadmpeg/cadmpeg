// SPDX-License-Identifier: Apache-2.0
//! Bounded byte decoding primitives shared by cadmpeg codecs.

pub mod be;
pub mod container;
pub mod decode;
pub mod error;
pub mod io;
pub mod le;
pub mod read;

pub use container::{ContainerEntry, ContainerSummary};
pub use error::CodecError;
pub use io::ReadSeek;
