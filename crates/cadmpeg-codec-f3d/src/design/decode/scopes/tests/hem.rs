// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

/// Field values written into a synthetic gap-and-length `Hem` frame.
struct HemFixture {
    header_shift: usize,
    wrapper: u32,
    settings: u32,
    gap_owner: u32,
    length_owner: u32,
    aggregate_group: u32,
    edge_group: u32,
    bend_radius: f64,
}

/// A synthetic frame plus the offsets the reader is expected to derive.
struct HemFrame {
    bytes: Vec<u8>,
    paired_at: usize,
    bend_radius_offset: u64,
}

#[test]
fn hem_scope_binds_parameters_edge_groups_and_rule_radius() {
    // Groups before owners; roles come from marked slots under both header shifts.
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    for header_shift in [0usize, 4] {
        let frame = hem_frame(&HemFixture {
            header_shift,
            wrapper: 308,
            settings: 311,
            gap_owner: 301,
            length_owner: 304,
            aggregate_group: 240,
            edge_group: 251,
            bend_radius: 0.25,
        });

        let operation = crate::design::decode::scopes::extrude_sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            references.iter().copied(),
            &[(301, "HemGap"), (304, "HemLength")],
        )
        .expect("fixed Hem operation");
        assert_eq!(operation.edge_wrapper_record_index, 308);
        assert_eq!(operation.settings_record_index, 311);
        assert_eq!(
            operation.parameter_owners,
            crate::records::DesignHemParameterOwners::GapLength {
                gap_owner_record_index: 301,
                length_owner_record_index: 304,
            }
        );
        assert_eq!(operation.aggregate_group_record_index, 240);
        assert_eq!(operation.aggregate_operand_record_index, 243);
        assert_eq!(operation.edge_group_record_index, 251);
        assert_eq!(operation.edge_operand_record_index, 254);
        assert_eq!(operation.bend_radius.get(), 0.25);
        assert_eq!(operation.bend_radius_offset, frame.bend_radius_offset);
    }
}

#[test]
fn hem_scope_refuses_a_frame_whose_owner_slot_is_absent() {
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    let mut frame = hem_frame(&HemFixture {
        header_shift: 0,
        wrapper: 308,
        settings: 311,
        gap_owner: 301,
        length_owner: 304,
        aggregate_group: 240,
        edge_group: 251,
        bend_radius: 0.25,
    });
    // Move the length-owner reference one byte later, as the rolled form does.
    let at = 85 + 53;
    frame.bytes[at..at + 11].fill(0);
    frame.bytes[at + 1] = 1;
    frame.bytes[at + 2..at + 6].copy_from_slice(&304u32.to_le_bytes());
    assert!(
        crate::design::decode::scopes::extrude_sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            references.iter().copied(),
            &[(301, "HemGap"), (304, "HemLength")],
        )
        .is_none()
    );
    assert!(
        crate::design::decode::scopes::extrude_sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            references.iter().copied(),
            &[(301, "HemGap"), (301, "HemGap"), (304, "HemLength")],
        )
        .is_none()
    );
}

#[test]
fn hem_scope_reads_the_rolled_owner_layout() {
    let references = [708, 717, 720, 724, 775, 788, 790, 793];
    let frame = rolled_hem_frame();
    let operation = crate::design::decode::scopes::extrude_sheet_metal::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        references.iter().copied(),
        &[(775, "HemRadius"), (788, "HemAngle")],
    )
    .expect("rolled Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 775,
            angle_owner_record_index: 788,
        }
    );
    assert_eq!(operation.bend_radius.get(), 0.25);
    assert_eq!(operation.bend_radius_offset, 160);
}

#[test]
fn hem_scope_reads_the_teardrop_owner_layout() {
    let references = [703, 706, 708, 717, 720, 724, 775, 777, 780];
    let frame = teardrop_hem_frame();
    let operation = crate::design::decode::scopes::extrude_sheet_metal::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        references.iter().copied(),
        &[(703, "HemGap"), (706, "HemLength"), (775, "HemRadius")],
    )
    .expect("teardrop Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 703,
            length_owner_record_index: 706,
            radius_owner_record_index: 775,
        }
    );
    assert_eq!(operation.bend_radius.get(), 0.25);
    assert_eq!(operation.bend_radius_offset, 170);
}

#[test]
fn hem_scope_refuses_an_owner_layout_whose_parameter_kinds_name_another_form() {
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    let frame = hem_frame(&HemFixture {
        header_shift: 0,
        wrapper: 308,
        settings: 311,
        gap_owner: 301,
        length_owner: 304,
        aggregate_group: 240,
        edge_group: 251,
        bend_radius: 0.25,
    });

    assert!(
        crate::design::decode::scopes::extrude_sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            references.iter().copied(),
            &[(301, "HemRadius"), (304, "HemAngle")],
        )
        .is_none()
    );
}

/// Build a gap-and-length `Hem` frame from the settled fixed-section layout.
///
/// Every offset is computed from the layout rather than counted by hand.
fn hem_frame(fixture: &HemFixture) -> HemFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 85 + fixture.header_shift;
    let paired_at = 494 + fixture.header_shift;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, common + 8, fixture.wrapper);
    reference(&mut bytes, common + 19, fixture.settings);
    bytes[common + 30..common + 34].copy_from_slice(&1u32.to_le_bytes());
    bytes[common + 36..common + 40].copy_from_slice(&4u32.to_le_bytes());
    reference(&mut bytes, common + 42, fixture.gap_owner);
    reference(&mut bytes, common + 53, fixture.length_owner);
    let radius_at = common + 71;
    bytes[radius_at..radius_at + 8].copy_from_slice(&fixture.bend_radius.to_le_bytes());
    reference(&mut bytes, common + 108, fixture.aggregate_group);
    reference(&mut bytes, common + 135, fixture.edge_group);

    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

/// Build the rolled `Hem` frame. Its header shift is four bytes and its owner
/// slots are thirteen bytes apart.
fn rolled_hem_frame() -> HemFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 89;
    let paired_at = 498;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, common + 8, 708);
    reference(&mut bytes, common + 19, 724);
    reference(&mut bytes, common + 41, 788);
    reference(&mut bytes, common + 54, 775);
    bytes[common + 71..common + 79].copy_from_slice(&0.25f64.to_le_bytes());
    reference(&mut bytes, common + 108, 717);
    reference(&mut bytes, common + 135, 790);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 160,
    }
}

/// Build the teardrop `Hem` frame. The third parameter owner shifts the group
/// slots by ten bytes and moves the fixed rule radius to offset eighty-one.
fn teardrop_hem_frame() -> HemFrame {
    fn reference(bytes: &mut [u8], at: usize, record_index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let common = 89;
    let paired_at = 519;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, common + 8, 708);
    reference(&mut bytes, common + 19, 724);
    reference(&mut bytes, common + 42, 703);
    reference(&mut bytes, common + 53, 706);
    reference(&mut bytes, common + 64, 775);
    bytes[common + 81..common + 89].copy_from_slice(&0.25f64.to_le_bytes());
    reference(&mut bytes, common + 118, 717);
    reference(&mut bytes, common + 145, 777);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 170,
    }
}
