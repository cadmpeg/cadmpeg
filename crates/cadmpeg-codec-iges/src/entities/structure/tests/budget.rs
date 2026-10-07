// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::global::GlobalTable;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

#[test]
fn network_connectivity_charges_only_the_visited_prefix() {
    let definition = [None, Some(3), Some(5)];
    let instance = [Some(1), Some(3), Some(5)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(!super::super::network_connectivity_valid(
            &definition,
            &instance,
            GlobalTable::V5_0,
            ctx
        )
        .unwrap());
    });
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(!super::super::network_connectivity_valid(
            &definition,
            &instance[..1],
            GlobalTable::V5_0,
            ctx
        )
        .unwrap());
    });
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges network connectivity",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                super::super::network_connectivity_valid(
                    &definition,
                    &instance,
                    GlobalTable::V5_0,
                    ctx,
                )
            })
        },
    );
}

#[test]
fn property_lists_and_owner_indexes_preserve_work_refusals() {
    use crate::test_support::test_solids_and_structure::{
        product_property_file, scalar_property_forms_file,
    };
    for (bytes, operation) in [
        (
            product_property_file(),
            "iges structure property references",
        ),
        (scalar_property_forms_file(), "iges property fields"),
    ] {
        super::assert_structure_refusal(&bytes, operation, ResourceDimension::WorkUnits);
    }
}

#[test]
fn functional_level_suffix_uses_the_text_budget() {
    let value = b"Signal_0000000000000000000000000002";
    crate::test_support::with_service_context(&[], |ctx| {
        assert!(super::super::functional_level_identifier_valid(value, ctx).unwrap());
        assert!(!super::super::functional_level_identifier_valid(b"Signal_+2", ctx).unwrap());
    });
    for operation in [
        "iges functional level suffix UTF-8",
        "iges functional level suffix number",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    super::super::functional_level_identifier_valid(value, ctx)
                })
            },
        );
    }
}

#[test]
fn decode_projects_many_groups_with_one_shared_back_pointer_list() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::fmt::Write;
    use std::io::Cursor;

    const GROUP_COUNT: usize = 2_000;
    let mut parameters = format!("116,1,2,3,0,{GROUP_COUNT}");
    let mut entities = Vec::with_capacity(GROUP_COUNT + 1);
    for index in 0..GROUP_COUNT {
        write!(parameters, ",{}", 3 + index * 2).unwrap();
    }
    parameters.push_str(",0;");
    entities.push(OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "MEMBER".into(),
        status: "00000000",
        parameters,
    });
    for _ in 0..GROUP_COUNT {
        entities.push(OwnedTestEntity {
            entity_type: 402,
            form: 14,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,1,1;".into(),
        });
    }
    let result = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let native = result.ir().native.namespace("iges").unwrap();
    let groups = &native.arenas()["groups"];
    assert_eq!(groups.len(), GROUP_COUNT);
    assert!(groups
        .iter()
        .all(|group| group.fields()["members"] == serde_json::json!(["iges:entity:directory#1"])));
    assert_eq!(
        native.arenas()["entities"][0].fields()["association_links"]
            .as_array()
            .unwrap()
            .len(),
        GROUP_COUNT
    );
}

#[test]
fn decode_preserves_zero_width_attribute_fields_for_many_instances() {
    use crate::test_support::test_owned::{owned_test_file_with_structures, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::fmt::Write;
    use std::io::Cursor;

    const FIELD_COUNT: usize = 256;
    const INSTANCE_COUNT: usize = 256;
    let mut parameters = format!("322,4HMETA,1,{FIELD_COUNT}");
    for field in 0..FIELD_COUNT {
        write!(parameters, ",{field},1,0").unwrap();
    }
    parameters.push(';');
    let mut entities = vec![OwnedTestEntity {
        entity_type: 322,
        form: 0,
        label: "ATTRDEF".into(),
        status: "00000000",
        parameters,
    }];
    let mut structures = Vec::new();
    for index in 0..INSTANCE_COUNT {
        entities.push(OwnedTestEntity {
            entity_type: 422,
            form: 0,
            label: "ATTRINST".into(),
            status: "00000000",
            parameters: "422;".into(),
        });
        structures.push((u32::try_from(3 + index * 2).unwrap(), -1));
    }
    let result = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_structures(&entities, &structures)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(
        native.arenas()["attribute_table_definitions"][0].fields()["attributes"]
            .as_array()
            .unwrap()
            .len(),
        FIELD_COUNT
    );
    assert_eq!(
        native.arenas()["attribute_table_instances"].len(),
        INSTANCE_COUNT
    );
}
