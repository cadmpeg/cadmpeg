//! Shared low-level byte-reading primitives for the CATIA codec.
//!
//! `wire` owns the checked [`Cursor`] and the compact-int / reference-token
//! readers shared by the `b5` and `e5` families.

pub(crate) mod bytes;
pub(crate) mod cursor;
pub(crate) mod records;
pub(crate) mod tokens;

pub(crate) use tokens::{compact_uint, counted_refs, object_ref};
