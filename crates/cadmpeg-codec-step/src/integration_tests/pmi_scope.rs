// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiMagnitude, PmiQuantity, PmiTarget,
    PmiValue,
};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn profile(id: &str, targets: Vec<PmiTarget>, modifiers: Vec<String>) -> PmiAnnotation {
    PmiAnnotation {
        id: PmiId::mint(format!("test:scope:pmi#{id}")).expect("fixture identity"),
        name: Some(id.into()),
        visible: None,
        targets,
        definition: PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude: PmiMagnitude::new(
                PmiValue::new(0.5, PmiQuantity::Length).expect("finite fixture length"),
            )
            .expect("nonnegative fixture magnitude"),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers,
            constraints: cadmpeg_ir::pmi::zones::ToleranceConstraints::default(),
        },
    }
}

#[test]
fn all_around_scope_round_trips_components_without_sibling_leakage() {
    for ordinary in [None, Some("free_state")] {
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
        let mut modifiers = vec!["all_around".into()];
        modifiers.extend(ordinary.map(str::to_owned));
        ir.model.pmi = vec![
            profile(
                "around",
                ir.model.faces[..2]
                    .iter()
                    .map(|face| PmiTarget::Face {
                        face: face.id.clone(),
                    })
                    .collect(),
                modifiers,
            ),
            profile(
                "ordinary",
                vec![PmiTarget::Face {
                    face: ir.model.faces[2].id.clone(),
                }],
                Vec::new(),
            ),
        ];
        let mut output = Vec::new();
        let report = write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("write scoped profiles");
        assert!(!report
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
        let text = String::from_utf8(output).expect("STEP text");
        assert!(text.contains("ALL_AROUND_SHAPE_ASPECT("));
        assert!(!text.contains("GEOMETRIC_TOLERANCE_WITH_MODIFIERS(())"));
        let (exchange, _) =
            crate::test_support::with_service_context(text.as_bytes(), crate::parse::parse_inner)
                .expect("parse emitted scope graph");
        let scopes = exchange
            .entities("ALL_AROUND_SHAPE_ASPECT")
            .collect::<Vec<_>>();
        let [(scope, _)] = scopes.as_slice() else {
            panic!("one all-around aspect")
        };
        let mut members = BTreeSet::new();
        for (_, record) in exchange.entities("SHAPE_ASPECT_RELATIONSHIP") {
            let parameters = &record.partials[0].parameters;
            if matches!(parameters.get(2), Some(crate::parse::Value::Reference(id)) if id == scope)
            {
                let Some(crate::parse::Value::Reference(member)) = parameters.get(3) else {
                    panic!("component reference")
                };
                members.insert(*member);
            }
        }
        assert_eq!(members.len(), 2);
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(text), &DecodeOptions::default())
            .expect("decode scoped profiles");
        let faces = |name: &str| {
            let annotation = decoded
                .ir()
                .model
                .pmi
                .iter()
                .find(|annotation| annotation.name.as_deref() == Some(name))
                .expect("profile");
            let PmiDefinition::GeometricTolerance {
                modifiers: actual, ..
            } = &annotation.definition
            else {
                panic!("geometric tolerance")
            };
            assert_eq!(
                actual.iter().any(|modifier| modifier == "all_around"),
                name == "around"
            );
            assert_eq!(
                actual.iter().any(|modifier| modifier == "free_state"),
                name == "around" && ordinary.is_some()
            );
            annotation
                .targets
                .iter()
                .filter_map(|target| match target {
                    PmiTarget::Face { face } => Some(face.clone()),
                    _ => None,
                })
                .collect::<BTreeSet<_>>()
        };
        let around = faces("around");
        let ordinary = faces("ordinary");
        assert_eq!(around.len(), 2);
        assert_eq!(ordinary.len(), 1);
        assert!(around.is_disjoint(&ordinary));
    }
}

#[test]
fn all_around_scope_reports_an_insufficient_component_graph() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.pmi = vec![profile(
        "around",
        vec![PmiTarget::Face {
            face: ir.model.faces[0].id.clone(),
        }],
        vec!["all_around".into()],
    )];
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("report unrepresentable scope");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "pmi.all-around-components-unresolved"));
    assert!(!String::from_utf8(output)
        .expect("STEP text")
        .contains("ALL_AROUND_SHAPE_ASPECT("));
}
