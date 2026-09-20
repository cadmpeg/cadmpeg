//! `standard` family record decoders, topology container, and B-rep parsers.

pub(super) mod decode;
pub(crate) mod fbb;
pub(crate) mod records;
pub(crate) mod topology;
mod trim_packet;

#[cfg(test)]
pub(crate) mod tests;
