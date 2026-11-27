// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::parameters::DesignConfiguration as LegacyConfiguration;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::tree::TreeChildren;
use cadmpeg_ir::features::{
    FeatureContent, FeatureDefinition as LegacyDefinition, FeatureId, SketchFeatureBinding,
};
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::topology::{Body, BodyKind};

use super::*;

fn feature(key: &str, ordinal: u64, operation: LegacyOperation) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("nx:feature-history:feature#{key}")).expect("id"),
        ordinal,
        name: Some("SKETCH".to_owned()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: Some("SKETCH".to_owned()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(LegacyDefinition::Operation(operation)),
        native_ref: None,
    }
}

fn sketch() -> LegacyOperation {
    LegacyOperation::Sketch {
        sketch: SketchFeatureBinding::Unresolved,
    }
}

fn equations() -> LegacyOperation {
    LegacyOperation::TreeNode {
        role: FeatureTreeNodeRole::Equations,
        children: TreeChildren::default(),
    }
}

fn body(key: &str) -> Body {
    Body {
        id: BodyId::mint(format!("nx:s0:body#{key}")).expect("id"),
        kind: BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
        unstated: None,
    }
}

fn configuration(ordinal: u32, name: &str, active: bool) -> LegacyConfiguration {
    LegacyConfiguration {
        id: cadmpeg_ir::features::ConfigurationId::mint(format!(
            "nx:arrangements:configuration#{ordinal}"
        ))
        .expect("id"),
        ordinal,
        active,
        source_index: Some(ordinal),
        name: Some(name.to_owned()),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    }
}

/// Projects `ir` and returns whether the root definition was stated.
fn project_ir(ir: &mut CadIr) -> bool {
    let (definition, minted) = root_definition(ir).expect("root definition");
    project(ir, definition).expect("projection");
    minted
}

fn design_findings(ir: &CadIr) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .expect("validation")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn history_is_one_timeline_in_construction_order() {
    let mut ir = CadIr::empty();
    ir.geometry.bodies = vec![body("1"), body("2")];
    let mut suppressed = feature("b", 2, sketch());
    suppressed.suppressed = Some(false);
    ir.model.features = vec![
        suppressed,
        feature("a", 1, sketch()),
        feature("equations", 0, equations()),
    ];
    let minted = project_ir(&mut ir);

    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    assert_eq!(scope.definition.as_str(), PART_ROOT);
    assert_eq!(scope.roots, [ContainerId::mint(TIMELINE).unwrap()]);
    let [Container::Ordered {
        members,
        rollback: None,
        kind: OrderedKind::Timeline,
        ..
    }] = ir.design.containers.as_slice()
    else {
        panic!("one timeline");
    };
    let ids = members
        .iter()
        .map(|member| match member {
            Member::Feature(id) => id.as_str(),
            Member::Container(id) => id.as_str(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "nx:feature-history:feature#a",
            "nx:feature-history:feature#b"
        ]
    );
    let [current] = scope.current.as_slice() else {
        panic!("one current state");
    };
    assert_eq!(
        current.at,
        Position::After(Member::Container(scope.roots[0].clone()))
    );
    assert_eq!(current.bodies, [body("1").id, body("2").id]);
    assert_eq!(ir.design.features.len(), 2);
    assert_eq!(
        ir.design.features[1].suppressed,
        Some(Dimension::Literal(false))
    );
    assert_eq!(ir.design.features[0].suppressed, None);
    assert_eq!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        }
    );
    let [definition] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(definition.is_root());
    assert_eq!(definition.kind, Some(ProductDefinitionKind::Part));
    assert_eq!(definition.bodies, [body("1").id, body("2").id]);
    assert!(minted);
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn a_part_with_no_history_states_no_design_records() {
    let mut ir = CadIr::empty();
    ir.geometry.bodies = vec![body("1"), body("2")];
    ir.model.features = vec![
        feature("equations", 0, equations()),
        feature(
            "initial-bodies",
            1,
            LegacyOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: DistinctMembers::try_from(vec![body("1").id]).unwrap(),
                    native: "nx:segment-body-bindings".to_owned(),
                },
            },
        ),
    ];
    ir.model.configurations = vec![configuration(0, "Default", true)];
    let minted = project_ir(&mut ir);
    assert_eq!(
        json::to_value(&ir.design).unwrap(),
        json::to_value(&CadIr::empty().design).unwrap()
    );
    assert!(minted);
    assert_eq!(
        ir.model.product_definitions[0].bodies,
        [body("1").id, body("2").id]
    );
}

#[test]
fn the_retained_history_input_is_no_member() {
    let mut ir = CadIr::empty();
    ir.geometry.bodies = vec![body("1")];
    ir.model.features = vec![
        feature(
            "initial-bodies",
            0,
            LegacyOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: DistinctMembers::try_from(vec![body("1").id]).unwrap(),
                    native: "nx:segment-body-bindings".to_owned(),
                },
            },
        ),
        feature("a", 1, sketch()),
    ];
    project_ir(&mut ir);
    let ids = ir
        .design
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["nx:feature-history:feature#a"]);
    assert_eq!(ir.design.scopes[0].current[0].bodies, [body("1").id]);
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn an_assembly_root_is_the_scope_definition() {
    const ASSEMBLY_ROOT: &str = "nx:assembly:product-definition#root";
    let mut ir = CadIr::empty();
    let mut root = ProductDefinition::root(
        ProductDefinitionId::mint(ASSEMBLY_ROOT).unwrap(),
        ExternalDocument::default(),
    );
    root.kind = Some(ProductDefinitionKind::Assembly);
    ir.model.product_definitions.push(root);
    ir.model.features = vec![feature("a", 0, sketch())];
    let minted = project_ir(&mut ir);
    assert_eq!(ir.model.product_definitions.len(), 1);
    assert_eq!(ir.design.scopes[0].definition.as_str(), ASSEMBLY_ROOT);
    assert!(!minted);
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn arrangements_are_positional_and_the_active_one_is_in_force() {
    let mut ir = CadIr::empty();
    ir.model.configurations = vec![
        configuration(0, "Default", false),
        configuration(1, "Open", true),
    ];
    ir.model.features = vec![feature("a", 0, sketch())];
    project_ir(&mut ir);
    let names = ir
        .design
        .arrangements
        .iter()
        .map(|arrangement| (arrangement.id.as_str(), arrangement.name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            ("nx:arrangements:arrangement#0", "Default"),
            ("nx:arrangements:arrangement#1", "Open")
        ]
    );
    assert!(ir
        .design
        .arrangements
        .iter()
        .all(
            |arrangement| arrangement.kind == ArrangementKind::Positional
                && arrangement.overrides == Overrides::default()
        ));
    assert_eq!(
        ir.design.scopes[0]
            .active_arrangements
            .iter()
            .map(ArrangementId::as_str)
            .collect::<Vec<_>>(),
        ["nx:arrangements:arrangement#1"]
    );
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

/// Each arrangement restates the configuration it is minted from, except
/// its position among the part's configurations, which no design record
/// orders; a configuration with no name states no arrangement and nothing
/// restates it.
#[test]
fn each_arrangement_restates_its_configuration() {
    let mut ir = CadIr::empty();
    ir.model.configurations = vec![
        configuration(0, "Default", true),
        configuration(1, " ", false),
    ];
    ir.model.features = vec![feature("a", 0, sketch())];
    project_ir(&mut ir);
    let restating = ir
        .model
        .restated()
        .get("nx:arrangements:configuration#0")
        .expect("restated");
    let by = Restater::Record("nx:arrangements:arrangement#0".to_owned());
    assert_eq!(
        restating.cut_by(|restater| restater == &by),
        Some(BTreeSet::from(["ordinal"]))
    );
    assert_eq!(
        ir.model.restated().get("nx:arrangements:configuration#1"),
        None
    );
}

#[test]
fn operations_keep_their_family_or_their_nx_kind() {
    let mut native = feature(
        "native",
        0,
        LegacyOperation::Native {
            kind: "TUBE".into(),
            parameters: BTreeMap::new(),
        },
    );
    native.source_tag = Some("TUBE".to_owned());
    assert_eq!(
        operation(&native).unwrap(),
        FeatureOperation::Native {
            kind: cadmpeg_ir::codec_name!("nx:TUBE"),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }
    );
    let datum = feature(
        "datum",
        0,
        LegacyOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        },
    );
    assert_eq!(
        operation(&datum).unwrap(),
        FeatureOperation::Unresolved {
            family: OperationFamily::Datum
        }
    );
    let base = feature(
        "base",
        0,
        LegacyOperation::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: DistinctMembers::try_from(vec![body("1").id]).unwrap(),
                native: "nx:segment-body-bindings".to_owned(),
            },
        },
    );
    assert_eq!(
        operation(&base).unwrap(),
        FeatureOperation::Stored {
            bodies: Some(vec![body("1").id]),
            tessellations: Vec::new(),
            origin: StoredOrigin::Base {},
        }
    );
    let mut extract = feature(
        "extract",
        0,
        LegacyOperation::Unresolved {
            family: UnresolvedFamily::ExtractFace,
        },
    );
    extract.source_tag = Some("EXTRACT_FACE".to_owned());
    assert_eq!(
        operation(&extract).unwrap(),
        FeatureOperation::Unresolved {
            family: OperationFamily::Extract
        }
    );
    let mut snapshot = feature(
        "snapshot",
        0,
        LegacyOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        },
    );
    snapshot.source_tag = Some("MASTER SNAPSHOT BODY".to_owned());
    assert_eq!(
        operation(&snapshot).unwrap(),
        FeatureOperation::Native {
            kind: cadmpeg_ir::codec_name!("nx:MASTER SNAPSHOT BODY"),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }
    );
}

fn wave_face_link(outputs: Vec<BodyId>) -> LegacyFeature {
    let mut link = feature(
        "link",
        0,
        LegacyOperation::Unresolved {
            family: UnresolvedFamily::LinkedFace,
        },
    );
    link.source_tag = Some("LINKED_FACE".to_owned());
    link.evaluation = FeatureEvaluation::new(
        link.evaluation.definition().clone(),
        DistinctMembers::try_from(outputs).unwrap(),
    );
    link
}

#[test]
fn a_wave_face_link_is_a_linked_stored_copy() {
    let mut ir = CadIr::empty();
    ir.geometry.bodies = vec![body("1")];
    ir.model.features = vec![wave_face_link(vec![body("1").id])];
    project_ir(&mut ir);
    let [feature] = ir.design.features.as_slice() else {
        panic!("one feature");
    };
    let FeatureOperation::Stored {
        bodies: Some(bodies),
        tessellations,
        origin: StoredOrigin::Linked(link),
    } = &feature.definition.operation
    else {
        panic!("a linked stored copy");
    };
    assert_eq!(bodies, &[body("1").id]);
    assert!(tessellations.is_empty());
    assert_eq!(json::to_value(link).unwrap(), cadmpeg_core::json!({}));
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn a_wave_face_link_with_unresolved_bodies_keeps_its_nx_kind() {
    assert_eq!(
        operation(&wave_face_link(Vec::new())).unwrap(),
        FeatureOperation::Native {
            kind: cadmpeg_ir::codec_name!("nx:LINKED_FACE"),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }
    );
}

/// The fields a CADIR write of `ir` names for the model parameter `id`:
/// none where it carries the parameter whole.
fn parameter_charge(ir: &CadIr, id: &str) -> Vec<String> {
    use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};

    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None))
        .expect("the CADIR writer drops what it cannot write");
    let Some(loss) = plan.report().losses.iter().find(|loss| {
        loss.code.local_code() == "arena.omitted"
            && loss.message.contains("model arena `parameters`")
    }) else {
        return Vec::new();
    };
    let (omitted, part) = loss
        .message
        .split_once(" stated in part: ")
        .unwrap_or((loss.message.as_str(), ""));
    assert!(!omitted.contains(id), "{}", loss.message);
    part.split_once(&format!("{id} ("))
        .and_then(|(_, fields)| fields.split_once(')'))
        .map(|(fields, _)| fields.split(", ").map(str::to_owned).collect())
        .unwrap_or_default()
}

/// An NX expression `name` of the expression table `table`, in
/// millimetres, with the formula `formula` and the value the decoder
/// evaluated.
fn expression(
    table: &FeatureId,
    name: &str,
    formula: &str,
    value: f64,
    dependencies: &[&str],
) -> cadmpeg_ir::features::parameters::DesignParameter {
    let id = |name: &str| {
        cadmpeg_ir::features::ParameterId::mint(format!("nx:om-entry-0:parameter#{name}"))
            .expect("id")
    };
    cadmpeg_ir::features::parameters::DesignParameter {
        product_definition: None,
        id: id(name),
        owner: Some(table.clone()),
        ordinal: 0,
        name: name.to_owned(),
        expression: formula.to_owned(),
        display: None,
        value: Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(value).expect("length"),
        )),
        dependencies: dependencies.iter().map(|name| id(name)).collect(),
        properties: BTreeMap::from([(
            cadmpeg_core::text::NonBlankString::new("unit".to_owned()).expect("key"),
            "millimeter".to_owned(),
        )]),
        pmi: None,
        native_ref: Some(format!("nx:om-entry-0:expression#{name}")),
    }
}

/// An NX expression is a user parameter of the part (§5.12) and states no
/// value NX does not store, so its design parameter has no owner and a
/// formula of references has no cache; the design still holds the
/// expression table as its owner and the value its tree evaluates to, and
/// a CADIR write names neither.
#[test]
fn a_write_carries_the_owner_and_value_of_an_nx_expression() {
    use cadmpeg_ir::design::edit::{evaluate, Value};
    use cadmpeg_ir::design::expr::{Expression, Neutral};
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let mut ir = CadIr::empty();
    let table = feature("equations", 0, equations());
    ir.model.parameters = vec![
        expression(&table.id, "p1", "10", 10.0, &[]),
        expression(&table.id, "p2", "15", 15.0, &[]),
        expression(&table.id, "p3", "p1 + p2", 25.0, &["p1", "p2"]),
    ];
    ir.model.features = vec![table, feature("a", 1, sketch())];
    project_ir(&mut ir);

    let design = |name: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| parameter.id.as_str() == format!("nx:om-entry-0:parameter#{name}"))
            .expect("design parameter")
    };
    let sum = design("p3");
    assert_eq!(sum.owner, None);
    assert_eq!(sum.value, None);
    let ParameterDefinition::Expression(Expression::Neutral(Neutral(tree))) = &sum.definition
    else {
        panic!("a neutral tree: {:?}", sum.definition)
    };
    let evaluated = evaluate(tree, &mut |reference| {
        let cadmpeg_ir::design::dimension::RefTarget::Parameter(target) = &reference.target else {
            return None;
        };
        let addend = design(target.as_str().rsplit('#').next()?);
        match addend.value {
            Some(cadmpeg_ir::design::parameter::ParameterValue::Number(value)) => {
                Some(Value::Number {
                    value: value.get(),
                    quantity: cadmpeg_ir::design::units::Quantity::LENGTH,
                })
            }
            _ => None,
        }
    })
    .expect("the tree evaluates");
    assert_eq!(
        evaluated,
        Value::Number {
            value: 25.0,
            quantity: cadmpeg_ir::design::units::Quantity::LENGTH,
        }
    );
    for name in ["p1", "p2", "p3"] {
        let charge = parameter_charge(&ir, &format!("nx:om-entry-0:parameter#{name}"));
        assert!(
            !charge
                .iter()
                .any(|field| field == "owner" || field == "value"),
            "{name}: {charge:?}"
        );
    }
}
