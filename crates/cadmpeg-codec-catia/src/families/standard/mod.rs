//! `standard` family record decoders, topology container, and B-rep parsers.

pub mod decode;
pub mod fbb;
pub mod records;
pub mod topology;
pub(crate) mod trim_packet;

#[cfg(test)]
mod tests;
