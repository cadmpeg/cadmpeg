// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::operation_settings::{BooleanKind, CombineDestination};
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{
    CombineOperands, Feature, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};

use crate::records::bodies::{BodyVisibility, BodyVisibilityWire};
use crate::records::feature::combine::{
    DesignCombineBodySelection, DesignCombineForm, DesignCombineLegacyExternalBodyIdentity,
    DesignCombineOperation, DesignCombineTools,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::xref::{XrefPlacementSource, XrefReference};

const SCOPE: &str = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
const FEATURE: &str = "f3d:model:feature#combine";
const OCCURRENCE: &str = "f3d:model:occurrence#xref-0-0";
const REFERENCE: &str = "f3d:xref:reference#0-occurrence-0";
const MEMBER: &str = "f3d:role-r/reference-0/occurrence-0@xref:product-definition#part.f3d";
const MEMBER_BODY: &str = "f3d:role-r/reference-0/occurrence-0@brep:entity#1";
const OTHER_BODY: &str = "f3d:role-r/reference-0/occurrence-0@brep:entity#2";
const HOST_BODY: &str = "f3d:brep:entity#1";
/// The source occurrence identity that the legacy placement fixture stores.
const SOURCE_OCCURRENCE: u64 = 0x0102_0304_0506_0708;
const BROWSER_NODE: u64 = 303;

/// One tool selector: the source occurrence, browser node, and occurrence
/// reference it stores.
type Tool = (u64, u64, u64);

fn identity(
    (source_occurrence_identity, browser_node_entity, occurrence_reference): Tool,
) -> DesignCombineLegacyExternalBodyIdentity {
    DesignCombineLegacyExternalBodyIdentity {
        selector_asset_id: "11111111-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .expect("GUID"),
        selector_asset_id_offset: 44,
        selector_context_id: "22222222-2222-4222-8222-222222222222"
            .to_owned()
            .try_into()
            .expect("GUID"),
        selector_context_id_offset: 120,
        occurrence_reference,
        occurrence_reference_offset: 205,
        browser_node_entity,
        browser_node_entity_offset: 220,
        source_occurrence_identity,
        source_occurrence_identity_offset: 229,
        tail_values: [0, 0],
        tail_value_offsets: [285, 297],
    }
}

fn scope(tools: &[Tool]) -> DesignParameterScope {
    let selection = |(index, tool): (usize, &Tool)| DesignCombineBodySelection {
        record_index: 12 + u32::try_from(index).unwrap(),
        external_identity: None,
        legacy_external_identity: Some(identity(*tool)),
        body_node: None,
    };
    let mut selections = tools.iter().enumerate().map(selection);
    let first = selections.next().unwrap();
    let mut scope = DesignParameterScope::empty(SCOPE, DesignFeatureKind::Combine, 10);
    if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
        slot.operation = Some(DesignCombineOperation {
            form: DesignCombineForm::ExtendedReference,
            operation: BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 11,
            target_body_node: None,
            tools: DesignCombineTools {
                first,
                additional: selections.collect(),
            },
            destination: crate::records::feature::combine::DesignCombineDestination::default(),
            created_body_nodes: Vec::new(),
            consumed_body_nodes: Vec::new(),
        });
    }
    scope
}

fn visibility(body: &str, selector: u64, browser_node_entity: u64) -> BodyVisibility {
    BodyVisibility::try_from(BodyVisibilityWire {
        id: format!(
            "f3d:role-r/reference-0/occurrence-0@Breps/BREP.synthetic.smbh:body-visibility#{selector}"
        ),
        body: BodyId::mint(body).unwrap(),
        stream: "Design/BulkStream.dat".into(),
        byte_offset: 10,
        asm_body_key_offset: 20,
        asm_body_key: selector,
        entity_suffix: 297,
        browser_node_entity,
        visible: true,
    })
    .unwrap()
}

/// A host document whose `Combine` tools are the legacy selectors `tools`,
/// beside an occurrence that inserts a member whose bodies carry
/// `visibilities`.
fn archive(tools: &[Tool], visibilities: &[BodyVisibility]) -> cadmpeg_ir::CadIr {
    placed_archive(
        crate::test_support::legacy_attributed_external_placement(
            crate::test_support::assembly_fixture::XREF_ROLE,
            false,
            None,
        ),
        tools,
        visibilities,
    )
}

/// [`archive`] with the occurrence placement record `frame`.
fn placed_archive(
    frame: Vec<u8>,
    tools: &[Tool],
    visibilities: &[BodyVisibility],
) -> cadmpeg_ir::CadIr {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let scope = scope(tools);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let local = tools
        .iter()
        .map(|tool| crate::ids::neutral_combine_legacy_external_body_id(&identity(*tool)))
        .collect();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint(FEATURE).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Combine {
                operands: CombineOperands::new(
                    BodySelection::Bodies(
                        vec![BodyId::mint(HOST_BODY).unwrap()].try_into().unwrap(),
                    ),
                    BodySelection::local(local, SCOPE.into()).unwrap(),
                )
                .unwrap(),
                op: BooleanKind::Join,
                keep_tools: false,
                destination: CombineDestination::TargetComponent,
            },
        )),
        native_ref: Some(SCOPE.to_owned()),
    });
    let member = ProductDefinitionId::mint(MEMBER).unwrap();
    let mut definition = ProductDefinition::new(
        member.clone(),
        Storage::File {
            document: ExternalDocument::default(),
            source: None,
        },
    );
    definition.bodies = vec![
        BodyId::mint(MEMBER_BODY).unwrap(),
        BodyId::mint(OTHER_BODY).unwrap(),
    ];
    ir.model.product_definitions.push(definition);
    let mut occurrence = Occurrence::new(OccurrenceId::mint(OCCURRENCE).unwrap(), member, None);
    occurrence.native_ref = Some(REFERENCE.to_owned());
    ir.model.occurrences.push(occurrence);
    let reference = XrefReference {
        current_pose: None,
        operation_pose: None,
        contained_poses: Vec::new(),
        owned_poses: Vec::new(),
        id: REFERENCE.into(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: Some(XrefPlacementSource {
            frame_bytes: frame.into(),
            occurrence_guid: None,
            stream: "f3d:Design/BulkStream.dat".into(),
            carrier_record_index: 10,
            owner: None,
        }),
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: crate::test_support::assembly_fixture::XREF_ROLE
            .to_owned()
            .try_into()
            .unwrap(),
        neutron_data: String::new(),
        transform: None,
    };
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
    namespace
        .set_arena(&ctx, "design_parameter_scopes", &[scope])
        .unwrap();
    namespace
        .set_arena(&ctx, "xref_references", &[reference])
        .unwrap();
    namespace
        .set_arena(&ctx, "body_visibilities", visibilities)
        .unwrap();
    ir
}

fn tool_selection(ir: &cadmpeg_ir::CadIr) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        ir.model.features[0].evaluation.definition()
    else {
        panic!("Combine feature");
    };
    operands.tools()
}

fn bind(ir: &mut cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut losses = Vec::new();
    super::bind(&ctx, ir, &mut losses).unwrap();
    losses
}

#[test]
fn legacy_tool_binds_the_inserted_design_body_in_the_occurrence_frame() {
    let mut ir = archive(
        &[(SOURCE_OCCURRENCE, BROWSER_NODE, 500)],
        &[visibility(MEMBER_BODY, 0, BROWSER_NODE)],
    );
    let losses = bind(&mut ir);
    let BodySelection::Occurrence {
        occurrence,
        occurrence_path,
        bodies,
        native,
    } = tool_selection(&ir)
    else {
        panic!("occurrence tool selection: {:?}", tool_selection(&ir));
    };
    assert_eq!(occurrence.as_str(), OCCURRENCE);
    assert!(occurrence_path.is_empty());
    assert_eq!(
        bodies.iter().map(BodyId::as_str).collect::<Vec<_>>(),
        [MEMBER_BODY]
    );
    assert_eq!(native, SCOPE);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureCombineToolExternalFinalStateAssumed.kind()
    );
}

#[test]
fn legacy_tool_stays_local_without_a_unique_join() {
    let tool = (SOURCE_OCCURRENCE, BROWSER_NODE, 500);
    let member = || visibility(MEMBER_BODY, 0, BROWSER_NODE);
    for (tools, visibilities) in [
        (
            vec![(SOURCE_OCCURRENCE + 1, BROWSER_NODE, 500)],
            vec![member()],
        ),
        (
            vec![tool],
            vec![member(), visibility(OTHER_BODY, 1, BROWSER_NODE)],
        ),
        (vec![tool], Vec::new()),
        (
            vec![tool],
            vec![visibility("f3d:brep:entity#9", 0, BROWSER_NODE)],
        ),
        (
            vec![tool, (SOURCE_OCCURRENCE, BROWSER_NODE, 501)],
            vec![member()],
        ),
    ] {
        let mut ir = archive(&tools, &visibilities);
        let losses = bind(&mut ir);
        assert!(
            matches!(tool_selection(&ir), BodySelection::Local { .. }),
            "{:?}",
            tool_selection(&ir)
        );
        assert_eq!(
            losses.iter().map(|loss| &loss.code).collect::<Vec<_>>(),
            [&crate::loss::F3dLossCode::FeatureCombineToolExternalBodyUnbound.kind()]
        );
    }
}

#[test]
fn legacy_tool_binds_through_a_legacy_typed_placement() {
    let mut ir = placed_archive(
        crate::test_support::legacy_occurrence_record(
            crate::test_support::assembly_fixture::XREF_ROLE,
            10,
            None,
        ),
        &[(SOURCE_OCCURRENCE, BROWSER_NODE, 500)],
        &[visibility(MEMBER_BODY, 0, BROWSER_NODE)],
    );
    let losses = bind(&mut ir);
    let BodySelection::Occurrence {
        occurrence, bodies, ..
    } = tool_selection(&ir)
    else {
        panic!("occurrence tool selection: {:?}", tool_selection(&ir));
    };
    assert_eq!(occurrence.as_str(), OCCURRENCE);
    assert_eq!(
        bodies.iter().map(BodyId::as_str).collect::<Vec<_>>(),
        [MEMBER_BODY]
    );
    assert_eq!(
        losses.iter().map(|loss| &loss.code).collect::<Vec<_>>(),
        [&crate::loss::F3dLossCode::FeatureCombineToolExternalFinalStateAssumed.kind()]
    );
}

#[test]
fn combine_without_legacy_selectors_charges_no_tool_loss() {
    let mut ir = archive(
        &[(SOURCE_OCCURRENCE, BROWSER_NODE, 500)],
        &[visibility(MEMBER_BODY, 0, BROWSER_NODE)],
    );
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut scope = scope(&[(SOURCE_OCCURRENCE, BROWSER_NODE, 500)]);
    scope
        .combine_operation_mut()
        .unwrap()
        .tools
        .first
        .legacy_external_identity = None;
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .set_arena(&ctx, "design_parameter_scopes", &[scope])
        .unwrap();
    let losses = bind(&mut ir);
    assert!(matches!(tool_selection(&ir), BodySelection::Local { .. }));
    assert!(losses.is_empty());
}

#[test]
fn qualifier_reads_the_archive_scope_of_an_identity() {
    assert_eq!(super::qualifier(SCOPE), None);
    assert_eq!(
        super::qualifier(MEMBER_BODY),
        Some("role-r/reference-0/occurrence-0")
    );
    assert_eq!(super::qualify_like(HOST_BODY, MEMBER_BODY), MEMBER_BODY);
}

#[test]
fn legacy_tools_bind_distinct_bodies_of_one_occurrence_in_tool_order() {
    let mut ir = archive(
        &[
            (SOURCE_OCCURRENCE, BROWSER_NODE + 1, 500),
            (SOURCE_OCCURRENCE, BROWSER_NODE, 501),
        ],
        &[
            visibility(MEMBER_BODY, 0, BROWSER_NODE),
            visibility(OTHER_BODY, 1, BROWSER_NODE + 1),
        ],
    );
    let losses = bind(&mut ir);
    let BodySelection::Occurrence { bodies, .. } = tool_selection(&ir) else {
        panic!("occurrence tool selection: {:?}", tool_selection(&ir));
    };
    assert_eq!(
        bodies.iter().map(BodyId::as_str).collect::<Vec<_>>(),
        [OTHER_BODY, MEMBER_BODY]
    );
    assert_eq!(losses.len(), 1);
}

#[test]
fn occurrence_tool_selection_follows_an_identity_rewrite() {
    let mut ir = archive(
        &[(SOURCE_OCCURRENCE, BROWSER_NODE, 500)],
        &[visibility(MEMBER_BODY, 0, BROWSER_NODE)],
    );
    bind(&mut ir);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let selection = tool_selection(&ir).clone();
    let mut map = cadmpeg_ir::schema::rewrite::typed::IdentityMap::new(|id: &str| {
        Ok::<_, cadmpeg_core::CodecError>(id.replace("role-r/reference-0/occurrence-0", "part.f3d"))
    });
    let rewritten = cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::rewrite_identities(
        selection, &ctx, &mut map,
    )
    .unwrap();
    map.finish().unwrap();
    let BodySelection::Occurrence {
        occurrence, bodies, ..
    } = rewritten
    else {
        panic!("occurrence tool selection");
    };
    assert_eq!(occurrence.as_str(), OCCURRENCE);
    assert_eq!(bodies[0].as_str(), "f3d:part.f3d@brep:entity#1");
}
