// SPDX-License-Identifier: Apache-2.0
//! Component-occurrence identity and placement validation.
#![allow(clippy::unwrap_used)]

use crate::test_support::native_test::f3d_native_mut;

#[test]
fn validation_admits_ordinal_one_placement_outside_class_256() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };
    use crate::records::sketch_placement::SketchPlacementMatrix;

    const PLACED_AT_ORDINAL_ONE: &str =
        "Fusion Design class-256 component occurrence has a placed frame at ordinal one";
    let placed = |class_tag: &str, record_index: u32, occurrence_guid: &str| {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
            class_tag: crate::records::references::DesignClassTag::try_from(class_tag.to_owned())
                .unwrap(),
            record_index,
            byte_offset: u64::from(record_index) * 1_000,
            component_record_index: 700,
            component_guid: "11111111-2222-4333-8444-555555555555"
                .to_owned()
                .try_into()
                .expect("GUID"),
            occurrence_guid: occurrence_guid.to_owned().try_into().expect("GUID"),
            placement: DesignComponentOccurrencePlacement::Explicit {
                ordinal: std::num::NonZeroU32::MIN,
                transform: SketchPlacementMatrix::try_from([
                    [1.0, 0.0, 0.0, 0.5],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            },
        })
        .unwrap()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let placed_339 = placed("339", 100, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
    let placed_256 = placed("256", 101, "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff");
    let (placed_339_id, placed_256_id) = (placed_339.id.clone(), placed_256.id.clone());
    f3d_native_mut(&mut ir)
        .design_component_occurrences
        .extend([placed_339, placed_256]);

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native_charged(ctx, &ir).expect("service native validation")
    });
    let flagged = |id: &str| {
        findings.iter().any(|finding| {
            finding.message == PLACED_AT_ORDINAL_ONE && finding.entity.as_deref() == Some(id)
        })
    };
    assert!(!flagged(&placed_339_id));
    assert!(flagged(&placed_256_id));
}

#[test]
fn validation_names_a_repeated_occurrence_guid() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };

    const GUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let occurrence = |record_index: u32| {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
            class_tag: crate::records::references::DesignClassTag::try_from("339".to_owned())
                .unwrap(),
            record_index,
            byte_offset: u64::from(record_index) * 1_000,
            component_record_index: 700,
            component_guid: "11111111-2222-4333-8444-555555555555"
                .to_owned()
                .try_into()
                .expect("GUID"),
            occurrence_guid: GUID.to_owned().try_into().expect("GUID"),
            placement: DesignComponentOccurrencePlacement::Base,
        })
        .unwrap()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    f3d_native_mut(&mut ir)
        .design_component_occurrences
        .extend([occurrence(100), occurrence(101)]);

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native_charged(ctx, &ir).expect("service native validation")
    });
    let messages: Vec<_> = findings
        .iter()
        .filter(|finding| finding.message.contains("component occurrence"))
        .map(|finding| finding.message.as_str())
        .collect();
    assert_eq!(
        messages,
        ["Fusion Design component occurrence GUID repeats in its Design stream"]
    );
}
