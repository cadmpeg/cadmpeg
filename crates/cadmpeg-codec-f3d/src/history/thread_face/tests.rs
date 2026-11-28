// SPDX-License-Identifier: Apache-2.0
use super::{bind, Inputs};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalTransition, AsmHistory,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::evaluation::{FeatureEvaluation, FeatureInputTopology};
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::topology::Face;

const MEMBER: &str = "Active/BREP.test.smbh";
const HISTORY: &str = "f3d:Active/BREP.test.smbh:asm-history#0";

struct Fixture {
    features: Vec<Feature>,
    scopes: Vec<DesignParameterScope>,
    topologies: Vec<FeatureInputTopology>,
    histories: Vec<AsmHistory>,
    faces: Vec<Face>,
}

fn state(
    state_id: i64,
    previous_ref: Option<i64>,
    next_ref: Option<i64>,
    node_index: i64,
    previous_state_id: Option<i64>,
) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("{HISTORY}:state#{state_id}"),
        parent: HISTORY.into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref,
        next_ref,
        node_index,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        transition: Some(AsmHistoricalTransition {
            geometry_unchanged: false,
            previous_state_id,
            records: Default::default(),
            topology: Default::default(),
            record_revisions: Vec::new(),
            rows: Default::default(),
        }),
    }
}

impl Fixture {
    fn new() -> Self {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#7",
            DesignFeatureKind::Thread,
            7,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(2);
                draft.previous_history_state_id = Some(1);
                draft.layout_fixture_tail();
            })
            .unwrap();
        let id = crate::ids::neutral_feature_id(&scope);
        let prefix = crate::ids::feature_input_prefix(&id, 1);
        let historical = crate::ids::history_input_face_id(&prefix, 10);
        let selection = FaceSelection::historical(
            crate::ids::history_input_state_id(&prefix),
            vec![historical.clone()],
            "f3d:Design/BulkStream.dat:group#8".into(),
        )
        .unwrap();
        let feature = Feature {
            product_definition: None,
            id: id.clone(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: Some("Thread".into()),
            source_text: None,
            source_content: Default::default(),
            native_ref: Some(scope.id.clone()),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::CosmeticThread {
                    face: selection,
                    standard: None,
                    designation: None,
                    diameter: None,
                    pitch: None,
                    modeled: None,
                    extent: None,
                },
            )),
        };
        let input = state(1, Some(0), None, 1, None);
        let mut result = state(2, None, Some(1), 0, Some(1));
        let transition = result.transition.as_mut().unwrap();
        transition.topology.faces.updated = vec![10];
        transition.topology.surfaces.inserted = vec![77];
        transition.topology.surfaces.deleted = vec![88];
        let topology = FeatureInputTopology {
            face_owners: Vec::new(),
            face_geometry: Vec::new(),
            body_writers: Vec::new(),
            id: crate::ids::history_input_state_id(&prefix),
            input_of: id,
            bodies: Default::default(),
            faces: vec![historical].try_into().unwrap(),
            edges: Default::default(),
            vertices: Default::default(),
            edge_geometry: Vec::new(),
            native_ref: Some(input.id.clone()),
        };
        let face = Face {
            id: "f3d:brep:entity#10".try_into().unwrap(),
            shell: "f3d:brep:entity#1".try_into().unwrap(),
            surface: "f3d:brep:entity#77".try_into().unwrap(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        };
        Self {
            features: vec![feature],
            scopes: vec![scope],
            topologies: vec![topology],
            histories: vec![AsmHistory::new(
                HISTORY.into(),
                0,
                None,
                false,
                vec![result, input],
            )],
            faces: vec![face],
        }
    }
    fn selection(&self) -> &FaceSelection {
        let FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face, modeled, ..
        }) = self.features[0].evaluation.definition()
        else {
            panic!("Thread definition")
        };
        assert_eq!(*modeled, None);
        face
    }
    fn run(&mut self, primary_brep: Option<&str>) -> Vec<cadmpeg_ir::report::loss::LossNote> {
        bind(
            &mut self.features,
            &Inputs {
                scopes: &self.scopes,
                topologies: &self.topologies,
                histories: &self.histories,
                faces: &self.faces,
                primary_brep,
            },
        )
    }
    fn later(&mut self) {
        let states = self.histories[0].states_mut();
        states[0].previous_ref = Some(2);
        states.insert(0, state(3, None, Some(0), 2, Some(2)));
    }
    fn refuses(&mut self, primary_brep: Option<&str>) {
        let before = self.selection().clone();
        let losses = self.run(primary_brep);
        assert_eq!(self.selection(), &before);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::ThreadLiveFaceUnproven.kind()
        );
        assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Info);
    }
}

#[test]
fn thread_preserved_connectivity_resolves_live_despite_replaced_geometry() {
    let mut fixture = Fixture::new();
    assert!(fixture.run(Some(MEMBER)).is_empty());
    assert_eq!(
        fixture.selection(),
        &FaceSelection::Resolved {
            faces: vec!["f3d:brep:entity#10".try_into().unwrap()],
            native: "f3d:Design/BulkStream.dat:group#8".into(),
        }
    );
}

#[test]
fn thread_live_face_requires_its_exact_brep_member() {
    let mut fixture = Fixture::new();
    fixture.refuses(Some("Active/BREP.other.smbh"));
    fixture.faces[0].id = "f3d:brep/test.smbh/brep:entity#10".try_into().unwrap();
    assert!(fixture.run(None).is_empty());
    assert!(
        matches!(fixture.selection(), FaceSelection::Resolved { faces, .. } if faces[0] == fixture.faces[0].id)
    );
    let mut fixture = Fixture::new();
    fixture.faces[0].id = "f3d:brep/other.smbh/brep:entity#10".try_into().unwrap();
    fixture.refuses(Some(MEMBER));
}

#[test]
fn thread_missing_live_slot_keeps_historical_selection_and_loss() {
    let mut fixture = Fixture::new();
    fixture.faces.clear();
    fixture.refuses(Some(MEMBER));
}

impl Fixture {
    /// A later join that extends the threaded face: the face is updated, an
    /// end face is replaced, and the threaded face keeps its surface.
    fn later_join(&mut self) -> &mut AsmHistoricalTransition {
        self.later();
        let transition = self.histories[0].states_mut()[0]
            .transition
            .as_mut()
            .unwrap();
        transition.topology.bodies.updated = vec![1];
        transition.topology.faces.updated = vec![10];
        transition.topology.faces.inserted = vec![11];
        transition.topology.faces.deleted = vec![12];
        transition.topology.loops.inserted = vec![13];
        transition.topology.loops.deleted = vec![14];
        transition.topology.surfaces.inserted = vec![15];
        transition.topology.surfaces.deleted = vec![16];
        transition.rows.face_loops.set = vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![13, 17],
        }];
        transition.rows.face_loops.removed = vec![12];
        transition.rows.face_surfaces.set = vec![AsmHistoricalCarrierBinding {
            entity: 11,
            carrier: 15,
        }];
        transition.rows.face_surfaces.removed = vec![12];
        transition
    }
}

#[test]
fn thread_later_face_extension_with_kept_surface_resolves_live() {
    let mut fixture = Fixture::new();
    fixture.later_join();
    assert!(fixture.run(Some(MEMBER)).is_empty());
    assert_eq!(
        fixture.selection(),
        &FaceSelection::Resolved {
            faces: vec!["f3d:brep:entity#10".try_into().unwrap()],
            native: "f3d:Design/BulkStream.dat:group#8".into(),
        }
    );
}

#[test]
fn thread_later_face_rewrite_keeps_historical_selection_and_loss() {
    let mut fixture = Fixture::new();
    fixture.later_join().rows.face_surfaces.set = vec![AsmHistoricalCarrierBinding {
        entity: 10,
        carrier: 15,
    }];
    fixture.refuses(Some(MEMBER));
    let mut fixture = Fixture::new();
    fixture.later_join().topology.faces.deleted = vec![10, 12];
    fixture.refuses(Some(MEMBER));
    let mut fixture = Fixture::new();
    fixture.later_join().rows.surface_cylinders.removed = vec![77];
    fixture.refuses(Some(MEMBER));
    let mut fixture = Fixture::new();
    fixture.later_join();
    fixture.faces[0].surface = "f3d:brep/other.smbh/brep:entity#77".try_into().unwrap();
    fixture.refuses(Some(MEMBER));
}

#[test]
fn thread_later_carrier_rewrite_keeps_historical_selection_and_loss() {
    let mut fixture = Fixture::new();
    fixture.later();
    fixture.histories[0].states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .topology
        .surfaces
        .updated = vec![77];
    fixture.refuses(Some(MEMBER));
}

#[test]
fn thread_later_body_attribute_revision_preserves_live_selection() {
    let mut fixture = Fixture::new();
    fixture.later();
    fixture.histories[0].states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .topology
        .bodies
        .updated = vec![1];
    assert!(fixture.run(Some(MEMBER)).is_empty());
    assert!(matches!(
        fixture.selection(),
        FaceSelection::Resolved { .. }
    ));
}

#[test]
fn thread_topology_membership_or_connectivity_change_keeps_history() {
    let mut fixture = Fixture::new();
    fixture.histories[0].states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .topology
        .faces
        .inserted = vec![11];
    fixture.refuses(Some(MEMBER));
    let mut fixture = Fixture::new();
    fixture.histories[0].states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .rows
        .face_loops
        .removed = vec![10];
    fixture.refuses(Some(MEMBER));
}

#[test]
fn thread_ambiguous_sources_keep_history() {
    for duplicate in 0..5 {
        let mut fixture = Fixture::new();
        match duplicate {
            0 => fixture.scopes.push(fixture.scopes[0].clone()),
            1 => fixture.topologies.push(fixture.topologies[0].clone()),
            2 => fixture.histories.push(fixture.histories[0].clone()),
            3 => fixture.faces.push(fixture.faces[0].clone()),
            4 => fixture.histories[0].states_mut()[1].previous_ref = None,
            _ => unreachable!(),
        }
        fixture.refuses(Some(MEMBER));
    }
}

#[test]
fn thread_broken_transition_chains_keep_history() {
    let mut fixture = Fixture::new();
    fixture.histories[0].states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = None;
    fixture.refuses(Some(MEMBER));
    let mut fixture = Fixture::new();
    fixture.later();
    fixture.histories[0].states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(1);
    fixture.refuses(Some(MEMBER));
}

#[test]
fn thread_foreign_historical_slot_keeps_history() {
    let mut fixture = Fixture::new();
    let foreign = crate::ids::history_input_face_id(
        &crate::ids::feature_input_prefix(&fixture.features[0].id, 4),
        10,
    );
    fixture.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) =
            definition
        else {
            panic!("Thread definition")
        };
        *face = FaceSelection::historical(
            fixture.topologies[0].id.clone(),
            vec![foreign.clone()],
            "f3d:Design/BulkStream.dat:group#8".into(),
        )
        .unwrap();
    });
    fixture.topologies[0].faces = vec![foreign].try_into().unwrap();
    fixture.refuses(Some(MEMBER));
}

#[test]
fn thread_keeps_input_cylinder_before_live_face_binding() {
    use crate::history_records::{AsmHistoricalFaceSample, AsmHistoricalTopology};
    use cadmpeg_ir::features::evaluation::{HistoricalFaceGeometry, HistoricalFaceSurface};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut fixture = Fixture::new();
    let cylinder = HistoricalFaceSurface::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
        )
        .unwrap(),
    );
    let history = &fixture.histories[0];
    let states = history.states().to_vec();
    fixture.histories = vec![crate::test_support::history::history(
        HISTORY.into(),
        0,
        None,
        false,
        states
            .into_iter()
            .map(|state| {
                let topology = Some(AsmHistoricalTopology {
                    bodies: vec![1],
                    faces: vec![10],
                    face_samples: vec![AsmHistoricalFaceSample {
                        face: 10,
                        surface: cylinder.clone(),
                        reversed: false,
                    }],
                    ..Default::default()
                });
                (state, topology, Vec::new())
            })
            .collect(),
    )];
    fixture.topologies = crate::history::input_topologies::project_feature_input_topologies(
        &fixture.features,
        &fixture.scopes,
        &mut fixture.histories,
        &[],
        &Default::default(),
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::input_topologies::retain_selected_face_geometry(
        &ctx,
        &fixture.features,
        &mut fixture.topologies,
    )
    .unwrap();
    assert!(fixture.run(Some(MEMBER)).is_empty());
    crate::history::discard_projection_caches(&mut fixture.histories);
    assert!(matches!(
        fixture.selection(),
        FaceSelection::Resolved { .. }
    ));
    assert_eq!(fixture.topologies.len(), 1);
    let input = &fixture.topologies[0];
    assert_eq!(input.faces.as_slice().len(), 1);
    assert_eq!(
        input.face_geometry,
        vec![HistoricalFaceGeometry {
            face: input.faces.as_slice()[0].clone(),
            surface: cylinder,
            reversed: false,
        }]
    );
}
