// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::design::selection::StepRef;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

fn occurrence_id(id: &str) -> OccurrenceId {
    OccurrenceId::mint(id).unwrap()
}

fn definition_id(id: &str) -> ProductDefinitionId {
    ProductDefinitionId::mint(id).unwrap()
}

fn translated(x: f64) -> Transform {
    let mut rows = Transform::identity().affine_rows();
    rows[0][3] = x;
    Transform::affine(rows).unwrap()
}

fn occurrence(id: &str, prototype: &str, x: f64) -> Occurrence {
    Occurrence::new(
        occurrence_id(id),
        definition_id(prototype),
        Some(translated(x)),
    )
}

fn component(id: &str, occurrences: &[&str]) -> ProductDefinition {
    let mut definition = ProductDefinition::new(definition_id(id), Storage::Embedded);
    definition.occurrences = occurrences.iter().map(|id| occurrence_id(id)).collect();
    definition
}

fn root(occurrences: &[&str]) -> ProductDefinition {
    let mut definition =
        ProductDefinition::root(super::file_root_id(), ExternalDocument::default());
    definition.occurrences = occurrences.iter().map(|id| occurrence_id(id)).collect();
    definition
}

fn own_once(
    ir: &mut CadIr,
    scopes: &BTreeMap<OccurrenceId, String>,
) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::own_once(&ctx, ir, scopes)
        .unwrap()
        .into_iter()
        .map(|loss| loss.code)
        .collect()
}

fn definition<'a>(ir: &'a CadIr, id: &str) -> &'a ProductDefinition {
    ir.model
        .product_definitions
        .iter()
        .find(|definition| definition.id.as_str() == id)
        .unwrap_or_else(|| panic!("definition {id}"))
}

fn placed<'a>(ir: &'a CadIr, id: &str) -> &'a Occurrence {
    ir.model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str() == id)
        .unwrap_or_else(|| panic!("occurrence {id}"))
}

fn listed(definition: &ProductDefinition) -> Vec<&str> {
    definition
        .occurrences
        .iter()
        .map(OccurrenceId::as_str)
        .collect()
}

fn path(entry: &cadmpeg_ir::products::NestedOverride) -> Vec<&str> {
    entry
        .path
        .0
        .iter()
        .map(|step| match &step.occurrence {
            StepRef::Occurrence(id) => id.as_str(),
            StepRef::Native(_) => panic!("native step"),
        })
        .collect()
}

const ROOT_COMPONENT: &str = "f3d:model:component#root-component";
const BRACKET: &str = "f3d:model:component#bracket";
const BOLT: &str = "f3d:model:component#bolt";
const ROOT_OCCURRENCE: &str = "f3d:model:occurrence#root-context";

/// The root component, placed by its identity occurrence, holds two
/// placements of a bracket, which holds one bolt; the decoder lists the bolt
/// of each placement.
fn shared_bracket() -> CadIr {
    let mut ir = CadIr::empty();
    let mut root_occurrence = occurrence(ROOT_OCCURRENCE, ROOT_COMPONENT, 0.0);
    root_occurrence.transform = Some(Transform::identity());
    let mut second_bolt = occurrence("f3d:model:occurrence#b:bolt", BOLT, 7.0);
    second_bolt.visible = Some(false);
    ir.model.product_definitions = vec![
        root(&[ROOT_OCCURRENCE, "f3d:model:occurrence#xref-0-0"]),
        component(
            ROOT_COMPONENT,
            &["f3d:model:occurrence#a", "f3d:model:occurrence#b"],
        ),
        component(
            BRACKET,
            &["f3d:model:occurrence#a:bolt", "f3d:model:occurrence#b:bolt"],
        ),
        component(BOLT, &[]),
        ProductDefinition::new(
            definition_id("f3d:xref:document#part.f3d"),
            Storage::External(ExternalDocument::path("part.f3d")),
        ),
    ];
    ir.model.occurrences = vec![
        root_occurrence,
        occurrence(
            "f3d:model:occurrence#xref-0-0",
            "f3d:xref:document#part.f3d",
            3.0,
        ),
        occurrence("f3d:model:occurrence#a", BRACKET, 10.0),
        occurrence("f3d:model:occurrence#b", BRACKET, 20.0),
        occurrence("f3d:model:occurrence#a:bolt", BOLT, 5.0),
        second_bolt,
    ];
    ir
}

#[test]
fn the_root_component_is_the_root_definition() {
    let mut ir = shared_bracket();
    own_once(&mut ir, &BTreeMap::new());
    let root = definition(&ir, ROOT_COMPONENT);
    assert!(
        root.is_root(),
        "§8.1: the root component is the root definition"
    );
    assert_eq!(
        listed(root),
        [
            "f3d:model:occurrence#a",
            "f3d:model:occurrence#b",
            "f3d:model:occurrence#xref-0-0"
        ],
        "the identity occurrence's place in the file root holds the root component's own occurrences"
    );
    assert!(ir
        .model
        .product_definitions
        .iter()
        .all(|definition| definition.id != super::file_root_id()));
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|occurrence| occurrence.id.as_str() != ROOT_OCCURRENCE));
    assert_eq!(
        ir.model
            .product_definitions
            .iter()
            .filter(|definition| definition.is_root())
            .count(),
        1
    );
}

#[test]
fn a_definition_placed_twice_owns_its_children_once() {
    let mut ir = shared_bracket();
    let losses = own_once(&mut ir, &BTreeMap::new());
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(
        listed(definition(&ir, BRACKET)),
        ["f3d:model:occurrence#bolt"],
        "§8.1: a sub-assembly used twice states its children once"
    );
    let bolt = placed(&ir, "f3d:model:occurrence#bolt");
    assert_eq!(bolt.transform, Some(translated(5.0)));
    assert_eq!(bolt.visible, None, "the placements disagree on visibility");
    assert!(placed(&ir, "f3d:model:occurrence#a").overrides.is_empty());
    let second = placed(&ir, "f3d:model:occurrence#b");
    assert_eq!(second.overrides.len(), 1);
    let entry = &second.overrides[0];
    assert_eq!(path(entry), ["f3d:model:occurrence#bolt"]);
    assert_eq!(entry.transform, Some(translated(7.0)));
    assert_eq!(entry.visible, Some(false));
    assert_eq!(ir.model.occurrences.len(), 4);
    let owners = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| {
            ir.model
                .product_definitions
                .iter()
                .filter(|definition| definition.occurrences.contains(&occurrence.id))
                .count()
        })
        .collect::<Vec<_>>();
    assert_eq!(owners, [1; 4], "every occurrence has one owner");
}

#[test]
fn placements_that_agree_state_no_override() {
    let mut ir = shared_bracket();
    let bolt = ir
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.id.as_str() == "f3d:model:occurrence#b:bolt")
        .unwrap();
    bolt.transform = Some(translated(5.0 + 1.0e-7));
    bolt.visible = None;
    own_once(&mut ir, &BTreeMap::new());
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|occurrence| occurrence.overrides.is_empty()));
}

#[test]
fn a_reference_to_one_instance_names_its_occurrence() {
    let mut ir = shared_bracket();
    let connector = |operand| cadmpeg_ir::products::JointConnector {
        operand,
        frame: Transform::identity(),
        detached: false,
    };
    ir.model
        .assembly_joints
        .push(cadmpeg_ir::products::AssemblyJoint::paired(
            cadmpeg_ir::products::JointId::mint("f3d:model:joint#bolt").unwrap(),
            cadmpeg_ir::products::PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                connector(cadmpeg_ir::products::JointOperand::root("root", Vec::new())),
                connector(cadmpeg_ir::products::JointOperand::occurrence(
                    occurrence_id("f3d:model:occurrence#b:bolt"),
                    "bolt",
                    Vec::new(),
                )),
            ],
            None,
        ));
    let losses = own_once(&mut ir, &BTreeMap::new());
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::InstanceReferenceShared.kind()]
    );
    assert_eq!(
        ir.model.assembly_joints[0]
            .connectors()
            .nth(1)
            .unwrap()
            .operand
            .container,
        cadmpeg_ir::products::OperandContainer::Occurrence {
            occurrence: occurrence_id("f3d:model:occurrence#bolt"),
            occurrence_path: Vec::new(),
        }
    );
}

#[test]
fn expanding_restates_the_decoder_instances() {
    let original = shared_bracket();
    let mut ir = original.clone();
    own_once(&mut ir, &BTreeMap::new());
    let expanded = super::expand(
        &cadmpeg_core::decode::ResourceLimits::default(),
        &ir.model.product_definitions,
        &ir.model.occurrences,
        &std::collections::BTreeMap::new(),
    )
    .unwrap();
    let instances = expanded
        .occurrences
        .iter()
        .map(|occurrence| {
            (
                occurrence.id.as_str(),
                occurrence.transform.map(|transform| transform.rows()[0][3]),
                occurrence.visible,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        instances,
        [
            ("f3d:model:occurrence#a", Some(10.0), None),
            ("f3d:model:occurrence#a:bolt", Some(5.0), None),
            ("f3d:model:occurrence#b", Some(20.0), None),
            ("f3d:model:occurrence#b:bolt", Some(7.0), Some(false)),
            ("f3d:model:occurrence#xref-0-0", Some(3.0), None),
        ]
    );
    assert_eq!(
        expanded.occurrence_of[&occurrence_id("f3d:model:occurrence#b:bolt")].as_str(),
        "f3d:model:occurrence#bolt"
    );
    assert_eq!(
        expanded.parents[&occurrence_id("f3d:model:occurrence#b:bolt")].as_str(),
        "f3d:model:occurrence#b"
    );
}

#[test]
fn an_archive_member_placed_twice_is_its_root_component_once() {
    const MEMBER: &str = "f3d:xref/s1/archive:product-definition#0";
    const MEMBER_ROOT: &str = "f3d:xref/s1/model:component#root";
    let mut ir = CadIr::empty();
    let mut member = ProductDefinition::new(
        definition_id(MEMBER),
        Storage::File {
            document: ExternalDocument::path("part.f3d"),
            source: Some(cadmpeg_ir::SourceMeta::classified(
                cadmpeg_core::dialect::DialectLayers::of(
                    cadmpeg_core::dialect::DialectMatch::admitted(
                        cadmpeg_core::dialect::DialectId::parse("f3d:manifest-3-2-0-0").unwrap(),
                    ),
                ),
                BTreeMap::new(),
            )),
        },
    );
    member.label = Some("Part".to_owned());
    member.occurrences = [
        "f3d:xref/s1/model:occurrence#root-c",
        "f3d:xref/s2/model:occurrence#root-c",
    ]
    .iter()
    .map(|id| occurrence_id(id))
    .collect();
    ir.model.product_definitions = vec![
        root(&[
            "f3d:model:occurrence#xref-0-0",
            "f3d:model:occurrence#xref-0-1",
        ]),
        member,
        component(
            MEMBER_ROOT,
            &[
                "f3d:xref/s1/model:occurrence#g",
                "f3d:xref/s2/model:occurrence#g",
            ],
        ),
        component(BOLT, &[]),
    ];
    let identity = |id: &str| {
        let mut occurrence = occurrence(id, MEMBER_ROOT, 0.0);
        occurrence.transform = Some(Transform::identity());
        occurrence
    };
    ir.model.occurrences = vec![
        occurrence("f3d:model:occurrence#xref-0-0", MEMBER, 1.0),
        occurrence("f3d:model:occurrence#xref-0-1", MEMBER, 2.0),
        identity("f3d:xref/s1/model:occurrence#root-c"),
        identity("f3d:xref/s2/model:occurrence#root-c"),
        occurrence("f3d:xref/s1/model:occurrence#g", BOLT, 4.0),
        occurrence("f3d:xref/s2/model:occurrence#g", BOLT, 6.0),
    ];
    let scopes = BTreeMap::from([
        (
            occurrence_id("f3d:model:occurrence#xref-0-0"),
            "f3d:xref/s1".to_owned(),
        ),
        (
            occurrence_id("f3d:model:occurrence#xref-0-1"),
            "f3d:xref/s2".to_owned(),
        ),
    ]);
    own_once(&mut ir, &scopes);
    let member_root = definition(&ir, MEMBER_ROOT);
    assert!(
        matches!(
            &member_root.storage,
            Storage::File {
                source: Some(_),
                ..
            }
        ),
        "§8.1: an archive member's root component is the member file's definition"
    );
    assert_eq!(member_root.label.as_deref(), Some("Part"));
    assert_eq!(listed(member_root), ["f3d:xref/s1/model:occurrence#g"]);
    assert!(ir
        .model
        .product_definitions
        .iter()
        .all(|definition| definition.id.as_str() != MEMBER));
    for id in [
        "f3d:model:occurrence#xref-0-0",
        "f3d:model:occurrence#xref-0-1",
    ] {
        assert_eq!(placed(&ir, id).prototype.as_str(), MEMBER_ROOT);
    }
    assert!(placed(&ir, "f3d:model:occurrence#xref-0-0")
        .overrides
        .is_empty());
    let second = &placed(&ir, "f3d:model:occurrence#xref-0-1").overrides;
    assert_eq!(second.len(), 1);
    assert_eq!(path(&second[0]), ["f3d:xref/s1/model:occurrence#g"]);
    assert_eq!(second[0].transform, Some(translated(6.0)));
    assert_eq!(ir.model.occurrences.len(), 3);
}

#[test]
fn several_root_components_stay_below_the_document_root() {
    let mut ir = shared_bracket();
    ir.model.product_definitions[0]
        .occurrences
        .push(occurrence_id("f3d:model:occurrence#root-other"));
    let mut other = occurrence("f3d:model:occurrence#root-other", BOLT, 0.0);
    other.transform = Some(Transform::identity());
    ir.model.occurrences.push(other);
    own_once(&mut ir, &BTreeMap::new());
    let root = definition(&ir, super::file_root_id().as_str());
    assert!(root.is_root());
    assert_eq!(listed(root)[0], ROOT_OCCURRENCE);
}

#[test]
fn a_child_only_one_placement_holds_is_owned_once_with_a_loss() {
    let mut ir = shared_bracket();
    ir.model
        .occurrences
        .push(occurrence("f3d:model:occurrence#b:nut", BOLT, 9.0));
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id.as_str() == BRACKET)
        .unwrap()
        .occurrences
        .push(occurrence_id("f3d:model:occurrence#b:nut"));
    let losses = own_once(&mut ir, &BTreeMap::new());
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::PlacementChildrenDiffer.kind()]
    );
    assert_eq!(
        listed(definition(&ir, BRACKET)),
        ["f3d:model:occurrence#bolt", "f3d:model:occurrence#nut"]
    );
}
