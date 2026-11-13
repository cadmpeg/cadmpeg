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

fn extension_owner(bytes: &mut Vec<u8>, extension: f64, scope: u32, ordinal: u32) -> usize {
    let start = bytes.len();
    indexed_header(bytes, *b"394", OWNER);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0]);
    marked(bytes, scope);
    bytes.extend_from_slice(&ordinal.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&extension.to_le_bytes());
    marked(bytes, OWNER + 1);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    marked(bytes, scope);
    bytes.extend_from_slice(&[0; 2]);
    marked(bytes, OWNER + 2);
    bytes.push(0);
    marked(bytes, scope);
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
    owner_count: usize,
    owner_scope: u32,
    owner_ordinal: u32,
    loop_count: Option<u32>,
    truncate_prefix: usize,
    scope_shift: u64,
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
            owner_count: 1,
            owner_scope: SCOPE,
            owner_ordinal: 0,
            loop_count: None,
            truncate_prefix: 0,
            scope_shift: 0,
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
        let (operation, owner_at, _) = self.decode_with_losses();
        (operation, owner_at)
    }

    fn decode_with_losses(
        &self,
    ) -> (
        Option<DesignSurfaceUntrimOperation>,
        usize,
        Vec<cadmpeg_ir::report::loss::LossNote>,
    ) {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"393", SCOPE);
        bytes.extend_from_slice(&self.zero_run);
        bytes.extend_from_slice(
            &self
                .loop_count
                .unwrap_or_else(|| u32::try_from(self.loop_groups.len()).unwrap())
                .to_le_bytes(),
        );
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
        let mut owner_at = bytes.len();
        for index in 0..self.owner_count {
            let offset = extension_owner(
                &mut bytes,
                self.extension,
                self.owner_scope,
                self.owner_ordinal,
            );
            if index == 0 {
                owner_at = offset;
            }
        }
        let mut scope = DesignParameterScope::empty(
            "f3d:test:surface-untrim#1",
            DesignFeatureKind::SurfaceUntrim,
            SCOPE,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(frame_length);
                draft.byte_offset = self.scope_shift;
                draft.reference_count_offset =
                    u64_from_index(reference_count_offset - self.truncate_prefix)
                        + self.scope_shift;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(self.table.to_vec());
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let mut losses = Vec::new();
        let operation = crate::test_support::with_decode_context(|ctx| {
            exact_surface_untrim_operation(
                ctx,
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                &mut losses,
            )
            .unwrap()
        });
        (operation, owner_at, losses)
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
                    extension: Some(crate::test_support::real(0.3)),
                    extension_offset: Some(u64_from_index(owner_at + 40)),
                    extension_record_index: OWNER,
                    face_group_record_index: FACE_GROUP,
                    loop_group_record_indices: Vec::new(),
                    padded,
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
fn zero_and_negative_extensions_retain_the_numeric_owner() {
    let zero = UntrimFrame {
        extension: 0.0,
        ..UntrimFrame::all_loops()
    };
    assert_eq!(zero.decode().0.unwrap().extension.unwrap().get(), 0.0);
    let negative = UntrimFrame {
        extension: -0.3,
        ..UntrimFrame::all_loops()
    };
    assert_eq!(negative.decode().0.unwrap().extension.unwrap().get(), -0.3);
}

#[test]
fn missing_ambiguous_and_wrong_owner_definitions_keep_the_frame() {
    let cases = [
        UntrimFrame {
            owner_count: 0,
            ..UntrimFrame::manual_loops()
        },
        UntrimFrame {
            owner_count: 2,
            ..UntrimFrame::manual_loops()
        },
        UntrimFrame {
            owner_scope: 90,
            ..UntrimFrame::manual_loops()
        },
        UntrimFrame {
            owner_ordinal: 1,
            ..UntrimFrame::manual_loops()
        },
    ];
    for frame in cases {
        let (operation, _, losses) = frame.decode_with_losses();
        let operation = operation.unwrap();
        assert_eq!(operation.loops, DesignSurfaceUntrimLoops::Manual);
        assert_eq!(operation.loop_group_record_indices, [50]);
        assert_eq!(operation.face_group_record_index, 44);
        assert_eq!(operation.extension, None);
        assert_eq!(operation.extension_offset, None);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.surface-untrim-extension-unresolved"
        );
    }
}

#[test]
fn impossible_counts_and_out_of_range_prefixes_stay_bounded() {
    for frame in [
        UntrimFrame {
            loop_count: Some(u32::MAX),
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            truncate_prefix: 1,
            ..UntrimFrame::all_loops()
        },
        UntrimFrame {
            scope_shift: 100_000,
            ..UntrimFrame::all_loops()
        },
    ] {
        assert!(frame.decode().0.is_none());
    }
}

#[test]
fn unpadded_closed_runs_charge_their_named_assumption() {
    let (operation, _, losses) = UntrimFrame {
        padded: false,
        ..UntrimFrame::manual_loops()
    }
    .decode_with_losses();
    assert!(!operation.unwrap().padded);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.surface-untrim-padding-assumed"
    );
}

#[test]
fn counted_manual_loop_references_keep_their_order() {
    let operation = UntrimFrame {
        loop_groups: &[50, 60],
        table: &[41, 44, 47, 50, 53, 60, 63],
        ..UntrimFrame::manual_loops()
    }
    .decode()
    .0
    .unwrap();
    assert_eq!(operation.loop_group_record_indices, [50, 60]);
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
