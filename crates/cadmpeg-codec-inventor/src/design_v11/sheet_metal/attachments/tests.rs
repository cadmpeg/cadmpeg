// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::history_states::StateRef;
use crate::feature::sheet_metal::contour::{AttachmentEdge, AttachmentEdges};
use crate::feature::sheet_metal::flange;
use crate::feature::{PmDcFeatureEnumFamily, PmDcFeatureReferenceFamily};
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::sheet_metal::{feature, list, property, reference};
use crate::test_support::test_fixtures::{content_at, parse};
use cadmpeg_ir::ids::IdentityKey;

fn fixture() -> (Flange, PmDcFeature, Vec<PmDcFeatureProperty>) {
    let mut bytes = content_at(13, 29, &[]);
    bytes.extend_from_slice(&(-1_i32).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
    for value in [3_u32, 3, 16, 101, 201, 301, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut payload = parse(&bytes, |ctx, view| flange::parse(ctx, view, 29)).expect("flange");
    payload.attachment_edges = Some(AttachmentEdges {
        selection: reference(121),
        state: Some(StateRef {
            ordinal: 2,
            state_id: 77,
        }),
        edges: [903, 901]
            .into_iter()
            .map(|source| AttachmentEdge {
                item: reference(124),
                selector_source: source,
                entity_index: i64::from(source - 860),
                revision: Some(5),
            })
            .collect(),
    });
    let parent = Located::new(
        payload,
        RecordTypeId::from_bytes(flange::CONTOUR_TYPE),
        IdentityKey::try_new("fixture").expect("token"),
        12,
    );
    let source = feature(100, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 121, 0, 0, 0, 14]);
    let properties = vec![
        property(
            1,
            &PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: reference(0),
                value: 0,
            },
        ),
        property(
            14,
            &PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            },
        ),
        property(
            121,
            &PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
                items: list(&[122]),
            },
        ),
    ];
    (parent, source, properties)
}

fn read(
    parent: &Flange,
    source: &PmDcFeature,
    properties: &[PmDcFeatureProperty],
) -> Option<FeatureOperation> {
    definition(parent, source, |reference| {
        properties
            .iter()
            .find(|property| reference.record_ordinal() == Some(property.identity.record_ordinal))
    })
    .expect("definition")
}

#[test]
fn attached_contour_keeps_edge_order_state_and_revisions() {
    let (parent, source, properties) = fixture();
    let FeatureOperation::SheetMetal(operation) =
        read(&parent, &source, &properties).expect("attached")
    else {
        panic!("sheet metal")
    };
    let SheetMetalKind::ContourFlange {
        edges: Some(edges),
        width,
        extent,
        thickness,
        ..
    } = operation.kind
    else {
        panic!("attached contour")
    };
    assert_eq!(width, None);
    assert_eq!(extent, None);
    assert_eq!(thickness, None);
    let Picks::Named(picks) = edges.picks else {
        panic!("named edges")
    };
    let names = picks
        .into_vec()
        .into_iter()
        .map(|pick| pick.native.expect("native").value)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            NativeNameValue::Integers(vec![2, 77, 124, 903, 43, 5]),
            NativeNameValue::Integers(vec![2, 77, 124, 901, 41, 5])
        ]
    );
}

#[test]
fn attached_contour_requires_state_revision_and_operation_form() {
    let (parent, source, properties) = fixture();
    let mut missing = parent.clone();
    missing.edit(|payload| payload.attachment_edges.as_mut().expect("attachment").state = None);
    assert!(read(&missing, &source, &properties).is_none());
    missing = parent.clone();
    missing.edit(|payload| {
        payload.attachment_edges.as_mut().expect("attachment").edges[0].revision = None;
    });
    assert!(read(&missing, &source, &properties).is_none());
    let mut invalid = properties.clone();
    invalid[1].edit(|payload| {
        let PmDcFeaturePropertyKind::Enumeration { value, .. } = &mut payload.kind else {
            panic!("enumeration")
        };
        *value = 3;
    });
    assert!(read(&parent, &source, &invalid).is_none());
    invalid = properties;
    invalid[2].edit(|payload| {
        payload.kind = PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items: list(&[122]),
        };
    });
    assert!(read(&parent, &source, &invalid).is_none());
}

#[test]
fn native_validation_rejects_an_altered_attachment_pick() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation as LegacyOperation,
    };
    let (parent, source, properties) = fixture();
    let mut ir = crate::test_support::root_part_ir();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("inventor:design:feature#contour").expect("id"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            LegacyOperation::StoredGeometry {},
        )),
        native_ref: Some(source.id()),
    });
    let mut inventory = FeatureInventory::default();
    inventory.features.push(source);
    inventory.properties = properties;
    inventory.sheet_metal_flanges.push(parent);
    let sketches = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        text_members: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::design_v11::project(
        &ctx,
        &mut ir,
        &inventory,
        &sketches,
        &[],
        &[],
        &mut Vec::new(),
    )
    .expect("projection");
    let namespace = ir.native.namespace_mut("inventor");
    namespace
        .set_arena(&ctx, "pm_dc_features", &inventory.features)
        .expect("native features");
    namespace
        .set_arena(&ctx, "pm_dc_feature_properties", &inventory.properties)
        .expect("native properties");
    namespace
        .set_arena(
            &ctx,
            "pm_dc_sheet_metal_flanges",
            &inventory.sheet_metal_flanges,
        )
        .expect("native flanges");
    assert!(crate::design_v11::tests::validate_from_native(&ctx, &ir)
        .expect("validation")
        .is_empty());
    let FeatureOperation::SheetMetal(operation) = &mut ir.design.features[0].definition.operation
    else {
        panic!("sheet metal");
    };
    let SheetMetalKind::ContourFlange {
        edges: Some(edges), ..
    } = &mut operation.kind
    else {
        panic!("contour flange");
    };
    edges.picks = Picks::Unread;
    let findings = crate::design_v11::tests::validate_from_native(&ctx, &ir).expect("validation");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
}
