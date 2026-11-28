// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::selection::{OccurrencePath, PathStep, StepRef};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, NestedOverride, Occurrence, ProductDefinition, Storage,
};
use cadmpeg_ir::CadIr;

use crate::test_support::builders::{assert_measured, cuboid, service_context};
use crate::{MeasureInput, MeasureOptions, Measurer, Selection};

fn definition(key: &str) -> ProductDefinitionId {
    ProductDefinitionId::mint(format!("test:model:product-definition#{key}")).unwrap()
}

fn occurrence(key: &str) -> OccurrenceId {
    OccurrenceId::mint(format!("test:model:occurrence#{key}")).unwrap()
}

fn instance<'a>(ir: &'a mut CadIr, key: &str) -> &'a mut Occurrence {
    ir.model
        .occurrences
        .iter_mut()
        .find(|entry| entry.id == occurrence(key))
        .unwrap()
}

fn assembly() -> CadIr {
    let mut ir = cuboid([0.0; 3], [2.0, 3.0, 4.0]);
    ir.geometry.bodies[0].visible = Some(true);
    let mut root = ProductDefinition::root(definition("root"), ExternalDocument::default());
    root.occurrences.push(occurrence("outer"));
    let mut group = ProductDefinition::new(definition("group"), Storage::Embedded);
    group.occurrences.push(occurrence("inner"));
    let mut part = ProductDefinition::new(definition("part"), Storage::Embedded);
    part.bodies.push(ir.geometry.bodies[0].id.clone());
    ir.model.product_definitions = vec![root, group, part];
    ir.model.occurrences = vec![
        Occurrence::new(occurrence("outer"), definition("group"), None),
        Occurrence::new(occurrence("inner"), definition("part"), None),
    ];
    ir.finalize();
    ir
}

fn shown(ir: &CadIr, selected: Option<&str>) -> Option<bool> {
    let ctx = service_context();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let selection = Selection {
        occurrence: selected.map(occurrence),
        ..Selection::default()
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &selection)
        .unwrap();
    assert_eq!(report.bodies().len(), 1);
    assert_measured(report.bodies()[0].volume().as_ref().unwrap(), 24.0, 64.0);
    *report.bodies()[0].visible()
}

#[test]
fn body_and_every_ancestor_are_visibility_gates() {
    let mut ir = assembly();
    assert_eq!(shown(&ir, None), Some(true));
    for hidden in 0..ir.model.occurrences.len() {
        ir.model.occurrences[hidden].visible = Some(false);
        for selection in [None, Some("outer"), Some("inner")] {
            assert_eq!(shown(&ir, selection), Some(false));
        }
        assert_eq!(ir.geometry.bodies[0].visible, Some(true));
        ir.model.occurrences[hidden].visible = Some(true);
    }
    ir.geometry.bodies[0].visible = Some(false);
    assert_eq!(shown(&ir, None), Some(false));
    assert_eq!(shown(&ir, Some("inner")), Some(false));
}

#[test]
fn a_shown_instance_keeps_a_shared_body_visible_in_model_space() {
    let mut ir = assembly();
    instance(&mut ir, "outer").visible = Some(false);
    ir.model
        .product_definitions
        .iter_mut()
        .find(|entry| entry.id == definition("root"))
        .unwrap()
        .occurrences
        .push(occurrence("other"));
    ir.model.occurrences.push(Occurrence::new(
        occurrence("other"),
        definition("part"),
        None,
    ));
    ir.finalize();
    assert_eq!(shown(&ir, None), Some(true));
    assert_eq!(shown(&ir, Some("inner")), Some(false));
    assert_eq!(shown(&ir, Some("other")), Some(true));
    instance(&mut ir, "other").visible = Some(false);
    assert_eq!(shown(&ir, None), Some(false));
}

#[test]
fn nested_visibility_override_selects_one_instance() {
    let mut ir = assembly();
    instance(&mut ir, "outer").overrides.push(NestedOverride {
        path: OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(occurrence("inner")),
            configuration: None,
        }]),
        transform: None,
        suppressed: None,
        visible: Some(false),
        representation: None,
    });
    assert_eq!(shown(&ir, None), Some(false));
    assert_eq!(shown(&ir, Some("inner")), Some(false));
    instance(&mut ir, "inner").visible = Some(false);
    instance(&mut ir, "outer").overrides[0].visible = Some(true);
    assert_eq!(shown(&ir, Some("inner")), Some(true));
    instance(&mut ir, "outer").visible = Some(false);
    assert_eq!(shown(&ir, Some("inner")), Some(false));
}

#[test]
fn unplaced_body_defaults_to_shown_and_keeps_its_own_hidden_gate() {
    let mut ir = cuboid([0.0; 3], [2.0, 3.0, 4.0]);
    assert_eq!(shown(&ir, None), Some(true));
    ir.geometry.bodies[0].visible = Some(false);
    assert_eq!(shown(&ir, None), Some(false));
}

#[test]
fn unresolved_configuration_keeps_an_explicit_hidden_body() {
    let mut ir = assembly();
    ir.design
        .scopes
        .push(cadmpeg_ir::design::scope::DesignScope {
            definition: definition("root"),
            roots: Vec::new(),
            active_configuration: Some(
                cadmpeg_ir::features::ConfigurationId::mint("test:model:configuration#missing")
                    .unwrap(),
            ),
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current: Vec::new(),
            unproduced: BTreeSet::new(),
        });
    assert_eq!(shown(&ir, None), None);
    ir.geometry.bodies[0].visible = Some(false);
    assert_eq!(shown(&ir, None), Some(false));
}
