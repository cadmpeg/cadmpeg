// SPDX-License-Identifier: Apache-2.0
//! The passes that bind a stored design to its product definitions and
//! configurations.

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json as json_wire;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::CadIr;

use crate::loss::F3dLossCode;
use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationActiveRow, DesignConfigurationKind,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::recipes::DesignComponentNamingSpace;
use crate::test_support::configuration_table::{
    bool_cell, neutral_feature, object_target, table_document,
};

fn space(record: u64, uuid: &str) -> DesignComponentNamingSpace {
    DesignComponentNamingSpace {
        id: format!("f3d:Design/BulkStream.dat:component-naming-space#{record}"),
        byte_offset: 0,
        component_record_index: record,
        context_uuid: uuid.to_owned().try_into().unwrap(),
        context_uuid_offset: 0,
        name: None,
        name_offset: None,
    }
}

/// Components named at records 10 and 30 hold Extrude scope 40, which
/// stores history state 8 after state 7 and no suppression: feature
/// projection infers its feature active. The body history binds the feature
/// a body that no product definition holds, and active row `a` suppresses
/// it. The owner pass drops that output link. The feature produces a body,
/// so it keeps its state and the row contradicts it.
#[test]
fn a_feature_whose_output_the_owner_pass_drops_keeps_its_state_under_a_suppressing_row() {
    let mut table = DesignConfiguration::try_new(
        "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.o.dsgcfg"
            .into(),
        DesignConfigurationKind::Table,
        table_document(
            "1",
            &[("F", object_target("40", "DcFeature"))],
            &[("a", "A", vec![("F", bool_cell(false))])],
            &["a"],
        )
        .as_object()
        .unwrap()
        .clone(),
    )
    .unwrap();
    table
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: "a".into(),
        })
        .unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        DesignFeatureKind::Extrude,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let native = crate::native::F3dNative {
        design_component_naming_spaces: vec![
            space(10, "11111111-1111-1111-1111-111111111111"),
            space(30, "22222222-2222-2222-2222-222222222222"),
        ],
        design_parameter_scopes: vec![scope.clone()],
        design_configurations: vec![table],
        ..crate::native::F3dNative::default()
    };
    let mut ir = CadIr::empty();
    for space in &native.design_component_naming_spaces {
        ir.model.product_definitions.push(
            json_wire::from_value(json!({
                "id": crate::ids::neutral_component_id(&space.context_uuid), "kind": "part",
                "bom_properties": {}, "storage": {"kind": "embedded"}, "occurrences": [],
                "instance_bodies": [], "representations": [], "bodies": []
            }))
            .unwrap(),
        );
    }
    let mut feature = neutral_feature(&scope, "f3d:model:feature#origin");
    feature.ordinal = 40;
    feature.native_ref = Some(scope.id.clone());
    feature.evaluation.set_outputs(
        DistinctMembers::try_from(vec![BodyId::mint("test:model:body#unowned").unwrap()]).unwrap(),
    );
    ir.model.features.push(feature);
    let mut losses = Vec::new();
    let archive = crate::test_support::zip_fixture::f3d_with_configurations(
        &crate::test_support::smbh_geometry_fixture::synthetic_geometry_smbh(),
        &[],
    );
    crate::test_support::zip_fixture::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            crate::decode::attach_design(ctx, scan, &native, &mut ir, &mut losses)
        })
    })
    .unwrap();

    assert!(
        losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::ScopeOutputOwnerMismatch.kind()),
        "{losses:#?}"
    );
    let attached = &ir.model.features[0];
    assert!(attached.evaluation.outputs().is_empty());
    assert_eq!(attached.suppressed, Some(false));
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        report.findings.iter().any(|finding| finding.rule.as_str()
            == "topology.active-configuration-suppression-disagrees-current-feature"),
        "{:#?}",
        report.findings
    );
}
