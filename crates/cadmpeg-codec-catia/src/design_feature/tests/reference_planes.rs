// SPDX-License-Identifier: Apache-2.0
//! Reference-plane owner transfer tests.

use crate::design_feature::tests::design_object;
use crate::design_feature::tests::native_operation_object;
use crate::design_feature::tests::object_record;
use crate::design_feature::transfer_design_features;
use crate::native::CatiaNative;
use crate::native::CatiaObjectGraph;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::UnresolvedFamily;
use std::collections::HashSet;

#[test]
fn transfers_exact_reference_plane_owners_as_unresolved_datum_planes() {
    for (class_name, class_entry) in [
        ("GSMPlaneAngle", "plane-angle-entry"),
        ("GSMPlaneOffset", "plane-offset-entry"),
    ] {
        let parent = design_object("synthetic:test:object#parent-object", None);
        let plane = native_operation_object(
            "synthetic:test:object#plane-object",
            Some("synthetic:test:object#parent-object"),
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
                        Some("synthetic:test:object#parent-object"),
                        Some(21),
                        Some(15),
                        Some(class_name),
                        Some(class_entry),
                    ),
                ],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();

        let transfer = transfer_design_features(
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
        )
        .unwrap();

        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(ir.model.features[0].source_tag.as_deref(), Some(class_name));
        assert!(matches!(
            ir.model.features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane
            })
        ));
        assert_eq!(
            ir.model.features[0].native_ref.as_deref(),
            Some("synthetic:test:object#plane-object")
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
