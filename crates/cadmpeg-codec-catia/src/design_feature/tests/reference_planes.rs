// SPDX-License-Identifier: Apache-2.0
//! Reference-plane owner transfer tests.

use crate::design_feature::tests::compact_entity;
use crate::design_feature::tests::declared_object;
use crate::design_feature::tests::design_object;
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
        let plane = declared_object(
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

        let transfer = crate::test_support::with_service_context(|ctx| {
            transfer_design_features(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        })
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
        assert_eq!(
            transfer.reference_plane_records,
            HashSet::from(["plane-record".to_string()])
        );
        assert_eq!(
            transfer.consumed_records().cloned().collect::<HashSet<_>>(),
            HashSet::from(["plane-record".to_string()])
        );
    }
}

/// A reference-plane owner transfers only where the owner entity's compact
/// class is a plane class (`catia.md` §7.3.2).
#[test]
fn a_reference_plane_owner_whose_entity_is_another_class_is_no_plane() {
    let planes = |class: &str| {
        let parent = design_object("synthetic:test:object#parent-object", None);
        let plane = declared_object(
            "synthetic:test:object#plane-object",
            Some("synthetic:test:object#parent-object"),
            21,
            "plane-record",
            "GSMPlaneOffset",
            "plane-offset-entry",
        );
        let mut native = CatiaNative {
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
                        Some("GSMPlaneOffset"),
                        Some("plane-offset-entry"),
                    ),
                ],
            }],
            ..CatiaNative::default()
        };
        native.entity_records = vec![compact_entity(
            &mut native.object_graphs[0].records[1],
            class,
        )];
        let mut ir = CadIr::empty();
        crate::test_support::with_service_context(|ctx| {
            transfer_design_features(
                ctx,
                &mut ir,
                &native,
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        })
        .unwrap();
        ir.model.features.len()
    };
    assert_eq!(planes("GSMPlaneOffset"), 1);
    assert_eq!(planes("Pad"), 0);
    assert_eq!(planes(""), 1);
}
