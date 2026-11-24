// SPDX-License-Identifier: Apache-2.0
//! Configuration-local produced bodies from the partition's producer attributes.

use crate::history::bind::FeatureOutputSources;
use crate::history::configuration::bind_configuration_topology_selections;
use crate::history::project::project_features;
use crate::history::tests::{
    design_configuration, feature, feature_input_lane, with_configuration_id,
};
use crate::records::FeatureHistory;
use cadmpeg_ir::features::parameters::{ConfigurationEvaluation, ConfigurationFeatureState};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::ids::{BodyId, FaceId, LoopId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Face, Region, Sense, Shell};
use std::collections::BTreeMap;

fn body(index: u32) -> BodyId {
    BodyId::mint(format!("sldprt:brep:body#{index}")).expect("identity grammar")
}

/// One body per index, each owning one face `face#<index>`.
fn push_body_with_face(ir: &mut cadmpeg_ir::CadIr, index: u32) {
    let region =
        RegionId::mint(format!("test:model:entity#region{index}")).expect("identity grammar");
    let shell = ShellId::mint(format!("test:model:entity#shell{index}")).expect("identity grammar");
    ir.geometry.regions.push(Region::new(
        region.clone(),
        body(index),
        vec![shell.clone()],
    ));
    ir.geometry.shells.push(Shell {
        id: shell.clone(),
        region,
        members: Vec::new(),
        native_ref: None,
        unstated: None,
    });
    ir.geometry.faces.push(Face {
        id: FaceId::mint(format!("test:model:entity#face{index}")).expect("identity grammar"),
        shell,
        surface: SurfaceId::mint("test:model:entity#surface").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(
            "test:model:entity#loop",
        )
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
}

/// A document with features `producer` (source 700) and `modifier` (source
/// 900), bodies 1 and 2, and one configuration per entry of `memberships`,
/// each with its own lane and empty active states.
fn document(memberships: &[Option<Vec<BodyId>>]) -> (cadmpeg_ir::CadIr, Vec<FeatureHistory>) {
    let histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("producer", Some("700"), 0),
            feature("modifier", Some("900"), 1),
        ],
    }];
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features = project_features(&histories).unwrap();
    push_body_with_face(&mut ir, 1);
    push_body_with_face(&mut ir, 2);
    for (index, membership) in memberships.iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        let mut configuration = with_configuration_id(
            design_configuration(&format!("config{index}"), index, Some(index), None),
            index + 1,
        );
        configuration.bodies = membership
            .clone()
            .map(|bodies| DistinctMembers::try_from(bodies).unwrap());
        for feature in &ir.model.features {
            configuration.feature_states.insert(
                feature.id.clone(),
                ConfigurationFeatureState {
                    evaluation: ConfigurationEvaluation::Active {
                        outputs: DistinctMembers::default(),
                    },
                    dependencies: DistinctMembers::default(),
                    definition: feature.evaluation.definition().clone(),
                },
            );
        }
        ir.model.configurations.push(configuration);
    }
    (ir, histories)
}

fn state_outputs(ir: &cadmpeg_ir::CadIr, configuration: usize) -> Vec<Vec<BodyId>> {
    ir.model
        .features
        .iter()
        .map(|feature| {
            match &ir.model.configurations[configuration].feature_states[&feature.id].evaluation {
                ConfigurationEvaluation::Active { outputs } => outputs.to_vec(),
                ConfigurationEvaluation::Suppressed {} => Vec::new(),
            }
        })
        .collect()
}

fn bind(ir: &mut cadmpeg_ir::CadIr, histories: &[FeatureHistory], configurations: usize) {
    let lanes = (1..=configurations)
        .map(|id| feature_input_lane(&format!("lane{id}"), Some(&id.to_string())))
        .collect::<Vec<_>>();
    let face_producers = vec![
        ("test:model:entity#face1".to_owned(), 700),
        ("test:model:entity#face2".to_owned(), 700),
    ];
    let body_modifiers = vec![
        ("sldprt:brep:body#1".to_owned(), 900),
        ("sldprt:brep:body#2".to_owned(), 900),
    ];
    bind_configuration_topology_selections(
        ir,
        histories,
        &lanes,
        &[],
        FeatureOutputSources {
            face_producers: &face_producers,
            body_modifiers: &body_modifiers,
        },
    )
    .unwrap();
}

#[test]
fn configuration_states_name_only_the_bodies_of_their_own_partition() {
    let (mut ir, histories) = document(&[Some(vec![body(1)]), Some(vec![body(2)])]);

    bind(&mut ir, &histories, 2);

    assert_eq!(state_outputs(&ir, 0), [vec![body(1)], vec![body(1)]]);
    assert_eq!(state_outputs(&ir, 1), [vec![body(2)], vec![body(2)]]);
}

#[test]
fn unresolved_configuration_body_membership_gains_no_produced_body() {
    let (mut ir, histories) = document(&[None]);

    bind(&mut ir, &histories, 1);

    assert_eq!(state_outputs(&ir, 0), [Vec::<BodyId>::new(), Vec::new()]);
}
