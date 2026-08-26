// SPDX-License-Identifier: Apache-2.0
//! Reference-plane owner transfer tests.

use super::*;

#[test]
fn transfers_exact_reference_plane_owners_as_unresolved_datum_planes() {
    for (class_name, class_entry) in [
        ("GSMPlaneAngle", "plane-angle-entry"),
        ("GSMPlaneOffset", "plane-offset-entry"),
    ] {
        let parent = design_object("parent-object", None);
        let plane = native_operation_object(
            "plane-object",
            Some("parent-object"),
            21,
            "plane-record",
            class_name,
            class_entry,
        );
        let native = CatiaNative {
            design_objects: vec![parent, plane],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![
                    object_record("parent-record", None, Some(15), None, None, None),
                    object_record(
                        "plane-record",
                        Some("parent-object"),
                        Some(21),
                        Some(15),
                        Some(class_name),
                        Some(class_entry),
                    ),
                ],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transfer = transfer_design_features(&mut ir, &native, None);

        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(ir.model.features[0].source_tag.as_deref(), Some(class_name));
        assert!(matches!(
            ir.model.features[0].definition,
            FeatureDefinition::DatumPlaneUnresolved
        ));
        assert_eq!(
            ir.model.features[0].native_ref.as_deref(),
            Some("plane-object")
        );
        assert!(transfer.native_operation_records.is_empty());
        assert_eq!(
            transfer.reference_plane_records,
            HashSet::from(["plane-record".to_string()])
        );
        assert_eq!(
            transfer.consumed_records(),
            HashSet::from(["plane-record".to_string()])
        );
    }
}
