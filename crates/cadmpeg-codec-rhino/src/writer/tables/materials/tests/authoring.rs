// SPDX-License-Identifier: Apache-2.0
//! Allowed shared native references retain values through unique target identities.

use super::super::MaterialPlan;
use super::linkage::{read, source};
use crate::loss::RhinoLossCode;
use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::{CadIr, NativeRecord};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const SOURCE_UUID: &str = "11111111-2222-3333-4444-555555555555";
const FIRST_ID: &str = "rhino:model:material#000000";
const SECOND_ID: &str = "test:appearance:material#second";
const FIRST_UUID: &str = "50d0cc16-7836-e3b4-caf0-6e6e4181dbe7";
const SECOND_UUID: &str = "cbc7fd19-eef2-dede-1edc-12cb2ea8efb2";

fn records(ir: &CadIr) -> Vec<&NativeRecord> {
    let archive_index = |record: &NativeRecord| {
        record
            .field("archive_index")
            .and_then(|value| value.as_u64())
            .and_then(|index| usize::try_from(index).ok())
            .expect("authored material has a nonnegative archive index")
    };
    let mut records = ir.native.namespace("rhino").expect("namespace").arenas()["materials"]
        .iter()
        .collect::<Vec<_>>();
    records.sort_by_key(|record| archive_index(record));
    assert_eq!(
        records
            .iter()
            .map(|record| archive_index(record))
            .collect::<Vec<_>>(),
        (0..records.len()).collect::<Vec<_>>(),
        "authored material archive indices are unique and contiguous"
    );
    records
}

fn aliases() -> CadIr {
    let mut ir = source();
    assert_eq!(ir.model.appearances[0].id.as_str(), FIRST_ID);
    let mut second = ir.model.appearances[0].clone();
    second.id = SECOND_ID.try_into().expect("second appearance identity");
    second.name = Some("Second current material".into());
    second.base_color =
        Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("second current color"));
    ir.model.appearances.push(second);
    ir
}

fn uuid_map(ir: &CadIr, plan: &MaterialPlan) -> BTreeMap<String, String> {
    let output = read(&plan.encode(ir).expect("authored materials"), false);
    assert_eq!(
        records(&output)
            .iter()
            .map(|record| record.id())
            .collect::<BTreeSet<_>>()
            .len(),
        records(&output).len()
    );
    ir.model
        .appearances
        .iter()
        .map(|appearance| {
            let index = usize::try_from(
                plan.appearance_index(appearance.id.as_str())
                    .expect("authored appearance index"),
            )
            .expect("nonnegative index");
            (
                appearance.id.to_string(),
                records(&output)[index]
                    .field("source_uuid")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .expect("physical material uuid"),
            )
        })
        .collect()
}

fn written_binding_index(ir: &CadIr, plan: &MaterialPlan, identity: &str) -> i32 {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let payload = crate::writer::object_attributes_payload(ir, identity, None, None, None, plan)
        .expect("bound object attributes");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("attribute read context");
    let mut warnings = crate::loss::Diagnostics::default();
    let attributes = crate::objects::parse_attributes(
        &ctx,
        &payload,
        0..payload.len(),
        0..payload.len(),
        crate::chunks::ArchiveVersion::V8,
        Some(crate::writer::OPENNURBS_WRITER_VERSION),
        &mut warnings,
    )
    .expect("authored binding grammar");
    assert!(warnings.is_empty(), "{warnings:?}");
    attributes.material_index
}

#[test]
fn shared_native_ref_authors_distinct_current_materials_and_reports_exact_guid_reductions() {
    let ir = aliases();
    let before = serde_json::to_value(&ir).expect("current input snapshot");
    let native_id = records(&ir)[0].id();
    let plan = MaterialPlan::new(&ir).expect("allowed material aliases");
    let output = read(&plan.encode(&ir).expect("unique material output"), false);
    assert_eq!(records(&output).len(), 2);
    assert_eq!(
        uuid_map(&ir, &plan),
        BTreeMap::from([
            (FIRST_ID.into(), FIRST_UUID.into()),
            (SECOND_ID.into(), SECOND_UUID.into())
        ])
    );
    assert_eq!(
        records(&output)[0].field("name"),
        Some(json!("Source material"))
    );
    assert_eq!(
        records(&output)[1].field("name"),
        Some(json!("Second current material"))
    );
    assert_eq!(
        records(&output)[1].field("diffuse"),
        Some(
            serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::new([
                0_u8, 255, 0, 0
            ]))
            .expect("second packed color")
        )
    );
    for (ordinal, (record, uuid)) in records(&output)
        .iter()
        .zip([FIRST_UUID, SECOND_UUID])
        .enumerate()
    {
        let mut expected = records(&ir)[0]
            .fields()
            .expect("complete source optical values");
        expected.insert("archive_index".into(), json!(ordinal));
        expected.insert("source_uuid".into(), json!(uuid));
        if ordinal == 1 {
            expected.insert("name".into(), json!("Second current material"));
            expected.insert(
                "diffuse".into(),
                serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::new([
                    0_u8, 255, 0, 0,
                ]))
                .expect("second diffuse channels"),
            );
        }
        assert_eq!(
            record.fields().expect("complete written material values"),
            expected
        );
    }
    assert_eq!(plan.losses().len(), 4);
    for (id, uuid) in [(FIRST_ID, FIRST_UUID), (SECOND_ID, SECOND_UUID)] {
        let canonical=format!("appearance {id} /asset_guid {:?} -> {:?}; distinct Rhino material components require unique physical UUIDs",Some(SOURCE_UUID),Some(uuid));
        let native=format!("appearance {id} linked native {native_id} /source_uuid {} -> {:?}; distinct Rhino material components require unique physical UUIDs",json!(SOURCE_UUID),uuid);
        for message in [canonical, native] {
            assert!(plan.losses().iter().any(|loss| loss.code
                == RhinoLossCode::WriterAttributesOmitted.kind()
                && loss.message == message));
        }
    }
    assert_eq!(serde_json::to_value(&ir).expect("unchanged input"), before);
}

#[test]
fn repeated_and_reordered_shared_materials_keep_stable_canonical_targets_and_bindings() {
    let mut ir = aliases();
    for (ordinal, appearance) in ir.model.appearances.clone().iter().enumerate() {
        let point_id: cadmpeg_ir::ids::PointId = format!("test:model:point#bound-{ordinal}")
            .try_into()
            .expect("point identity");
        ir.model.points.push(cadmpeg_ir::topology::Point::new(
            point_id.clone(),
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                [0.0, 1.0][ordinal],
                0.0,
                0.0,
            ))
            .expect("bound point"),
            None,
        ));
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("test:model:appearance-binding#{ordinal}")
                .try_into()
                .expect("binding identity"),
            native_ref: None,
            target: AppearanceTarget::Point(point_id),
            appearance: appearance.id.clone(),
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    let first = MaterialPlan::new(&ir).expect("first plan");
    let repeated = MaterialPlan::new(&ir).expect("repeat plan");
    assert_eq!(
        first.encode(&ir).expect("first output"),
        repeated.encode(&ir).expect("repeat output")
    );
    let identities = uuid_map(&ir, &first);
    for ordinal in 0..2 {
        assert_eq!(
            Some(written_binding_index(
                &ir,
                &first,
                &format!("test:model:point#bound-{ordinal}")
            )),
            Some(ordinal)
        );
    }
    ir.model.appearances.reverse();
    let reordered = MaterialPlan::new(&ir).expect("reordered plan");
    assert_eq!(uuid_map(&ir, &reordered), identities);
    assert_eq!(
        Some(written_binding_index(
            &ir,
            &reordered,
            "test:model:point#bound-0"
        )),
        Some(1)
    );
    assert_eq!(
        Some(written_binding_index(
            &ir,
            &reordered,
            "test:model:point#bound-1"
        )),
        Some(0)
    );
}

#[test]
fn native_only_uuid_reservation_resolves_a_deterministic_authored_collision() {
    let mut ir = source();
    let mut fields = records(&ir)[0].fields().expect("native source fields");
    let collision = "b1406d4e-a020-cb0c-1446-871882de623b";
    fields.insert("source_uuid".into(), json!(collision));
    let record = NativeRecord::new(
        format!("rhino:presentation:material#{collision}")
            .try_into()
            .expect("reserved native identity"),
        fields,
    )
    .expect("current native material");
    let native_id = record.id().to_owned();
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("materials".into(), vec![record]);
    ir.model.appearances[0].id = "test:appearance:material#collision"
        .try_into()
        .expect("colliding authored identity");
    ir.model.appearances[0].native_ref = None;
    ir.model.appearances[0].asset_guid = None;
    let plan = MaterialPlan::new(&ir).expect("collision-resolving plan");
    let output = read(&plan.encode(&ir).expect("collision-resolved output"), false);
    assert_eq!(records(&output).len(), 2);
    assert_eq!(
        records(&output)[0].field("source_uuid"),
        Some(json!("b1406d4e-a020-cb0c-1446-871882de623c"))
    );
    assert_eq!(
        records(&output)[1].field("source_uuid"),
        Some(json!(collision))
    );
    assert_eq!(plan.native_targets(&native_id), [1]);
    let repeat = MaterialPlan::new(&ir).expect("repeat collision plan");
    assert_eq!(
        plan.encode(&ir).expect("first collision output"),
        repeat.encode(&ir).expect("repeat collision output")
    );
}

#[test]
fn unique_one_to_one_and_native_only_uuid_states_remain_exact() {
    let mut ir = source();
    let before = records(&ir)[0].fields().expect("source native fields");
    let plan = MaterialPlan::new(&ir).expect("one counterpart");
    assert!(plan.losses().is_empty());
    let output = read(&plan.encode(&ir).expect("one-to-one output"), false);
    assert_eq!(
        records(&output)[0].fields().expect("one-to-one fields"),
        before
    );
    ir.model.appearances.clear();
    let plan = MaterialPlan::new(&ir).expect("native-only plan");
    assert!(plan.losses().is_empty());
    let output = read(&plan.encode(&ir).expect("native-only output"), false);
    assert_eq!(
        records(&output)[0].fields().expect("native-only fields"),
        before
    );
}

#[test]
fn ambiguous_current_selector_omits_only_that_setting_with_its_actual_targets() {
    let mut ir = source();
    let native_id = records(&ir)[0].id().to_owned();
    let setting_id = "rhino:document:settings#selector";
    let record=NativeRecord::new(setting_id.try_into().expect("setting identity"),serde_json::from_value(json!({"current_material_index":0,"current_material_source":2,"model_url":"https://example.invalid/keep"})).expect("setting values")).expect("setting record");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("document_settings".into(), vec![record]);
    let plan = MaterialPlan::new(&ir).expect("single target plan");
    let mut losses = Vec::new();
    let single = crate::writer::tables::document::settings(&ir, &plan, &mut losses)
        .expect("single target setting");
    assert!(losses.is_empty());
    let selector = crate::writer::crc_chunk(
        0x2000_8039,
        &[0_i32.to_le_bytes(), 2_i32.to_le_bytes()].concat(),
    )
    .expect("selector");
    assert!(single.contains(&selector));
    let mut second = ir.model.appearances[0].clone();
    second.id = SECOND_ID.try_into().expect("second alias identity");
    ir.model.appearances.push(second);
    let before = serde_json::to_value(&ir).expect("selector input");
    let plan = MaterialPlan::new(&ir).expect("allowed alias selector plan");
    let multiple = crate::writer::tables::document::settings(&ir, &plan, &mut losses)
        .expect("setting-only reduction");
    assert!(!multiple.contains(&selector));
    assert_eq!(multiple.len() + 1, single.len());
    let names = vec![FIRST_ID.to_owned(), SECOND_ID.to_owned()];
    let expected=format!("document setting {setting_id} /current_material_index 0 -> absent; native material {native_id} has multiple authored appearances {names:?}, so no unique target exists");
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        RhinoLossCode::WriterAttributesOmitted.kind()
    );
    assert_eq!(losses[0].message, expected);
    assert_eq!(
        serde_json::to_value(&ir).expect("unchanged selector input"),
        before
    );
    assert_eq!(
        read(
            &plan.encode(&ir).expect("both materials still written"),
            false
        )
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["materials"]
            .len(),
        2
    );
}

#[test]
fn explicit_unique_current_asset_guid_wins_and_reports_only_the_changed_native_uuid() {
    let mut ir = source();
    let edited = "77777777-8888-9999-aaaa-bbbbbbbbbbbb";
    ir.model.appearances[0].asset_guid = Some(edited.into());
    let before = serde_json::to_value(&ir).expect("edited material snapshot");
    let plan = MaterialPlan::new(&ir).expect("explicit current asset identity");
    let output = read(&plan.encode(&ir).expect("edited asset output"), false);
    assert_eq!(records(&output).len(), 1);
    assert_eq!(
        records(&output)[0].field("source_uuid"),
        Some(json!(edited))
    );
    let mut expected = records(&ir)[0].fields().expect("original native values");
    expected.insert("source_uuid".into(), json!(edited));
    assert_eq!(
        records(&output)[0]
            .fields()
            .expect("written material fields"),
        expected
    );
    assert_eq!(plan.losses().len(), 1);
    assert_eq!(
        plan.losses()[0].code,
        RhinoLossCode::WriterAttributesOmitted.kind()
    );
    assert_eq!(plan.losses()[0].message, format!(
        "appearance {FIRST_ID} linked native {} /source_uuid {} -> {edited:?}; distinct Rhino material components require unique physical UUIDs",
        records(&ir)[0].id(), json!(SOURCE_UUID)
    ));
    assert_eq!(
        serde_json::to_value(&ir).expect("unchanged current document"),
        before
    );
}

#[test]
fn unchanged_one_to_one_uuid_is_reserved_before_an_independent_canonical_collision() {
    let mut ir = source();
    let original = records(&ir)[0]
        .fields()
        .expect("original native material values");
    let original_native = records(&ir)[0].id().to_owned();
    let mut independent = ir.model.appearances[0].clone();
    independent.id = SECOND_ID.try_into().expect("independent identity");
    independent.native_ref = None;
    ir.model.appearances.insert(0, independent);
    let plan = MaterialPlan::new(&ir).expect("native UUID reservation plan");
    let output = read(
        &plan.encode(&ir).expect("independent UUID collision output"),
        false,
    );
    let mapping = uuid_map(&ir, &plan);
    assert_eq!(mapping[FIRST_ID], SOURCE_UUID);
    assert_eq!(mapping[SECOND_ID], SECOND_UUID);
    assert_eq!(plan.native_targets(&original_native), [1]);
    let mut expected = original;
    expected.insert("archive_index".into(), json!(1));
    assert_eq!(
        records(&output)[1]
            .fields()
            .expect("unchanged native counterpart"),
        expected
    );
    assert_eq!(plan.losses().len(), 1);
    assert_eq!(
        plan.losses()[0].code,
        RhinoLossCode::WriterAttributesOmitted.kind()
    );
    assert_eq!(plan.losses()[0].message, format!(
        "appearance {SECOND_ID} /asset_guid {:?} -> {:?}; distinct Rhino material components require unique physical UUIDs",
        Some(SOURCE_UUID), Some(SECOND_UUID)
    ));
}
