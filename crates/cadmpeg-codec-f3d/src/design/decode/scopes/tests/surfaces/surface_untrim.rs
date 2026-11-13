// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::surfaces::exact_surface_untrim_operation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::surface_ops::{
    DesignSurfaceUntrimLoops, DesignSurfaceUntrimOperation,
};
use crate::test_support::indexed_header;

const SCOPE: u32 = 40;
const OWNER: u32 = 41;
const FACE_GROUP: u32 = 44;
const FACE: u32 = 47;
const LOOP_GROUP: u32 = 50;
const LOOP: u32 = 53;

fn marked(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

fn extension_owner(bytes: &mut Vec<u8>, extension: f64) -> usize {
    let start = bytes.len();
    indexed_header(bytes, *b"394", OWNER);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0]);
    marked(bytes, SCOPE);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&extension.to_le_bytes());
    marked(bytes, OWNER + 1);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    marked(bytes, SCOPE);
    bytes.extend_from_slice(&[0; 2]);
    marked(bytes, OWNER + 2);
    bytes.push(0);
    marked(bytes, SCOPE);
    indexed_header(bytes, *b"258", OWNER);
    start
}

struct UntrimFrame<'a> {
    zero_run: [u8; 10],
    loop_groups: &'a [u32],
    loop_type: u32,
    padded: bool,
    runs: [&'a [u32]; 3],
    table: &'a [u32],
    extension: f64,
}

impl UntrimFrame<'_> {
    fn all_loops() -> Self {
        Self {
            zero_run: [0; 10],
            loop_groups: &[],
            loop_type: 0,
            padded: true,
            runs: [&[], &[], &[FACE_GROUP]],
            table: &[OWNER, FACE_GROUP, FACE],
            extension: 0.3,
        }
    }

    fn manual_loops() -> Self {
        Self {
            loop_groups: &[LOOP_GROUP],
            loop_type: 3,
            table: &[OWNER, FACE_GROUP, FACE, LOOP_GROUP, LOOP],
            ..Self::all_loops()
        }
    }

    fn decode(&self) -> (Option<DesignSurfaceUntrimOperation>, usize) {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"393", SCOPE);
        bytes.extend_from_slice(&self.zero_run);
        bytes.extend_from_slice(&u32::try_from(self.loop_groups.len()).unwrap().to_le_bytes());
        for group in self.loop_groups {
            marked(&mut bytes, *group);
        }
        marked(&mut bytes, OWNER);
        bytes.extend_from_slice(&self.loop_type.to_le_bytes());
        if self.padded {
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        for run in self.runs {
            bytes.extend_from_slice(&u32::try_from(run.len()).unwrap().to_le_bytes());
            for reference in run {
                marked(&mut bytes, *reference);
            }
        }
        bytes.extend_from_slice(&36u32.to_le_bytes());
        for unit in "00000000-0000-0000-0000-000000000000".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 3]);
        let reference_count_offset = bytes.len();
        bytes.extend_from_slice(&u32::try_from(self.table.len()).unwrap().to_le_bytes());
        for reference in self.table {
            marked(&mut bytes, *reference);
        }
        bytes.resize(bytes.len() + 120, 0);
        let frame_length = bytes.len();
        indexed_header(&mut bytes, *b"258", SCOPE);
        let owner_at = extension_owner(&mut bytes, self.extension);
        let mut scope = DesignParameterScope::empty(
            "f3d:test:surface-untrim#1",
            DesignFeatureKind::SurfaceUntrim,
            SCOPE,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(frame_length);
                draft.reference_count_offset = u64_from_index(reference_count_offset);
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(self.table.to_vec());
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        (
            exact_surface_untrim_operation(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
            ),
            owner_at,
        )
    }
}

#[test]
fn every_loop_type_reads_the_extension_and_the_face_group() {
    for (loop_type, loops) in [
        (0, DesignSurfaceUntrimLoops::All),
        (1, DesignSurfaceUntrimLoops::Internal),
        (2, DesignSurfaceUntrimLoops::External),
    ] {
        for padded in [false, true] {
            let (operation, owner_at) = UntrimFrame {
                loop_type,
                padded,
                ..UntrimFrame::all_loops()
            }
            .decode();
            assert_eq!(
                operation,
                Some(DesignSurfaceUntrimOperation {
                    loops,
                    loops_offset: 36,
                    extension: crate::test_support::real(0.3),
                    extension_offset: u64_from_index(owner_at + 40),
                    extension_record_index: OWNER,
                    face_group_record_index: FACE_GROUP,
                    loop_group_record_indices: Vec::new(),
                })
            );
        }
    }
}

#[test]
fn manual_loops_read_the_loop_group_before_the_extension_reference() {
    let (operation, _) = UntrimFrame::manual_loops().decode();
    let operation = operation.unwrap();
    assert_eq!(operation.loops, DesignSurfaceUntrimLoops::Manual);
    assert_eq!(operation.loops_offset, 47);
    assert_eq!(operation.loop_group_record_indices, [LOOP_GROUP]);
    assert_eq!(operation.face_group_record_index, FACE_GROUP);
    assert_eq!(operation.extension_record_index, OWNER);
}

#[test]
fn a_zero_extension_is_admitted_and_a_negative_extension_stays_unread() {
    let zero = UntrimFrame {
        extension: 0.0,
        ..UntrimFrame::all_loops()
    };
    assert_eq!(zero.decode().0.unwrap().extension.get(), 0.0);
    let negative = UntrimFrame {
        extension: -0.3,
        ..UntrimFrame::all_loops()
    };
    assert_eq!(negative.decode().0, None);
}

#[test]
fn frames_outside_the_untrim_grammar_stay_unread() {
    let rejected = [
        UntrimFrame {
            zero_run: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            loop_type: 4,
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            runs: [&[FACE_GROUP], &[], &[]],
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            runs: [&[], &[], &[FACE_GROUP, FACE]],
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            table: &[FACE_GROUP, FACE, OWNER],
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            table: &[OWNER, FACE, FACE_GROUP],
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            table: &[OWNER, FACE_GROUP, FACE],
            ..UntrimFrame::manual_loops()
        },
    ];
    for frame in &rejected {
        assert_eq!(frame.decode().0, None);
    }
}
