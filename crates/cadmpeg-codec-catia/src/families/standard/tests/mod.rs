//! Behavioral tests for standard B-rep topology solvers and parsers.

pub(super) use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

fn repeated_domain(domain: HashSet<usize>, count: usize) -> Vec<Arc<HashSet<usize>>> {
    let domain = Arc::new(domain);
    vec![domain; count]
}

fn sparse_degrees(faces: &[&[u8]]) -> Vec<BTreeMap<usize, u8>> {
    faces
        .iter()
        .map(|degrees| {
            degrees
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(point, degree)| (degree != 0).then_some((point, degree)))
                .collect()
        })
        .collect()
}

fn triangle_packet(handles: [u16; 3]) -> Vec<u8> {
    let mut bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for handle in handles {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes
}

mod coordinate_closure;
mod incidence_components;
mod incidence_search;
mod mesh_quotient;
mod record_decoders;
mod trim;
