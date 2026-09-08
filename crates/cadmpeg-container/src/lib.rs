// SPDX-License-Identifier: Apache-2.0
//! Compression and archive support shared by container codecs.

mod archive;
pub mod compound;
pub mod compression;

pub use archive::{ArchiveSnapshot, EntryRecord, PhysicalSpan, SpanRole, ZipCompression};
