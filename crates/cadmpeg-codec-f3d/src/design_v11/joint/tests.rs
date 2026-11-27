// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use cadmpeg_ir::design::feature::FeatureDefinition;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::FeatureContent;
use cadmpeg_ir::products::{ExternalDocument, JointOperand, Storage};

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

/// A root that places `a` and `b`, and a part of `a` that places `c`.
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
        Self {
            definitions: vec![root, part_a, part_b, part_c],
            occurrences: vec![
                Occurrence::new(occurrence("a"), definition("part-a"), None),
                Occurrence::new(occurrence("b"), definition("part-b"), None),
                Occurrence::new(occurrence("c"), definition("part-c"), None),
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
            OperandKeys::new(&[], &[], &[]),
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
            own: None,
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
/// resolves to name an element of that occurrence's document: the connector
/// picks it by its native name, at the stored frame, with no resolved value.
#[test]
fn an_operand_with_unresolved_keys_states_a_native_pick_at_the_stored_frame() {
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
    assert_eq!(pick.target, PickTarget::Native { within: None });
    assert_eq!(pick.context, Some(PickContext::Occurrence(path(&["b"]))));
    assert_eq!(
        pick.native.as_ref().map(|name| &name.value),
        Some(&NativeNameValue::Text(
            NonBlankString::new("b-origin/deeper-key").unwrap()
        ))
    );
    assert_eq!(fixed_rows(construction), STORED);
    assert_eq!(construction.resolved, None);
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
            NonBlankString::new("b-origin/deeper-key").unwrap()
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

/// A coupled joint that binds no feature of the scope is picked by its
/// native name (§3.3): the coupling still names its two joints, and the
/// link's restatement leaves out the coupled joints.
#[test]
fn a_coupled_joint_of_no_scope_feature_is_a_native_pick() {
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
    let link = |name: &str, coupled: Vec<JointId>| {
        joint(
            name,
            94,
            PairedJointKind::Gears {
                distance: None,
                distance2: None,
                ratio: None,
                coupled_joints: coupled,
            },
            [connector(on("a"), STORED), connector(on("b"), STORED)],
        )
    };
    let first = hinge("first", 90);
    let second = hinge("second", 92);
    let coupled = link("link", vec![first.id.clone(), second.id.clone()]);
    let direct = link("direct", Vec::new());
    let twice = link("twice", vec![second.id.clone(), second.id.clone()]);
    let mut features = [
        unresolved("second"),
        unresolved("link"),
        unresolved("direct"),
        unresolved("twice"),
    ];
    let (losses, restated) = restate_with(
        &mut features,
        &[
            legacy("first", &first),
            legacy("second", &second),
            legacy("link", &coupled),
            legacy("direct", &direct),
            legacy("twice", &twice),
        ],
        &[first, second, coupled, direct, twice],
        &[],
    );
    let JointKind::Coupling { joints, .. } = &the_joint(&features[1]).kind else {
        panic!("a coupling")
    };
    let Picks::Named(members) = &joints.as_ref().unwrap().picks else {
        panic!("named picks")
    };
    let [native, feature] = members.as_slice() else {
        panic!("two picks")
    };
    assert_eq!(native.target, PickTarget::Native { within: None });
    assert_eq!(
        native.native.as_ref().map(|name| &name.value),
        Some(&NativeNameValue::Text(
            NonBlankString::new(joint_id("first")).unwrap()
        ))
    );
    assert_eq!(
        feature.target,
        PickTarget::Feature(FeatureId::mint(feature_id("second")).unwrap())
    );
    let cut = |joint: &str| {
        restated
            .iter()
            .find(|(legacy, _, _)| *legacy == joint_id(joint))
            .map(|(_, _, cut)| cut.clone())
    };
    assert_eq!(cut("link"), Some(vec![NATIVE_REF, COUPLED_JOINTS]));

    let JointKind::Coupling { joints, .. } = &the_joint(&features[2]).kind else {
        panic!("a coupling")
    };
    assert_eq!(joints, &None);
    assert_eq!(cut("direct"), Some(vec![NATIVE_REF]));

    assert_eq!(
        features[3].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Joint
        }
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert!(
        loss.message.contains(&feature_id("twice")),
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
        loss.message.contains(&feature_id("doubled")),
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

/// A CADIR write carries each joint a joint feature restates: the group
/// feature restates every joint of its scope and the hinge feature its
/// joint, so the write charges only the joint no feature restates.
#[test]
fn a_cadir_write_carries_the_joints_their_features_restate() {
    use cadmpeg_ir::codec::write::target::TargetRequest;
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
        .plan(EncodeInput::new(&ir, None), TargetRequest::Unnamed)
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
