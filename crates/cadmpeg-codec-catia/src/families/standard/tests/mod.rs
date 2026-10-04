//! Behavioral tests for standard B-rep topology solvers and parsers.

fn triangle_packet(handles: [u16; 3]) -> Vec<u8> {
    let mut bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for handle in handles {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes
}

mod body_orientation;
mod coordinate_closure;
mod incidence_reconstruction;
mod record_decoders;
mod trim;
