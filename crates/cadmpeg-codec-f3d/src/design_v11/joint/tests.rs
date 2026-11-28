// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::design::feature::FeatureDefinition;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::FeatureContent;
use cadmpeg_ir::products::{ExternalDocument, JointOperand, Storage};

use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
};
use crate::records::feature::joint_motion::{DesignJointLimitLanes, DesignJointRest};
use crate::records::identity::Located;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn scope(ordinal: u32) -> String {
    format!("{STREAM}:design-parameter-scope#{ordinal}")
}

fn occurrence(name: &str) -> OccurrenceId {
    OccurrenceId::mint(format!("f3d:model:occurrence#{name}")).unwrap()
}

fn definition(name: &str) -> ProductDefinitionId {
    ProductDefinitionId::mint(format!("f3d:model:product-definition#{name}")).unwrap()
}

/// The occurrence key of the carrier of `c`.
const CARRIED: &str = "e1ac1a2e-15ee-4bb1-b9ab-1f1b3ec6ad54";

/// The record that carries `c` under the occurrence key [`CARRIED`].
fn carrier() -> DesignComponentOccurrence {
    DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
        id: format!("{STREAM}:design-component-occurrence#40"),
        class_tag: crate::records::references::DesignClassTag::try_from("339".to_owned()).unwrap(),
        record_index: 40,
        byte_offset: 40,
        component_record_index: 159,
        component_guid: "2ad641b3-9ea3-4e5f-a4da-926c3faeccae"
            .to_owned()
            .try_into()
            .unwrap(),
        occurrence_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(CARRIED.to_owned())
            .unwrap()
            .into(),
        placement: DesignComponentOccurrencePlacement::Explicit {
            ordinal: std::num::NonZeroU32::new(1).unwrap(),
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
        },
        owner_entity: 6,
    })
    .unwrap()
}

/// A root that places `a` and `b`, a part of `a` that places `c`, which
/// [`carrier`] carries, and a definition that the root does not place, which
/// places `d`.
struct Model {
    definitions: Vec<ProductDefinition>,
    occurrences: Vec<Occurrence>,
}

impl Model {
    fn new() -> Self {
        let mut root = ProductDefinition::new(
            definition("root"),
            Storage::File {
                document: ExternalDocument::default(),
                source: None,
            },
        );
        root.occurrences = vec![occurrence("a"), occurrence("b")];
        let mut part_a = ProductDefinition::new(definition("part-a"), Storage::Embedded);
        part_a.occurrences = vec![occurrence("c")];
        let part_b = ProductDefinition::new(definition("part-b"), Storage::Embedded);
        let part_c = ProductDefinition::new(definition("part-c"), Storage::Embedded);
        let mut unplaced = ProductDefinition::new(definition("unplaced"), Storage::Embedded);
        unplaced.occurrences = vec![occurrence("d")];
        Self {
            definitions: vec![root, part_a, part_b, part_c, unplaced],
            occurrences: vec![
                Occurrence::new(occurrence("a"), definition("part-a"), None),
                Occurrence::new(occurrence("b"), definition("part-b"), None),
                {
                    let mut c = Occurrence::new(occurrence("c"), definition("part-c"), None);
                    c.native_ref = Some(carrier().id);
                    c
                },
                Occurrence::new(occurrence("d"), definition("part-b"), None),
            ],
        }
    }
}

fn frame(rows: [[f64; 4]; 3]) -> Transform {
    Transform::affine(rows).unwrap()
}

/// A stored joint frame: a translation of (1, 2, 3).
const STORED: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 1.0],
    [0.0, 1.0, 0.0, 2.0],
    [0.0, 0.0, 1.0, 3.0],
];

/// [`STORED`] turned so that its y axis is its z axis: axes (x, -z, y).
const TURNED: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 1.0],
    [0.0, 0.0, 1.0, 2.0],
    [0.0, -1.0, 0.0, 3.0],
];

fn connector(operand: JointOperand, rows: [[f64; 4]; 3]) -> products::JointConnector {
    products::JointConnector {
        operand,
        frame: frame(rows),
        detached: false,
    }
}

fn joint(
    name: &str,
    ordinal: u32,
    kind: PairedJointKind,
    connectors: [products::JointConnector; 2],
) -> AssemblyJoint {
    let mut joint = AssemblyJoint::paired(
        JointId::mint(format!("f3d:model:joint#{name}")).unwrap(),
        kind,
        connectors,
        None,
    );
    joint.native_ref = Some(scope(ordinal));
    joint
}

fn legacy(key: &str, joint: &AssemblyJoint) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("f3d:model:feature#{key}")).unwrap(),
        ordinal: 0,
        name: Some(key.to_owned()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(LegacyDefinition::Operation(
            LegacyOperation::AssemblyJoint {
                joint: joint.id.clone(),
            },
        )),
        native_ref: joint.native_ref.clone(),
    }
}

fn design(key: &str, operation: FeatureOperation) -> Feature {
    Feature {
        id: FeatureId::mint(format!("f3d:model:feature#{key}")).unwrap(),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn unresolved(key: &str) -> Feature {
    design(
        key,
        FeatureOperation::Unresolved {
            family: OperationFamily::Joint,
        },
    )
}

fn motion(ordinal: u32, codes: &[u32], lanes: Option<DesignJointLimitLanes>) -> DesignJointMotion {
    DesignJointMotion {
        id: format!("{STREAM}:design-joint-motion#{ordinal}"),
        joint_record_index: ordinal - 1,
        joint_byte_offset: 0,
        motion_record_index: ordinal + 1,
        motion_byte_offset: 0,
        freedoms: codes
            .iter()
            .map(|&value| Located { value, offset: 0 })
            .collect(),
        entry_words: Vec::new(),
        limits: lanes,
    }
}

fn lanes(
    flags: (u8, u8),
    minimum: f64,
    maximum: f64,
    rest: Option<(u8, f64)>,
) -> DesignJointLimitLanes {
    DesignJointLimitLanes {
        flags_offset: 0,
        first_flag: flags.0,
        second_flag: flags.1,
        maximum,
        minimum,
        values_offset: 0,
        rest: rest.map(|(flag, value)| DesignJointRest { flag, value }),
        rest_malformed: false,
    }
}

fn finite(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

/// Restate `features` from `legacy` and `joints`, with `sources` by scope
/// ordinal.
fn restate(
    features: &mut [Feature],
    legacy: &[LegacyFeature],
    joints: &[AssemblyJoint],
    sources: &[(u32, Source<'_>)],
) -> Vec<LossNote> {
    restate_with(features, legacy, joints, sources).0
}

/// Each restated record, its restater, and the fields it leaves out.
type Restatements = Vec<(String, String, Vec<&'static str>)>;

/// [`restate`], with the restatements.
fn restate_with(
    features: &mut [Feature],
    legacy: &[LegacyFeature],
    joints: &[AssemblyJoint],
    sources: &[(u32, Source<'_>)],
) -> (Vec<LossNote>, Restatements) {
    restate_keyed(features, legacy, joints, sources, &[])
}

/// [`restate_with`], where `carriers` key the occurrences.
fn restate_keyed(
    features: &mut [Feature],
    legacy: &[LegacyFeature],
    joints: &[AssemblyJoint],
    sources: &[(u32, Source<'_>)],
    carriers: &[DesignComponentOccurrence],
) -> (Vec<LossNote>, Restatements) {
    let model = Model::new();
    let scopes = sources
        .iter()
        .map(|(ordinal, _)| scope(*ordinal))
        .collect::<Vec<_>>();
    let inputs = Inputs::new(
        legacy,
        joints,
        scopes
            .iter()
            .zip(sources)
            .map(|(scope, (_, source))| (scope.as_str(), *source))
            .collect(),
        Places::new(
            &model.definitions,
            &model.occurrences,
            OperandKeys::new(carriers, &[], &[]),
        ),
    );
    let mut losses = Vec::new();
    let restated = project(features, &definition("root"), &inputs, &mut losses)
        .into_iter()
        .map(|Restated { legacy, by, cut }| (legacy, by, cut))
        .collect();
    (losses, restated)
}

fn joint_id(name: &str) -> String {
    format!("f3d:model:joint#{name}")
}

fn feature_id(name: &str) -> String {
    format!("f3d:model:feature#{name}")
}

fn the_joint(feature: &Feature) -> &Joint {
    let FeatureOperation::Joint(joint) = &feature.definition.operation else {
        panic!("a joint: {:?}", feature.definition.operation)
    };
    joint
}

fn construction(
    connector: &JointConnector,
) -> &Construction<cadmpeg_ir::design::construction::kinds::Frame> {
    let JointConnector::Frame(construction) = connector else {
        panic!("a frame connector")
    };
    construction
}

fn only_pick(
    construction: &Construction<cadmpeg_ir::design::construction::kinds::Frame>,
) -> &Picked {
    let [input] = construction.inputs.as_slice() else {
        panic!("one input")
    };
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("a pick input")
    };
    let Picks::Named(members) = &selection.picks else {
        panic!("named picks")
    };
    let [pick] = members.as_slice() else {
        panic!("one pick")
    };
    pick
}

fn path(names: &[&str]) -> OccurrencePath {
    OccurrencePath(
        names
            .iter()
            .map(|name| PathStep {
                occurrence: StepRef::Occurrence(occurrence(name)),
                configuration: None,
            })
            .collect(),
    )
}

fn fixed_rows(
    construction: &Construction<cadmpeg_ir::design::construction::kinds::Frame>,
) -> [[f64; 4]; 3] {
    let [PostStep::Fixed(step)] = construction.after.as_slice() else {
        panic!("one fixed step")
    };
    step.affine_rows()
}

#[test]
fn a_revolute_about_y_states_the_stored_frames_and_its_limits_and_rest() {
    let mut joint = joint(
        "hinge",
        40,
        PairedJointKind::Revolute {
            angle: Some(finite(0.3)),
            angular_limits: JointLimits::new(Some(-0.5), Some(0.5)),
        },
        [
            connector(
                JointOperand::occurrence(occurrence("a"), "a-origin", Vec::new()),
                TURNED,
            ),
            connector(
                JointOperand::occurrence(occurrence("c"), "c-origin", Vec::new()),
                TURNED,
            ),
        ],
    );
    joint.set_frame_sense(Some(PlaneSense::Opposed));
    let motion = motion(40, &[1], Some(lanes((1, 1), -0.5, 0.5, Some((1, 0.25)))));
    let mut features = [unresolved("hinge")];
    let losses = restate(
        &mut features,
        &[legacy("hinge", &joint)],
        &[joint],
        &[(
            40,
            Source {
                motion: Some(&motion),
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let joint = the_joint(&features[0]);
    let JointKind::Motion {
        motion,
        offset,
        limits,
        ..
    } = &joint.kind
    else {
        panic!("a motion")
    };
    assert_eq!(motion, &JointMotion::Revolute { axis: FrameAxis::Y });
    assert_eq!(
        offset,
        &JointOffset {
            x: Some(Dimension::Literal(Length::new(0.0).unwrap())),
            y: Some(Dimension::Literal(Length::new(0.0).unwrap())),
            z: Some(Dimension::Literal(Length::new(0.0).unwrap())),
            angle: Some(Dimension::Literal(Angle::new(0.3).unwrap())),
        }
    );
    assert_eq!(
        limits,
        &[AxisLimits::Rotation {
            axis: FrameAxis::Y,
            limits: Some(Bounds {
                min: Some(Dimension::Literal(Angle::new(-0.5).unwrap())),
                max: Some(Dimension::Literal(Angle::new(0.5).unwrap())),
            }),
            rest: Some(Dimension::Literal(Angle::new(0.25).unwrap())),
        }]
    );
    assert_eq!(joint.alignment, Some(Alignment::Opposed));
    for (connector, names, object) in [
        (&joint.connectors[0], &["a"][..], "a-origin"),
        (&joint.connectors[1], &["a", "c"][..], "c-origin"),
    ] {
        let construction = construction(connector);
        let pick = only_pick(construction);
        assert_eq!(pick.target, PickTarget::Component);
        assert_eq!(pick.context, Some(PickContext::Occurrence(path(names))));
        assert_eq!(
            pick.native.as_ref().map(|name| &name.value),
            Some(&NativeNameValue::Text(NonBlankString::new(object).unwrap()))
        );
        assert_eq!(fixed_rows(construction), STORED);
        let resolved = construction.resolved.as_ref().unwrap();
        assert_eq!(Vector3::from(resolved.y_axis), Vector3::new(0.0, 1.0, 0.0));
    }
}

#[test]
fn a_slider_on_a_joint_origin_keeps_a_disabled_rest_as_a_source_setting() {
    let joint = joint(
        "slide",
        50,
        PairedJointKind::Slider {
            distance: None,
            translation_offset: Some([finite(1.0), finite(0.0), finite(0.0)]),
            linear_limits: JointLimits::new(Some(0.0), Some(20.0)),
        },
        [
            connector(
                JointOperand::root("f3d:model:feature#origin", Vec::new()),
                STORED,
            ),
            connector(
                JointOperand::occurrence(occurrence("b"), "b-origin", Vec::new()),
                STORED,
            ),
        ],
    );
    let motion = motion(50, &[8], Some(lanes((1, 1), 0.0, 2.0, Some((0, 1.5)))));
    let mut features = [
        design(
            "origin",
            FeatureOperation::Native {
                kind: codec_name("JointOrigin").unwrap(),
                native_options: NativeOptions::new(),
                source_settings: NativeOptions::new(),
            },
        ),
        unresolved("slide"),
    ];
    let losses = restate(
        &mut features,
        &[legacy("slide", &joint)],
        &[joint],
        &[(
            50,
            Source {
                as_built: true,
                motion: Some(&motion),
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let joint = the_joint(&features[1]);
    let JointKind::Motion { motion, limits, .. } = &joint.kind else {
        panic!("a motion")
    };
    assert_eq!(motion, &JointMotion::Slider { axis: FrameAxis::Z });
    assert_eq!(
        limits,
        &[AxisLimits::Translation {
            axis: FrameAxis::Z,
            limits: Some(Bounds {
                min: Some(Dimension::Literal(Length::new(0.0).unwrap())),
                max: Some(Dimension::Literal(Length::new(20.0).unwrap())),
            }),
            rest: None,
        }]
    );
    assert_eq!(
        joint
            .source_settings
            .get(&codec_name("rest_value_disabled").unwrap()),
        Some(&NativeOption::Length(Dimension::Literal(
            Length::new(15.0).unwrap()
        )))
    );
    assert_eq!(
        joint.native_options.get(&codec_name("joint_mode").unwrap()),
        Some(&NativeOption::Text(Dimension::Literal(
            NonBlankString::new("As-built").unwrap()
        )))
    );
    let origin = construction(&joint.connectors[0]);
    assert_eq!(
        only_pick(origin).target,
        PickTarget::Datum {
            feature: FeatureId::mint("f3d:model:feature#origin").unwrap(),
            part: None,
        }
    );
    assert!(origin.after.is_empty());
    assert!(origin.resolved.is_some());
}

#[test]
fn other_freedom_codes_state_a_native_motion_with_native_lanes() {
    let joint = joint(
        "pin",
        60,
        PairedJointKind::Fixed {
            angle: Some(finite(0.0)),
            translation_offset: Some([finite(0.0); 3]),
            angular_limits: None,
            linear_limits: None,
        },
        [
            connector(
                JointOperand::occurrence(occurrence("a"), "a-origin", Vec::new()),
                STORED,
            ),
            connector(
                JointOperand::occurrence(occurrence("b"), "b-origin", Vec::new()),
                STORED,
            ),
        ],
    );
    let absent = self::joint(
        "absent",
        61,
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [
            connector(
                JointOperand::occurrence(occurrence("a"), "a-origin", Vec::new()),
                STORED,
            ),
            connector(
                JointOperand::occurrence(occurrence("b"), "b-origin", Vec::new()),
                STORED,
            ),
        ],
    );
    let motion = motion(60, &[1, 8], Some(lanes((1, 1), -1.0, 1.0, None)));
    let mut features = [unresolved("pin"), unresolved("absent")];
    let losses = restate(
        &mut features,
        &[legacy("pin", &joint), legacy("absent", &absent)],
        &[joint],
        &[(
            60,
            Source {
                motion: Some(&motion),
                ..Source::default()
            },
        )],
    );
    let joint = the_joint(&features[0]);
    let JointKind::Motion { motion, limits, .. } = &joint.kind else {
        panic!("a motion")
    };
    assert_eq!(
        motion,
        &JointMotion::Native(NonBlankString::new("f3d:freedom:1,8").unwrap())
    );
    assert!(limits.is_empty());
    assert_eq!(
        joint
            .native_options
            .get(&codec_name("limit_minimum").unwrap()),
        Some(&NativeOption::Real(Dimension::Literal(finite(-1.0))))
    );
    assert_eq!(
        features[1].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Joint
        }
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        F3dLossCode::AssemblyDesignJointUnprojected.kind()
    );
    assert!(
        loss.message.contains("f3d:model:feature#absent"),
        "{}",
        loss.message
    );
}

#[test]
fn a_joint_with_no_motion_record_is_rigid() {
    let joint = joint(
        "weld",
        70,
        PairedJointKind::Fixed {
            angle: Some(finite(0.0)),
            translation_offset: Some([finite(0.0); 3]),
            angular_limits: None,
            linear_limits: None,
        },
        [
            connector(
                JointOperand::occurrence(occurrence("a"), "a-origin", Vec::new()),
                STORED,
            ),
            connector(
                JointOperand::occurrence(occurrence("b"), "b-origin", Vec::new()),
                STORED,
            ),
        ],
    );
    let mut features = [unresolved("weld")];
    let losses = restate(&mut features, &[legacy("weld", &joint)], &[joint], &[]);
    assert!(losses.is_empty(), "{losses:?}");
    let JointKind::Motion { motion, .. } = &the_joint(&features[0]).kind else {
        panic!("a motion")
    };
    assert_eq!(motion, &JointMotion::Rigid);
}

fn fixed() -> PairedJointKind {
    PairedJointKind::Fixed {
        angle: Some(finite(0.0)),
        translation_offset: Some([finite(0.0); 3]),
        angular_limits: None,
        linear_limits: None,
    }
}

fn on(name: &str) -> JointOperand {
    JointOperand::occurrence(occurrence(name), format!("{name}-origin"), Vec::new())
}

#[test]
fn an_as_built_group_states_its_members_and_keeps_their_locators() {
    let first = joint(
        "group",
        80,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let member = joint(
        "group:2",
        80,
        fixed(),
        [connector(on("a"), STORED), connector(on("c"), TURNED)],
    );
    let unbound = joint(
        "group:3",
        80,
        fixed(),
        [
            connector(on("a"), STORED),
            connector(
                JointOperand::external(ExternalDocument::default(), "listed", Vec::new()),
                STORED,
            ),
        ],
    );
    let mut features = [unresolved("group")];
    let losses = restate(
        &mut features,
        &[legacy("group", &first)],
        &[first, member, unbound],
        &[(
            80,
            Source {
                group: true,
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let FeatureOperation::RigidGroup(group) = &features[0].definition.operation else {
        panic!("a rigid group: {:?}", features[0].definition.operation)
    };
    assert_eq!(
        group.members,
        [path(&["a"]), path(&["b"]), path(&["a", "c"])].into()
    );
    let text = |name: &str| match group.native_options.get(&codec_name(name).unwrap()) {
        Some(NativeOption::Text(Dimension::Literal(text))) => text.as_str().to_owned(),
        other => panic!("{name}: {other:?}"),
    };
    assert_eq!(
        text("locator.2"),
        format!(
            "{}/{}: 1 0 0 1 0 0 1 2 0 -1 0 3",
            occurrence("a"),
            occurrence("c")
        )
    );
    assert_eq!(text("unresolved_member.3"), "listed");
    assert!(text("locator.3").starts_with("listed: "));
}

#[test]
fn a_group_with_one_occurrence_member_stays_unresolved() {
    let first = joint(
        "lone",
        81,
        fixed(),
        [
            connector(on("a"), STORED),
            connector(JointOperand::root("unlisted", Vec::new()), STORED),
        ],
    );
    let mut features = [unresolved("lone")];
    let losses = restate(
        &mut features,
        &[legacy("lone", &first)],
        &[first],
        &[(
            81,
            Source {
                group: true,
                ..Source::default()
            },
        )],
    );
    assert_eq!(
        features[0].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Joint
        }
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert!(
        loss.message.contains("f3d:model:feature#lone"),
        "{}",
        loss.message
    );
}

/// A feature keeps its component where the connector paths do not start
/// at it: each pick reaches its occurrence through the scope's definition
/// (§5.8, `InContext`).
#[test]
fn a_feature_of_an_operand_component_keeps_it_and_picks_through_the_scope_definition() {
    let fixed_pair = joint(
        "pair",
        82,
        fixed(),
        [connector(on("a"), STORED), connector(on("c"), STORED)],
    );
    let mut feature = unresolved("pair");
    feature.component = Some(definition("part-b"));
    let mut features = [feature];
    let (losses, restated) = restate_with(
        &mut features,
        &[legacy("pair", &fixed_pair)],
        &[fixed_pair],
        &[(82, Source::default())],
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(features[0].component, Some(definition("part-b")));
    let contexts = the_joint(&features[0])
        .connectors
        .iter()
        .map(|connector| only_pick(construction(connector)).context.clone())
        .collect::<Vec<_>>();
    let through = |names: &[&str]| {
        Some(PickContext::InContext {
            assembly: DefinitionRef {
                definition: definition("root"),
                configuration: None,
            },
            own: Some(path(&["b"])),
            target: Some(path(names)),
        })
    };
    assert_eq!(contexts, [through(&["a"]), through(&["a", "c"])]);
    assert_eq!(
        restated,
        [(joint_id("pair"), feature_id("pair"), vec![NATIVE_REF])]
    );
}

/// A rigid group's member paths start at the definition that owns the
/// group: the group roots at the scope's definition and leaves out the
/// legacy feature's definition.
#[test]
fn a_group_of_an_operand_component_roots_at_the_scope_and_cuts_the_definition() {
    let first = joint(
        "set",
        85,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let mut feature = unresolved("set");
    feature.component = Some(definition("part-b"));
    feature.native_ref = first.native_ref.clone();
    let mut features = [feature];
    let (losses, restated) = restate_with(
        &mut features,
        &[legacy("set", &first)],
        &[first],
        &[(
            85,
            Source {
                group: true,
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(features[0].component, None);
    let FeatureOperation::RigidGroup(group) = &features[0].definition.operation else {
        panic!("a rigid group")
    };
    assert_eq!(group.members, [path(&["a"]), path(&["b"])].into());
    assert_eq!(
        restated,
        [
            (
                feature_id("set"),
                feature_id("set"),
                vec!["product_definition"]
            ),
            (joint_id("set"), feature_id("set"), Vec::new()),
        ]
    );
}

/// The keys of an operand that name no occurrence below the one its path
/// resolves to name occurrences the document does not hold: the connector
/// picks the component through their native steps (§5.8), at the stored
/// frame in that component's coordinates.
#[test]
fn an_operand_below_the_held_occurrences_picks_its_component_through_native_steps() {
    let deep = joint(
        "deep",
        86,
        fixed(),
        [
            connector(on("a"), STORED),
            connector(
                JointOperand::occurrence(
                    occurrence("b"),
                    "b-origin",
                    vec!["deeper-key".to_owned()],
                ),
                STORED,
            ),
        ],
    );
    let mut features = [unresolved("deep")];
    let (losses, restated) = restate_with(
        &mut features,
        &[legacy("deep", &deep)],
        &[deep],
        &[(86, Source::default())],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let construction = construction(&the_joint(&features[0]).connectors[1]);
    let pick = only_pick(construction);
    assert_eq!(pick.target, PickTarget::Component);
    let mut below = path(&["b"]);
    below.0.push(PathStep {
        occurrence: StepRef::Native(native_name(["deeper-key"]).unwrap()),
        configuration: None,
    });
    assert_eq!(pick.context, Some(PickContext::Occurrence(below)));
    assert_eq!(
        pick.native.as_ref().map(|name| &name.value),
        Some(&NativeNameValue::Text(
            NonBlankString::new("b-origin").unwrap()
        ))
    );
    assert_eq!(fixed_rows(construction), STORED);
    assert_eq!(construction.resolved, frame_value(&frame(STORED)));
    assert_eq!(
        restated,
        [(joint_id("deep"), feature_id("deep"), vec![NATIVE_REF])]
    );

    let group = joint(
        "deep-group",
        87,
        fixed(),
        [
            connector(on("a"), STORED),
            connector(
                JointOperand::occurrence(
                    occurrence("b"),
                    "b-origin",
                    vec!["deeper-key".to_owned()],
                ),
                STORED,
            ),
        ],
    );
    let member = joint(
        "deep-group:2",
        87,
        fixed(),
        [connector(on("a"), STORED), connector(on("c"), STORED)],
    );
    let mut features = [unresolved("deep-group")];
    let losses = restate(
        &mut features,
        &[legacy("deep-group", &group)],
        &[group, member],
        &[(
            87,
            Source {
                group: true,
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let FeatureOperation::RigidGroup(group) = &features[0].definition.operation else {
        panic!("a rigid group")
    };
    assert_eq!(group.members, [path(&["a"]), path(&["a", "c"])].into());
    assert_eq!(
        group
            .native_options
            .get(&codec_name("unresolved_member.1").unwrap()),
        Some(&NativeOption::Text(Dimension::Literal(
            NonBlankString::new(format!("{}/b-origin/deeper-key", occurrence("b"))).unwrap()
        )))
    );
}

#[test]
fn a_joint_on_an_occurrence_of_no_definition_stays_unresolved() {
    let dangling = joint(
        "dangling",
        83,
        fixed(),
        [connector(on("a"), STORED), connector(on("z"), STORED)],
    );
    let mut features = [unresolved("dangling")];
    let losses = restate(
        &mut features,
        &[legacy("dangling", &dangling)],
        &[dangling],
        &[(83, Source::default())],
    );
    assert_eq!(
        features[0].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Joint
        }
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert!(
        loss.message.contains("f3d:model:feature#dangling"),
        "{}",
        loss.message
    );
}

#[test]
fn a_motion_link_couples_the_features_of_its_two_revolute_joints() {
    let hinge = |name: &str, ordinal: u32| {
        joint(
            name,
            ordinal,
            PairedJointKind::Revolute {
                angle: Some(finite(0.0)),
                angular_limits: None,
            },
            [connector(on("a"), STORED), connector(on("b"), STORED)],
        )
    };
    let first = hinge("first", 90);
    let second = hinge("second", 92);
    let link = joint(
        "link",
        94,
        PairedJointKind::Gears {
            distance: None,
            distance2: None,
            ratio: cadmpeg_ir::scalar::NonZeroReal::new(-2.0),
            coupled_joints: vec![first.id.clone(), second.id.clone()],
        },
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let mut features = [
        unresolved("first"),
        unresolved("second"),
        unresolved("link"),
    ];
    let losses = restate(
        &mut features,
        &[
            legacy("first", &first),
            legacy("second", &second),
            legacy("link", &link),
        ],
        &[first, second, link],
        &[],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let JointKind::Coupling { coupling, joints } = &the_joint(&features[2]).kind else {
        panic!("a coupling")
    };
    assert_eq!(
        coupling,
        &Coupling::Gear {
            ratio: Some(Dimension::Literal(finite(-2.0))),
            radii: [None, None],
        }
    );
    let Picks::Named(members) = &joints.as_ref().unwrap().picks else {
        panic!("named picks")
    };
    assert_eq!(
        members
            .as_slice()
            .iter()
            .map(|pick| pick.target.clone())
            .collect::<Vec<_>>(),
        [
            PickTarget::Feature(FeatureId::mint("f3d:model:feature#first").unwrap()),
            PickTarget::Feature(FeatureId::mint("f3d:model:feature#second").unwrap()),
        ]
    );
}

/// A coupling picks the design features of its joints (§6.7 Couplings): a
/// coupled joint that binds no design feature leaves the coupling
/// unresolved, and the loss names the joint by its scope record.
#[test]
fn a_coupling_of_a_joint_with_no_design_feature_stays_unresolved() {
    let hinge = |name: &str, ordinal: u32| {
        joint(
            name,
            ordinal,
            PairedJointKind::Revolute {
                angle: Some(finite(0.0)),
                angular_limits: None,
            },
            [connector(on("a"), STORED), connector(on("b"), STORED)],
        )
    };
    let first = hinge("first", 90);
    let second = hinge("second", 92);
    let link = joint(
        "link",
        94,
        PairedJointKind::Gears {
            distance: None,
            distance2: None,
            ratio: None,
            coupled_joints: vec![first.id.clone(), second.id.clone()],
        },
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let mut features = [unresolved("second"), unresolved("link")];
    let losses = restate(
        &mut features,
        &[
            legacy("first", &first),
            legacy("second", &second),
            legacy("link", &link),
        ],
        &[first, second, link],
        &[],
    );
    assert_eq!(
        features[1].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Joint
        }
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        F3dLossCode::AssemblyDesignJointUnprojected.kind()
    );
    assert!(
        loss.message.contains(&format!(
            "{} (its coupled joint {} has no design feature)",
            feature_id("link"),
            scope(90)
        )),
        "{}",
        loss.message
    );
}

/// Two motion records that bind one scope leave its motion unread: the joint
/// is stated rigid, and a loss names the assumption.
#[test]
fn a_joint_of_two_motion_records_is_rigid_with_a_loss() {
    let joint = joint(
        "doubled",
        71,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let mut features = [unresolved("doubled")];
    let losses = restate(
        &mut features,
        &[legacy("doubled", &joint)],
        &[joint],
        &[(
            71,
            Source {
                ambiguous: true,
                ..Source::default()
            },
        )],
    );
    let JointKind::Motion { motion, .. } = &the_joint(&features[0]).kind else {
        panic!("a motion")
    };
    assert_eq!(motion, &JointMotion::Rigid);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        F3dLossCode::AssemblyJointMotionAssumedRigid.kind()
    );
    assert!(
        loss.message.contains(&format!(
            "{} (assembly joint {})",
            feature_id("doubled"),
            joint_id("doubled")
        )),
        "{}",
        loss.message
    );
}

/// A group member whose placement or connectors the group does not state is
/// restated without its operands.
#[test]
fn a_group_member_off_the_group_placement_is_cut_in_its_operands() {
    let first = joint(
        "cluster",
        89,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let placed = joint(
        "cluster:2",
        89,
        fixed(),
        [connector(on("a"), STORED), connector(on("c"), STORED)],
    );
    let offset = joint(
        "cluster:3",
        89,
        PairedJointKind::Fixed {
            angle: Some(finite(0.5)),
            translation_offset: Some([finite(0.0); 3]),
            angular_limits: None,
            linear_limits: None,
        },
        [connector(on("a"), STORED), connector(on("c"), STORED)],
    );
    let elsewhere = joint(
        "cluster:4",
        89,
        fixed(),
        [connector(on("b"), TURNED), connector(on("c"), STORED)],
    );
    let mut feature = unresolved("cluster");
    feature.native_ref = first.native_ref.clone();
    let mut features = [feature];
    let (losses, restated) = restate_with(
        &mut features,
        &[legacy("cluster", &first)],
        &[first, placed, offset, elsewhere],
        &[(
            89,
            Source {
                group: true,
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(
        restated,
        [
            (joint_id("cluster"), feature_id("cluster"), Vec::new()),
            (joint_id("cluster:2"), feature_id("cluster"), Vec::new()),
            (joint_id("cluster:3"), feature_id("cluster"), vec![OPERANDS]),
            (joint_id("cluster:4"), feature_id("cluster"), vec![OPERANDS]),
        ]
    );
}

/// An external document's placement is not one occurrence's: its frame is
/// the operand's offset, with no resolved value in the owner's coordinates.
/// The pick names the document, the object, and the keys below it.
#[test]
fn an_external_operand_frame_is_an_offset_with_no_resolved_value() {
    let document = ExternalDocument::document_id("inserted-document");
    let inserted = joint(
        "inserted",
        88,
        fixed(),
        [
            connector(on("a"), STORED),
            connector(
                JointOperand::external(document, "inserted-key", vec!["origin".to_owned()]),
                TURNED,
            ),
        ],
    );
    let mut features = [unresolved("inserted")];
    let losses = restate(
        &mut features,
        &[legacy("inserted", &inserted)],
        &[inserted],
        &[(88, Source::default())],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let construction = construction(&the_joint(&features[0]).connectors[1]);
    let pick = only_pick(construction);
    assert_eq!(pick.target, PickTarget::Native { within: None });
    assert_eq!(pick.context, None);
    assert_eq!(
        pick.native.as_ref().map(|name| &name.value),
        Some(&NativeNameValue::Text(
            NonBlankString::new("inserted-document/inserted-key/origin").unwrap()
        ))
    );
    assert_eq!(fixed_rows(construction), TURNED);
    assert_eq!(construction.resolved, None);
}

/// A group member whose keys descend in part is listed with every value its
/// operand stores: the occurrence, the object, and each key, the descended
/// key included.
#[test]
fn a_group_listing_of_a_partly_descended_member_keeps_every_stored_key() {
    let first = joint(
        "listed",
        92,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let partial = joint(
        "listed:2",
        92,
        fixed(),
        [
            connector(on("a"), STORED),
            connector(
                JointOperand::occurrence(
                    occurrence("a"),
                    "a-key",
                    vec![CARRIED.to_owned(), "deeper-key".to_owned()],
                ),
                STORED,
            ),
        ],
    );
    let mut feature = unresolved("listed");
    feature.native_ref = first.native_ref.clone();
    let mut features = [feature];
    let (losses, restated) = restate_keyed(
        &mut features,
        &[legacy("listed", &first)],
        &[first, partial],
        &[(
            92,
            Source {
                group: true,
                ..Source::default()
            },
        )],
        &[carrier()],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let FeatureOperation::RigidGroup(group) = &features[0].definition.operation else {
        panic!("a rigid group: {:?}", features[0].definition.operation)
    };
    assert_eq!(group.members, [path(&["a"]), path(&["b"])].into());
    assert_eq!(
        group
            .native_options
            .get(&codec_name("unresolved_member.2").unwrap()),
        Some(&NativeOption::Text(Dimension::Literal(
            NonBlankString::new(format!("{}/a-key/{CARRIED}/deeper-key", occurrence("a"))).unwrap()
        )))
    );
    assert_eq!(
        restated,
        [
            (joint_id("listed"), feature_id("listed"), Vec::new()),
            (joint_id("listed:2"), feature_id("listed"), Vec::new()),
        ]
    );
}

/// A group whose component is the scope's definition keeps it, though a
/// member names no occurrence of it: its member paths start there.
#[test]
fn a_root_group_keeps_the_scope_definition_as_its_component() {
    let first = joint(
        "root-set",
        89,
        fixed(),
        [connector(on("a"), STORED), connector(on("z"), STORED)],
    );
    let second = joint(
        "root-set:2",
        89,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let mut feature = unresolved("root-set");
    feature.component = Some(definition("root"));
    feature.native_ref = first.native_ref.clone();
    let mut features = [feature];
    let (losses, restated) = restate_with(
        &mut features,
        &[legacy("root-set", &first)],
        &[first, second],
        &[(
            89,
            Source {
                group: true,
                ..Source::default()
            },
        )],
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(features[0].component, Some(definition("root")));
    let FeatureOperation::RigidGroup(group) = &features[0].definition.operation else {
        panic!("a rigid group")
    };
    assert_eq!(group.members, [path(&["a"]), path(&["b"])].into());
    assert!(
        restated
            .iter()
            .all(|(legacy, _, _)| *legacy != feature_id("root-set")),
        "{restated:?}"
    );
}

/// A `Root` operand names an object in the document root, the scope's
/// definition: where the feature's definition is another, the joint picks it
/// through the scope's definition (§5.8, `InContext`).
#[test]
fn a_root_operand_of_a_component_feature_picks_in_the_scope_definition() {
    let mut root_frame = STORED;
    root_frame[0][3] = 12.0;
    let pair = joint(
        "root-pair",
        90,
        fixed(),
        [
            connector(JointOperand::root("root-key", Vec::new()), root_frame),
            connector(on("b"), STORED),
        ],
    );
    for repeated_owner in [false, true] {
        let mut feature = unresolved("root-pair");
        feature.component = Some(definition("part-a"));
        let mut features = [feature];
        let mut model = Model::new();
        let mut placed_at = Transform::identity().affine_rows();
        placed_at[0][3] = 10.0;
        model.occurrences[0].transform = Some(frame(placed_at));
        if repeated_owner {
            let repeated = occurrence("a-second");
            model.definitions[0].occurrences.push(repeated.clone());
            model
                .occurrences
                .push(Occurrence::new(repeated, definition("part-a"), None));
        }
        let legacy = [legacy("root-pair", &pair)];
        let joints = [pair.clone()];
        let inputs = Inputs::new(
            &legacy,
            &joints,
            HashMap::new(),
            Places::new(
                &model.definitions,
                &model.occurrences,
                OperandKeys::new(&[], &[], &[]),
            ),
        );
        let mut losses = Vec::new();
        project(&mut features, &definition("root"), &inputs, &mut losses);
        assert!(losses.is_empty(), "{losses:?}");
        assert_eq!(features[0].component, Some(definition("part-a")));
        let through = |target: Option<OccurrencePath>| {
            Some(PickContext::InContext {
                assembly: DefinitionRef {
                    definition: definition("root"),
                    configuration: None,
                },
                own: (!repeated_owner).then(|| path(&["a"])),
                target,
            })
        };
        let joint = the_joint(&features[0]);
        let root = construction(&joint.connectors[0]);
        let cached_x = root.resolved.as_ref().unwrap().origin.coordinates()[0].get();
        assert_eq!(cached_x, 12.0);
        assert_eq!(cached_x - placed_at[0][3], 2.0);
        let root = only_pick(root);
        assert_eq!(root.target, PickTarget::Native { within: None });
        assert_eq!(root.context, through(None));
        assert_eq!(
            only_pick(construction(&joint.connectors[1])).context,
            through(Some(path(&["b"])))
        );
    }
}

/// Where the scope's definition has no path to the feature's definition, a
/// joint whose picks start there stays at it: a `Root` operand is picked in
/// the scope's definition.
#[test]
fn a_root_operand_of_an_unplaced_component_feature_is_picked_in_the_scope_definition() {
    let pair = joint(
        "unplaced-pair",
        94,
        fixed(),
        [
            connector(JointOperand::root("root-key", Vec::new()), STORED),
            connector(on("d"), STORED),
        ],
    );
    let mut feature = unresolved("unplaced-pair");
    feature.component = Some(definition("unplaced"));
    let mut features = [feature];
    let losses = restate(
        &mut features,
        &[legacy("unplaced-pair", &pair)],
        &[pair],
        &[(94, Source::default())],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let contexts = the_joint(&features[0])
        .connectors
        .iter()
        .map(|connector| only_pick(construction(connector)).context.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        contexts,
        [
            Some(PickContext::Definition(DefinitionRef {
                definition: definition("root"),
                configuration: None,
            })),
            Some(PickContext::Occurrence(path(&["d"]))),
        ]
    );
}

/// A joint origin is a datum of its own feature's definition: the joint
/// picks it through the path to that definition's occurrence.
#[test]
fn a_datum_of_another_definition_is_picked_through_its_occurrence() {
    let pair = joint(
        "datum-pair",
        91,
        fixed(),
        [
            connector(
                JointOperand::root("f3d:model:feature#b-origin", Vec::new()),
                STORED,
            ),
            connector(on("a"), STORED),
        ],
    );
    let mut origin = design(
        "b-origin",
        FeatureOperation::Native {
            kind: codec_name("JointOrigin").unwrap(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    );
    origin.component = Some(definition("part-b"));
    let mut features = [origin, unresolved("datum-pair")];
    let losses = restate(
        &mut features,
        &[legacy("datum-pair", &pair)],
        &[pair],
        &[(91, Source::default())],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let datum = only_pick(construction(&the_joint(&features[1]).connectors[0]));
    assert_eq!(
        datum.target,
        PickTarget::Datum {
            feature: FeatureId::mint("f3d:model:feature#b-origin").unwrap(),
            part: None,
        }
    );
    assert_eq!(datum.context, Some(PickContext::Occurrence(path(&["b"]))));
}

/// A datum feature whose definition the scope places other than once is
/// picked in that definition.
#[test]
fn a_datum_of_an_unplaced_definition_is_picked_in_it() {
    let pair = joint(
        "loose-pair",
        93,
        fixed(),
        [
            connector(
                JointOperand::root("f3d:model:feature#loose-origin", Vec::new()),
                STORED,
            ),
            connector(on("a"), STORED),
        ],
    );
    let mut origin = design(
        "loose-origin",
        FeatureOperation::Native {
            kind: codec_name("JointOrigin").unwrap(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    );
    origin.component = Some(definition("loose"));
    let mut features = [origin, unresolved("loose-pair")];
    let losses = restate(
        &mut features,
        &[legacy("loose-pair", &pair)],
        &[pair],
        &[(93, Source::default())],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let datum = only_pick(construction(&the_joint(&features[1]).connectors[0]));
    assert_eq!(
        datum.context,
        Some(PickContext::Definition(DefinitionRef {
            definition: definition("loose"),
            configuration: None,
        }))
    );
}

/// The occurrence keys an operand stores below its occurrence name the
/// occurrences they carry: the pick reaches the carried occurrence.
#[test]
fn an_operand_key_that_names_a_child_occurrence_reaches_it() {
    let carried = joint(
        "carried",
        84,
        fixed(),
        [
            connector(on("b"), STORED),
            connector(
                JointOperand::occurrence(occurrence("a"), "c-origin", vec![CARRIED.to_owned()]),
                STORED,
            ),
        ],
    );
    let mut features = [unresolved("carried")];
    let (losses, _) = restate_keyed(
        &mut features,
        &[legacy("carried", &carried)],
        &[carried],
        &[(84, Source::default())],
        &[carrier()],
    );
    assert!(losses.is_empty(), "{losses:?}");
    let construction = construction(&the_joint(&features[0]).connectors[1]);
    let pick = only_pick(construction);
    assert_eq!(pick.target, PickTarget::Component);
    assert_eq!(
        pick.context,
        Some(PickContext::Occurrence(path(&["a", "c"])))
    );
    assert_eq!(construction.resolved, frame_value(&frame(STORED)));
}

/// A CADIR write carries each joint a joint feature restates: the group
/// feature restates every joint of its scope and the hinge feature its
/// joint, so the write charges only the joint no feature restates.
#[test]
fn a_cadir_write_carries_the_joints_their_features_restate() {
    use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};

    let group = [
        joint(
            "group",
            80,
            fixed(),
            [connector(on("a"), STORED), connector(on("b"), STORED)],
        ),
        joint(
            "group:2",
            80,
            fixed(),
            [connector(on("a"), STORED), connector(on("c"), TURNED)],
        ),
    ];
    let hinge = joint(
        "hinge",
        84,
        PairedJointKind::Revolute {
            angle: Some(finite(0.0)),
            angular_limits: None,
        },
        [connector(on("a"), TURNED), connector(on("b"), TURNED)],
    );
    let alone = joint(
        "alone",
        88,
        fixed(),
        [connector(on("a"), STORED), connector(on("b"), STORED)],
    );
    let mut ir = cadmpeg_ir::CadIr::empty();
    let model = Model::new();
    ir.model.product_definitions = model.definitions;
    ir.model.occurrences = model.occurrences;
    ir.model.features = vec![legacy("group", &group[0]), legacy("hinge", &hinge)];
    ir.model.assembly_joints = group.into_iter().chain([hinge, alone]).collect();
    let mut features = [unresolved("group"), unresolved("hinge")];
    features[0].native_ref = Some(scope(80));
    features[1].native_ref = Some(scope(84));
    let motions = [motion(84, &[1], None)];
    let scopes = [scope(80), scope(84)];
    let restated = {
        let inputs = Inputs::new(
            &ir.model.features,
            &ir.model.assembly_joints,
            HashMap::from([
                (
                    scopes[0].as_str(),
                    Source {
                        group: true,
                        ..Source::default()
                    },
                ),
                (
                    scopes[1].as_str(),
                    Source {
                        motion: Some(&motions[0]),
                        ..Source::default()
                    },
                ),
            ]),
            Places::new(
                &ir.model.product_definitions,
                &ir.model.occurrences,
                OperandKeys::new(&[], &[], &[]),
            ),
        );
        let mut losses = Vec::new();
        let restated = project(&mut features, &definition("root"), &inputs, &mut losses);
        assert!(losses.is_empty(), "{losses:?}");
        restated
    };
    record(&mut ir.model, restated);
    ir.design.features.extend(features);
    cadmpeg_ir::design::projection::restated::restate_features(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut ir,
    )
    .expect("restate");

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("plan");
    let joints = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.message.contains("joint#"))
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        joints,
        [
            "the cadir write does not represent model arena `assembly_joints`: \
          1 record(s) omitted: f3d:model:joint#alone"
        ]
    );
    let features = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.message.contains("arena `features`"))
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    assert!(
        features
            .iter()
            .all(|message| !message.contains("definition")),
        "{features:?}"
    );
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("write");
    let read = cadmpeg_ir::CadIr::from_json(std::str::from_utf8(&written).unwrap()).unwrap();
    let operations = read
        .design
        .features
        .iter()
        .map(|feature| match &feature.definition.operation {
            FeatureOperation::RigidGroup(_) => "rigid group",
            FeatureOperation::Joint(_) => "joint",
            _ => "other",
        })
        .collect::<Vec<_>>();
    assert_eq!(operations, ["rigid group", "joint"]);
}

mod projection;
