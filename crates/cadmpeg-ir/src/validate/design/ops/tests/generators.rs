// SPDX-License-Identifier: Apache-2.0
//! Part 10, generators (§6.3): the container and generator of a pattern's
//! members, and the template inputs a member picks.

use std::collections::BTreeMap;

use super::super::super::fixture::{self, assert_finding, design_findings, id};
use super::{entity, extrude, feature, linear, named, pattern_of, pick, profile_sketch, with};
use crate::design::construction::Construction;
use crate::design::container::Container;
use crate::design::feature::{FeatureOperation, OperationFamily};
use crate::design::names::{InstanceIndex, NameKind};
use crate::design::ops::body::{PatternInstances, TemplateInput, TemplateInstance};
use crate::design::ops::sketch::{Sketch, SketchPlane};
use crate::design::refs::Member;
use crate::design::selection::PickTarget;

#[test]
fn pattern_members_belong_to_its_container_and_to_one_generator() {
    use crate::design::container::UnorderedKind;
    use crate::design::ops::body::PatternMember;
    let members_of = |seed: &str| {
        let FeatureOperation::Pattern(mut pattern) = pattern_of(seed, linear(None, 3), Vec::new())
        else {
            unreachable!()
        };
        pattern.instances = PatternInstances::Members(vec![PatternMember {
            index: InstanceIndex(vec![1]),
            features: vec![feature("copy")],
        }]);
        FeatureOperation::Pattern(pattern)
    };
    let mut ir = fixture::document();
    ir.design.containers[0] = Container::Unordered {
        id: id("container", "timeline"),
        kind: UnorderedKind::Group,
        members: [
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
        ]
        .into_iter()
        .collect(),
    };
    ir.design.features[0].definition.operation = members_of("fillet");
    ir.design
        .features
        .push(fixture::unresolved("copy", OperationFamily::Fillet));
    assert_finding(
        &ir,
        "pattern member test:design:feature#copy is no member of the pattern's container",
    );
    let Container::Unordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.insert(Member::Feature(feature("copy")));
    assert!(!design_findings(&ir).iter().any(|f| f
        .message
        .contains("no member of the pattern's container")
        || f.message.contains("several generators")));
    ir.design.features[1].definition.operation = members_of("extrude");
    assert_finding(&ir, "feature is a member of several generators");
}

#[test]
fn a_template_input_pick_names_an_input_of_its_instance() {
    let instance = TemplateInstance {
        template: crate::design::ops::body::Template {
            name: None,
            library: None,
            variant: None,
        },
        inputs: vec![TemplateInput {
            role: None,
            reference: named(vec![entity(NameKind::Face, 1)]),
        }],
        parameters: Vec::new(),
        members: vec![feature("fillet")],
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    let mut ir = with(FeatureOperation::TemplateInstance(Box::new(instance)));
    let input = |input| {
        let mut picking = extrude();
        picking.profile = named(vec![pick(PickTarget::TemplateInput {
            instance: feature("extrude"),
            input,
        })]);
        FeatureOperation::Extrude(Box::new(picking))
    };
    ir.design.features[1].definition.operation = input(0);
    assert!(!design_findings(&ir)
        .iter()
        .any(|f| f.message.contains("TemplateInput")));
    ir.design.features[1].definition.operation = input(1);
    assert_finding(&ir, "TemplateInput picks input 1");

    let plane = |input| {
        SketchPlane::Placed(Box::new(Construction {
            inputs: vec![crate::design::construction::ConstructionInput {
                role: None,
                reference: Some(crate::design::construction::InputReference::Picks(named(
                    vec![pick(PickTarget::TemplateInput {
                        instance: feature("extrude"),
                        input,
                    })],
                ))),
                value: None,
                reversed: None,
            }],
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
        }))
    };
    ir.design.features[1].definition.operation = input(0);
    ir.design.sketches.push(Sketch {
        owner: ir.design.features[1].id.clone(),
        plane: plane(0),
        ..profile_sketch()
    });
    assert!(!design_findings(&ir)
        .iter()
        .any(|f| f.message.contains("TemplateInput")));
    ir.design.sketches[0].plane = plane(1);
    assert_finding(&ir, "TemplateInput picks input 1");
}
