// SPDX-License-Identifier: Apache-2.0
//! The stored order of a Fusion history chain (f3d.md §3.1, "History chain").

use cadmpeg_asm::history::{delta_state_offsets, read_delta_state_header, DeltaStateHeaderRead};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;

use crate::history::{take_int, PREAMBLE};

/// The `.smbh` history stream of the Fusion archive `archive`.
fn history_stream(archive: &'static [u8]) -> Vec<u8> {
    crate::test_support::with_decode_context(|ctx| {
        let scan =
            crate::container::scan(ctx, View::over_retained(archive)).expect("the archive scans");
        let entry = crate::container::brep_entries(&scan)
            .find(|entry| matches!(entry.role, ContainerRole::BrepSmbh))
            .expect("a history stream");
        scan.entry_bytes(&entry.name)
            .expect("the history stream")
            .to_vec()
    })
}

#[test]
fn a_stored_chain_of_five_states_stands_in_node_index_order() {
    let stream = history_stream(include_bytes!(
        "../../../../../corpus/f3d/combine_tools_n2.f3d"
    ));
    let at = stream
        .windows(PREAMBLE.len())
        .position(|window| window == PREAMBLE)
        .expect("a history_stream preamble");
    let mut position = at + PREAMBLE.len();
    for _ in 0..4 {
        take_int(&stream, &mut position, 0x04, RefWidth::Four).expect("a preamble value");
    }
    let references = (0..4)
        .map(|_| take_int(&stream, &mut position, 0x0c, RefWidth::Four))
        .collect::<Option<Vec<_>>>()
        .expect("four preamble references");
    assert_eq!(references, [-1, 0, 1, -1]);
    // Identity, previous, next and node references in stream order: the
    // head state first, the root state second, and a state between them
    // last.
    let headers = delta_state_offsets(&stream)
        .into_iter()
        .map(|offset| {
            let DeltaStateHeaderRead::Framed(header) =
                read_delta_state_header(&stream, offset, RefWidth::Four)
            else {
                panic!("a framed delta_state at {offset}");
            };
            (
                header.state_id,
                header.previous,
                header.next,
                header.node_index,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        headers,
        [
            (6, -1, 2, 0),
            (2, 3, -1, 1),
            (5, 0, 4, 2),
            (3, 4, 1, 3),
            (4, 2, 3, 4),
        ]
    );
}
