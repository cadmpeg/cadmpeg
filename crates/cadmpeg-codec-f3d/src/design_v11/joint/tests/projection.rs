// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_member_root_connector_stays_in_the_member_when_the_legacy_pair_places_it() {
    let parent = definition("root");
    let member = crate::ids::member_definition_id(None, "member").unwrap();
    let part = definition("part-b");
    let mut outer = ProductDefinition::new(
        parent.clone(),
        Storage::File {
            document: ExternalDocument::default(),
            source: None,
        },
    );
    outer.occurrences = vec![occurrence("m")];
    let mut inner = ProductDefinition::new(member.clone(), Storage::Embedded);
    inner.occurrences = vec![occurrence("b")];
    let definitions = [
        outer,
        inner,
        ProductDefinition::new(part.clone(), Storage::Embedded),
    ];
    let occurrences = [
        Occurrence::new(occurrence("m"), member.clone(), None),
        Occurrence::new(occurrence("b"), part, None),
    ];
    for root_object in ["root-object", "f3d:model:feature#member-origin"] {
        let mut pair = joint(
            "member-pair",
            106,
            fixed(),
            [
                connector(
                    JointOperand::occurrence(occurrence("m"), root_object, Vec::new()),
                    STORED,
                ),
                connector(on("b"), STORED),
            ],
        );
        pair.native_ref = crate::ids::qualify(&scope(106), "member");
        let roots = HashMap::from([(
            pair.id.clone(),
            MemberRoots {
                definition: member.clone(),
                subelements: [
                    Some(if root_object == "root-object" {
                        vec!["stored-key".into()]
                    } else {
                        Vec::new()
                    }),
                    None,
                ],
            },
        )]);
        let mut origin = legacy("member-origin", &pair);
        let frame = cadmpeg_ir::features::geometry::FeatureCoordinateFrame::new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .unwrap();
        origin
            .evaluation
            .set_definition(LegacyDefinition::Operation(
                LegacyOperation::DatumCoordinateSystem { frame },
            ));
        let datum = crate::design_v11::datum::project(&origin, &mut Vec::new()).unwrap();
        let mut datum_feature = design("member-origin", datum);
        datum_feature.id = crate::ids::qualify(datum_feature.id.as_str(), "member")
            .and_then(|id| FeatureId::mint(id).ok())
            .unwrap();
        let mut features = [datum_feature, unresolved("member-pair")];
        let legacy = [legacy("member-pair", &pair)];
        let joints = [pair];
        let inputs = Inputs::new(
            &legacy,
            &joints,
            HashMap::new(),
            Places::new(&definitions, &occurrences, OperandKeys::new(&[], &[], &[])),
        )
        .with_roots(&roots);
        let mut losses = Vec::new();
        let written = project(&mut features, &member, &inputs, &mut losses);
        assert!(losses.is_empty(), "{losses:?}");
        assert_eq!(written.len(), 1);
        let [root, peer] = &the_joint(&features[1]).connectors;
        let root = construction(root);
        assert_eq!(
            root.resolved
                .as_ref()
                .unwrap()
                .origin
                .coordinates()
                .map(FiniteReal::get),
            [1.0, 2.0, 3.0]
        );
        let root = only_pick(root);
        assert_eq!(root.context, None);
        if root_object == "root-object" {
            assert_eq!(root.target, PickTarget::Native { within: None });
            assert_eq!(root.native, native_name(["root-object", "stored-key"]));
        } else {
            assert_eq!(
                root.target,
                PickTarget::Datum {
                    feature: features[0].id.clone(),
                    part: None
                }
            );
        }
        assert_eq!(
            only_pick(construction(peer)).context,
            Some(PickContext::Occurrence(path(&["b"])))
        );
    }
}

#[test]
fn a_group_keeps_the_decoded_locator_when_legacy_descent_refuses_its_residual_key() {
    use crate::design::assembly::project_assembly_joints_with_paths;
    use crate::design::components::{paths::LocalOccurrencePaths, ComponentInserts};
    use crate::records::feature::assembly::{
        DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
        DesignAssemblyOperandPath, DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier,
        TYPED_OCCURRENCE_PATH_TYPE_GUID,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayload,
    };

    let keys = [
        CARRIED,
        "aaaaaaaa-1111-4111-8111-111111111111",
        "bbbbbbbb-2222-4222-8222-222222222222",
        "cccccccc-3333-4333-8333-333333333333",
    ];
    let qualifier = |locator: u32, keys: &[&str]| {
        let class = |tag: &str| tag.to_owned().try_into().unwrap();
        let link = DesignAssemblyOperandPathLink {
            locator_reference_offset: 0,
            locator_record_index: locator,
            locator_class_tag: class("337"),
            locator_byte_offset: 0,
            locator_scope_reference_offset: 0,
            wrapper_record_index: locator + 3,
            wrapper_reference_offset: 0,
            wrapper_class_tag: class("341"),
            wrapper_byte_offset: 0,
            path_reference_offset: 0,
        };
        let path = DesignAssemblyOperandPath::try_new_typed(
            link,
            locator + 1,
            class("384"),
            Some(
                TYPED_OCCURRENCE_PATH_TYPE_GUID
                    .to_owned()
                    .try_into()
                    .unwrap(),
            ),
            100,
            keys.iter()
                .zip([200, 276])
                .map(|(key, offset)| Located {
                    value: key.to_string().try_into().unwrap(),
                    offset,
                })
                .collect(),
            (0..4)
                .map(|ordinal| Located {
                    value: "22222222-2222-4222-8222-222222222222"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 350 + ordinal,
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(path.two_keys_in_one_typed_record(), keys.len() == 2);
        DesignAssemblyOperandQualifier::OccurrencePath { path }
    };
    let frame = |index, x_cm| {
        let mut rows = Transform::identity().rows();
        rows[0][3] = x_cm;
        DesignAssemblyOperandFrame {
            reference_record_index: index,
            reference_offset: 0,
            transform: rows.try_into().unwrap(),
            transform_offset: 0,
        }
    };
    let scope_id = scope(109);
    let mut native = DesignParameterScope::empty(&scope_id, DesignFeatureKind::Assemble, 109);
    native
        .try_edit(|draft| {
            crate::design::test_support::lengthen_scope_frame(draft, 268);
            draft.reference_count_offset = 164;
            draft.feature_ordinal_offset = 192;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(Vec::new());
            draft.layout_fixture_references();
            let alignment = DesignAssemblyAlignment::try_new(
                0.0,
                [0.0; 3],
                Vec::new(),
                Some(DesignAssemblyAlignmentForm::qualified(
                    [frame(30, 0.0), frame(40, 0.0)],
                    [qualifier(30, &[keys[0]]), qualifier(40, &[keys[1]])],
                )),
            )
            .unwrap()
            .with_additional_members(vec![
                DesignQualifiedAssemblyOperand {
                    qualifier: qualifier(50, &[keys[0], keys[2]]),
                    frame: frame(50, 1.25),
                },
                DesignQualifiedAssemblyOperand {
                    qualifier: qualifier(60, &[keys[3]]),
                    frame: frame(60, 0.0),
                },
            ])
            .unwrap();
            draft.payload = DesignScopePayload::AsBuilt(Some(alignment));
        })
        .unwrap();
    assert!(crate::design::assembly::rigid_as_built_scope(&native));

    let mut second = carrier();
    second.id = format!("{STREAM}:design-component-occurrence#41");
    second.record_index = 41;
    second.occurrence_guid = keys[1].to_owned().try_into().unwrap();
    let mut third = carrier();
    third.id = format!("{STREAM}:design-component-occurrence#42");
    third.record_index = 42;
    third.occurrence_guid = keys[3].to_owned().try_into().unwrap();
    let carriers = [carrier(), second, third];
    let mut model = Model::new();
    model.occurrences[0].native_ref = Some(carriers[0].id.clone());
    model.occurrences[1].native_ref = Some(carriers[1].id.clone());
    model.occurrences[2].native_ref = Some(carriers[2].id.clone());
    model.definitions[0].occurrences.push(occurrence("c"));
    model.definitions[1].occurrences.clear();
    let scopes = [native];
    let local = LocalOccurrencePaths::new(&carriers, &[], &[], &model.occurrences, &scopes);
    let Some((first, consumed)) = local.resolve(STREAM, &[keys[0].to_owned(), keys[2].to_owned()])
    else {
        panic!("the first member is local")
    };
    assert_eq!(first, occurrence("a"));
    assert_eq!(consumed, 1);
    assert!(local.refuses_residual_keys(STREAM, &first, true));
    let legacy_group = project_assembly_joints_with_paths(
        &scopes,
        &carriers,
        &ComponentInserts::default(),
        &[],
        &local,
    );
    let [pair, later] = legacy_group.joints.as_slice() else {
        panic!(
            "the leading pair and the last member survive: {:?}",
            legacy_group.losses
        )
    };
    assert_eq!(
        later.id,
        crate::ids::neutral_assembly_group_joint_id(&scopes[0], 3)
    );
    assert!(legacy_group
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::AssemblyJointUnprojected.kind()));
    let mut features = [unresolved("partial-group")];
    let legacy = [legacy("partial-group", pair)];
    let inputs = Inputs::new(
        &legacy,
        &legacy_group.joints,
        HashMap::from([(
            scope_id.as_str(),
            Source {
                group: true,
                additional_members: &scopes[0].assembly_alignment().unwrap().additional_members,
                ..Source::default()
            },
        )]),
        Places::new(
            &model.definitions,
            &model.occurrences,
            OperandKeys::new(&carriers, &[], &scopes),
        ),
    );
    let mut losses = Vec::new();
    project(&mut features, &definition("root"), &inputs, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    let FeatureOperation::RigidGroup(group) = &features[0].definition.operation else {
        panic!("a rigid group")
    };
    assert_eq!(
        group.members,
        [path(&["a"]), path(&["b"]), path(&["c"])].into()
    );
    let listing = format!("{}/{}", keys[0], keys[2]);
    for (key, expected) in [
        ("unresolved_member.2", listing.clone()),
        (
            "locator.2",
            format!("{listing}: 1 0 0 12.5 0 1 0 0 0 0 1 0"),
        ),
        (
            "locator.3",
            format!("{}: 1 0 0 0 0 1 0 0 0 0 1 0", steps(&path(&["c"]))),
        ),
    ] {
        assert_eq!(
            group.native_options.get(&codec_name(key).unwrap()),
            Some(&NativeOption::Text(Dimension::Literal(
                NonBlankString::new(expected).unwrap()
            )))
        );
    }
}
