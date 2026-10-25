// SPDX-License-Identifier: Apache-2.0
//! Kept native states equal the pairs, except where a given-up stub links the pair.

use std::collections::HashSet;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::{Identity, UnknownId};
use cadmpeg_ir::report::check::Finding;
use cadmpeg_ir::{CadIr, NativeUnknownRecord};

use crate::test_support::test_fixtures::{
    primary_envelope_fixture, primary_envelope_fixture_with_design,
};
use crate::InventorCodec;

const PAIR_TOKEN: &str = "seg";
const MISSING: &str = "Inventor segment metadata states do not cover paired segments exactly";
const UNPAIRED: &str = "Inventor segment metadata state has no paired segment";

#[test]
fn a_pair_without_a_state_or_a_linking_stub_needs_its_state() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(&mut findings, &pairs, &HashSet::new(), [], [], "metadata");
    assert_eq!(findings.len(), 1);
    assert!(findings[0]
        .message
        .contains("states do not cover paired segments exactly"));
}

#[test]
fn every_kept_metadata_state_still_needs_its_actual_pair() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(
        &mut findings,
        &pairs,
        &HashSet::from(["seg"]),
        ["orphan"],
        [],
        "metadata",
    );
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("has no paired segment"));
}

#[test]
fn duplicate_kept_metadata_states_are_still_rejected() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(
        &mut findings,
        &pairs,
        &HashSet::new(),
        ["seg", "seg"],
        [],
        "metadata",
    );
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("repeats"));
}

fn decoded() -> CadIr {
    decode(primary_envelope_fixture())
}

fn decode(bytes: Vec<u8>) -> CadIr {
    InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("primary envelope decodes")
        .ir()
        .clone()
}

fn segment_state_findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = crate::native::test_ctx();
    InventorCodec
        .validate_native(&ctx, ir)
        .expect("validation")
        .into_iter()
        .filter(|finding| {
            finding
                .message
                .ends_with("states do not cover paired segments exactly")
                || finding.message.ends_with("state has no paired segment")
        })
        .collect()
}

fn edit_metadata_states(ir: &mut CadIr, edit: impl FnOnce(&mut Vec<serde_json::Value>)) {
    let ctx = crate::native::test_ctx();
    let namespace = ir.native.namespace_mut("inventor");
    let mut states = namespace
        .arena_as::<serde_json::Value>("segment_meta")
        .expect("metadata states");
    edit(&mut states);
    namespace
        .set_arena(&ctx, "segment_meta", &states)
        .expect("edited metadata states");
}

fn remove_metadata_state(ir: &mut CadIr) {
    edit_metadata_states(ir, |states| {
        let before = states.len();
        states.retain(|state| state["token"] != PAIR_TOKEN);
        assert_eq!(before - states.len(), 1);
    });
}

fn add_stub(ir: &mut CadIr, links: Vec<Identity>) {
    let ctx = crate::native::test_ctx();
    let namespace = ir.native.namespace_mut("inventor");
    let mut stubs = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stubs");
    stubs.push(NativeUnknownRecord {
        id: UnknownId::mint("inventor:part:test#given-up").expect("stub identity"),
        links,
    });
    namespace
        .set_arena(&ctx, "unknowns", &stubs)
        .expect("stub arena");
}

#[test]
fn the_decoded_states_equal_the_pairs() {
    assert!(segment_state_findings(&decoded()).is_empty());
}

#[test]
fn a_missing_state_whose_segment_record_a_given_up_stub_links_is_accepted() {
    let mut ir = decoded();
    remove_metadata_state(&mut ir);
    add_stub(
        &mut ir,
        vec![Identity::new(crate::rse::segment_record_id(PAIR_TOKEN)).expect("pair id")],
    );
    assert!(segment_state_findings(&ir).is_empty());
}

#[test]
fn a_missing_state_without_a_linking_stub_is_refused() {
    let mut ir = decoded();
    remove_metadata_state(&mut ir);
    add_stub(
        &mut ir,
        vec![Identity::new("inventor:rse:segment-meta#seg".to_owned()).expect("other id")],
    );
    let findings = segment_state_findings(&ir);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, MISSING);
}

#[test]
fn an_extra_state_that_matches_no_pair_is_refused() {
    let mut ir = decoded();
    edit_metadata_states(&mut ir, |states| {
        let mut extra = states[0].clone();
        extra["id"] = "inventor:rse:segment-meta#orphan".into();
        extra["token"] = "orphan".into();
        states.push(extra);
    });
    let findings = segment_state_findings(&ir);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, UNPAIRED);
}

#[test]
fn a_given_up_type_table_links_its_segment_record_and_its_missing_states_validate() {
    let design = vec![([0x11; 16], Vec::new()); 300];
    let ir = decode(primary_envelope_fixture_with_design(&[], &design));
    let namespace = ir.native.namespace("inventor").expect("namespace");
    let metadata = namespace
        .arena_as::<serde_json::Value>("segment_meta")
        .expect("metadata states");
    assert!(metadata.iter().all(|state| state["token"] != "dc"));
    let stubs = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stubs");
    assert!(stubs.iter().any(|stub| stub
        .links
        .iter()
        .any(|link| link.as_str() == crate::rse::segment_record_id("dc"))));
    assert!(segment_state_findings(&ir).is_empty());
}

use super::face_property_is_admitted;
use crate::feature::{
    PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, FACE_COLLECTION_TYPE,
    FACE_ITEM_TYPE,
};
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList, PmDcU32List};
use cadmpeg_ir::features::{
    DistinctMembers, FaceSelection, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureId, FeatureOperation,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn annotation(native: String) -> Feature {
    Feature {
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            cadmpeg_ir::identity_key!("generated"),
        ),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: Some("thread".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::CosmeticThread {
                face: FaceSelection::Native("inventor:pmdc:feature-property#generated-0".into()),
                diameter: None,
                extent: None,
            },
        )),
        native_ref: Some(native),
    }
}

fn property(kind: &PmDcFeaturePropertyKind, type_id: [u8; 16]) -> TestResult<PmDcFeatureProperty> {
    Ok(serde_json::from_value(serde_json::json!({
        "id": "inventor:pmdc:feature-property#generated-0",
        "segment_token": "generated",
        "record_ordinal": 0,
        "type_id": crate::pmdc::type_id_string(type_id),
        "save_version_major": 19,
        "header": {
            "header_value": 0,
            "header_id": 0,
            "next": PmDcReference::from_packed(0),
            "flags": 0,
            "context": PmDcReference::from_packed(0),
            "source_index": 0
        },
        "kind": kind
    }))?)
}

fn face_item(marker: u16, values: &[u32]) -> TestResult<PmDcFeatureProperty> {
    property(
        &PmDcFeaturePropertyKind::FaceItem {
            index_references: PmDcU32List::new(
                marker,
                (!values.is_empty())
                    .then_some(PmDcListMetadata::U32([u32::try_from(values.len())?, 0])),
                values.to_vec(),
            )
            .ok_or("invalid face-selection fixture list")?,
        },
        FACE_ITEM_TYPE,
    )
}

fn face_collection(marker: u16, references: &[u32]) -> TestResult<PmDcFeatureProperty> {
    property(
        &PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            items: PmDcReferenceList::new(
                marker,
                (!references.is_empty())
                    .then_some(PmDcListMetadata::U32([u32::try_from(references.len())?, 0])),
                references
                    .iter()
                    .copied()
                    .map(PmDcReference::from_packed)
                    .collect(),
            )
            .ok_or("invalid face-selection fixture list")?,
        },
        FACE_COLLECTION_TYPE,
    )
}

#[test]
fn face_items_require_their_record_type_and_nonempty_type_two_indices() -> TestResult {
    let valid = face_item(2, &[0, 0x8000_0001, u32::MAX])?;
    assert!(face_property_is_admitted(&valid, |_| None));
    let wire = serde_json::to_value(&valid)?;
    let restored: PmDcFeatureProperty = serde_json::from_value(wire.clone())?;
    assert_eq!(serde_json::to_value(restored)?, wire);
    assert!(!face_property_is_admitted(&face_item(2, &[])?, |_| None));
    assert!(!face_property_is_admitted(&face_item(3, &[1])?, |_| None));
    let wrong_type = property(&valid.kind, FACE_COLLECTION_TYPE)?;
    assert!(!face_property_is_admitted(&wrong_type, |_| None));
    let wrong_kind = property(
        &PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: false,
        },
        FACE_ITEM_TYPE,
    )?;
    assert!(!face_property_is_admitted(&wrong_kind, |_| None));
    Ok(())
}

#[test]
fn face_collections_require_decoded_face_items() -> TestResult {
    let valid_item = face_item(2, &[0x8000_0001])?;
    let valid_collection = face_collection(2, &[0x8000_0001])?;
    let resolves = |ordinal| (ordinal == 0).then_some(&valid_item);
    assert!(face_property_is_admitted(&valid_collection, resolves));
    assert!(face_property_is_admitted(&face_collection(2, &[])?, |_| {
        None
    }));
    assert!(!face_property_is_admitted(&valid_collection, |_| None));
    for collection in [
        face_collection(2, &[0])?,
        face_collection(2, &[2])?,
        face_collection(3, &[1])?,
    ] {
        assert!(!face_property_is_admitted(&collection, resolves));
    }
    let invalid_items = [
        face_item(2, &[])?,
        face_item(3, &[1])?,
        property(&valid_item.kind, FACE_COLLECTION_TYPE)?,
        property(
            &PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
            FACE_ITEM_TYPE,
        )?,
    ];
    for item in &invalid_items {
        assert!(!face_property_is_admitted(&valid_collection, |_| Some(
            item
        )));
    }
    let wrong_type = property(&valid_collection.kind, FACE_ITEM_TYPE)?;
    assert!(!face_property_is_admitted(&wrong_type, resolves));
    Ok(())
}

fn empty_data() -> super::NativeData {
    super::NativeData {
        storage_bands: Vec::new(),
        databases: Vec::new(),
        database_issues: Vec::new(),
        registry: Vec::new(),
        revisions: Vec::new(),
        pairs: Vec::new(),
        metadata: Vec::new(),
        meta_sections: Vec::new(),
        meta_types: Vec::new(),
        metadata_issues: Vec::new(),
        bulk: Vec::new(),
        records: Vec::new(),
        bulk_issues: Vec::new(),
        unpaired: Vec::new(),
        structural_issues: Vec::new(),
        property_sets: Vec::new(),
        property_sections: Vec::new(),
        properties: Vec::new(),
        property_issues: Vec::new(),
        protein: super::ProteinRecord::Absent { id: String::new() },
        protein_assets: Vec::new(),
        protein_rejections: Vec::new(),
        ufrx: super::UfrxRecord::Absent { id: String::new() },
        assembly_occurrences: Vec::new(),
        assembly_placements: Vec::new(),
        pm_app_default_styles: Vec::new(),
        pm_app_rendering_styles: Vec::new(),
        pm_graphics_faces: Vec::new(),
        pm_graphics_style_collections: Vec::new(),
        pm_graphics_primary_color_styles: Vec::new(),
        face_native_keys: Vec::new(),
        pm_dc_parameters: Vec::new(),
        pm_dc_expressions: Vec::new(),
        pm_dc_units: Vec::new(),
        pm_dc_parameter_tables: Vec::new(),
        pm_dc_model_roots: Vec::new(),
        design_record_issues: Vec::new(),
        pm_dc_sketches: Vec::new(),
        pm_dc_sketch_entities: Vec::new(),
        pm_dc_sketch_constraints: Vec::new(),
        pm_dc_transforms: Vec::new(),
        pm_dc_directions: Vec::new(),
        sketch_record_issues: Vec::new(),
        pm_dc_features: Vec::new(),
        pm_dc_pattern_features: Vec::new(),
        pm_dc_feature_terminators: Vec::new(),
        pm_dc_feature_properties: Vec::new(),
        pm_dc_feature_labels: Vec::new(),
        pm_dc_entity_style_links: Vec::new(),
        pm_dc_kernel_entity_links: Vec::new(),
        pm_dc_work_planes: Vec::new(),
        feature_record_issues: Vec::new(),
        active_carrier: super::ActiveCarrierRecord::NotApplicable { id: String::new() },
        unknowns: Vec::new(),
    }
}

#[test]
fn a_thread_annotation_cannot_use_a_sketch_as_its_native_feature() -> TestResult {
    let mut data = empty_data();
    let null = PmDcReference::from_packed(0);
    let sketch = crate::record_identity::Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 19,
            header: crate::pmdc::PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: null,
                flags: 0,
                flags_extension: None,
                context: null,
                source_index: 0,
                stamps: Vec::new(),
            },
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(8, None, Vec::new())
                .ok_or("invalid sketch fixture list")?,
            transform: null,
            direction: null,
            values: [0; 2],
            auxiliary: None,
        },
        crate::record_identity::RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d11)),
        cadmpeg_ir::identity_key!("generated"),
        0,
    );
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(annotation(sketch.id()));
    data.pm_dc_sketches.push(sketch);
    let mut findings = Vec::new();
    super::validate_features(&ir, &data, &mut findings);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
    assert_eq!(
        findings[0].message,
        "Inventor neutral feature does not resolve to its PmDc source record"
    );
    Ok(())
}

fn thread_data() -> TestResult<super::NativeData> {
    let mut data = empty_data();
    let null_reference = PmDcReference::from_packed(0);
    let header = crate::pmdc::PmDcContentHeader {
        header_value: 0,
        header_id: 0,
        next: null_reference,
        flags: 0,
        flags_extension: None,
        context: null_reference,
        source_index: 0,
        stamps: Vec::new(),
    };
    let properties = [
        face_collection(2, &[2])?,
        face_item(2, &[0x8000_0001])?,
        property(
            &PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
            crate::feature::BOOLEAN_TYPE,
        )?,
        property(
            &PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: PmDcReferenceList::new(2, None, Vec::new())
                    .ok_or("invalid feature fixture list")?,
            },
            crate::pmdc::inventor_id(0x9087_4d51),
        )?,
        property(
            &PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                items: PmDcReferenceList::new(2, None, Vec::new())
                    .ok_or("invalid feature fixture list")?,
            },
            [
                0x71, 0xf2, 0x3e, 0xd8, 0xd2, 0x11, 0x50, 0x94, 0xa0, 0x00, 0x49, 0x80, 0x36, 0x03,
                0xc8, 0xc9,
            ],
        )?,
    ];
    for (property, ordinal) in properties.into_iter().zip([0, 1, 2, 3, 5]) {
        data.pm_dc_feature_properties
            .push(crate::record_identity::Located::new(
                (*property).clone(),
                property.identity.type_id.clone(),
                cadmpeg_ir::identity_key!("generated"),
                ordinal,
            ));
    }
    let slots = PmDcReferenceList::new(
        2,
        Some(PmDcListMetadata::U32([9, 0])),
        [1, 3, 3, 7, 8, 4, 3, 3, 6]
            .into_iter()
            .map(PmDcReference::from_packed)
            .collect(),
    )
    .ok_or("invalid Thread property fixture list")?;
    data.pm_dc_features
        .push(serde_json::from_value(serde_json::json!({
            "id": "inventor:pmdc:feature#generated-4",
            "segment_token": "generated", "record_ordinal": 4,
            "type_id": "914d8790d011f8d10008cabc0663dc09",
            "save_version_major": 19, "header": header,
            "state": 69, "outline_value": 0, "properties": slots, "value": 0
        }))?);
    data.pm_dc_feature_labels.push(
        serde_json::from_value(serde_json::json!({
            "id": "inventor:pmdc:feature-label#generated-8",
            "segment_token": "generated", "record_ordinal": 8,
            "type_id": "2ba4482bd2115864600074b79b49ebb0",
            "save_version_major": 19,
            "header": {
                "header_value": 0, "header_id": 0, "values": [0, 0],
                "owner": PmDcReference::from_packed(5), "parent": null_reference, "next": null_reference
            },
            "index": 1,
            "participants": PmDcReferenceList::new(2, None, Vec::new()).ok_or("invalid feature fixture list")?,
            "name": "", "class_id": crate::feature::THREAD_CLASS_ID
        }))
        ?,
    );
    for ordinal in [6, 7] {
        data.pm_dc_parameters
            .push(crate::record_identity::Located::new(
                crate::design::PmDcParameterPayload {
                    save_version_major: 19,
                    header: header.clone(),
                    name: String::new(),
                    name_value: 0,
                    unit: null_reference,
                    formula: null_reference,
                    nominal_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
                    model_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
                    tolerance: 0,
                    terminal_value: 0,
                },
                crate::record_identity::RecordTypeId::from_bytes(crate::pmdc::inventor_id(
                    0x9087_4d26,
                )),
                cadmpeg_ir::identity_key!("generated"),
                ordinal,
            ));
    }
    for identity in data
        .pm_dc_features
        .iter()
        .map(|record| &record.identity)
        .chain(
            data.pm_dc_feature_properties
                .iter()
                .map(|record| &record.identity),
        )
        .chain(
            data.pm_dc_feature_labels
                .iter()
                .map(|record| &record.identity),
        )
        .chain(data.pm_dc_parameters.iter().map(|record| &record.identity))
    {
        data.records.push(serde_json::from_value(serde_json::json!({
            "id": format!("inventor:rse:record#generated-{}", identity.record_ordinal),
            "token": "generated", "ordinal": identity.record_ordinal,
            "selector": 0, "type_index": 0, "type_id": identity.type_id.as_str(),
            "payload_offset": 0, "payload_len": 0, "payload_sha256": "0".repeat(64),
            "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": "0".repeat(64)
        }))?);
    }
    Ok(data)
}

#[test]
fn thread_validation_requires_decoded_numeric_parameters_and_the_thread_class() -> TestResult {
    let mut data = thread_data()?;
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .features
        .push(annotation(data.pm_dc_features[0].id()));
    let mut findings = Vec::new();
    super::validate_features(&ir, &data, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    let parameter = data
        .pm_dc_parameters
        .pop()
        .ok_or("missing parameter fixture")?;
    super::validate_features(&ir, &data, &mut findings);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Inventor thread annotation does not match its PmDc face selection"
    );
    data.pm_dc_parameters.push(parameter);
    let mut label = serde_json::to_value(&data.pm_dc_feature_labels[0])?;
    label["class_id"] = serde_json::json!("00000000000000000000000000000000");
    data.pm_dc_feature_labels[0] = serde_json::from_value(label)?;
    findings.clear();
    super::validate_features(&ir, &data, &mut findings);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Inventor thread annotation does not match its PmDc face selection"
    );
    Ok(())
}
