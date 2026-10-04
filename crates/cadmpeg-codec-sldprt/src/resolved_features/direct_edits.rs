//! Direct face and body edit inputs.

use super::axes::{compact_line_reference_directions, declared_line_reference_directions};
use super::scalars::feature_object_name;
use crate::classification::{classify, FeatureClass};
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::View;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDirection3, FiniteVector3};
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::BTreeMap;

const EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E9: f64 = 1.0e-9;
const EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E12: f64 = 1.0e-12;
const EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12: f64 = 1.0e-12;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct MoveBodyTranslationRecord {
    pub(super) selection_offset: usize,
    pub(super) local_body_ids: Vec<u32>,
    translation_m: FiniteVector3,
}

pub(super) fn move_body_translation_record(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    data_class_offset: u64,
) -> Result<Option<MoveBodyTranslationRecord>, CodecError> {
    const TRAILER_OFFSET: usize = 200;
    const NON_COPY_TRAILER: [u8; 8] = [1, 0, 0, 0, 0, 0, 1, 0];
    const OPERATION: &str = "decode SLDPRT move-body translation record";
    let (Ok(data_class_offset), Some(end)) = (
        usize::try_from(data_class_offset),
        super::DeclaredEnd::of(object_end, payload.len()),
    ) else {
        return Ok(None);
    };
    let end = end.get();
    if data_class_offset < object_start || data_class_offset >= end {
        return Ok(None);
    }
    let scalar = |offset: usize| {
        let value = View::f64_le_at(payload, offset)?;
        FiniteReal::new(value)
    };
    let mut candidate = None;
    let Some(scan_end) = end.checked_sub(TRAILER_OFFSET + 20) else {
        return Ok(None);
    };
    for selection_offset in data_class_offset..scan_end {
        ctx.charge_work(400, OPERATION)?;
        let Some(count) = View::u32_le_at(payload, selection_offset)
            .and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        if !(1..=4096).contains(&count) {
            continue;
        }
        let ids_start = selection_offset + 4;
        let Some(ids_end) = count
            .checked_mul(4)
            .and_then(|length| ids_start.checked_add(length))
        else {
            continue;
        };
        let Some(matrix_offset) = ids_end.checked_add(12) else {
            continue;
        };
        if matrix_offset
            .checked_add(TRAILER_OFFSET + 8)
            .is_none_or(|required| required > end)
            || payload.get(ids_end..ids_end + 4) != Some(u32::MAX.to_le_bytes().as_slice())
            || payload.get(ids_end + 4..matrix_offset) != Some([0; 8].as_slice())
            || payload.get(matrix_offset + TRAILER_OFFSET..matrix_offset + TRAILER_OFFSET + 8)
                != Some(NON_COPY_TRAILER.as_slice())
        {
            continue;
        }
        let matrix = (|| {
            let mut values = [FiniteReal::ZERO; 9];
            for (index, value) in values.iter_mut().enumerate() {
                *value = scalar(matrix_offset + index * 8)?;
            }
            Some(values)
        })();
        let Some(matrix) = matrix else {
            continue;
        };
        if matrix
            .iter()
            .zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
            .any(|(actual, expected)| {
                (actual.get() - expected).abs() > EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E9
            })
            || payload.get(matrix_offset + 72..matrix_offset + 80)
                != Some(1u64.to_le_bytes().as_slice())
            || (0..3).any(|index| scalar(matrix_offset + 80 + index * 8).is_none())
            || scalar(matrix_offset + 104).is_none_or(|value| {
                (value.get() - 1.0).abs() > EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E9
            })
        {
            continue;
        }
        let (Some(x), Some(y), Some(z)) = (
            scalar(matrix_offset + 112),
            scalar(matrix_offset + 120),
            scalar(matrix_offset + 128),
        ) else {
            continue;
        };
        let canonical = |component: FiniteReal| {
            if component.get().abs() <= EPS_DIRECT_EDITS_MOVE_BODY_TRANSLATION_RECORD_E12 {
                FiniteReal::ZERO
            } else {
                component
            }
        };
        let translation_m =
            FiniteVector3::from_components(canonical(x), canonical(y), canonical(z));
        let Some(ids) = payload.get(ids_start..ids_end) else {
            continue;
        };
        let local_body_ids = super::selections::read_compact_body_ids(ctx, ids, OPERATION)?;
        ctx.charge_work(u64_from_index(local_body_ids.len()), OPERATION)?;
        if local_body_ids.contains(&0) {
            continue;
        }
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some(MoveBodyTranslationRecord {
            selection_offset,
            local_body_ids,
            translation_m,
        });
    }
    Ok(candidate)
}

pub(super) fn move_body_selection_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    Ok(
        move_body_translation_record(ctx, payload, offset, payload.len(), u64_from_index(offset))?
            .filter(|record| record.selection_offset == offset)
            .map(|record| record.local_body_ids),
    )
}

/// Charge each Move Face candidate and its per-feature slot before insertion.
fn push_move_face_candidate(
    ctx: &DecodeContext<'_>,
    candidates: &mut BTreeMap<(usize, usize), Vec<Option<FeatureDirection3>>>,
    key: (usize, usize),
    candidate: Option<FeatureDirection3>,
) -> Result<(), CodecError> {
    ctx.push_btree_group(
        candidates,
        key,
        candidate,
        "index SLDPRT move-face directions",
        "collect SLDPRT move-face directions",
    )
}

/// Add translation laws carried by Move Face direction-spec children.
pub(crate) fn enrich_history_move_face_translations(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut candidates = BTreeMap::<(usize, usize), Vec<Option<FeatureDirection3>>>::new();
    for lane in lanes {
        let mut starts = Vec::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                ctx.charge_work(1, "scan SLDPRT move-face feature starts")?;
                if let Some(name) = feature_object_name(feature, lane) {
                    ctx.reserve_vec(&mut starts, 1, "collect SLDPRT move-face feature starts")?;
                    starts.push((name.offset, history_index, feature_index));
                }
            }
        }
        ctx.sort_unstable_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT move-face feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if classify(ctx, feature)? != Some(FeatureClass::MoveFace)
                || feature.properties.contains_key("Mode")
                || feature.properties.contains_key("Direction")
            {
                continue;
            }
            let Some(end) = super::DeclaredEnd::of(
                starts
                    .get(index + 1)
                    .and_then(|entry| usize::try_from(entry.0).ok())
                    .unwrap_or(lane.native_payload.len()),
                lane.native_payload.len(),
            )
            .map(super::DeclaredEnd::get) else {
                continue;
            };
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            if start >= end {
                push_move_face_candidate(
                    ctx,
                    &mut candidates,
                    (history_index, feature_index),
                    None,
                )?;
                continue;
            }
            ctx.charge_work(
                u64_from_index(lane.classes.len()),
                "scan SLDPRT move-face direction classes",
            )?;
            let direction_specs = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moDirectionSpec_c"
                        && (u64_from_index(start)..u64_from_index(end)).contains(&class.offset)
                })
                .count();
            let mut line_refs = lane.classes.iter().filter(|class| {
                class.name == "moLineRef_w"
                    && (u64_from_index(start)..u64_from_index(end)).contains(&class.offset)
            });
            let line_ref = line_refs.next();
            if direction_specs != 1 || line_ref.is_none() || line_refs.next().is_some() {
                push_move_face_candidate(
                    ctx,
                    &mut candidates,
                    (history_index, feature_index),
                    None,
                )?;
                continue;
            }
            let Some(line_ref) = line_ref else {
                continue;
            };
            let mut directions = declared_line_reference_directions(
                ctx,
                &lane.native_payload,
                line_ref.offset,
                end,
            )?;
            let excluded_handles = std::iter::once(line_ref)
                .filter_map(|class| usize::try_from(class.offset).ok())
                .flat_map(|offset| [offset + 136, offset + 144])
                .collect::<Vec<_>>();
            let compact = compact_line_reference_directions(
                ctx,
                &lane.native_payload,
                start,
                end,
                &excluded_handles,
            )?;
            ctx.reserve_capacity(
                &mut directions,
                compact.len(),
                "merge SLDPRT move-face directions",
            )?;
            directions.extend(compact);
            let mut unique = Vec::new();
            for direction in directions
                .into_iter()
                .map(FeatureDirection3::from_unit_without_small_components)
            {
                if !unique.contains(&direction) {
                    ctx.reserve_vec(&mut unique, 1, "collect SLDPRT unique move-face directions")?;
                    unique.push(direction);
                }
            }
            push_move_face_candidate(
                ctx,
                &mut candidates,
                (history_index, feature_index),
                match unique.as_slice() {
                    [direction] => Some(*direction),
                    _ => None,
                },
            )?;
        }
    }
    for ((history_index, feature_index), candidates) in candidates {
        let Some((&Some(first), rest)) = candidates.split_first() else {
            continue;
        };
        if rest.iter().any(|candidate| {
            candidate.is_none_or(|candidate| {
                let candidate = candidate.get();
                let first = first.get();
                (candidate.x - first.x).abs()
                    > EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12
                    || (candidate.y - first.y).abs()
                        > EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12
                    || (candidate.z - first.z).abs()
                        > EPS_DIRECT_EDITS_ENRICH_HISTORY_MOVE_FACE_TRANSLATIONS_E12
            })
        }) {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        ctx.charge_collection_items(2, "insert SLDPRT move-face direction properties")?;
        feature
            .properties
            .insert(cadmpeg_core::nonblank_literal!("Mode"), "Translate".into());
        let direction = ctx.format_retained(
            format_args!("{},{},{}", first.get().x, first.get().y, first.get().z),
            "format SLDPRT move-face direction",
        )?;
        feature
            .properties
            .insert(cadmpeg_core::nonblank_literal!("Direction"), direction);
    }
    Ok(())
}

/// Add non-copy translations carried by Move/Copy Body data children.
pub(crate) fn enrich_history_move_body_translations(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut candidates = BTreeMap::<(usize, usize), Vec<Option<FiniteVector3>>>::new();
    for lane in lanes {
        let mut starts = Vec::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                ctx.charge_work(1, "scan SLDPRT move-body feature starts")?;
                if let Some(name) = feature_object_name(feature, lane) {
                    ctx.reserve_vec(&mut starts, 1, "collect SLDPRT move-body feature starts")?;
                    starts.push((name.offset, history_index, feature_index));
                }
            }
        }
        ctx.sort_unstable_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT move-body feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if classify(ctx, feature)? != Some(FeatureClass::MoveBody)
                || feature.properties.contains_key("Translation")
            {
                continue;
            }
            let Some(end) = super::DeclaredEnd::of(
                starts
                    .get(index + 1)
                    .and_then(|entry| usize::try_from(entry.0).ok())
                    .unwrap_or(lane.native_payload.len()),
                lane.native_payload.len(),
            )
            .map(super::DeclaredEnd::get) else {
                continue;
            };
            let Some(start) = usize::try_from(start).ok().filter(|start| *start < end) else {
                continue;
            };
            let mut data_classes = lane.classes.iter().filter(|class| {
                class.name == "moMoveCopyBodyData_c"
                    && (u64_from_index(start)..u64_from_index(end)).contains(&class.offset)
            });
            let candidate = match (data_classes.next(), data_classes.next()) {
                (Some(class), None) => move_body_translation_record(
                    ctx,
                    &lane.native_payload,
                    start,
                    end,
                    class.offset,
                )?
                .map(|record| record.translation_m),
                _ => None,
            };
            let key = (history_index, feature_index);
            ctx.push_btree_group(
                &mut candidates,
                key,
                candidate,
                "index SLDPRT move-body candidates",
                "collect SLDPRT move-body candidates",
            )?;
        }
    }
    for ((history_index, feature_index), candidates) in candidates {
        let Some((&Some(first), rest)) = candidates.split_first() else {
            continue;
        };
        if rest.iter().any(|candidate| *candidate != Some(first)) {
            continue;
        }
        let first = first.get();
        let properties = &mut histories[history_index].features[feature_index].properties;
        let translation = ctx.format_retained(
            format_args!(
                "{}mm,{}mm,{}mm",
                first.x * 1000.0,
                first.y * 1000.0,
                first.z * 1000.0
            ),
            "format SLDPRT move-body translation",
        )?;
        ctx.insert_btree_map(
            properties,
            cadmpeg_core::nonblank_literal!("Translation"),
            translation,
            "insert SLDPRT move-body translation",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        enrich_history_move_body_translations, enrich_history_move_face_translations,
        move_body_translation_record, MoveBodyTranslationRecord,
    };
    use crate::records::FeatureInputLane;
    use crate::records::FeatureSource;
    use crate::records::ObjectId;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputName, FeatureInputScalar,
        FeatureInputScalarRole,
    };

    use cadmpeg_core::decode::u64_from_index;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::{
        features::{FaceMotion, FaceSelection, FeatureDefinition, FeatureOperation, FiniteVector3},
        scalar::{FiniteReal, Length},
    };
    use std::collections::BTreeMap;

    fn enrich_history_move_face_translations_test(
        histories: &mut [FeatureHistory],
        lanes: &[FeatureInputLane],
    ) {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let bytes = lanes
            .first()
            .map_or(&[][..], |lane| lane.native_payload.as_slice());
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("move-face test input fits service policy");
        enrich_history_move_face_translations(&ctx, histories, lanes)
            .expect("move-face test enrichment succeeds");
    }

    fn move_face_history() -> FeatureHistory {
        FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![Feature {
                id: "move-face".into(),
                parent: "history".into(),
                xml_tag: "Feature".into(),
                tree_parent: None,
                source_id: FeatureSource::from_value(7),
                ordinal: 0,
                name: "Move Face".into(),
                kind: "Move Face".into(),
                input_class: Some("moMoveFace_c".into()),
                suppressed: false,
                parameters: BTreeMap::from([(cadmpeg_core::nonblank_literal!("D1"), "0.2".into())]),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        }
    }

    fn move_body_enrichment_input() -> (Vec<FeatureHistory>, FeatureInputLane) {
        let mut histories = vec![move_face_history()];
        let feature = &mut histories[0].features[0];
        feature.name = "Move Body".into();
        feature.kind = "Body-Move/Copy".into();
        feature.input_class = Some("moMoveCopyBody_c".into());
        let mut lane = line_reference_lane(&[], 0);
        lane.names[0].value = "Move Body".into();
        (histories, lane)
    }

    #[test]
    fn move_body_enrichment_refuses_collection_limit() {
        let (mut histories, lane) = move_body_enrichment_input();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &policy,
        )
        .expect("move-body input fits root policy");
        let error = enrich_history_move_body_translations(
            &ctx,
            &mut histories,
            std::slice::from_ref(&lane),
        )
        .expect_err("one matching feature start requires collection admission");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn move_body_enrichment_refuses_work_limit() {
        let (mut histories, lane) = move_body_enrichment_input();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &policy,
        )
        .expect("move-body input fits root policy");
        let error = enrich_history_move_body_translations(
            &ctx,
            &mut histories,
            std::slice::from_ref(&lane),
        )
        .expect_err("one feature scan requires work admission");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn move_body_enrichment_refuses_retained_limit() {
        let (mut histories, mut lane) = move_body_enrichment_input();
        let selection_offset = 64;
        let payload = &mut lane.native_payload;
        payload[selection_offset..selection_offset + 4].copy_from_slice(&2u32.to_le_bytes());
        payload[selection_offset + 4..selection_offset + 8].copy_from_slice(&17u32.to_le_bytes());
        payload[selection_offset + 8..selection_offset + 12].copy_from_slice(&23u32.to_le_bytes());
        payload[selection_offset + 12..selection_offset + 16]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        let matrix_offset = selection_offset + 24;
        for (index, value) in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .enumerate()
        {
            payload[matrix_offset + index * 8..matrix_offset + index * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        payload[matrix_offset + 72..matrix_offset + 80].copy_from_slice(&1u64.to_le_bytes());
        payload[matrix_offset + 104..matrix_offset + 112].copy_from_slice(&1.0f64.to_le_bytes());
        for (index, value) in [0.01f64, -0.02, 0.03].into_iter().enumerate() {
            payload[matrix_offset + 112 + index * 8..matrix_offset + 120 + index * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        payload[matrix_offset + 200..matrix_offset + 208]
            .copy_from_slice(&[1, 0, 0, 0, 0, 0, 1, 0]);
        lane.classes.push(FeatureInputClass {
            id: "move-body-data".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: u64_from_index(selection_offset),
            name: "moMoveCopyBodyData_c".into(),
        });

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let service = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &service,
        )
        .expect("move-body input fits root policy");
        enrich_history_move_body_translations(&ctx, &mut histories, std::slice::from_ref(&lane))
            .expect("move-body translation fits service policy");
        assert!(histories[0].features[0]
            .properties
            .contains_key("Translation"));
        histories[0].features[0].properties.remove("Translation");

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &policy,
        )
        .expect("move-body input fits root policy");
        let error = enrich_history_move_body_translations(
            &ctx,
            &mut histories,
            std::slice::from_ref(&lane),
        )
        .expect_err("translation text requires retained bytes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    fn line_reference_lane(directions: &[Vector3], direction_specs: usize) -> FeatureInputLane {
        let mut native_payload = vec![0; 640];
        for (index, direction) in directions.iter().enumerate() {
            let handle = 160 + index * 160;
            native_payload[handle..handle + 8]
                .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
            native_payload[handle + 104..handle + 112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
            for (component, value) in [direction.x, direction.y, direction.z]
                .into_iter()
                .enumerate()
            {
                let offset = handle + 64 + component * 8;
                native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        let mut classes = (0..direction_specs)
            .map(|index| FeatureInputClass {
                id: format!("direction-spec-{index}"),
                parent: "lane".into(),
                ordinal: u32::try_from(index).expect("test index fits u32"),
                offset: 32 + u64_from_index(index),
                name: "moDirectionSpec_c".into(),
            })
            .collect::<Vec<_>>();
        classes.push(FeatureInputClass {
            id: "line-ref".into(),
            parent: "lane".into(),
            ordinal: u32::try_from(direction_specs).expect("test direction spec count fits u32"),
            offset: 80,
            name: "moLineRef_w".into(),
        });
        FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes,
            names: vec![
                FeatureInputName {
                    id: "name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 8,
                    value: "Move Face".into(),
                    object_id: ObjectId::from_value(7),
                },
                FeatureInputName {
                    id: "d1-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 100,
                    value: "D1".into(),
                    object_id: None,
                },
            ],
            scalars: vec![FeatureInputScalar {
                id: "d1-scalar".into(),
                parent: "lane".into(),
                feature_ref: Some("move-face".into()),
                ordinal: 0,
                offset: 128,
                object_id: 8,
                name: "d1-name".into(),
                value: cadmpeg_ir::scalar::FiniteReal::new(0.005).expect("finite test scalar"),
                role: FeatureInputScalarRole::Driving,

                operands: Vec::new(),
            }],
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }
    }

    #[test]
    fn move_face_translation_requires_one_direction_spec_and_one_direction() {
        let mut histories = vec![move_face_history()];
        let lane = line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 1);
        enrich_history_move_face_translations_test(&mut histories, std::slice::from_ref(&lane));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &lane.native_payload,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("move-face lane fits service policy");
        crate::resolved_features::parameters::enrich_history_parameters(
            &ctx,
            &mut histories,
            [&lane],
            true,
        )
        .expect("move-face parameter enrichment succeeds");
        assert_eq!(histories[0].features[0].parameters["D1"], "5mm");
        let projected = crate::history::project::project_features(
            &cadmpeg_test_support::service_decode_context(),
            &histories,
        )
        .unwrap();
        assert!(matches!(
            projected[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::MoveFace {
                faces: FaceSelection::Unresolved,
                motion: FaceMotion::Translate { direction, distance },
            }) if *direction == Vector3::new(0.0, -1.0, 0.0) && *distance == Length::new(5.0).unwrap()
        ));

        for lane in [
            line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 0),
            line_reference_lane(
                &[Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)],
                1,
            ),
        ] {
            let mut histories = vec![move_face_history()];
            enrich_history_move_face_translations_test(&mut histories, &[lane]);
            assert!(matches!(
                crate::history::project::project_features(
                    &cadmpeg_test_support::service_decode_context(),
                    &histories
                )
                .unwrap()[0]
                    .evaluation
                    .definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ));
        }

        let mut histories = vec![move_face_history()];
        enrich_history_move_face_translations_test(
            &mut histories,
            &[
                line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 1),
                line_reference_lane(&[Vector3::new(0.0, -1.0, 0.0)], 0),
            ],
        );
        assert!(matches!(
            crate::history::project::project_features(
                &cadmpeg_test_support::service_decode_context(),
                &histories
            )
            .unwrap()[0]
                .evaluation
                .definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
    }

    #[test]
    fn move_body_translation_requires_fixed_identity_and_non_copy_trailer() {
        let selection_offset = 64;
        let mut payload = vec![0; 384];
        payload[selection_offset..selection_offset + 4].copy_from_slice(&2u32.to_le_bytes());
        payload[selection_offset + 4..selection_offset + 8].copy_from_slice(&17u32.to_le_bytes());
        payload[selection_offset + 8..selection_offset + 12].copy_from_slice(&23u32.to_le_bytes());
        payload[selection_offset + 12..selection_offset + 16]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        let matrix_offset = selection_offset + 24;
        for (index, value) in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .enumerate()
        {
            payload[matrix_offset + index * 8..matrix_offset + index * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        payload[matrix_offset + 72..matrix_offset + 80].copy_from_slice(&1u64.to_le_bytes());
        payload[matrix_offset + 104..matrix_offset + 112].copy_from_slice(&1.0f64.to_le_bytes());
        for (index, value) in [0.01f64, -0.02, 0.03].into_iter().enumerate() {
            payload[matrix_offset + 112 + index * 8..matrix_offset + 120 + index * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        payload[matrix_offset + 200..matrix_offset + 208]
            .copy_from_slice(&[1, 0, 0, 0, 0, 0, 1, 0]);

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        assert_eq!(
            move_body_translation_record(&ctx, &payload, 0, payload.len(), 0).unwrap(),
            Some(MoveBodyTranslationRecord {
                selection_offset,
                local_body_ids: vec![17, 23],
                translation_m: FiniteVector3::from_components(
                    FiniteReal::new(0.01).expect("finite x"),
                    FiniteReal::new(-0.02).expect("finite y"),
                    FiniteReal::new(0.03).expect("finite z"),
                ),
            })
        );
        let mut rotated = payload.clone();
        rotated[matrix_offset..matrix_offset + 8].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(
            move_body_translation_record(&ctx, &rotated, 0, rotated.len(), 0).unwrap(),
            None
        );
        payload[matrix_offset + 200] = 0;
        assert_eq!(
            move_body_translation_record(&ctx, &payload, 0, payload.len(), 0).unwrap(),
            None
        );
    }
}
