// SPDX-License-Identifier: Apache-2.0

use crate::test_support::write_marked_reference;

fn reference_run(references: &[u32]) -> crate::records::identity::ReferenceRun<u32> {
    crate::records::identity::ReferenceRun::unlocated(references.to_vec())
}

fn located_reference_run(references: &[u32]) -> crate::records::identity::ReferenceRun<u32> {
    crate::records::identity::ReferenceRun::located(
        references
            .iter()
            .map(|value| crate::records::identity::Located {
                value: *value,
                offset: 0_u64,
            })
            .collect(),
    )
}

fn source_kinds<'a>(
    kinds: &'a [(u32, &'a str)],
) -> impl Fn(u32, &str) -> Result<bool, cadmpeg_core::CodecError> + 'a {
    move |record_index, expected| {
        let mut matches = kinds.iter().filter(|(owner, _)| *owner == record_index);
        Ok(matches.next().is_some_and(|(_, kind)| *kind == expected) && matches.next().is_none())
    }
}

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

        let operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &cadmpeg_test_support::service_decode_context(),
            &frame.bytes,
            0,
            frame.paired_at,
            &reference_run(&references),
            &source_kinds(&[(301, "HemGap"), (304, "HemLength")]),
        )
        .unwrap()
        .expect("fixed Hem operation");
        let located_references = located_reference_run(&references);
        let located_operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &cadmpeg_test_support::service_decode_context(),
            &frame.bytes,
            0,
            frame.paired_at,
            &located_references,
            &source_kinds(&[(301, "HemGap"), (304, "HemLength")]),
        )
        .unwrap()
        .expect("fixed Hem operation with located references");
        assert_eq!(located_operation, operation);
        assert_eq!(operation.edge_wrapper_record_index, 308);
        assert_eq!(operation.settings_record_index, 311);
        assert_eq!(
            operation.parameter_owners,
            crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLength {
                gap_owner_record_index: 301,
                length_owner_record_index: 304,
            }
        );
        assert_eq!(operation.aggregate_group_record_index.get(), 240);
        assert_eq!(operation.aggregate_operand_record_index(), 243);
        assert_eq!(operation.edge_group_record_index.get(), 251);
        assert_eq!(operation.edge_operand_record_index(), 254);
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
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &cadmpeg_test_support::service_decode_context(),
            &frame.bytes,
            0,
            frame.paired_at,
            &reference_run(&references),
            &source_kinds(&[(301, "HemGap"), (304, "HemLength")]),
        )
        .unwrap()
        .is_none()
    );
    assert!(
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &cadmpeg_test_support::service_decode_context(),
            &frame.bytes,
            0,
            frame.paired_at,
            &reference_run(&references),
            &source_kinds(&[(301, "HemGap"), (301, "HemGap"), (304, "HemLength")]),
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn hem_scope_reads_the_rolled_owner_layout() {
    let references = [708, 717, 720, 724, 775, 788, 790, 793];
    let frame = rolled_hem_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &cadmpeg_test_support::service_decode_context(),
        &frame.bytes,
        0,
        frame.paired_at,
        &reference_run(&references),
        &source_kinds(&[(775, "HemRadius"), (788, "HemAngle")]),
    )
    .unwrap()
    .expect("rolled Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::feature::sheet_metal::DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 775,
            angle_owner_record_index: 788,
        }
    );
    assert_eq!(operation.bend_radius.get(), 0.25);
    assert_eq!(operation.bend_radius_offset, 160);
    let located_operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &cadmpeg_test_support::service_decode_context(),
        &frame.bytes,
        0,
        frame.paired_at,
        &located_reference_run(&references),
        &source_kinds(&[(775, "HemRadius"), (788, "HemAngle")]),
    )
    .unwrap()
    .expect("rolled Hem operation with located references");
    assert_eq!(located_operation, operation);
}

#[test]
fn hem_scope_reads_the_teardrop_owner_layout() {
    let references = [703, 706, 708, 717, 720, 724, 775, 777, 780];
    let frame = teardrop_hem_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &cadmpeg_test_support::service_decode_context(),
        &frame.bytes,
        0,
        frame.paired_at,
        &reference_run(&references),
        &source_kinds(&[(703, "HemGap"), (706, "HemLength"), (775, "HemRadius")]),
    )
    .unwrap()
    .expect("teardrop Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 703,
            length_owner_record_index: 706,
            radius_owner_record_index: 775,
        }
    );
    assert_eq!(operation.bend_radius.get(), 0.25);
    assert_eq!(operation.bend_radius_offset, 170);
    let located_operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &cadmpeg_test_support::service_decode_context(),
        &frame.bytes,
        0,
        frame.paired_at,
        &located_reference_run(&references),
        &source_kinds(&[(703, "HemGap"), (706, "HemLength"), (775, "HemRadius")]),
    )
    .unwrap()
    .expect("teardrop Hem operation with located references");
    assert_eq!(located_operation, operation);
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
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &cadmpeg_test_support::service_decode_context(),
            &frame.bytes,
            0,
            frame.paired_at,
            &reference_run(&references),
            &source_kinds(&[(301, "HemRadius"), (304, "HemAngle")]),
        )
        .unwrap()
        .is_none()
    );
}

/// Build a gap-and-length `Hem` frame from the settled fixed-section layout.
///
/// Every offset is computed from the layout rather than counted by hand.
fn hem_frame(fixture: &HemFixture) -> HemFrame {
    let common = 85 + fixture.header_shift;
    let paired_at = 494 + fixture.header_shift;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 8, fixture.wrapper);
    write_marked_reference(&mut bytes, common + 19, fixture.settings);
    bytes[common + 30..common + 34].copy_from_slice(&1u32.to_le_bytes());
    bytes[common + 36..common + 40].copy_from_slice(&4u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 42, fixture.gap_owner);
    write_marked_reference(&mut bytes, common + 53, fixture.length_owner);
    let radius_at = common + 71;
    bytes[radius_at..radius_at + 8].copy_from_slice(&fixture.bend_radius.to_le_bytes());
    write_marked_reference(&mut bytes, common + 108, fixture.aggregate_group);
    write_marked_reference(&mut bytes, common + 135, fixture.edge_group);

    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

/// Build the rolled `Hem` frame. Its header shift is four bytes and its owner
/// slots are thirteen bytes apart.
fn rolled_hem_frame() -> HemFrame {
    let common = 89;
    let paired_at = 498;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 8, 708);
    write_marked_reference(&mut bytes, common + 19, 724);
    write_marked_reference(&mut bytes, common + 41, 788);
    write_marked_reference(&mut bytes, common + 54, 775);
    bytes[common + 71..common + 79].copy_from_slice(&0.25f64.to_le_bytes());
    write_marked_reference(&mut bytes, common + 108, 717);
    write_marked_reference(&mut bytes, common + 135, 790);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 160,
    }
}

/// Build the teardrop `Hem` frame. The third parameter owner shifts the group
/// slots by ten bytes and moves the fixed rule radius to offset eighty-one.
fn teardrop_hem_frame() -> HemFrame {
    let common = 89;
    let paired_at = 519;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 8, 708);
    write_marked_reference(&mut bytes, common + 19, 724);
    write_marked_reference(&mut bytes, common + 42, 703);
    write_marked_reference(&mut bytes, common + 53, 706);
    write_marked_reference(&mut bytes, common + 64, 775);
    bytes[common + 81..common + 89].copy_from_slice(&0.25f64.to_le_bytes());
    write_marked_reference(&mut bytes, common + 118, 717);
    write_marked_reference(&mut bytes, common + 145, 777);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 170,
    }
}

#[test]
fn hem_scope_admits_reference_sources_before_collecting_each_form() {
    let gap_references = [240, 243, 251, 254, 301, 304, 308, 311];
    let gap_frame = hem_frame(&HemFixture {
        header_shift: 0,
        wrapper: 308,
        settings: 311,
        gap_owner: 301,
        length_owner: 304,
        aggregate_group: 240,
        edge_group: 251,
        bend_radius: 0.25,
    });
    assert_hem_reference_collection_refusals(
        &gap_frame,
        &gap_references,
        &source_kinds(&[(301, "HemGap"), (304, "HemLength")]),
        HemCandidate::GapLength,
        0,
        "scan F3D gap-length Hem reference source",
        "collect F3D gap-length Hem references",
        8,
    );

    let rolled_references = [708, 717, 720, 724, 775, 788, 790, 793];
    let rolled_frame = rolled_hem_frame();
    assert_hem_reference_collection_refusals(
        &rolled_frame,
        &rolled_references,
        &source_kinds(&[(775, "HemRadius"), (788, "HemAngle")]),
        HemCandidate::Rolled,
        4,
        "scan F3D rolled Hem reference source",
        "collect F3D rolled Hem references",
        8,
    );

    let teardrop_references = [703, 706, 708, 717, 720, 724, 775, 777, 780];
    let teardrop_frame = teardrop_hem_frame();
    assert_hem_reference_collection_refusals(
        &teardrop_frame,
        &teardrop_references,
        &source_kinds(&[(703, "HemGap"), (706, "HemLength"), (775, "HemRadius")]),
        HemCandidate::Teardrop,
        4,
        "scan F3D teardrop Hem reference source",
        "collect F3D teardrop Hem references",
        9,
    );
}

#[derive(Clone, Copy)]
enum HemCandidate {
    GapLength,
    Rolled,
    Teardrop,
}

fn parse_hem_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &HemFrame,
    references: &crate::records::identity::ReferenceRun<u32>,
    candidate: HemCandidate,
    header_shift: usize,
) -> Result<
    Option<crate::records::feature::sheet_metal::DesignHemOperation>,
    cadmpeg_core::CodecError,
> {
    use crate::design::decode::scopes::sheet_metal::{
        hem_gap_length_operation_at, hem_gap_length_radius_operation_at,
        hem_radius_angle_operation_at,
    };

    match candidate {
        HemCandidate::GapLength => hem_gap_length_operation_at(
            ctx,
            &frame.bytes,
            0,
            frame.paired_at,
            references,
            header_shift,
        ),
        HemCandidate::Rolled => hem_radius_angle_operation_at(
            ctx,
            &frame.bytes,
            0,
            frame.paired_at,
            references,
            header_shift,
        ),
        HemCandidate::Teardrop => hem_gap_length_radius_operation_at(
            ctx,
            &frame.bytes,
            0,
            frame.paired_at,
            references,
            header_shift,
        ),
    }
}

fn assert_hem_reference_collection_refusals(
    frame: &HemFrame,
    reference_values: &[u32],
    kinds: &impl Fn(u32, &str) -> Result<bool, cadmpeg_core::CodecError>,
    candidate: HemCandidate,
    header_shift: usize,
    source_operation: &'static str,
    collect_operation: &'static str,
    source_work: u64,
) {
    use cadmpeg_core::decode::ResourceDimension;

    let references = reference_run(reference_values);
    let success = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &cadmpeg_test_support::service_decode_context(),
        &frame.bytes,
        0,
        frame.paired_at,
        &references,
        kinds,
    );
    assert!(matches!(success, Ok(Some(_))));

    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            ctx,
            &frame.bytes,
            0,
            frame.paired_at,
            &references,
            kinds,
        )
        .map(|_| ())
    };

    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        source_operation,
        0,
        |ctx| decode(ctx),
    );
    // Source work counts every reference admitted before candidate parsing.
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == source_operation
                && failure.additional == source_work
    ));

    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        collect_operation,
        0,
        |ctx| decode(ctx),
    );
    // Collector work counts its first admitted output step.
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == collect_operation
                && failure.additional == 1
    ));

    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        collect_operation,
        0,
        |ctx| decode(ctx),
    );
    // Collection storage counts the first admitted reference slot.
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == collect_operation
                && failure.additional == 1
    ));

    // The public dispatcher tries earlier candidates. Test scoped storage
    // through this exact private candidate so an earlier temporary pool cannot
    // consume the monotone MaterializedBytes ceiling first.
    for located in [false, true] {
        let references = if located {
            located_reference_run(reference_values)
        } else {
            reference_run(reference_values)
        };
        let success = crate::test_support::with_decode_context(|ctx| {
            parse_hem_candidate(ctx, frame, &references, candidate, header_shift)
        })
        .expect("service admission");
        assert!(success.is_some(), "the exact Hem candidate is valid");

        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            collect_operation,
            0,
            |ctx| parse_hem_candidate(ctx, frame, &references, candidate, header_shift).map(|_| ()),
        );
        // Each scoped pool first reserves four u32 slots.
        let minimum_bytes = std::mem::size_of::<u32>()
            .checked_mul(4)
            .expect("four u32 reference slots");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::MaterializedBytes
                    && failure.operation == collect_operation
                    && failure.additional == u64::try_from(minimum_bytes).expect("u32 slot bytes fit")
        ));
    }
}

mod contains {
    use super::write_marked_reference;

    #[test]
    fn edge_flange_to_object_membership_propagates_work_refusal() {
        use crate::layout::edge_flange_fixed_operation_section as edge_flange;
        use crate::layout::edge_flange_to_object_fixed_operation_section as to_object;

        const REFERENCES: [u32; 11] = [100, 101, 102, 103, 104, 106, 108, 107, 109, 111, 110];
        const COMMON: usize = 85;
        let mut bytes = vec![0; 576];
        bytes[COMMON + edge_flange::EDGE_COUNT..COMMON + edge_flange::EDGE_COUNT + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        write_marked_reference(
            &mut bytes,
            COMMON + edge_flange::EDGE_WRAPPER_REFERENCE,
            100,
        );
        write_marked_reference(&mut bytes, COMMON + edge_flange::SETTINGS_REFERENCE, 101);
        write_marked_reference(&mut bytes, COMMON + edge_flange::ANGLE_OWNER_REFERENCE, 102);
        write_marked_reference(
            &mut bytes,
            COMMON + edge_flange::HEIGHT_OWNER_REFERENCE,
            103,
        );
        bytes[COMMON + edge_flange::INSIDE_BEND_RADIUS
            ..COMMON + edge_flange::INSIDE_BEND_RADIUS + 8]
            .copy_from_slice(&0.25f64.to_le_bytes());
        bytes[COMMON + edge_flange::INSIDE_BEND_RADIUS + 14
            ..COMMON + edge_flange::INSIDE_BEND_RADIUS + 18]
            .copy_from_slice(&1u32.to_le_bytes());
        write_marked_reference(&mut bytes, COMMON + to_object::TARGET_GROUP_REFERENCE, 104);
        bytes[COMMON + to_object::TARGET_REFERENCE_COUNT
            ..COMMON + to_object::TARGET_REFERENCE_COUNT + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        write_marked_reference(&mut bytes, COMMON + to_object::INSERTED_REFERENCE_ONE, 9001);
        bytes[COMMON + to_object::INSERTED_REFERENCE_COUNT
            ..COMMON + to_object::INSERTED_REFERENCE_COUNT + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        write_marked_reference(&mut bytes, COMMON + to_object::INSERTED_REFERENCE_TWO, 9002);
        bytes[COMMON + to_object::AGGREGATE_REFERENCE_COUNT
            ..COMMON + to_object::AGGREGATE_REFERENCE_COUNT + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        write_marked_reference(
            &mut bytes,
            COMMON + to_object::AGGREGATE_GROUP_REFERENCE,
            106,
        );
        bytes[COMMON + to_object::EDGE_REFERENCE_COUNT
            ..COMMON + to_object::EDGE_REFERENCE_COUNT + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        write_marked_reference(&mut bytes, COMMON + to_object::EDGE_GROUP_REFERENCE, 108);

        let context = cadmpeg_test_support::service_decode_context();
        let Ok(Some(operation)) = super::super::edge_flange_to_object_operation_at(
            &context,
            &bytes,
            0,
            576,
            &REFERENCES,
            0,
        ) else {
            panic!("valid ToObject frame must produce an operation");
        };
        assert_eq!(operation.height_owner_record_index, 103);
        assert_eq!(operation.angle_owner_record_index, 102);
        assert_eq!(operation.settings_record_index, 101);
        assert!(matches!(
            operation.selection.shape().height(),
            crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                target_group_record_index: 104,
                target_operand_record_index: 107,
                offset_owner_record_index: 110,
                reference_record_indices: [9001, 9002],
            }
        ));

        let refusal = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "search F3D sheet-metal ToObject reference members",
            0,
            |ctx| {
                super::super::edge_flange_to_object_operation_at(
                    ctx,
                    &bytes,
                    0,
                    576,
                    &REFERENCES,
                    0,
                )
            },
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "search F3D sheet-metal ToObject reference members"
                    && limit.additional == 1
        ));
    }
}
