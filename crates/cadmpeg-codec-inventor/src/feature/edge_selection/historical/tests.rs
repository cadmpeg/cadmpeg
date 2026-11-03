// SPDX-License-Identifier: Apache-2.0
use super::*;

fn parse(payload: View<'_>, version: u8) -> Result<Option<StoredSelector>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    super::parse(&ctx, payload, version)
}
use cadmpeg_asm::sab::Token;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferGroup, ChamferSpec, FilletGroup, RadiusSpec},
        FeatureEvaluation, NonEmptyMembers,
    },
    scalar::PositiveLength,
};

fn selector_bytes(generators: &[u32], secondary: &[u32], version: u8) -> Vec<u8> {
    selector_bytes_with_form(generators, secondary, version, 5)
}

fn selector_bytes_with_form(
    generators: &[u32],
    secondary: &[u32],
    version: u8,
    form: u8,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    for word in [0_u32, 0] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    if version >= 28 {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    }
    for word in [0x8000_0003_u32, 9, u32::MAX] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[1, form]);
    for word in [0x8000_000a_u32, 2] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    if version != 19 {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for values in [generators, secondary] {
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(values.len())
                .expect("small list")
                .to_le_bytes(),
        );
        if !values.is_empty() {
            bytes.extend_from_slice(&[8, 0, 0, 0]);
        }
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(0);
    for word in [0x8000_0008_u32, 0x8000_0004, 7] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

#[test]
fn selector_forms_check_array_counts_secondary_names_and_suffix() {
    for version in [19, 23, 29] {
        for generators in [&[31][..], &[31, 37][..]] {
            let bytes = selector_bytes(generators, &[], version);
            let selector = parse(View::over_retained(&bytes), version)
                .expect("framed")
                .expect("direct");
            assert_eq!(selector.generators, generators);
            assert_eq!(selector.source, 9);
            assert_eq!(selector.key, 7);
            assert!(parse(View::over_retained(&bytes[..bytes.len() - 1]), version).is_err());
            let mut extra = bytes.clone();
            extra.push(0);
            assert!(parse(View::over_retained(&extra), version).is_err());
            for form in [0, 1, 6] {
                let changed = selector_bytes_with_form(generators, &[], version, form);
                assert!(parse(View::over_retained(&changed), version)
                    .expect("framed")
                    .is_none());
            }
        }
        for (generators, secondary) in [
            (&[][..], &[][..]),
            (&[31, 31], &[]),
            (&[0], &[]),
            (&[31, 37, 41], &[]),
            (&[31], &[3]),
        ] {
            assert!(parse(
                View::over_retained(&selector_bytes(generators, secondary, version)),
                version
            )
            .expect("framed refusal")
            .is_none());
        }
    }
}

fn record(index: usize, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn attribute(index: usize, owner: i64, schema: u32, family: &str, suffix: &[i64]) -> Record {
    let mut tokens = vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(owner),
    ];
    let name = if schema == 44 {
        let family = if family == "NMx_Matched_Entity" {
            "INV_NMX_MATCHED_ATTRIB"
        } else {
            "INV_NMX_GEN_TAG"
        };
        tokens.extend([Token::Str(family.into()), Token::Long(1), Token::Long(44)]);
        "ATTRIB_CUSTOM-attrib".into()
    } else {
        tokens.push(Token::Long(i64::from(schema)));
        format!("{family}-NamingMatching-attrib")
    };
    tokens.extend(suffix.iter().copied().map(Token::Long));
    record(index, &name, tokens)
}

fn records(schema: u32, generators: &[u32]) -> Vec<Record> {
    let mut generated = vec![i64::try_from(generators.len()).expect("count")];
    for generator in generators {
        generated.extend([i64::from(*generator), -1]);
    }
    generated.extend([3, 0, 18]);
    vec![
        record(
            1,
            "edge",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Double(1.0),
                Token::Ref(3),
                Token::Double(5.0),
                Token::Ref(-1),
                Token::Ref(6),
                Token::False,
            ],
        ),
        record(
            2,
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(0),
                Token::Ref(4),
            ],
        ),
        record(
            3,
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(1),
                Token::Ref(5),
            ],
        ),
        record(
            4,
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([3.0, 0.0, 0.0]),
            ],
        ),
        record(
            5,
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([11.0, 0.0, 0.0]),
            ],
        ),
        record(
            6,
            "straight",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([1.0, 0.0, 0.0]),
                Token::Vector3([2.0, 0.0, 0.0]),
            ],
        ),
        attribute(10, 1, schema, "NMx_Matched_Entity", &[1, 9, -1, 7, 0]),
        attribute(
            11,
            1,
            schema,
            "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag",
            &generated,
        ),
    ]
}

#[test]
fn each_generator_form_joins_one_stored_edge_with_signed_geometry() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    for schema in [38, 40, 42, 44] {
        for generators in [&[31][..], &[31, 37][..]] {
            let bytes = selector_bytes(generators, &[], 29);
            let selector = parse(View::over_retained(&bytes), 29)
                .expect("framed")
                .expect("selector");
            let mut records = records(schema, generators);
            let edges = selected(&records, &[selector], schema);
            assert_eq!(edges[&9].owner, 1);
            assert_eq!(
                edges[&9].geometry.interior.as_raw(),
                &cadmpeg_ir::math::Point3::new(70.0, 0.0, 0.0)
            );
            let edge_tokens = std::sync::Arc::make_mut(&mut records[0].tokens);
            edge_tokens[4] = Token::Double(-1.0);
            edge_tokens[6] = Token::Double(-5.0);
            edge_tokens[9] = Token::True;
            let selector = parse(View::over_retained(&bytes), 29)
                .expect("framed")
                .expect("selector");
            let edges = selected(&records, &[selector], schema);
            assert_eq!(
                edges[&9].geometry.tangent.as_raw(),
                &cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0)
            );
            let (mut inventory, mut feature) = fixture(&ctx, false);
            let token = inventory.features[0]
                .identity
                .segment_token
                .as_str()
                .to_owned();
            inventory.edge_inputs.historical = edges
                .into_iter()
                .map(|(source, edge)| ((token.clone(), source), edge))
                .collect();
            let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory)
                .expect("project selected revision");
            assert_eq!(states.len(), 1);
            assert_eq!(
                states[0].edge_geometry[0]
                    .tangent
                    .expect("signed tangent")
                    .as_raw(),
                &cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0)
            );
        }
    }
}

#[test]
fn mismatched_keys_owners_schemas_and_non_line_revisions_stay_native() {
    for change in [0, 1, 2, 4, 5, 7] {
        let mut records = records(38, &[31, 37]);
        match change {
            0 => {
                let tokens = std::sync::Arc::make_mut(&mut records[6].tokens);
                tokens[9] = Token::Long(8);
            }
            1 => records[0].name = "face".into(),
            2 => records.push(attribute(
                12,
                2,
                38,
                "NMx_Matched_Entity",
                &[1, 9, -1, 7, 0],
            )),
            3 => {
                let tokens = std::sync::Arc::make_mut(&mut records[7].tokens);
                tokens[7] = Token::Long(41);
            }
            4 => {
                let tokens = std::sync::Arc::make_mut(&mut records[6].tokens);
                tokens[5] = Token::Long(40);
            }
            5 => records[5].name = "ellipse".into(),
            6 => {
                records.pop();
            }
            _ => {
                let duplicate = records[0].clone();
                records.push(duplicate);
            }
        }
        let bytes = selector_bytes(&[31, 37], &[], 29);
        let selector = parse(View::over_retained(&bytes), 29)
            .expect("framed")
            .expect("selector");
        assert!(
            selected(&records, &[selector], 38).is_empty(),
            "control {change}"
        );
    }
}

pub(crate) fn fixture(
    ctx: &DecodeContext<'_>,
    chamfer: bool,
) -> (crate::feature::FeatureInventory, Feature) {
    let (original_source, mut properties) = if chamfer {
        crate::feature::chamfer::tests::fixture(0)
    } else {
        crate::feature::fillet::tests::fixture()
    };
    let control_slots: &[(usize, bool)] = if chamfer {
        &[(6, true), (9, true)]
    } else {
        &[(2, true), (3, true), (4, true), (5, true), (8, false)]
    };
    let mut slots = original_source
        .properties
        .references()
        .iter()
        .enumerate()
        .filter_map(|(slot, reference)| Some((slot, reference.record_ordinal()?)))
        .collect::<Vec<_>>();
    for (position, (slot, value)) in control_slots.iter().enumerate() {
        let ordinal = 80 + u32::try_from(position).expect("control ordinal");
        slots.retain(|(previous, _)| previous != slot);
        slots.push((*slot, ordinal));
        properties.push(crate::feature::tests::test_property(
            ordinal,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: *value,
            },
        ));
    }
    let source = crate::feature::tests::test_feature(
        original_source.identity.record_ordinal,
        original_source.properties.references().len(),
        &slots,
    );
    let label = crate::feature::fillet::tests::label(chamfer);
    let mut feature =
        crate::feature::native_feature(ctx, &source.identity, Some(&label), source.id())
            .expect("feature");
    let native = properties[if chamfer { 0 } else { 3 }].id();
    let distance = PositiveLength::new(2.0).expect("distance");
    let operation = if chamfer {
        FeatureOperation::Chamfer {
            groups: NonEmptyMembers::one(ChamferGroup {
                edges: EdgeSelection::Native(native),
                spec: ChamferSpec::Distance { distance },
            }),
            flip_direction: true,
        }
    } else {
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::Native(native),
                radius: RadiusSpec::Constant { radius: distance },
                tangency_weight: None,
            }),
        }
    };
    feature.evaluation =
        FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation));
    let item = properties[if chamfer { 1 } else { 6 }]
        .identity
        .record_ordinal;
    let sample = EdgeGeometry {
        start: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            0.0, 0.0, 0.0,
        ))
        .expect("point"),
        end: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0))
            .expect("point"),
        interior: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            1.0, 0.0, 0.0,
        ))
        .expect("point"),
        tangent: cadmpeg_ir::units::UnitVector3::X_AXIS,
    };
    let mut inventory = crate::feature::FeatureInventory {
        features: vec![source],
        properties,
        labels: vec![label],
        ..crate::feature::FeatureInventory::default()
    };
    inventory.edge_inputs.historical.insert(
        (
            inventory.features[0]
                .identity
                .segment_token
                .as_str()
                .to_owned(),
            9,
        ),
        SelectedEdge {
            owner: 1,
            item,
            geometry: sample,
        },
    );
    (inventory, feature)
}

#[test]
fn historical_projection_is_atomic_and_pins_each_unresolved_selection_loss() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    for chamfer in [false, true] {
        for change in 0..5 {
            let (mut inventory, mut feature) = fixture(&ctx, chamfer);
            match change {
                1 => inventory.edge_inputs.historical.clear(),
                2 => {
                    inventory
                        .edge_inputs
                        .historical
                        .values_mut()
                        .next()
                        .expect("sample")
                        .item = 100;
                }
                3 => {
                    let item = &mut inventory.properties[if chamfer { 1 } else { 6 }];
                    item.edit(|payload| {
                        if let PmDcFeaturePropertyKind::EdgeItem { value, .. } = &mut payload.kind {
                            *value = 1;
                        }
                    });
                }
                4 => {
                    let ordinal = if chamfer { 80 } else { 81 };
                    inventory
                        .properties
                        .iter_mut()
                        .find(|p| p.identity.record_ordinal == ordinal)
                        .expect("Boolean control")
                        .edit(|payload| {
                            if let PmDcFeaturePropertyKind::Boolean { value, .. } =
                                &mut payload.kind
                            {
                                *value = false;
                            }
                        });
                }
                _ => {}
            }
            let states =
                project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("projection");
            assert_eq!(states.len(), usize::from(change == 0));
            let losses = native_losses(std::slice::from_ref(&feature));
            assert_eq!(losses.len(), usize::from(change != 0));
            if let Some(loss) = losses.first() {
                assert_eq!(
                    loss.code,
                    crate::loss::InventorLossCode::FeatureEdgeSelectionNative
                        .note("expected")
                        .code
                );
            }
            if change == 0 {
                assert!(
                    matches!(selections(&feature)[0],EdgeSelection::Historical {edges,..} if edges.len()==1)
                );
                assert!(valid(
                    &feature,
                    &inventory.features[0],
                    &states,
                    &carrier(38),
                    false,
                    |id| inventory.properties.iter().find(|p| p.id() == id),
                    |r| inventory
                        .properties
                        .iter()
                        .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
                ));
            }
        }
    }
}

#[test]
fn historical_selector_references_require_qualified_typed_targets() {
    let bytes = selector_bytes(&[31, 37], &[], 29);
    for change in 0..7 {
        let mut selector = parse(View::over_retained(&bytes), 29)
            .expect("framed")
            .expect("direct");
        match change {
            1 => selector.body = PmDcReference::from_packed(10),
            2 => selector.item = PmDcReference::from_packed(8),
            3 => selector.context = PmDcReference::from_packed(4),
            _ => {}
        }
        let target = |ordinal| match ordinal {
            9 if change != 4 => Some(crate::pmdc::inventor_id(0x9087_4d47)),
            7 if change != 5 => Some(super::super::super::EDGE_ITEM_TYPE),
            3 if change != 6 => Some([
                0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0, 8, 0xa7, 0xba, 0x32, 0xa3, 0xdc,
                9,
            ]),
            _ => None,
        };
        assert_eq!(links(&selector, target), change == 0);
    }
}

#[test]
fn historical_matching_uses_exact_revisions_with_opaque_generated_names() {
    let bytes = selector_bytes(&[31, 37], &[], 29);
    let first = parse(View::over_retained(&bytes), 29)
        .expect("framed")
        .expect("direct");
    let mut second = parse(View::over_retained(&bytes), 29)
        .expect("framed")
        .expect("direct");
    second.key = 8;
    assert!(selected(&records(38, &[31, 37]), &[first, second], 38).is_empty());
    let first = parse(View::over_retained(&bytes), 29)
        .expect("framed")
        .expect("direct");
    let mut records = records(38, &[31, 37]);
    records.push(attribute(
        12,
        1,
        38,
        "unknown-NMx_Brep_Name_tag",
        &[1, 31, -1, 0],
    ));
    assert_eq!(selected(&records, &[first], 38)[&9].owner, 1);
}

#[test]
fn selector_list_counts_are_bounded_before_allocation() {
    let mut bytes = selector_bytes(&[31, 37], &[], 29);
    let marker = [8, 0, 0, 0x30];
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generator array marker")
        + marker.len();
    bytes[start..start + std::mem::size_of::<u32>()].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        parse(View::over_retained(&bytes), 29),
        Err(CodecError::Malformed(_))
    ));
}

pub(crate) fn carrier(schema: u32) -> crate::native::ActiveCarrierRecord {
    crate::native::ActiveCarrierRecord::Selected {
        id: "synthetic:carrier".into(),
        segment_token: "generated".into(),
        record_ordinal: 1,
        segment_version_major: 29,
        family: crate::kernel::KernelFamily::Asm,
        header_state: 0,
        header_kind: 1,
        header_value: 0,
        schema,
        carrier_len: std::num::NonZeroU64::new(1).expect("length"),
        carrier_offset: 0,
        carrier_sha256: "0".repeat(64),
        selected_key: 1,
        enabled: true,
        delta_state: 0,
        history_reference: 0,
    }
}

#[test]
fn every_member_of_a_historical_group_must_join_a_distinct_revision() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    for chamfer in [false, true] {
        for change in 0..3 {
            let (mut inventory, mut feature) = fixture(&ctx, chamfer);
            let token = inventory.features[0]
                .identity
                .segment_token
                .as_str()
                .to_owned();
            let first = inventory
                .edge_inputs
                .historical
                .get(&(token.clone(), 9))
                .expect("first revision");
            let second = SelectedEdge {
                owner: if change == 2 { 1 } else { 2 },
                item: first.item,
                geometry: first.geometry.clone(),
            };
            if change != 1 {
                inventory.edge_inputs.historical.insert((token, 10), second);
            }
            let item = &mut inventory.properties[if chamfer { 1 } else { 6 }];
            item.edit(|payload| {
                if let PmDcFeaturePropertyKind::EdgeItem {
                    index_references, ..
                } = &mut payload.kind
                {
                    *index_references = crate::pmdc::PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([2, 0])),
                        vec![9, 10],
                    )
                    .expect("two references");
                }
            });
            let states =
                project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("projection");
            assert_eq!(states.len(), usize::from(change == 0));
            if change == 0 {
                assert_eq!(states[0].edges.len(), 2);
                assert!(
                    matches!(selections(&feature)[0],EdgeSelection::Historical {edges,..} if edges.len()==2)
                );
            } else {
                assert_eq!(native_losses(std::slice::from_ref(&feature)).len(), 1);
            }
        }
    }
}

#[test]
fn historical_projection_checks_the_fillet_class_control_tuple() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    for corner in [false, true] {
        let (mut inventory, mut feature) = fixture(&ctx, false);
        if corner {
            inventory.labels[0] = crate::feature::tests::test_label(
                100,
                1,
                crate::feature::CORNER_ROUND_CLASS_ID,
                &[],
            );
            for ordinal in [80, 83] {
                inventory
                    .properties
                    .iter_mut()
                    .find(|p| p.identity.record_ordinal == ordinal)
                    .expect("corner control")
                    .edit(|payload| {
                        if let PmDcFeaturePropertyKind::Boolean { value, .. } = &mut payload.kind {
                            *value = false;
                        }
                    });
            }
        } else {
            inventory
                .properties
                .iter_mut()
                .find(|p| p.identity.record_ordinal == 83)
                .expect("legacy control")
                .edit(|payload| {
                    if let PmDcFeaturePropertyKind::Boolean { value, .. } = &mut payload.kind {
                        *value = false;
                    }
                });
        }
        let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("historical projection");
        assert_eq!(states.len(), 1);
        assert!(valid(
            &feature,
            &inventory.features[0],
            &states,
            &carrier(38),
            corner,
            |id| inventory.properties.iter().find(|p| p.id() == id),
            |r| inventory
                .properties
                .iter()
                .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
        ));
    }
}
