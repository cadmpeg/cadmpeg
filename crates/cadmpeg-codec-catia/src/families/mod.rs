//! Per-family CATIA record decoders.
//!
//! Each family owns a `records` module holding its record vocabulary: the
//! struct/enum types it produces and the parser functions that decode them.

pub mod a5a8;
pub mod b2;
pub mod consolidated;
pub mod e5;
pub mod standard;
pub mod zero_entity;
