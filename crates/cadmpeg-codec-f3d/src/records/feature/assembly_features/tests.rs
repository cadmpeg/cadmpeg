// SPDX-License-Identifier: Apache-2.0

use super::{
    DesignComponentInsertConstruction, DesignComponentInsertConstructionWire,
    COMPONENT_INSERT_CLONE_COUNT,
};

fn component_insert() -> DesignComponentInsertConstruction {
    serde_json::from_str(
        r#"{"relation_record_index":1,"carrier_record_index":2,"neutron_role":"role","neutron_role_offset":30,"transform":[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":50,"carrier_transform_offset":40}"#,
    )
    .unwrap()
}

#[test]
fn component_insert_borrowed_wire_matches_owned_wire_bytes() {
    for record in [
        component_insert(),
        serde_json::from_str(
            r#"{"relation_record_index":1,"carrier_record_index":2,"neutron_role":"role","neutron_role_offset":30,"transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]}"#,
        )
        .unwrap(),
    ] {
        let owned = DesignComponentInsertConstructionWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn component_insert_native_retained_limit_refuses_before_record_clone() {
    #[derive(serde::Serialize)]
    struct NativeRecord<'a> {
        id: &'static str,
        construction: &'a DesignComponentInsertConstruction,
    }

    let construction = component_insert();
    let record = NativeRecord {
        id: "f3d:native:component-insert#0",
        construction: &construction,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || COMPONENT_INSERT_CLONE_COUNT.with(|count| count.set(0)),
        || COMPONENT_INSERT_CLONE_COUNT.with(std::cell::Cell::get),
    );
}
