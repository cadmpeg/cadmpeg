// SPDX-License-Identifier: Apache-2.0

use super::{declares_assembly, member_document, member_transform};
use crate::assembly::{Member, MemberKind};

fn member(kind: MemberKind, name: &str, family_name: &str) -> Member {
    Member {
        offset: 0,
        id: 1,
        kind,
        name: name.to_owned(),
        family_name: family_name.to_owned(),
        frame: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    }
}

#[test]
fn the_header_kind_token_declares_an_assembly() {
    assert!(declares_assembly("#UGC:2 ASSEMBLY 2562 2120 802"));
    assert!(!declares_assembly("#UGC:2 PART 2562 2120 802"));
    assert!(!declares_assembly("#UGC:2 PART/SHEETMETAL 2562"));
}

#[test]
fn rows_name_their_child_model_file() {
    assert_eq!(
        member_document(&member(MemberKind::Assembly, "SUB", "SUB")),
        Some("SUB.asm".to_owned())
    );
    assert_eq!(
        member_document(&member(MemberKind::Part, "BRACKET", "BRACKET")),
        Some("BRACKET.prt".to_owned())
    );
    assert_eq!(
        member_document(&member(MemberKind::Part, "BRACKET_A", "BRACKET")),
        None
    );
    assert_eq!(
        member_document(&member(MemberKind::Other(3), "X", "X")),
        None
    );
}

#[test]
fn frame_columns_are_the_child_axes_and_origin() {
    let transform = member_transform(&[
        0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, -355.0, 2.5, 619.0,
    ])
    .expect("rigid placement");
    assert_eq!(
        transform.affine_rows(),
        [
            [0.0, -1.0, 0.0, -355.0],
            [1.0, 0.0, 0.0, 2.5],
            [0.0, 0.0, 1.0, 619.0],
        ]
    );
}

#[test]
fn mirrored_or_scaled_frames_are_not_placements() {
    assert_eq!(
        member_transform(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0]),
        None
    );
    assert_eq!(
        member_transform(&[2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
        None
    );
}
