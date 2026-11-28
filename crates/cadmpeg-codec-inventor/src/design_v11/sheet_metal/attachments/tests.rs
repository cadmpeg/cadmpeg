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
    definition(
        parent,
        source,
        |reference| {
            properties.iter().find(|property| {
                reference.record_ordinal() == Some(property.identity.record_ordinal)
            })
        },
        |_| None,
    )
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
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::selections::DistinctMembers;
    use cadmpeg_ir::features::{
        Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation as LegacyOperation,
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
        // A refused contour plate stays a native legacy operation.
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            LegacyOperation::Native {
                kind: "contour_flange".into(),
                parameters: BTreeMap::new(),
            },
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
        crate::design_v11::ParameterRecords::default(),
        &mut Vec::new(),
    )
    .expect("projection");
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("inventor"));
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

const CONTOUR: &str = "inventor:design:feature#contour";
const STATE: &str = "inventor:design:feature-input#contour";

fn input_edge(source: u32) -> cadmpeg_ir::ids::HistoricalEdgeId {
    cadmpeg_ir::ids::HistoricalEdgeId::mint(format!("inventor:input-edge#fixture-{source}"))
        .expect("edge")
}

/// The stored sample of the input edge with selector source `source`.
fn sample(source: u32) -> cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry {
    use cadmpeg_core::serde_erase::json;
    let offset = f64::from(source - 900);
    let point = |values| json::from_value(cadmpeg_core::json!(values)).expect("point");
    cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry {
        edge: input_edge(source),
        interior: point([1., offset, 0.]),
        tangent: Some(cadmpeg_ir::units::UnitVector3::X_AXIS),
        start: Some(point([0., offset, 0.])),
        end: Some(point([2., offset, 0.])),
        faces: cadmpeg_ir::features::selections::DistinctMembers::default(),
        carrier: None,
        supports: Vec::new(),
    }
}

/// A part whose contour plate attaches to the edges with selector sources
/// 903 and 901 of its input state, which stores a sample of each, under
/// the flange `parent`.
fn attached_plate(parent: Flange) -> CadIr {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_ir::features::evaluation::{FeatureEvaluation, FeatureInputTopology};
    use cadmpeg_ir::features::selections::DistinctMembers;
    use cadmpeg_ir::features::{Feature, FeatureContent, FeatureDefinition, FeatureId};
    let (_, source, properties) = fixture();
    let operation = json::from_value(cadmpeg_core::json!({
        "kind": "sheet_metal_attached_plate",
        "value": {
            "profile": {"kind": "sketch", "value": "inventor:model:sketch#profile"},
            "thickness": 1.0,
            "side": "forward",
            "attachment": {"kind": "historical", "value": {
                "state": STATE,
                "edges": [input_edge(903).as_str(), input_edge(901).as_str()],
                "native": "inventor:pmdc:selection#contour",
            }},
        },
    }))
    .expect("attached plate");
    let mut ir = crate::test_support::root_part_ir();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint(CONTOUR).expect("id"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some(source.id()),
    });
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            id: cadmpeg_ir::ids::FeatureInputTopologyId::mint(STATE).expect("state"),
            input_of: FeatureId::mint(CONTOUR).expect("feature"),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::default(),
            edges: [903, 901].into_iter().map(input_edge).collect(),
            vertices: DistinctMembers::default(),
            face_owners: Vec::new(),
            face_geometry: Vec::new(),
            body_writers: Vec::new(),
            edge_geometry: vec![sample(903), sample(901)],
            native_ref: None,
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
    ir
}

/// The witnesses of the attachment picks of the contour flange of `ir`.
fn witnesses(ir: &CadIr) -> Vec<Option<cadmpeg_ir::design::selection::Witness>> {
    let FeatureOperation::SheetMetal(operation) = &ir.design.features[0].definition.operation
    else {
        panic!("sheet metal");
    };
    let SheetMetalKind::ContourFlange {
        edges: Some(edges), ..
    } = &operation.kind
    else {
        panic!("contour flange");
    };
    let Picks::Named(picks) = &edges.picks else {
        panic!("named edges");
    };
    picks
        .as_slice()
        .iter()
        .map(|pick| pick.witness.clone())
        .collect()
}

/// The wire keys of the input state that its restatement leaves out.
fn input_cut(ir: &CadIr) -> Vec<&'static str> {
    ir.model
        .restated()
        .get(STATE)
        .expect("restated")
        .cut_by(|_| true)
        .expect("restater")
        .iter()
        .copied()
        .collect()
}

/// Each attachment pick holds the interior point, tangent and end points of
/// its edge in the plate's input state, so the contour flange restates the
/// input state in full.
#[test]
fn attachment_picks_witness_their_input_edges() {
    let (parent, _, _) = fixture();
    let ir = attached_plate(parent);
    let witness = Witness::edge_sample;
    assert_eq!(
        witnesses(&ir),
        [Some(witness(&sample(903))), Some(witness(&sample(901)))]
    );
    assert!(input_cut(&ir).is_empty(), "{:?}", input_cut(&ir));
}

/// An attachment edge whose input state stores no sample has no witness, and
/// the input edge 901 that the plate's input state holds is named by no pick,
/// so the restatement cuts the edges and the samples.
#[test]
fn an_input_sample_that_no_pick_witnesses_is_stated_in_part() {
    let (mut parent, _, _) = fixture();
    parent.edit(|payload| {
        payload.attachment_edges.as_mut().expect("attachment").edges[1].selector_source = 905;
    });
    let ir = attached_plate(parent);
    assert_eq!(
        witnesses(&ir),
        [Some(Witness::edge_sample(&sample(903))), None]
    );
    assert_eq!(input_cut(&ir), ["edge_geometry", "edges"]);
}
