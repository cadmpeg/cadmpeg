// SPDX-License-Identifier: Apache-2.0

#[test]
fn hem_bend_carriers_prove_directional_gap_forms() {
    use crate::history_records::AsmHistoricalCylinder;
    use cadmpeg_ir::math::{Point3, Vector3};

    let cylinder = |radius| AsmHistoricalCylinder {
        surface: 1,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius,
    };
    let flat_inner = cylinder(0.01);
    let flat_outer = cylinder(2.51);
    assert_eq!(
        crate::test_support::with_decode_context(|decode| {
            super::super::super::hem_gap_length_form(
                decode,
                &[flat_inner.clone(), flat_outer.clone()],
                &[1],
                None,
            )
        })
        .unwrap(),
        Some(super::super::super::HemGapLengthForm::Flat)
    );

    let open_inner = cylinder(1.25);
    let open_outer = cylinder(3.75);
    assert_eq!(
        crate::test_support::with_decode_context(|decode| {
            super::super::super::hem_gap_length_form(
                decode,
                &[open_inner.clone(), open_outer.clone()],
                &[1],
                None,
            )
        })
        .unwrap(),
        Some(super::super::super::HemGapLengthForm::Open)
    );

    assert_eq!(
        crate::test_support::with_decode_context(|decode| {
            super::super::super::hem_gap_length_form(
                decode,
                std::slice::from_ref(&flat_inner),
                &[1],
                None,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn hem_carrier_offsets_prove_fold_direction() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
        AsmHistoricalEntityDelta, AsmHistoricalPlane, AsmHistoricalRelation, AsmHistoricalTopology,
        AsmHistoricalTopologyDelta, AsmHistoricalTransition,
    };
    use cadmpeg_ir::features::SheetMetalHemDirection;
    use cadmpeg_ir::math::{Point3, Vector3};

    let previous = AsmHistoricalTopology {
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 11,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 11,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        }],
        ..Default::default()
    };
    let transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: AsmHistoricalTopologyDelta {
            surfaces: AsmHistoricalEntityDelta {
                inserted: vec![12, 13],
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let cylinder = |origin| AsmHistoricalCylinder {
        surface: 12,
        origin,
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius: 1.0,
    };
    let forward_first = cylinder(Point3::new(1.0, 0.0, 0.0));
    let forward_second = cylinder(Point3::new(2.0, 0.0, 0.0));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::super::hem_direction_from_transition(
                decode_ctx,
                7,
                &[forward_first.clone(), forward_second.clone()],
                &transition.topology.surfaces.inserted,
                None,
                &previous,
                &transition,
            )
        })
        .unwrap(),
        Some(SheetMetalHemDirection::Forward)
    );

    let reverse_first = cylinder(Point3::new(-1.0, 0.0, 0.0));
    let reverse_second = cylinder(Point3::new(-2.0, 0.0, 0.0));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::super::hem_direction_from_transition(
                decode_ctx,
                7,
                &[reverse_first.clone(), reverse_second.clone()],
                &transition.topology.surfaces.inserted,
                None,
                &previous,
                &transition,
            )
        })
        .unwrap(),
        Some(SheetMetalHemDirection::Reverse)
    );

    let zero_offset = cylinder(Point3::new(0.0, 0.0, 0.0));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::super::hem_direction_from_transition(
                decode_ctx,
                7,
                &[zero_offset.clone(), forward_second.clone()],
                &transition.topology.surfaces.inserted,
                None,
                &previous,
                &transition,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn hem_geometry_semantics_refuses_inserted_surface_scans() {
    use cadmpeg_core::decode::ResourceDimension;

    for operation in [
        "scan F3D inserted Hem cylinders",
        "find F3D inserted Hem cylinder surface",
        "match F3D inserted Hem cylinder surface",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |decode| {
                let (scope, histories) = hem_geometry_fixture();
                super::super::super::hem_geometry_semantics(decode, &scope, 7, &histories)
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn hem_direction_refuses_incident_loop_and_carrier_scans() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::math::Vector3;
    let (previous, transition, cylinders) = hem_source_fixture();

    for operation in [
        "scan F3D Hem incident loops",
        "find duplicate F3D Hem incident face",
        "find deleted F3D Hem face",
        "scan F3D Hem cylinder direction scale",
        "check F3D Hem cylinder direction offsets",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |decode| {
                super::super::super::hem_direction_from_transition(
                    decode,
                    7,
                    &cylinders,
                    &transition.topology.surfaces.inserted,
                    Some(Vector3::new(0.0, 1.0, 0.0)),
                    &previous,
                    &transition,
                )
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn hem_gap_length_carrier_scan_refuses_work() {
    use crate::history_records::AsmHistoricalCylinder;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::math::{Point3, Vector3};

    let cylinders = [
        AsmHistoricalCylinder {
            surface: 12,
            origin: Point3::new(1.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            radius: 1.25,
        },
        AsmHistoricalCylinder {
            surface: 12,
            origin: Point3::new(2.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            radius: 3.75,
        },
    ];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "scan F3D Hem gap-length carriers",
        0,
        |decode| super::super::super::hem_gap_length_form(decode, &cylinders, &[12], None),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D Hem gap-length carriers"
    ));
}

fn hem_source_fixture() -> (
    crate::history_records::AsmHistoricalTopology,
    crate::history_records::AsmHistoricalTransition,
    [crate::history_records::AsmHistoricalCylinder; 2],
) {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
        AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPlane,
        AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let previous = AsmHistoricalTopology {
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 7,
                next: 6,
                previous: 6,
                radial_next: 6,
            },
            AsmHistoricalCoedge {
                coedge: 8,
                owner_loop: 9,
                edge: 7,
                next: 8,
                previous: 8,
                radial_next: 8,
            },
        ],
        loop_coedges: vec![
            AsmHistoricalRelation {
                owner_ref: 5,
                member_refs: vec![6],
            },
            AsmHistoricalRelation {
                owner_ref: 9,
                member_refs: vec![8],
            },
        ],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5, 9],
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 11,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 11,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        }],
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 7,
            carrier: Some(10),
        }],
        curve_axes: vec![crate::history_records::AsmHistoricalCurveAxis {
            curve: 10,
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        }],
        ..Default::default()
    };
    let transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                deleted: vec![99],
                ..Default::default()
            },
            surfaces: AsmHistoricalEntityDelta {
                inserted: vec![12],
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let cylinders = [
        AsmHistoricalCylinder {
            surface: 12,
            origin: Point3::new(1.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            radius: 1.25,
        },
        AsmHistoricalCylinder {
            surface: 12,
            origin: Point3::new(2.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            radius: 3.75,
        },
    ];
    (previous, transition, cylinders)
}

fn hem_geometry_fixture() -> (
    crate::records::feature::scope::DesignParameterScope,
    Vec<crate::history_records::AsmHistory>,
) {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTopology, AsmHistory, AsmTopologyCache,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#42",
        DesignFeatureKind::Hem,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (previous, transition, cylinders) = hem_source_fixture();
    let current = AsmHistoricalTopology {
        surface_cylinders: cylinders.to_vec(),
        ..Default::default()
    };
    let state = |state_id, node_index, topology, next_ref, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref,
        node_index,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: AsmTopologyCache::Complete(topology),
        transition,
    };
    let histories = vec![AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(2, 2, current, Some(1), Some(transition)),
            state(1, 1, previous, None, None),
        ],
    }];
    (scope, histories)
}
