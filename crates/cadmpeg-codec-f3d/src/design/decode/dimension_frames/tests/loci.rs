// SPDX-License-Identifier: Apache-2.0

use super::super::{
    find_dimension_locus_groups, find_dimension_locus_pair, find_dimension_null_locus_pair,
    parse_dimension_locus_group, parse_dimension_locus_pair, parse_dimension_null_locus_pair,
};
use super::TEST_LINEAR_TOLERANCE;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    null_locus_dimension_definition, remove_dimension_frame_relations,
};
use crate::design::test_support::parameter_record;
use crate::records::{parameters::DesignParameterOwner, sketch_relations::SketchRelation};

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchAxis, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use std::collections::HashSet;

fn frame(
    entity_id: u64,
    start: usize,
    paired: usize,
    end: usize,
) -> crate::metastream::PrimaryRecordFrame {
    crate::metastream::PrimaryRecordFrame {
        entity_id,
        start,
        member_end: paired,
        end,
    }
}

#[test]
fn damaged_locus_frames_keep_valid_siblings_and_missing_geometry_is_not_damage() {
    for count in [2u32, 3] {
        let mut bytes = Vec::new();
        let mut frames = Vec::new();
        for (index, damaged) in [(233u32, true), (234, false)] {
            let start = bytes.len();
            crate::test_support::indexed_header(&mut bytes, *b"277", index);
            bytes.extend_from_slice(&[0; 8]);
            bytes.push(1);
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.push(1);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            for geometry in (0..count - 1).map(|i| 192 + i) {
                bytes.push(1);
                bytes.extend_from_slice(&geometry.to_le_bytes());
                let reserved = bytes.len();
                bytes.extend_from_slice(&[0; 6]);
                if damaged {
                    bytes[reserved] = 1;
                }
                bytes.extend_from_slice(&1u32.to_le_bytes());
            }
            bytes.push(0);
            let paired = bytes.len();
            crate::test_support::indexed_header(&mut bytes, *b"273", index);
            frames.push(frame(u64::from(index), start, paired, bytes.len()));
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, source) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut parts =
            cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
        for frame in &frames {
            super::super::contain_dimension_locus_frame(
                &ctx,
                &mut parts,
                source,
                "Design/BulkStream.dat",
                frame,
                228,
                count,
            )
            .unwrap();
        }
        let good = &frames[1];
        let geometry = HashSet::from([192, 193]);
        let result = if count == 3 {
            parse_dimension_locus_pair(&bytes, good, 228, &geometry)
        } else {
            parse_dimension_null_locus_pair(&bytes, good, 228, &geometry)
        };
        assert_eq!(result.unwrap().record_index, 234);
        let unresolved = if count == 3 {
            parse_dimension_locus_pair(&bytes, good, 228, &HashSet::new())
        } else {
            parse_dimension_null_locus_pair(&bytes, good, 228, &HashSet::new())
        };
        assert!(unresolved.is_none());
        let (_, native, fidelity, losses) = parts.into_parts();
        assert_eq!(
            native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
            1
        );
        assert!(fidelity
            .unrepresented()
            .contains(&cadmpeg_ir::document::ArenaClass::Design));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "record.unreadable");
    }
}

#[test]
fn damaged_counted_locus_group_keeps_valid_sibling() {
    let mut bytes = Vec::new();
    let mut frames = Vec::new();
    for (index, return_count) in [(249u32, 3u32), (250, 2)] {
        let start = bytes.len();
        crate::test_support::indexed_header(&mut bytes, *b"286", index);
        bytes.extend_from_slice(&[0; 8]);
        bytes.push(1);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for geometry in [175u32, 217] {
            bytes.push(1);
            bytes.extend_from_slice(&geometry.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&1u32.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 1]);
        bytes.extend_from_slice(&172u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&return_count.to_le_bytes());
        for geometry in [217u32, 175] {
            bytes.push(1);
            bytes.extend_from_slice(&geometry.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.push(0);
        let end = bytes.len();
        frames.push(frame(u64::from(index), start, end, end));
    }
    crate::test_support::indexed_header(&mut bytes, *b"314", 251);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, source) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let sketches = HashSet::from([172]);
    for frame in &frames {
        super::super::contain_dimension_locus_group(
            &ctx,
            &mut parts,
            source,
            "Design/BulkStream.dat",
            frame,
            240,
            &sketches,
        )
        .unwrap();
    }
    let group = parse_dimension_locus_group(
        &bytes,
        frames[1].start,
        240,
        &HashSet::from([175, 217]),
        &sketches,
    )
    .unwrap();
    assert_eq!(group.record_index, 250);
    assert!(
        parse_dimension_locus_group(&bytes, frames[1].start, 240, &HashSet::new(), &sketches)
            .is_none()
    );
    let (_, native, fidelity, losses) = parts.into_parts();
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert!(fidelity
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
}

#[test]
fn dimension_locus_pair_resolves_two_typed_geometry_records() {
    let mut bytes = vec![0; 80];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&233u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&3u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&4u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&192u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&0u32.to_le_bytes());
    bytes[54] = 1;
    bytes[55..59].copy_from_slice(&194u32.to_le_bytes());
    bytes[65..69].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&233u32.to_le_bytes());

    let mut pair = parse_dimension_locus_pair(
        &bytes,
        &frame(233, 0, 80, bytes.len()),
        228,
        &HashSet::from([192, 194]),
    )
    .expect("paired dimension locus frame");
    pair.id = "f3d:Design/BulkStream.dat:design-dimension-locus-pair#0".into();
    assert_eq!(pair.companion_record_index, 228);
    assert_eq!(pair.record_index, 233);
    assert_eq!(pair.frame_length(), Some(80));
    assert_eq!(pair.loci()[0].geometry_index(), 192);
    assert_eq!(pair.loci()[0].role, 0);
    assert_eq!(pair.loci()[1].geometry_index(), 194);
    assert_eq!(pair.loci()[1].role, 1);
    assert_eq!(pair.paired_class_tag.as_str(), "273");
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(300),
        "40 mm",
        "Linear Dimension-3",
        Some("mm"),
        "d3",
        4.0,
    ))
    .unwrap();
    parameter.id = "f3d:Design/BulkStream.dat:design-parameter#301".into();
    parameter.record_index = 301;
    let owner =
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:design-parameter-owner#300".into(),
            byte_offset: pair.paired_byte_offset().unwrap() + 59,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 300,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 4.0,
            evaluated_value_offset: pair.paired_byte_offset().unwrap() + 99,
            parameter_record_index: 301,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 302,
        })
        .unwrap();
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset().expect("retained test source"),
            std::slice::from_ref(&owner),
            std::slice::from_ref(&parameter),
        ),
        Some(302)
    );
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset().expect("retained test source"),
            &[owner.clone(), owner],
            std::slice::from_ref(&parameter),
        ),
        None
    );

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested = find_dimension_locus_pair(
        &nested,
        &[frame(233, 11, 91, nested_end)],
        0,
        nested_end,
        228,
        &HashSet::from([192, 194]),
    )
    .expect("nested paired dimension locus frame");
    assert_eq!(nested.byte_offset(), Some(11));
    assert_eq!(nested.paired_byte_offset(), Some(91));

    let mut competing = bytes.clone();
    competing.extend_from_slice(&bytes);
    let second = bytes.len();
    competing[second + 7..second + 11].copy_from_slice(&234u32.to_le_bytes());
    competing[second + 87..second + 91].copy_from_slice(&234u32.to_le_bytes());
    let frames = [
        frame(233, 0, 80, second),
        frame(234, second, second + 80, competing.len()),
    ];
    assert!(find_dimension_locus_pair(
        &competing,
        &frames,
        0,
        competing.len(),
        228,
        &HashSet::from([192, 194]),
    )
    .is_none());
}

#[test]
fn dimension_null_locus_pair_preserves_null_and_typed_roles() {
    let mut bytes = vec![0; 74];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&1394u32.to_le_bytes());

    let pair = parse_dimension_null_locus_pair(
        &bytes,
        &frame(1394, 0, 74, bytes.len()),
        1290,
        &HashSet::from([1109]),
    )
    .expect("null-locus dimension frame");
    assert_eq!(pair.companion_record_index, 1290);
    assert_eq!(pair.governing_companion_record_index, 1290);
    assert_eq!(pair.record_index, 1394);
    assert_eq!(pair.frame_length(), Some(74));
    assert_eq!(pair.loci()[0].role, 10);
    assert_eq!(pair.loci()[1].geometry_index(), 1109);
    assert_eq!(pair.loci()[1].role, 7);
    assert_eq!(pair.paired_class_tag.as_str(), "273");

    assert!(parse_dimension_null_locus_pair(
        &bytes,
        &frame(1394, 0, 74, bytes.len()),
        1290,
        &HashSet::from([1110]),
    )
    .is_none());

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested = find_dimension_null_locus_pair(
        &nested,
        &[frame(1394, 11, 85, nested_end)],
        0,
        nested_end,
        1290,
        &HashSet::from([1109]),
    )
    .expect("null-locus frame following another indexed frame");
    assert_eq!(nested.byte_offset(), Some(11));
    assert_eq!(nested.paired_byte_offset(), Some(85));

    let mut axis_pair = pair.clone().into_draft().unwrap();
    axis_pair.loci[0].role = 14;
    axis_pair.loci[1].role = 3;
    let mut axis_pair =
        crate::records::dimensions::DesignDimensionLocusPair::try_new(axis_pair).unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#line").unwrap(),
        SketchId::mint("f3d:model:sketch#axis-angle").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 1.0),
        })
        .unwrap(),
    );
    let parameter = cadmpeg_ir::features::ParameterId::mint("f3d:model:parameter#angle")
        .expect("identity grammar");
    assert!(matches!(
        null_locus_dimension_definition(&axis_pair, &entity, "Angular Dimension-2", std::f64::consts::FRAC_PI_4, parameter.clone(), TEST_LINEAR_TOLERANCE),
        Some(SketchConstraintDefinitionInput::AngleToAxis {
            entity: ref actual_entity,
            axis: SketchAxis::Horizontal,
            parameter: ref actual_parameter,
        }) if actual_entity == entity.id() && actual_parameter.parameter() == Some(&parameter)
    ));
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        0.5,
        parameter.clone(),
        TEST_LINEAR_TOLERANCE
    )
    .is_none());
    let mut draft = axis_pair.into_draft().unwrap();
    draft.loci[0].role = 13;
    axis_pair = crate::records::dimensions::DesignDimensionLocusPair::try_new(draft).unwrap();
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        std::f64::consts::FRAC_PI_4,
        parameter.clone(),
        TEST_LINEAR_TOLERANCE
    )
    .is_none());

    let radial_entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#f3d:model:sketch-entity:circle").unwrap(),
        SketchId::mint("f3d:model:sketch#radial").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(1.000_000_014_901_161_2).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        null_locus_dimension_definition(&pair, &radial_entity, "Diameter Dimension-2", 0.2, parameter.clone(), TEST_LINEAR_TOLERANCE),
        Some(SketchConstraintDefinitionInput::Diameter {
            entity: ref actual_entity,
            parameter: ref actual_parameter,
        }) if actual_entity == radial_entity.id() && actual_parameter.parameter() == Some(&parameter)
    ));
    assert!(null_locus_dimension_definition(
        &pair,
        &radial_entity,
        "Diameter Dimension-2",
        0.2,
        parameter,
        0.0
    )
    .is_none());
}

#[test]
fn dimension_locus_group_preserves_roles_owner_state_and_return_order() {
    let mut bytes = vec![0; 101];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"286");
    bytes[7..11].copy_from_slice(&249u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&175u32.to_le_bytes());
    bytes[35..39].copy_from_slice(&2u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&217u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&1u32.to_le_bytes());
    bytes[55] = 1;
    bytes[56..60].copy_from_slice(&172u32.to_le_bytes());
    bytes[66..70].copy_from_slice(&1u32.to_le_bytes());
    bytes[74..78].copy_from_slice(&2u32.to_le_bytes());
    bytes[78] = 1;
    bytes[79..83].copy_from_slice(&217u32.to_le_bytes());
    bytes[89] = 1;
    bytes[90..94].copy_from_slice(&175u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&250u32.to_le_bytes());

    let group = parse_dimension_locus_group(
        &bytes,
        0,
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    )
    .expect("counted dimension locus frame");
    assert_eq!(group.companion_record_index, 240);
    assert_eq!(group.record_index, 249);
    assert_eq!(group.frame_length, Some(101));
    assert_eq!(group.owner_reference, 172);
    assert_eq!(group.owner_role, 1);
    assert_eq!(group.state, 0);
    assert_eq!(group.loci[0].geometry_record_index, 175);
    assert_eq!(group.loci[0].role, 2);
    assert_eq!(group.loci[1].geometry_record_index, 217);
    assert_eq!(group.loci[1].role, 1);
    assert_eq!(
        group
            .loci
            .iter()
            .map(|locus| locus.returned.value)
            .collect::<Vec<_>>(),
        [217, 175]
    );
    assert_eq!(group.next_class_tag.as_str(), "314");
    assert_eq!(group.next_record_index, 250);

    let relation_at = |stream: &str, byte_offset| {
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: format!("f3d:{stream}:sketch-relation#{byte_offset}"),
            record_index: 249,
            class_tag: crate::records::references::DesignClassTag::try_from("286".to_owned())
                .unwrap(),
            byte_offset,
            state_offset: 66,
            owner_reference: 172,
            owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_172").unwrap()),
            auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: ([(175, 25), (217, 40)]
                .into_iter()
                .map(|(record_index, offset)| {
                    crate::records::sketch_relations::SketchRelationMember {
                        reference: crate::records::sketch_relations::SketchRelationReference::Index(
                            record_index,
                        ),
                        offset: Some(offset),
                        relation_ordinal: Some(0),
                    }
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 56,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(0, None)
                .expect("valid relation definition"),
            entity_genesis: None,
            return_members: ([(217, 79), (175, 90)]
                .into_iter()
                .map(|(record_index, offset)| {
                    crate::records::sketch_relations::SketchRelationReturnMember {
                        reference: crate::records::sketch_relations::SketchRelationReference::Index(
                            record_index,
                        ),
                        offset: Some(offset),
                    }
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: bytes[..101].to_vec(),
        })
        .unwrap()
    };
    let mut relations = vec![relation_at("native", 0), relation_at("other", 0)];
    let mut group = group;
    group.id = "f3d:native:design-dimension-locus-group#0".into();
    remove_dimension_frame_relations(&mut relations, &[], &[group], &[]);
    assert_eq!(relations.len(), 1);
    assert!(relations[0].id.starts_with("f3d:other:"));

    let body = bytes[11..101].to_vec();
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"315");
    bytes.extend_from_slice(&251u32.to_le_bytes());
    let frames = [
        frame(249, 0, 101, 101),
        frame(250, 101, 202, 202),
        frame(251, 202, bytes.len(), bytes.len()),
    ];
    let groups = find_dimension_locus_groups(
        &bytes,
        &frames,
        0,
        bytes.len(),
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [249, 250]
    );
}
