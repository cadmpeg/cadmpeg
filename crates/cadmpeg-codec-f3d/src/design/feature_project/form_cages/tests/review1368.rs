use super::super as form_cages;
use super::*;

#[test]
fn review1368_compatible_membership_refines_positions_in_both_directions() {
    let full = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    let partial = |ordinal| {
        FormConstruction::try_new(
            FormInputs::default(),
            full.observations().to_vec(),
            FormMembership::Partial {
                cardinality: Some(2),
                order: FormOrder::Unknown,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal,
                }],
            },
        )
        .unwrap()
    };
    for (left, right) in [(&full, &partial(Some(0))), (&partial(Some(0)), &full)] {
        let result = retain_ungrouped(left, right).unwrap();
        assert!(result.membership_is_complete());
        assert_eq!(
            result
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#a", "f3d:model:subd#b"]
        );
        assert_eq!(retain_ungrouped(&result, right).unwrap(), result);
    }
    let unknown = partial(None);
    let known = partial(Some(0));
    for (left, right) in [(&unknown, &known), (&known, &unknown)] {
        let result = retain_ungrouped(left, right).unwrap();
        assert!(
            matches!(result.membership(), FormMembership::Partial { members, .. }
            if members.len() == 1 && members[0].ordinal == Some(0))
        );
        assert_eq!(retain_ungrouped(&result, right).unwrap(), result);
    }
}

#[test]
fn review1368_direct_reference_runs_keep_prefixes_repetitions_and_conflicts() {
    let make = |values: &[u32]| {
        FormConstruction::try_new(
            FormInputs {
                references: values
                    .iter()
                    .map(|id| {
                        cadmpeg_ir::ids::NativeId::mint(format!("f3d:scope:record#{id}")).unwrap()
                    })
                    .collect(),
                ..FormInputs::default()
            },
            Vec::new(),
            FormConstruction::default().membership().clone(),
        )
        .unwrap()
    };
    let short = make(&[210, 210]);
    let long = make(&[210, 210, 220]);
    for (left, right) in [(&short, &long), (&long, &short)] {
        let result = retain_ungrouped(left, right).unwrap();
        assert_eq!(
            result
                .inputs()
                .references
                .iter()
                .map(cadmpeg_ir::ids::NativeId::as_str)
                .collect::<Vec<_>>(),
            [
                "f3d:scope:record#210",
                "f3d:scope:record#210",
                "f3d:scope:record#220"
            ]
        );
        assert!(result.inputs().reference_runs.is_empty());
        assert_eq!(retain_ungrouped(&result, right).unwrap(), result);
    }
    let other = make(&[210, 220, 210]);
    let conflict = retain_ungrouped(&long, &other).unwrap();
    assert!(conflict.inputs().references.is_empty());
    assert_eq!(conflict.inputs().reference_runs.len(), 2);
    assert_eq!(retain_ungrouped(&conflict, &other).unwrap(), conflict);
}

#[test]
fn review1368_tracking_variable_width_and_unreadable_prefixes() {
    let build = |members: &[u8], count: u32| {
        let mut bytes = indexed_frame(b"429", 226, 36);
        bytes[21] = 1;
        bytes[22..30].copy_from_slice(&220u64.to_le_bytes());
        bytes[32..36].copy_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(members);
        bytes.extend_from_slice(&[0; 52]);
        bytes.extend_from_slice(&indexed_frame(b"264", 226, 15));
        bytes
    };
    let mut many_nulls = vec![0; 6];
    push_marked(&mut many_nulls, 230);
    let mut typed = vec![1];
    typed.extend_from_slice(&230u64.to_le_bytes());
    typed.extend_from_slice(&36u32.to_le_bytes());
    typed.extend_from_slice(b"C46D3EEB-40E1-43FA-83DC-9CB204335417");
    typed.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    assert_eq!(typed.len(), 55);
    for (payload, count, expected) in [(many_nulls, 7, 6), (typed, 5, 0)] {
        let bytes = build(&payload, count);
        let meta = form_meta(&[
            ("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226]),
            ("C46D3EEB-40E1-43FA-83DC-9CB204335417", 1, &[230]),
        ]);
        let result = body_construction(&bytes, &meta, &form_scope(220, vec![226]), &[]);
        let list = &result.inputs().lists[0];
        assert_eq!(list.declared_count, count);
        assert_eq!(list.members.len(), 1);
        assert_eq!(list.members[0].ordinal, expected);
        assert!(list.members[0].reference.as_str().ends_with("#230"));
        assert_eq!(
            result
                .inputs()
                .reference_evidence
                .iter()
                .filter(|reference| matches!(reference.target, FormNativeReferenceTarget::Null {}))
                .count(),
            usize::try_from(count - 1).unwrap()
        );
    }
    let mut malformed = vec![2];
    push_marked(&mut malformed, 230);
    let bytes = build(&malformed, 2);
    let meta = form_meta(&[("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226])]);
    let result = body_construction(&bytes, &meta, &form_scope(220, vec![226]), &[]);
    for (ordinal, expected) in [
        (0, FormNativeReferenceTarget::Unreadable {}),
        (1, FormNativeReferenceTarget::Unvisited {}),
    ] {
        assert!(result
            .inputs()
            .reference_evidence
            .iter()
            .any(
                |reference| reference.role == FormReferenceRole::TrackingMember
                    && reference.ordinal == ordinal
                    && reference.target == expected
            ));
    }
}

#[test]
fn review1368_plain_body_registration_conflicts_refuse_joins() {
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_body_frame(300, 0, [201, 22, 296, 301], None),
    ]
    .concat();
    let cage = neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID);
    for target in [201, 22, 296, 299, 301] {
        let owner = [201];
        let container = [22];
        let surface = [296];
        let scene = [301];
        let conflict = [target];
        let meta = form_meta(&[
            ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
            ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
            ("11111111-1111-4111-8111-111111111111", 0, &owner),
            ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &container),
            ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &surface),
            ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", 1, &scene),
            ("22222222-2222-4222-8222-222222222222", 0, &conflict),
        ]);
        let result = body_construction(
            &bytes,
            &meta,
            &form_scope(201, vec![]),
            std::slice::from_ref(&cage),
        );
        assert!(!result.membership_is_complete(), "target {target}");
        assert_eq!(result.referenced_cages().count(), 1);
        if target == 201 || target == 296 || target == 299 {
            assert_eq!(result.committed_cages().count(), 0);
        }
        assert!(result.inputs().reference_evidence.iter().any(|reference| reference.source.as_str().ends_with(&format!("#{target}")) || matches!(&reference.target,
            FormNativeReferenceTarget::Local { target: id } if id.as_str().ends_with(&format!("#{target}")))));
    }
}

#[test]
fn review1368_plain_and_inline_surface_registration_cross_product() {
    let surface_guid = "22709494-9149-4A70-A829-7D748A7B5DB1";
    let cage = neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID);
    for inline in [false, true] {
        for registration in 0..3 {
            let mut rows: Vec<(&str, u32, &[u32])> = vec![
                ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
                ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
                ("11111111-1111-4111-8111-111111111111", 0, &[201]),
                ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
                ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", 1, &[301]),
            ];
            if registration != 0 {
                rows.push((surface_guid, 0, &[296]));
            }
            if registration == 2 {
                rows.push(("22222222-2222-4222-8222-222222222222", 0, &[296]));
            }
            let meta = form_meta(&rows);
            let mut body = indexed_frame(b"274", 300, 11);
            push_material_envelope(&mut body, "PrismMaterial-001");
            push_marked(&mut body, 201);
            push_marked(&mut body, 22);
            body.push(1);
            body.extend_from_slice(&296u64.to_le_bytes());
            if inline {
                body.extend_from_slice(&36u32.to_le_bytes());
                body.extend_from_slice(surface_guid.as_bytes());
            }
            body.extend_from_slice(&[0, 0]);
            push_marked(&mut body, 301);
            body.extend_from_slice(&indexed_frame(b"258", 300, 15));
            let bytes = [form_serializer_frame(299, 296, FIRST_CAGE_UUID), body].concat();
            let result = body_construction(
                &bytes,
                &meta,
                &form_scope(201, vec![]),
                std::slice::from_ref(&cage),
            );
            assert_eq!(
                result.membership_is_complete(),
                registration == 1,
                "inline {inline}, registration {registration}"
            );
            assert_eq!(
                result.committed_cages().count(),
                usize::from(registration == 1)
            );
            assert_eq!(result.referenced_cages().count(), 1);
            assert!(result.inputs().reference_evidence.iter().any(|reference| {
                reference.role == FormReferenceRole::Body && reference.ordinal == 2
                    && reference.inline_type_guid.as_deref() == inline.then_some(surface_guid)
                    && matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with("#296"))
            }));
        }
    }
}

#[test]
fn review1368_serializer_registration_cross_product() {
    let mut serializer = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    serializer[4..7].copy_from_slice(b"315");
    let bytes = [
        serializer,
        form_body_frame(300, 0, [201, 22, 296, 301], None),
    ]
    .concat();
    let cage = neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID);
    for registration in 0..5 {
        let mut rows: Vec<(&str, u32, &[u32])> = vec![
            ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
            ("11111111-1111-4111-8111-111111111111", 0, &[201]),
            ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
            ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
            ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", 1, &[301]),
        ];
        if registration == 1 || registration == 3 {
            rows.push(("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]));
        }
        if registration == 2 || registration == 3 {
            rows.push(("22222222-2222-4222-8222-222222222222", 0, &[299]));
        }
        if registration == 4 {
            rows.push(("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 1, &[299]));
        }
        let result = body_construction(
            &bytes,
            &form_meta(&rows),
            &form_scope(201, vec![]),
            std::slice::from_ref(&cage),
        );
        assert_eq!(
            result.membership_is_complete(),
            registration < 2,
            "registration {registration}"
        );
        assert_eq!(
            result.committed_cages().count(),
            usize::from(registration < 2)
        );
        assert_eq!(result.referenced_cages().count(), 1);
        assert!(result
            .inputs()
            .reference_evidence
            .iter()
            .any(|reference| reference.role == FormReferenceRole::SerializerSurface));
    }
}

#[test]
fn review1368_resource_state_cross_product_remains_valid_and_idempotent() {
    let resource = FormResource {
        source: cadmpeg_ir::ids::NativeId::mint("f3d:scope:record#299").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
    };
    let make = |state| {
        let binding = match state {
            0 => FormCageBinding::Undecoded {
                resource: resource.clone(),
                availability: FormResourceAvailability::Unknown,
            },
            1 => FormCageBinding::Undecoded {
                resource: resource.clone(),
                availability: FormResourceAvailability::ActiveUndecoded,
            },
            2 => FormCageBinding::Undecoded {
                resource: resource.clone(),
                availability: FormResourceAvailability::Inactive,
            },
            3 => FormCageBinding::Decoded {
                cage: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
                resource: Some(resource.clone()),
            },
            _ => unreachable!(),
        };
        FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: Vec::new(),
                binding,
            }],
            FormMembership::Partial {
                cardinality: None,
                order: FormOrder::Unknown,
                members: if state == 2 {
                    Vec::new()
                } else {
                    vec![FormMember {
                        observation: FormObservationIndex(0),
                        ordinal: None,
                    }]
                },
            },
        )
        .unwrap()
    };
    for left in 0..4 {
        for right in 0..4 {
            let old = make(left);
            let fresh = make(right);
            let result = retain_ungrouped(&old, &fresh).unwrap();
            let FormMembership::Partial { members, .. } = result.membership() else {
                panic!("partial");
            };
            assert_eq!(
                members.len(),
                usize::from(left != 2 || right != 2),
                "{left}, {right}"
            );
            for member in members {
                assert!(!matches!(
                    result.observations()[member.observation.0].binding,
                    FormCageBinding::Undecoded {
                        availability: FormResourceAvailability::Inactive,
                        ..
                    }
                ));
            }
            if (left == 2) != (right == 2) {
                assert!(result
                    .inputs()
                    .resource_state_evidence
                    .iter()
                    .any(|claim| claim.resource == resource
                        && claim.availability == FormResourceAvailability::Inactive));
            }
            assert_eq!(
                retain_ungrouped(&result, &fresh).unwrap(),
                result,
                "{left}, {right}"
            );
        }
    }
}

#[test]
fn review1368_supplemental_inactive_state_reconciles_selected_resource() {
    let resource = FormResource {
        source: cadmpeg_ir::ids::NativeId::mint("f3d:scope:record#299").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
    };
    let inactive = FormConstruction::try_new(
        FormInputs {
            resource_state_evidence: vec![FormResourceStateEvidence {
                resource: resource.clone(),
                availability: FormResourceAvailability::Inactive,
            }],
            ..FormInputs::default()
        },
        Vec::new(),
        FormConstruction::default().membership().clone(),
    )
    .unwrap();
    for availability in [
        FormResourceAvailability::Unknown,
        FormResourceAvailability::ActiveUndecoded,
    ] {
        let selected = FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Undecoded {
                    resource: resource.clone(),
                    availability,
                },
            }],
            FormMembership::Partial {
                cardinality: None,
                order: FormOrder::Unknown,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: None,
                }],
            },
        )
        .unwrap();
        for (old, fresh) in [(&selected, &inactive), (&inactive, &selected)] {
            let result = retain_ungrouped(old, fresh).unwrap();
            assert!(matches!(
                result.observations()[0].binding,
                FormCageBinding::Undecoded {
                    availability: FormResourceAvailability::Ambiguous,
                    ..
                }
            ));
            assert!(
                matches!(result.membership(), FormMembership::Partial { members, .. } if members.len() == 1)
            );
            assert!(result
                .inputs()
                .resource_state_evidence
                .iter()
                .any(|claim| claim.availability == FormResourceAvailability::Inactive));
            if availability == FormResourceAvailability::ActiveUndecoded {
                assert!(result
                    .inputs()
                    .resource_state_evidence
                    .iter()
                    .any(|claim| claim.availability == availability));
            }
            assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
        }
    }
}

#[test]
fn review1368_qualified_and_conflicting_foreign_references_keep_siblings() {
    const FIRST: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND: &str = "22222222-2222-4222-8222-222222222222";
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
        ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", 1, &[301]),
    ]);
    for qualified in [false, true] {
        let mut body = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut body, "PrismMaterial-001");
        push_marked(&mut body, 201);
        body.push(1);
        body.extend_from_slice(&22u64.to_le_bytes());
        if !qualified {
            body.extend_from_slice(&36u32.to_le_bytes());
            body.extend_from_slice(FIRST.as_bytes());
        }
        body.push(1);
        body.extend_from_slice(&7u32.to_le_bytes());
        push_lp_utf16(&mut body, FIRST);
        body.push(u8::from(qualified));
        if !qualified {
            body.extend_from_slice(&36u32.to_le_bytes());
            body.extend_from_slice(SECOND.as_bytes());
            push_lp_utf16(&mut body, "literal link");
            body.push(0);
        }
        push_marked(&mut body, 296);
        push_marked(&mut body, 301);
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let bytes = [form_serializer_frame(299, 296, FIRST_CAGE_UUID), body].concat();
        let result = body_construction(
            &bytes,
            &meta,
            &form_scope(201, vec![]),
            &[neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)],
        );
        assert!(!result.membership_is_complete());
        assert_eq!(result.committed_cages().count(), 1);
        let foreign = result
            .inputs()
            .reference_evidence
            .iter()
            .find(|reference| reference.role == FormReferenceRole::Body && reference.ordinal == 1)
            .unwrap();
        if qualified {
            assert!(
                matches!(&foreign.target, FormNativeReferenceTarget::Segment { target: 22, segment: 7, asset_guid: Some(asset) } if asset == FIRST)
            );
        } else {
            assert_eq!(foreign.inline_type_guid.as_deref(), Some(FIRST));
            assert!(
                matches!(&foreign.target, FormNativeReferenceTarget::Document { target: 22, segment: 7, asset_guid, target_type_guid: Some(external), link_name, .. } if asset_guid == FIRST && external == SECOND && link_name == "literal link")
            );
        }
        assert!(result.inputs().reference_evidence.iter().any(|reference|
            reference.role == FormReferenceRole::Body && reference.ordinal == 3 &&
            matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with("#301"))));
    }
}

#[test]
fn review1368_serializer_primary_identity_is_global_across_routes() {
    use std::io::{Cursor, Write};
    let mut list = indexed_frame(b"402", 210, 99);
    list[21] = 1;
    list[22..30].copy_from_slice(&201u64.to_le_bytes());
    list[32..36].copy_from_slice(&1u32.to_le_bytes());
    list[36] = 1;
    list[37..45].copy_from_slice(&211u64.to_le_bytes());
    let mut object = indexed_frame(b"301", 211, 200);
    object[189] = 1;
    object[190..198].copy_from_slice(&212u64.to_le_bytes());
    let mut first = indexed_frame(b"373", 212, 33);
    first[21] = 1;
    first[22..30].copy_from_slice(&213u64.to_le_bytes());
    let mut second = indexed_frame(b"362", 213, 29);
    second[21..29].copy_from_slice(&214u64.to_le_bytes());
    let mut carrier = indexed_frame(b"457", 214, 665);
    carrier[317] = 1;
    carrier[318..326].copy_from_slice(&201u64.to_le_bytes());
    carrier[339] = 1;
    carrier[340..348].copy_from_slice(&296u64.to_le_bytes());
    let prefix = [
        list,
        indexed_frame(b"264", 210, 15),
        object,
        first,
        second,
        carrier,
        indexed_frame(b"264", 214, 15),
    ]
    .concat();
    let cages = [
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
    ];
    for case in 0..4 {
        let mut a = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
        let mut b = form_serializer_frame(299, 496, SECOND_CAGE_UUID);
        a[4..7].copy_from_slice(b"315");
        b[4..7].copy_from_slice(b"315");
        let suffix = match case {
            0 => [a, b].concat(),
            1 => [indexed_frame(b"315", 299, 15), a].concat(),
            2 => [a, indexed_frame(b"315", 299, 15)].concat(),
            _ => [a, indexed_frame(b"264", 299, 15)].concat(),
        };
        let bytes = [prefix.clone(), suffix, indexed_frame(b"258", 400, 15)].concat();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let serializers = crate::design::test_support::with_test_decode_context(|ctx| {
            form_cage_serializers(ctx, &bytes, &records)
        })
        .unwrap();
        assert_eq!(serializers.entries[&296][0].ambiguous, case != 3);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
        for uuid in [FIRST_CAGE_UUID, SECOND_CAGE_UUID] {
            archive
                .start_file(
                    format!("FusionAssetName[Active]/TSplines.BlobParts/TSpline.{uuid}.tsm"),
                    stored,
                )
                .unwrap();
            archive.write_all(b"synthetic").unwrap();
        }
        let archive = archive.finish().unwrap().into_inner();
        let scope = form_scope(201, vec![210]);
        let mut result = None;
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::design::test_support::with_test_decode_context(|ctx| {
                let mut facts = form_cages::FormFacts {
                    inputs: form_cages::form_inputs(&scope)?,
                    observations: Vec::new(),
                };
                let source = form_cages::FormSource {
                    bytes: &bytes,
                    records: &records,
                    scope: &scope,
                    cages: &cages,
                    stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
                };
                let membership = form_cages::collect_membership(
                    ctx,
                    scan,
                    source,
                    std::slice::from_ref(&scope),
                    &mut facts,
                    &mut form_cages::ReferenceGroups::default(),
                    &mut Vec::new(),
                )?;
                result = Some(
                    FormConstruction::try_new(facts.inputs, facts.observations, membership)
                        .unwrap(),
                );
                Ok::<_, cadmpeg_core::CodecError>(())
            })
        })
        .unwrap();
        let result = result.unwrap();
        assert_eq!(result.membership_is_complete(), case == 3, "case {case}");
        if case == 0 {
            assert_eq!(result.referenced_cages().count(), 2);
        }
    }
    for malformed_first in [false, true] {
        let valid = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
        let short = indexed_frame(b"377", 299, 15);
        let bytes = if malformed_first {
            [short, valid].concat()
        } else {
            [valid, short].concat()
        };
        let bytes = [bytes, indexed_frame(b"258", 400, 15)].concat();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let mut types = FormRecordTypes::default();
        types.serializers.push(299);
        types.type_guids = vec![String::new(); 122];
        types.type_guids[121] = "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571".into();
        let serializers = crate::design::test_support::with_test_decode_context(|ctx| {
            form_typed_serializers(ctx, &bytes, &records, &types)
        })
        .unwrap();
        assert!(serializers.entries[&296][0].ambiguous);
    }
}

#[test]
fn review1368_serializer_null_and_unreadable_slots_remain_distinct() {
    let meta = form_meta(&[("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299])]);
    for (marker, expected) in [
        (0, FormNativeReferenceTarget::Null {}),
        (2, FormNativeReferenceTarget::Unreadable {}),
    ] {
        let mut serializer = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
        serializer.truncate(serializer.len() - 11);
        serializer.push(marker);
        let bytes = [serializer, indexed_frame(b"258", 400, 15)].concat();
        let result = body_construction(&bytes, &meta, &form_scope(201, vec![]), &[]);
        assert!(result
            .inputs()
            .reference_evidence
            .iter()
            .any(
                |reference| reference.role == FormReferenceRole::SerializerSurface
                    && reference.ordinal == 0
                    && reference.group.is_some()
                    && reference.target == expected
            ));
        assert_eq!(result.inputs().resource_evidence.len(), 1);
    }
}

#[test]
fn review1368_source_certainty_refines_compatible_complete_claims() {
    let source =
        FormConstruction::complete(vec![
            cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap()
        ])
        .unwrap();
    let assumed = FormConstruction::try_new(
        source.inputs().clone(),
        source.observations().to_vec(),
        FormMembership::Complete {
            members: vec![FormObservationIndex(0)],
            order: FormKnownOrder::Assumed,
            certainty: FormKnownOrder::Assumed,
        },
    )
    .unwrap();
    for (old, new) in [(&source, &assumed), (&assumed, &source)] {
        let result = retain_ungrouped(old, new).unwrap();
        assert!(matches!(
            result.membership(),
            FormMembership::Complete {
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
                ..
            }
        ));
        assert_eq!(retain_ungrouped(&result, new).unwrap(), result);
    }
    let partial = FormConstruction::try_new(
        FormInputs::default(),
        source.observations().to_vec(),
        FormMembership::Partial {
            cardinality: Some(1),
            order: FormOrder::Source,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(0),
            }],
        },
    )
    .unwrap();
    for (old, new) in [(&assumed, &partial), (&partial, &assumed)] {
        let result = retain_ungrouped(old, new).unwrap();
        assert!(matches!(
            result.membership(),
            FormMembership::Complete {
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Assumed,
                ..
            }
        ));
    }
}

#[test]
fn review1368_body_first_null_and_unreadable_keep_distinct_slots() {
    let meta = form_meta(&[("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300])]);
    for marker in [0, 2] {
        let mut body = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut body, "PrismMaterial-001");
        body.push(marker);
        for target in [22, 296, 301] {
            push_marked(&mut body, target);
        }
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let result = body_construction(&body, &meta, &form_scope(201, vec![]), &[]);
        let evidence = result
            .inputs()
            .reference_evidence
            .iter()
            .filter(|reference| reference.role == FormReferenceRole::Body)
            .collect::<Vec<_>>();
        assert_eq!(evidence.len(), 4);
        assert_eq!(
            matches!(evidence[0].target, FormNativeReferenceTarget::Null {}),
            marker == 0
        );
        assert_eq!(
            matches!(evidence[0].target, FormNativeReferenceTarget::Unreadable {}),
            marker == 2
        );
        if marker == 2 {
            assert!(evidence[1..].iter().all(|reference| matches!(
                reference.target,
                FormNativeReferenceTarget::Unvisited {}
            )));
        } else {
            assert!(evidence[1..].iter().all(|reference| matches!(
                reference.target,
                FormNativeReferenceTarget::Local { .. }
            )));
        }
    }
}

#[test]
fn review1368_null_only_material_run_does_not_compete_with_owned_body() {
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
        ("11111111-1111-4111-8111-111111111111", 0, &[201]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
        ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", 1, &[301]),
    ]);
    let cage = neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID);
    for null_only in [true, false] {
        let mut body = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut body, "PrismMaterial-synthetic-a");
        for target in [201, 22, 296, 301] {
            push_marked(&mut body, target);
        }
        push_material_envelope(&mut body, "PrismMaterial-synthetic-b");
        if null_only {
            body.extend_from_slice(&[0; 4]);
        } else {
            body.push(2);
        }
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let bytes = [form_serializer_frame(299, 296, FIRST_CAGE_UUID), body].concat();
        let result = body_construction(
            &bytes,
            &meta,
            &form_scope(201, vec![]),
            std::slice::from_ref(&cage),
        );
        assert_eq!(result.membership_is_complete(), null_only);
        assert_eq!(result.committed_cages().count(), usize::from(null_only));
        assert_eq!(result.referenced_cages().count(), 1);
        let body_evidence = result
            .inputs()
            .reference_evidence
            .iter()
            .filter(|reference| reference.role == FormReferenceRole::Body)
            .collect::<Vec<_>>();
        assert_eq!(body_evidence.len(), 8);
        assert_eq!(
            body_evidence
                .iter()
                .filter(|reference| matches!(reference.target, FormNativeReferenceTarget::Null {}))
                .count(),
            if null_only { 4 } else { 0 }
        );
        assert_eq!(
            body_evidence
                .iter()
                .filter(|reference| matches!(
                    reference.target,
                    FormNativeReferenceTarget::Unreadable {}
                ))
                .count(),
            usize::from(!null_only)
        );
        assert_eq!(
            body_evidence
                .iter()
                .filter(|reference| matches!(
                    reference.target,
                    FormNativeReferenceTarget::Unvisited {}
                ))
                .count(),
            if null_only { 0 } else { 3 }
        );
        if null_only {
            let other = body_construction(
                &bytes,
                &meta,
                &form_scope(202, vec![]),
                std::slice::from_ref(&cage),
            );
            assert_eq!(other.referenced_cages().count(), 0);
            assert!(other
                .inputs()
                .reference_evidence
                .iter()
                .all(|reference| reference.role != FormReferenceRole::Body));
        }
    }
    let mut body = indexed_frame(b"274", 300, 11);
    push_material_envelope(&mut body, "PrismMaterial-synthetic-empty");
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&indexed_frame(b"258", 300, 15));
    let result = body_construction(&body, &meta, &form_scope(201, vec![]), &[]);
    assert!(!result.membership_is_complete());
    assert_eq!(
        result
            .inputs()
            .reference_evidence
            .iter()
            .filter(|reference| matches!(reference.target, FormNativeReferenceTarget::Null {}))
            .count(),
        4
    );
}
#[test]
fn review1368_typed_serializer_primary_orderings_withhold_body_closure() {
    let mut meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", 1, &[301]),
    ]);
    let serializer_type = meta.types.remove(1);
    let dummy = form_meta(&[("11111111-1111-4111-8111-111111111111", 0, &[])])
        .types
        .remove(0);
    while meta.types.len() < 121 {
        meta.types.push(dummy.clone());
    }
    meta.types.push(serializer_type);
    for case in 0..3 {
        let valid = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
        let short = indexed_frame(b"377", 299, 15);
        let frames = match case {
            0 => [short, valid].concat(),
            1 => [valid, short].concat(),
            _ => [valid, indexed_frame(b"258", 299, 15)].concat(),
        };
        let bytes = [frames, form_body_frame(300, 0, [201, 22, 296, 301], None)].concat();
        let result = body_construction(
            &bytes,
            &meta,
            &form_scope(201, vec![]),
            &[neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)],
        );
        assert_eq!(result.membership_is_complete(), case == 2);
        assert_eq!(result.referenced_cages().count(), 1);
        assert_eq!(result.committed_cages().count(), usize::from(case == 2));
    }
}
#[test]
fn review1368_qualified_asset_text_obeys_retained_byte_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scope = form_scope(201, vec![]);
    let reference = crate::bytes::Reference::CrossSegment {
        target: 22,
        inline_type_guid: None,
        segment: 7,
        asset_guid: Some("qualified-asset".to_owned()),
    };
    let mut facts = form_cages::FormFacts {
        inputs: FormInputs::default(),
        observations: Vec::new(),
    };
    assert!(matches!(
        facts.reference(
            &ctx,
            &scope,
            form_cages::BoundGroup {
                source: 300,
                role: FormReferenceRole::Body,
                label: FormReferenceGroup::from_label(0),
            }
            .slot(1),
            &reference,
            None
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    assert!(facts.inputs.reference_evidence.is_empty());
}

#[test]
fn review1368_generic_and_typed_primary_evidence_reconcile_before_binding() {
    let mut generic = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    generic[4..7].copy_from_slice(b"315");
    let bytes = [
        generic,
        indexed_frame(b"377", 299, 15),
        indexed_frame(b"258", 400, 15),
    ]
    .concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut types = FormRecordTypes::default();
    types.serializers.push(299);
    types.type_guids = vec![String::new(); 122];
    types.type_guids[121] = "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571".into();
    let serializers = crate::design::test_support::with_test_decode_context(|ctx| {
        form_cages::form_serializers(ctx, &bytes, &records, Some(&types))
    })
    .unwrap();
    assert_eq!(serializers.entries[&296].len(), 1);
    assert!(serializers.entries[&296][0].ambiguous);
}
#[test]
fn review1368_partial_relative_orders_reconcile_without_inventing_order() {
    let complete = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    let partial = |indices: [usize; 2]| {
        FormConstruction::try_new(
            FormInputs::default(),
            complete.observations().to_vec(),
            FormMembership::Partial {
                cardinality: Some(2),
                order: FormOrder::Source,
                members: indices
                    .into_iter()
                    .map(|index| FormMember {
                        observation: FormObservationIndex(index),
                        ordinal: None,
                    })
                    .collect(),
            },
        )
        .unwrap()
    };
    let ordered = partial([0, 1]);
    let reversed = partial([1, 0]);
    for (old, fresh) in [
        (&ordered, &reversed),
        (&reversed, &ordered),
        (&complete, &reversed),
        (&reversed, &complete),
    ] {
        let result = retain_ungrouped(old, fresh).unwrap();
        assert!(
            matches!(result.membership(), FormMembership::Partial { order: FormOrder::Unknown, members, .. } if members.len() == 2)
        );
        assert!(result
            .inputs()
            .order_evidence
            .iter()
            .any(|claim| claim.members == [FormObservationIndex(0), FormObservationIndex(1)]));
        assert!(result
            .inputs()
            .order_evidence
            .iter()
            .any(|claim| claim.members == [FormObservationIndex(1), FormObservationIndex(0)]));
        assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
    }
    for (old, fresh) in [(&complete, &ordered), (&ordered, &complete)] {
        let result = retain_ungrouped(old, fresh).unwrap();
        assert!(result.membership_is_complete());
        assert_eq!(
            result
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#a", "f3d:model:subd#b"]
        );
    }
}

#[test]
fn review1368_single_member_orders_do_not_order_independent_members() {
    let complete = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    for positions_known in [false, true] {
        let partial = |index| {
            FormConstruction::try_new(
                FormInputs::default(),
                complete.observations().to_vec(),
                FormMembership::Partial {
                    cardinality: Some(2),
                    order: FormOrder::Source,
                    members: vec![FormMember {
                        observation: FormObservationIndex(index),
                        ordinal: positions_known.then_some(u32::try_from(index).unwrap()),
                    }],
                },
            )
            .unwrap()
        };
        let first = partial(0);
        let second = partial(1);
        for (old, fresh) in [(&first, &second), (&second, &first)] {
            let result = retain_ungrouped(old, fresh).unwrap();
            let FormMembership::Partial { order, members, .. } = result.membership() else {
                panic!("partial membership")
            };
            assert_eq!(
                *order,
                if positions_known {
                    FormOrder::Source
                } else {
                    FormOrder::Unknown
                }
            );
            assert_eq!(members.len(), 2);
            assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
        }
    }
}
#[test]
fn review1368_inactive_inventory_conflicts_with_complete_resource_membership() {
    let resource = FormResource {
        source: cadmpeg_ir::ids::NativeId::mint("f3d:scope:record#299").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
    };
    let decoded = FormConstruction::try_new(
        FormInputs::default(),
        vec![FormCageObservation {
            paths: Vec::new(),
            binding: FormCageBinding::Decoded {
                cage: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
                resource: Some(resource.clone()),
            },
        }],
        FormMembership::Complete {
            members: vec![FormObservationIndex(0)],
            order: FormKnownOrder::Source,
            certainty: FormKnownOrder::Source,
        },
    )
    .unwrap();
    let inactive = FormConstruction::try_new(
        FormInputs::default(),
        vec![FormCageObservation {
            paths: Vec::new(),
            binding: FormCageBinding::Undecoded {
                resource: resource.clone(),
                availability: FormResourceAvailability::Inactive,
            },
        }],
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap();
    for (old, fresh) in [(&decoded, &inactive), (&inactive, &decoded)] {
        let result = retain_ungrouped(old, fresh).unwrap();
        assert!(!result.membership_is_complete());
        assert_eq!(
            result
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#a"]
        );
        assert!(result
            .inputs()
            .resource_state_evidence
            .iter()
            .any(|state| state.resource == resource
                && state.availability == FormResourceAvailability::Inactive));
        assert!(result.inputs().closure_evidence.iter().any(|claim| {
            claim.members.len() == 1 && claim.certainty == FormKnownOrder::Source
        }));
        assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
    }
}

#[test]
fn review1368_incompatible_membership_preserves_original_closure_claim() {
    let complete = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    let conflicting = FormConstruction::try_new(
        FormInputs::default(),
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: Some(2),
            order: FormOrder::Source,
            members: vec![FormMember {
                observation: FormObservationIndex(1),
                ordinal: Some(0),
            }],
        },
    )
    .unwrap();
    for (old, fresh) in [(&complete, &conflicting), (&conflicting, &complete)] {
        let result = retain_ungrouped(old, fresh).unwrap();
        assert!(!result.membership_is_complete());
        assert!(result.inputs().closure_evidence.iter().any(|claim| {
            claim.members.len() == 2 && claim.certainty == FormKnownOrder::Source
        }));
        assert!(result
            .inputs()
            .position_evidence
            .iter()
            .any(|claim| { claim.observation == FormObservationIndex(0) && claim.ordinal == 0 }));
        assert!(result
            .inputs()
            .position_evidence
            .iter()
            .any(|claim| { claim.observation == FormObservationIndex(1) && claim.ordinal == 1 }));
        assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
    }
}

#[test]
fn review1368_retained_closure_is_matched_by_member_identity() {
    let complete = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    for (selected, compatible) in [(1, true), (0, false)] {
        let prior = FormConstruction::try_new(
            FormInputs {
                closure_evidence: vec![FormClosureEvidence {
                    members: vec![FormObservationIndex(0)],
                    order: FormKnownOrder::Source,
                    certainty: FormKnownOrder::Source,
                }],
                ..FormInputs::default()
            },
            complete.observations().to_vec(),
            FormMembership::Partial {
                cardinality: None,
                order: FormOrder::Unknown,
                members: Vec::new(),
            },
        )
        .unwrap();
        let incoming = FormConstruction::try_new(
            FormInputs::default(),
            vec![
                complete.observations()[1].clone(),
                complete.observations()[0].clone(),
            ],
            FormMembership::Complete {
                members: vec![FormObservationIndex(selected)],
                order: FormKnownOrder::Assumed,
                certainty: FormKnownOrder::Assumed,
            },
        )
        .unwrap();
        for (old, fresh) in [(&prior, &incoming), (&incoming, &prior)] {
            let result = retain_ungrouped(old, fresh).unwrap();
            assert_eq!(result.membership_is_complete(), compatible);
            if compatible {
                assert!(matches!(
                    result.membership(),
                    FormMembership::Complete {
                        order: FormKnownOrder::Source,
                        certainty: FormKnownOrder::Source,
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    result.membership(),
                    FormMembership::Partial {
                        cardinality: None,
                        members,
                        ..
                    } if members.len() == 2
                ));
            }
            assert!(result
                .inputs()
                .closure_evidence
                .iter()
                .any(|claim| claim.members.len() == 1
                    && claim.certainty == FormKnownOrder::Source
                    && matches!(&result.observations()[claim.members[0].0].binding,
                        FormCageBinding::Decoded { cage, .. } if cage.as_str() == "f3d:model:subd#a")));
            assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
        }
    }
}

#[test]
fn review1368_empty_closure_refines_only_empty_membership() {
    let prior = FormConstruction::try_new(
        FormInputs {
            closure_evidence: vec![FormClosureEvidence {
                members: Vec::new(),
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
            }],
            ..FormInputs::default()
        },
        Vec::new(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap();
    let empty = FormConstruction::try_new(
        FormInputs::default(),
        Vec::new(),
        FormMembership::Complete {
            members: Vec::new(),
            order: FormKnownOrder::Assumed,
            certainty: FormKnownOrder::Assumed,
        },
    )
    .unwrap();
    let nonempty =
        FormConstruction::complete(vec![
            cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap()
        ])
        .unwrap();
    for (fresh, compatible) in [(&empty, true), (&nonempty, false)] {
        for (old, new) in [(&prior, fresh), (fresh, &prior)] {
            let result = retain_ungrouped(old, new).unwrap();
            assert_eq!(result.membership_is_complete(), compatible);
            assert!(
                result
                    .inputs()
                    .closure_evidence
                    .iter()
                    .any(|claim| claim.members.is_empty()
                        && claim.certainty == FormKnownOrder::Source)
            );
            if compatible {
                assert!(
                    matches!(result.membership(), FormMembership::Complete { members, certainty: FormKnownOrder::Source, .. } if members.is_empty())
                );
            } else {
                assert!(
                    matches!(result.membership(), FormMembership::Partial { cardinality: None, members, .. } if members.len() == 1)
                );
            }
            assert_eq!(retain_ungrouped(&result, new).unwrap(), result);
        }
    }
}

#[test]
fn review1368_converging_resource_identities_preserve_conflicting_slots() {
    let resource = |entry| FormResource {
        source: cadmpeg_ir::ids::NativeId::mint("f3d:scope:record#299").unwrap(),
        entry: cadmpeg_core::text::NonBlankString::new(entry).unwrap(),
    };
    let first = resource("a/cage.tsm");
    let second = resource("z/cage.tsm");
    let cage = cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap();
    let decoded = |resource| FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Decoded {
            cage: cage.clone(),
            resource: Some(resource),
        },
    };
    let prior = FormConstruction::try_new(
        FormInputs::default(),
        vec![decoded(first.clone())],
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: None,
            }],
        },
    )
    .unwrap();
    let fresh = FormConstruction::try_new(
        FormInputs::default(),
        vec![
            decoded(second.clone()),
            FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Undecoded {
                    resource: first.clone(),
                    availability: FormResourceAvailability::Unknown,
                },
            },
        ],
        FormMembership::Partial {
            cardinality: Some(2),
            order: FormOrder::Source,
            members: vec![
                FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: Some(0),
                },
                FormMember {
                    observation: FormObservationIndex(1),
                    ordinal: Some(1),
                },
            ],
        },
    )
    .unwrap();
    for (old, new) in [(&prior, &fresh), (&fresh, &prior)] {
        let result = retain_ungrouped(old, new).unwrap();
        assert!(
            matches!(result.membership(), FormMembership::Partial { order: FormOrder::Unknown, members, .. } if members.len() == 1 && members[0].ordinal.is_none())
        );
        let selected = match result.membership() {
            FormMembership::Partial { members, .. } => members[0].observation,
            FormMembership::Complete { .. } => panic!("partial membership"),
        };
        assert!(result
            .inputs()
            .order_evidence
            .iter()
            .any(|claim| claim.members == [selected, selected]));
        assert!(result
            .inputs()
            .position_evidence
            .iter()
            .any(|claim| claim.observation == selected && claim.ordinal == 0));
        assert!(result
            .inputs()
            .position_evidence
            .iter()
            .any(|claim| claim.observation == selected && claim.ordinal == 1));
        assert!(result.inputs().resource_evidence.contains(&first));
        assert!(result.inputs().resource_evidence.contains(&second));
        assert_eq!(retain_ungrouped(&result, new).unwrap(), result);
    }
}

#[test]
fn review1368_absolute_positions_conflicting_with_relative_order_remain_evidence() {
    let complete = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    let positions = FormConstruction::try_new(
        FormInputs {
            position_evidence: vec![
                FormPositionEvidence {
                    source: None,
                    observation: FormObservationIndex(0),
                    ordinal: 0,
                },
                FormPositionEvidence {
                    source: None,
                    observation: FormObservationIndex(1),
                    ordinal: 1,
                },
            ],
            ..FormInputs::default()
        },
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap();
    let order = FormConstruction::try_new(
        FormInputs::default(),
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Source,
            members: vec![
                FormMember {
                    observation: FormObservationIndex(1),
                    ordinal: None,
                },
                FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: None,
                },
            ],
        },
    )
    .unwrap();
    for (old, new) in [(&positions, &order), (&order, &positions)] {
        let result = retain_ungrouped(old, new).unwrap();
        assert!(
            matches!(result.membership(), FormMembership::Partial { order: FormOrder::Unknown, members, .. } if members.len() == 2)
        );
        assert_eq!(result.inputs().position_evidence.len(), 2);
        assert!(result
            .inputs()
            .order_evidence
            .iter()
            .any(|claim| claim.members == [FormObservationIndex(1), FormObservationIndex(0)]));
        assert_eq!(retain_ungrouped(&result, new).unwrap(), result);
    }
}

#[test]
fn review1368_repeated_historical_closure_stays_partial() {
    let complete =
        FormConstruction::complete(vec![
            cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap()
        ])
        .unwrap();
    let prior = FormConstruction::try_new(
        FormInputs {
            closure_evidence: vec![FormClosureEvidence {
                members: vec![FormObservationIndex(0), FormObservationIndex(0)],
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
            }],
            ..FormInputs::default()
        },
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap();
    let selected = FormConstruction::try_new(
        FormInputs::default(),
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: None,
            }],
        },
    )
    .unwrap();
    for (old, fresh) in [(&prior, &selected), (&selected, &prior)] {
        let result = retain_ungrouped(old, fresh).unwrap();
        assert!(
            matches!(result.membership(), FormMembership::Partial { order: FormOrder::Unknown, members, .. }
            if members.len() == 1 && members[0].ordinal.is_none())
        );
        assert!(result
            .inputs()
            .closure_evidence
            .iter()
            .any(|claim| claim.members == [FormObservationIndex(0), FormObservationIndex(0)]));
        for ordinal in [0, 1] {
            assert!(result
                .inputs()
                .position_evidence
                .iter()
                .any(|claim| claim.observation == FormObservationIndex(0)
                    && claim.ordinal == ordinal));
        }
        assert_eq!(retain_ungrouped(&result, fresh).unwrap(), result);
    }
}
