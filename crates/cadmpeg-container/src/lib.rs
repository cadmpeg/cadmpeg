// SPDX-License-Identifier: Apache-2.0
//! Compression and archive support shared by container codecs.

mod archive;
pub mod compound;
pub mod compression;
mod layout;

pub use archive::{
    ArchiveSnapshot, EntryRecord, MemberFrame, MemberPayload, PhysicalSpan, ZipCompression,
    ZipSpanRole,
};

#[cfg(test)]
mod integration_tests;
