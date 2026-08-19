{
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        body: Vec::new(),
        offset: 40,
    };
    let arc = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [2, 3],
        center_id: Some(4),
        arc_orientation: Some(1),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 13,
        body: Vec::new(),
        offset: 41,
    };
    let point = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Point,
        directions: [None; 3],
        point_ids: [4, 4],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 14,
        body: Vec::new(),
        offset: 42,
    };
    let other_line = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [5, 6],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 15,
        body: Vec::new(),
        offset: 43,
    };
    let other_arc = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [5, 6],
        center_id: Some(7),
        arc_orientation: Some(1),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 16,
        body: Vec::new(),
        offset: 44,
    };
    let relations = crate::feature::FeatureRelationTable {
        declared_count: 3,
        entity_ref: None,
        rows: vec![crate::feature::FeatureRelation {
            relation_id: 8,
            used: 1,
            operands: vec![12, 4],
            operand_vectors: None,
            sign: 1,
            dimension_id: 0,
            relation_type: 99,
            body: Vec::new(),
            offset: 80,
        }],
        skamps: vec![
            crate::feature::FeatureSkamp {
                id: 3,
                kind: 1,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                }],
                offset: 50,
            },
            crate::feature::FeatureSkamp {
                id: 4,
                kind: 2,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                }],
                offset: 60,
            },
            crate::feature::FeatureSkamp {
                id: 5,
                kind: 7,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 4,
                }],
                offset: 70,
            },
            crate::feature::FeatureSkamp {
                id: 6,
                kind: 1,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                }],
                offset: 71,
            },
            crate::feature::FeatureSkamp {
                id: 7,
                kind: 0,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 2,
                    },
                ],
                offset: 72,
            },
            crate::feature::FeatureSkamp {
                id: 8,
                kind: 4,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 3,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 2,
                    },
                ],
                offset: 73,
            },
            crate::feature::FeatureSkamp {
                id: 9,
                kind: 14,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 2,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 3,
                    },
                ],
                offset: 74,
            },
            crate::feature::FeatureSkamp {
                id: 10,
                kind: 14,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 4,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 4,
                    },
                ],
                offset: 75,
            },
            crate::feature::FeatureSkamp {
                id: 11,
                kind: 3,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 4,
                    },
                ],
                offset: 76,
            },
            crate::feature::FeatureSkamp {
                id: 12,
                kind: 9,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                ],
                offset: 77,
            },
            crate::feature::FeatureSkamp {
                id: 13,
                kind: 5,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 0,
                    },
                ],
                offset: 78,
            },
            crate::feature::FeatureSkamp {
                id: 14,
                kind: 7,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 0,
                    },
                ],
                offset: 79,
            },
            crate::feature::FeatureSkamp {
                id: 15,
                kind: 8,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 0,
                    },
                ],
                offset: 80,
            },
            crate::feature::FeatureSkamp {
                id: 16,
                kind: 6,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 16,
                        sense: 0,
                    },
                ],
                offset: 81,
            },
            crate::feature::FeatureSkamp {
                id: 17,
                kind: 17,
                flags: 2,
                status: 1,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 2,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 2,
                    },
                ],
                offset: 82,
            },
        ],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 15,
            entity_ref: 1,
            offset: 46,
        }),
        triples: Vec::new(),
        triples_header: None,
        offset: 45,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(0.0),
                    v: Some(2.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 5,
                    u: Some(3.0),
                    v: Some(2.0),
                },
            ],
            offset: 89,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 5,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![segment, arc, point, other_line, other_arc],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 30,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureDimension {
                dimension_type: 2,
                value: Some(3.0),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 42,
                references: None,
                offset: 75,
            }],
            offset: 74,
        }),
        relations: Some(relations),
        saved_section: None,
        offset: 0,
    };
    let mut point_row_symmetry = definition.clone();
    point_row_symmetry.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![
            crate::feature::FeatureSectionPoint {
                point_id: 4,
                u: Some(1.0),
                v: Some(1.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 6,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 7,
                u: None,
                v: None,
            },
        ],
        offset: 90,
    });
    let segments = point_row_symmetry.segments.as_mut().expect("segments");
    segments.point_rows = vec![
        crate::feature::FeaturePointSegment {
            point_id: 4,
            external_id: 18,
            offset: 91,
        },
        crate::feature::FeaturePointSegment {
            point_id: 6,
            external_id: 19,
            offset: 92,
        },
        crate::feature::FeaturePointSegment {
            point_id: 7,
            external_id: 20,
            offset: 93,
        },
    ];
    segments.declared_count = 8;
    let relations = point_row_symmetry.relations.as_mut().expect("relations");
    relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 94,
        kind: 14,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 18,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 19,
                sense: 4,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 20,
                sense: 4,
            },
        ],
        offset: 94,
    }];
    relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        resolved_section_points(&point_row_symmetry).get(&7),
        Some(&[2.0, 2.0])
    );
    let mut circle_center_symmetry = point_row_symmetry.clone();
    circle_center_symmetry
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 8,
            u: Some(0.0),
            v: Some(0.0),
        });
    let segments = circle_center_symmetry.segments.as_mut().expect("segments");
    segments.circle_rows = vec![crate::feature::FeatureCircleSegment {
        center_id: 8,
        radius_ref: 101,
        external_id: 21,
        offset: 95,
    }];
    segments.declared_count = 9;
    circle_center_symmetry
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1]
        .entity_id = 21;
    assert_eq!(
        resolved_section_points(&circle_center_symmetry).get(&7),
        Some(&[2.0, 2.0])
    );
    let mut projected_copy = definition.clone();
    projected_copy.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::FeatureTrimEntity {
            external_id: 13,
            mode: Some(0),
            vertices: [2, 3],
            center_vertex: None,
            kind: crate::feature::TrimEntityKind::Line,
            offset: 84,
        }],
        solved_external_ids: vec![13],
        offset: 83,
    });
    projected_copy.relations.as_mut().expect("relations").skamps =
        vec![crate::feature::FeatureSkamp {
            id: 19,
            kind: 37,
            flags: 98_304,
            status: 34,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
            ],
            offset: 85,
        }];
    synchronize_skamp_count(&mut projected_copy);
    assert_eq!(
        section_skamp_constraints(&projected_copy, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::ProjectedCopy {
            source: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            result: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
        }
    );
    let mismatched_projected_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            SketchGeometry::Native {
                native_kind: "line".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            SketchGeometry::Native {
                native_kind: "arc".to_string(),
            },
        ),
    ]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &projected_copy,
            &SketchId("creo:model:sketch#917".into()),
            Some(&mismatched_projected_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    projected_copy.relations.as_mut().expect("relations").skamps[0]
        .items
        .swap(0, 1);
    assert!(matches!(
        section_skamp_constraints(&projected_copy, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    for (kind, angle) in [
        (10, std::f64::consts::FRAC_PI_2),
        (11, std::f64::consts::PI),
    ] {
        let mut fixed_arc = definition.clone();
        let relations = fixed_arc.relations.as_mut().expect("relations");
        relations.skamps = vec![crate::feature::FeatureSkamp {
            id: 18,
            kind,
            flags: 0,
            status: 35,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            }],
            offset: 83,
        }];
        synchronize_skamp_count(&mut fixed_arc);
        assert_eq!(
            section_skamp_constraints(&fixed_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::ArcAngle {
                entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                angle: Angle(angle),
            }
        );
        fixed_arc.relations.as_mut().expect("relations").skamps[0].status = 34;
        assert!(matches!(
            section_skamp_constraints(&fixed_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::ArcAngle { .. }
        ));
        fixed_arc.relations.as_mut().expect("relations").skamps[0].items[0] =
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            };
        assert!(matches!(
            section_skamp_constraints(&fixed_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native { .. }
        ));
        fixed_arc.relations.as_mut().expect("relations").skamps[0].items[0] =
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 2,
            };
        assert!(matches!(
            section_skamp_constraints(&fixed_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native { .. }
        ));
    }
    for kind in [12, 13] {
        let mut oriented_arc = definition.clone();
        let relations = oriented_arc.relations.as_mut().expect("relations");
        relations.skamps = vec![crate::feature::FeatureSkamp {
            id: 18,
            kind,
            flags: 0,
            status: 35,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            }],
            offset: 83,
        }];
        synchronize_skamp_count(&mut oriented_arc);
        let expected_entity = SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string());
        let expected = if kind == 12 {
            SketchConstraintDefinition::HorizontalLoci {
                first: SketchLocus::Start(expected_entity.clone()),
                second: SketchLocus::End(expected_entity),
            }
        } else {
            SketchConstraintDefinition::VerticalLoci {
                first: SketchLocus::Start(expected_entity.clone()),
                second: SketchLocus::End(expected_entity),
            }
        };
        assert_eq!(
            section_skamp_constraints(&oriented_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            expected
        );
        oriented_arc.relations.as_mut().expect("relations").skamps[0].status = 34;
        assert!(matches!(
            section_skamp_constraints(&oriented_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::HorizontalLoci { .. }
                | SketchConstraintDefinition::VerticalLoci { .. }
        ));
        oriented_arc.relations.as_mut().expect("relations").skamps[0].items[0] =
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            };
        assert!(matches!(
            section_skamp_constraints(&oriented_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native { .. }
        ));
        oriented_arc.relations.as_mut().expect("relations").skamps[0].items[0] =
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 2,
            };
        assert!(matches!(
            section_skamp_constraints(&oriented_arc, &SketchId("creo:model:sketch#917".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native { .. }
        ));
    }
    let mut type_33_bounded_curve = definition.clone();
    type_33_bounded_curve
        .segments
        .as_mut()
        .expect("segments")
        .bounded_curve_rows = vec![crate::feature::FeatureBoundedCurveSegment {
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: None,
        arc_orientation: Some(1),
        vertical_horizontal: Some(0),
        radius_ref: None,
        radius2_ref: None,
        external_id: 18,
        offset: 86,
    }];
    type_33_bounded_curve
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 6;
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 20,
        kind: 33,
        flags: 34,
        status: 35,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 18,
            sense: 10,
        }],
        offset: 87,
    }];
    synchronize_skamp_count(&mut type_33_bounded_curve);
    let type_33_constraints = section_skamp_constraints(
        &type_33_bounded_curve,
        &SketchId("creo:model:sketch#917".into()),
    );
    let SketchConstraintDefinition::Native {
        native_kind,
        native_state,
        native_flags,
        entities,
        operands,
        ..
    } = &type_33_constraints[0].0.definition
    else {
        panic!("native type-33 incidence");
    };
    assert_eq!(native_kind, "creo:skamp:33");
    assert_eq!(*native_state, Some(35));
    assert_eq!(*native_flags, Some(34));
    assert_eq!(
        entities,
        &[SketchEntityId(
            "creo:featdefs:sketch_entity#917:18".to_string()
        )]
    );
    assert_eq!(operands.len(), 1);
    assert_eq!(operands[0].native_role, Some(10));
    assert_eq!(type_33_constraints[0].0.active, Some(true));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 34;
    let inactive_type_33 = section_skamp_constraints(
        &type_33_bounded_curve,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(matches!(
        inactive_type_33[0].0.definition,
        SketchConstraintDefinition::Native {
            native_state: Some(34),
            native_flags: Some(34),
            ..
        }
    ));
    assert_eq!(inactive_type_33[0].0.active, Some(false));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .flags = 0;
    assert!(matches!(
        section_skamp_constraints(
            &type_33_bounded_curve,
            &SketchId("creo:model:sketch#917".into()),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native {
            native_flags: Some(0),
            ..
        }
    ));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .flags = 34;
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 0;
    assert!(matches!(
        section_skamp_constraints(
            &type_33_bounded_curve,
            &SketchId("creo:model:sketch#917".into()),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 10;
    type_33_bounded_curve
        .segments
        .as_mut()
        .expect("segments")
        .bounded_curve_rows
        .clear();
    assert!(matches!(
        section_skamp_constraints(
            &type_33_bounded_curve,
            &SketchId("creo:model:sketch#917".into()),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let emitted_type_33_geometry = BTreeMap::from([(
        SketchEntityId("creo:featdefs:sketch_entity#917:18".to_string()),
        SketchGeometry::Native {
            native_kind: "bounded_curve".to_string(),
        },
    )]);
    let emitted_type_33_constraints = section_skamp_constraints_for_geometry(
        &type_33_bounded_curve,
        &SketchId("creo:model:sketch#917".into()),
        Some(&emitted_type_33_geometry),
    );
    assert!(matches!(
        &emitted_type_33_constraints[0].0.definition,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == &[SketchEntityId(
                "creo:featdefs:sketch_entity#917:18".to_string()
            )]
    ));

    let mut type_33_line = definition.clone();
    type_33_line.relations.as_mut().expect("relations").skamps =
        vec![crate::feature::FeatureSkamp {
            id: 20,
            kind: 33,
            flags: 393_216,
            status: 35,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 10,
            }],
            offset: 88,
        }];
    synchronize_skamp_count(&mut type_33_line);
    assert!(matches!(
        section_skamp_constraints(&type_33_line, &SketchId("creo:model:sketch#917".into()),)[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            native_flags: Some(393_216),
            ..
        }
    ));

    type_33_line.relations.as_mut().expect("relations").skamps[0].flags = 0;
    assert!(matches!(
        section_skamp_constraints(&type_33_line, &SketchId("creo:model:sketch#917".into()),)[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            native_flags: Some(0),
            ..
        }
    ));
    let point_entity = crate::feature::FeatureSkampItem {
        entity_id: 14,
        sense: 0,
    };
    let mut mixed_tangent = definition.clone();
    let mixed_tangent_relations = mixed_tangent.relations.as_mut().expect("relations");
    mixed_tangent_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 18,
        kind: 4,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 3,
            },
        ],
        offset: 83,
    }];
    mixed_tangent_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        section_skamp_constraints(&mixed_tangent, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::TangentLoci {
            first: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            second: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
        }
    );
    mixed_tangent.relations.as_mut().expect("relations").skamps[0]
        .items
        .reverse();
    assert_eq!(
        section_skamp_constraints(&mixed_tangent, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::TangentLoci {
            first: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
        }
    );
    mixed_tangent.relations.as_mut().expect("relations").skamps[0].items[0] =
        crate::feature::FeatureSkampItem {
            entity_id: 15,
            sense: 2,
        };
    assert!(matches!(
        section_skamp_constraints(&mixed_tangent, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut mixed_perpendicular = definition.clone();
    mixed_perpendicular
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 19,
        kind: 5,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
        ],
        offset: 84,
    }];
    synchronize_skamp_count(&mut mixed_perpendicular);
    assert_eq!(
        section_skamp_constraints(
            &mixed_perpendicular,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Perpendicular {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    mixed_perpendicular
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 2;
    assert!(matches!(
        section_skamp_constraints(
            &mixed_perpendicular,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    mixed_perpendicular
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![
        crate::feature::FeatureSkamp {
            id: 18,
            kind: 0,
            flags: 0,
            status: 35,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
            ],
            offset: 83,
        },
        crate::feature::FeatureSkamp {
            id: 19,
            kind: 5,
            flags: 0,
            status: 34,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
            ],
            offset: 84,
        },
    ];
    synchronize_skamp_count(&mut mixed_perpendicular);
    assert_eq!(
        section_skamp_constraints(
            &mixed_perpendicular,
            &SketchId("creo:model:sketch#917".into())
        )[1]
        .0
        .definition,
        SketchConstraintDefinition::Perpendicular {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
        }
    );
    let mut point_pair = definition.clone();
    let point_pair_relations = point_pair.relations.as_mut().expect("relations");
    point_pair_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 19,
        kind: 3,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 14,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
        ],
        offset: 84,
    }];
    point_pair_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    let sketch = SketchId("creo:model:sketch#917".into());
    let point_pair_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:14".to_string()),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
    ]);
    assert_eq!(
        section_skamp_constraints_for_geometry(&point_pair, &sketch, Some(&point_pair_geometry))[0]
            .0
            .definition,
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:14".to_string()
                )),
                SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:99".to_string()
                )),
            ],
        }
    );
    let missing_point_geometry = BTreeMap::from([(
        SketchEntityId("creo:featdefs:sketch_entity#917:14".to_string()),
        SketchGeometry::Native {
            native_kind: "point".to_string(),
        },
    )]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(&point_pair, &sketch, Some(&missing_point_geometry))
            [0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut point_coincidence_definition = definition.clone();
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 0,
        kind: 0,
        flags: 0,
        status: 1,
        items: vec![
            point_entity.clone(),
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 4,
            },
        ],
        offset: 83,
    }];
    point_coincidence_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        section_skamp_constraints(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:14".to_string()
                )),
                SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
            ],
        }
    );
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[0].kind = 3;
    point_coincidence_relations.skamps[0].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 12,
            sense: 0,
        },
        point_entity,
    ];
    assert_eq!(
        section_skamp_constraints(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[0].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 14,
            sense: 0,
        },
    ];
    assert_eq!(
        section_skamp_constraints(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
        }
    );
    let native_endpoint = SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string());
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[0].kind = 0;
    point_coincidence_relations.skamps[0].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 12,
            sense: 3,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 2,
        },
    ];
    let incidence_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        ),
        (
            native_endpoint.clone(),
            SketchGeometry::Native {
                native_kind: "solver_only_section_entity".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            SketchGeometry::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::FRAC_PI_2),
            },
        ),
    ]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[0].kind = 3;
    point_coincidence_relations.skamps[0].items[0] = crate::feature::FeatureSkampItem {
        entity_id: 13,
        sense: 0,
    };
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1]
        .sense = 4;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[0].kind = 17;
    point_coincidence_relations.skamps[0].flags = 1;
    point_coincidence_relations.skamps[0].status = 0;
    point_coincidence_relations.skamps[0].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
    ];
    let mut unresolved_arc_geometry = incidence_geometry.clone();
    unresolved_arc_geometry.insert(
        SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
        SketchGeometry::Native {
            native_kind: "arc".to_string(),
        },
    );
    let inactive = section_skamp_constraints_for_geometry(
        &point_coincidence_definition,
        &SketchId("creo:model:sketch#917".into()),
        Some(&unresolved_arc_geometry),
    );
    assert_eq!(
        inactive[0].0.definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            second: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            axis: SketchCoordinateAxis::U,
        }
    );
    assert_eq!(inactive[0].0.active, Some(false));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 3,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 3,
        },
    ];
    unresolved_arc_geometry.insert(
        SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
        SketchGeometry::Native {
            native_kind: "solver_only_section_entity".to_string(),
        },
    );
    assert_eq!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_arc_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:99".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            axis: SketchCoordinateAxis::U,
        }
    );
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
    ];
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .kind = 15;
    let inactive_type_fifteen = section_skamp_constraints_for_geometry(
        &point_coincidence_definition,
        &SketchId("creo:model:sketch#917".into()),
        Some(&unresolved_arc_geometry),
    );
    assert_eq!(
        inactive_type_fifteen[0].0.definition,
        inactive[0].0.definition
    );
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .flags = 3;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_arc_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let type_fifteen = &mut point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0];
    type_fifteen.kind = 17;
    type_fifteen.flags = 1;
    let mut inactive_tangent_definition = point_coincidence_definition.clone();
    let inactive_tangent = &mut inactive_tangent_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0];
    inactive_tangent.kind = 4;
    inactive_tangent.items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 2,
        },
    ];
    assert_eq!(
        section_skamp_constraints_for_geometry(
            &inactive_tangent_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_arc_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::TangentLoci {
            first: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:99".to_string()
            )),
        }
    );
    inactive_tangent_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &inactive_tangent_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_arc_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut inactive_point_on_curve_definition = point_coincidence_definition.clone();
    let inactive_point_on_curve = &mut inactive_point_on_curve_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0];
    inactive_point_on_curve.kind = 3;
    inactive_point_on_curve.items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 4,
        },
    ];
    let unresolved_curve_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            SketchGeometry::Native {
                native_kind: "line".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
            SketchGeometry::Native {
                native_kind: "circle".to_string(),
            },
        ),
    ]);
    assert_eq!(
        section_skamp_constraints_for_geometry(
            &inactive_point_on_curve_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_curve_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:99".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
        }
    );
    inactive_point_on_curve_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &inactive_point_on_curve_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_curve_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Center(_),
            ..
        }
    ));
    let mut inactive_point_symmetry_definition = point_coincidence_definition.clone();
    let inactive_point_symmetry = &mut inactive_point_symmetry_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0];
    inactive_point_symmetry.kind = 14;
    inactive_point_symmetry.items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 97,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 98,
            sense: 4,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 4,
        },
    ];
    let unresolved_point_symmetry_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:97".to_string()),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:98".to_string()),
            SketchGeometry::Native {
                native_kind: "circle".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
            SketchGeometry::Native {
                native_kind: "circle".to_string(),
            },
        ),
    ]);
    assert_eq!(
        section_skamp_constraints_for_geometry(
            &inactive_point_symmetry_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_point_symmetry_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::PointSymmetric {
            first: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:98".to_string()
            )),
            second: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:99".to_string()
            )),
            center: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:97".to_string()
            )),
        }
    );
    inactive_point_symmetry_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &inactive_point_symmetry_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_point_symmetry_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    let active_native_arc = section_skamp_constraints_for_geometry(
        &point_coincidence_definition,
        &SketchId("creo:model:sketch#917".into()),
        Some(&unresolved_arc_geometry),
    );
    assert!(matches!(
        active_native_arc[0].0.definition,
        SketchConstraintDefinition::SameCoordinate { .. }
    ));
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[0].kind = 3;
    point_coincidence_relations.skamps[0].flags = 0;
    point_coincidence_relations.skamps[0].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 2,
        },
    ];
    point_coincidence_relations.skamps.insert(
        0,
        crate::feature::FeatureSkamp {
            id: 1,
            kind: 1,
            flags: 0,
            status: 1,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            }],
            offset: 84,
        },
    );
    point_coincidence_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    point_coincidence_relations.skamps[1].status = 0;
    assert_eq!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Horizontal {
            entity: native_endpoint,
        }
    );
    let point_coincidence_relations = point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations");
    point_coincidence_relations.skamps[1].kind = 35;
    point_coincidence_relations.skamps[1].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 12,
            sense: 2,
        },
    ];
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[1]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint { .. }
    ));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[1]
        .items[1]
        .sense = 0;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &point_coincidence_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&incidence_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    let mut collinear_definition = definition.clone();
    let collinear_relations = collinear_definition.relations.as_mut().expect("relations");
    collinear_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 9,
        kind: 9,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 15,
                sense: 0,
            },
        ],
        offset: 83,
    }];
    collinear_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        section_skamp_constraints(
            &collinear_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Collinear {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:15".to_string()),
        }
    );
    let mut midpoint_definition = definition.clone();
    let midpoint = crate::feature::FeatureSkamp {
        id: 35,
        kind: 35,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 14,
                sense: 0,
            },
        ],
        offset: 83,
    };
    let midpoint_relations = midpoint_definition.relations.as_mut().expect("relations");
    midpoint_relations.skamps = vec![midpoint];
    midpoint_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        section_skamp_constraints(
            &midpoint_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    midpoint_definition
        .variables
        .as_mut()
        .expect("variables")
        .points = vec![
        crate::feature::FeatureSectionPoint {
            point_id: 1,
            u: Some(0.0),
            v: Some(0.0),
        },
        crate::feature::FeatureSectionPoint {
            point_id: 2,
            u: Some(4.0),
            v: Some(2.0),
        },
        crate::feature::FeatureSectionPoint {
            point_id: 4,
            u: None,
            v: None,
        },
    ];
    assert_eq!(
        resolved_section_points(&midpoint_definition),
        BTreeMap::from([(1, [0.0, 0.0]), (2, [4.0, 2.0]), (4, [2.0, 1.0])])
    );
    let mut saved_line_midpoint = midpoint_definition.clone();
    saved_line_midpoint.order_table = Some(crate::feature::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::FeatureOrderRow {
            external_id: 99,
            internal_id: 20,
            bitmask: 1,
            offset: 95,
        }],
        offset: 94,
    });
    saved_line_midpoint.saved_section = Some(crate::feature::FeatureSavedSection {
        entities: vec![crate::feature::FeatureSavedEntity::Line(
            crate::feature::FeatureSavedLine {
                entity_id: 20,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(0.0), Some(0.0)],
                    [Some(4.0), Some(2.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 96,
            },
        )],
        offset: 96,
    });
    saved_line_midpoint
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .entity_id = 99;
    synchronize_skamp_count(&mut saved_line_midpoint);
    assert_eq!(
        resolved_section_points(&saved_line_midpoint).get(&4),
        Some(&[2.0, 1.0])
    );
    let mut arc_midpoint_definition = midpoint_definition.clone();
    arc_midpoint_definition
        .variables
        .as_mut()
        .expect("variables")
        .points = vec![
        crate::feature::FeatureSectionPoint {
            point_id: 2,
            u: Some(1.0),
            v: Some(0.0),
        },
        crate::feature::FeatureSectionPoint {
            point_id: 3,
            u: Some(0.0),
            v: Some(1.0),
        },
        crate::feature::FeatureSectionPoint {
            point_id: 4,
            u: Some(0.0),
            v: Some(0.0),
        },
        crate::feature::FeatureSectionPoint {
            point_id: 8,
            u: None,
            v: None,
        },
    ];
    arc_midpoint_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .iter_mut()
        .find(|segment| segment.external_id == 13)
        .expect("arc")
        .arc_orientation = Some(0);
    arc_midpoint_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .iter_mut()
        .find(|segment| segment.external_id == 14)
        .expect("point")
        .point_ids = [8, 8];
    let midpoint_skamps = &mut arc_midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps;
    midpoint_skamps[0].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 14,
            sense: 0,
        },
    ];
    assert!(resolved_section_points(&arc_midpoint_definition)
        .get(&8)
        .is_some_and(|[u, v]| {
            let expected = -std::f64::consts::FRAC_1_SQRT_2;
            (u - expected).abs() <= 1e-12 && (v - expected).abs() <= 1e-12
        }));
    for (kind, expected_coordinate) in [(12, 1), (13, 0)] {
        let mut arc_alignment_definition = midpoint_definition.clone();
        arc_alignment_definition
            .variables
            .as_mut()
            .expect("variables")
            .points = vec![
            crate::feature::FeatureSectionPoint {
                point_id: 2,
                u: Some(1.0),
                v: Some(2.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 3,
                u: if expected_coordinate == 0 {
                    None
                } else {
                    Some(3.0)
                },
                v: if expected_coordinate == 1 {
                    None
                } else {
                    Some(3.0)
                },
            },
        ];
        {
            let alignment_skamps = &mut arc_alignment_definition
                .relations
                .as_mut()
                .expect("relations")
                .skamps;
            alignment_skamps[0] = crate::feature::FeatureSkamp {
                id: 12,
                kind,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                }],
                offset: 84,
            };
        }
        assert_eq!(
            resolved_section_points(&arc_alignment_definition).get(&3),
            Some(&[
                if expected_coordinate == 0 { 1.0 } else { 3.0 },
                if expected_coordinate == 1 { 2.0 } else { 3.0 },
            ])
        );
        arc_alignment_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps[0]
            .status = 0;
        let unresolved = resolved_section_points(&arc_alignment_definition);
        assert!(!unresolved.contains_key(&3));
    }
    midpoint_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .last_mut()
        .expect("midpoint point")
        .u = Some(3.0);
    assert_eq!(
        resolved_section_coordinates(&midpoint_definition).get(&4),
        Some(&[Some(3.0), Some(1.0)])
    );
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1] = crate::feature::FeatureSkampItem {
        entity_id: 13,
        sense: 0,
    };
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .push(crate::feature::FeatureSkamp {
            id: 36,
            kind: 3,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 4,
                },
            ],
            offset: 84,
        });
    synchronize_skamp_count(&mut midpoint_definition);
    assert!(section_skamp_is_circular(
        &midpoint_definition,
        &midpoint_definition
            .relations
            .as_ref()
            .expect("relations")
            .skamps[0]
            .items[1],
    ));
    assert_eq!(
        section_skamp_constraints(
            &midpoint_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .truncate(1);
    synchronize_skamp_count(&mut midpoint_definition);
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1] = crate::feature::FeatureSkampItem {
        entity_id: 14,
        sense: 0,
    };
    let midpoint_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(2.0, 0.0),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#917:14".to_string()),
            SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
        ),
    ]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &midpoint_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&midpoint_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint { .. }
    ));
    let unresolved_midpoint_geometry = BTreeMap::from([(
        SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    )]);
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &midpoint_definition,
            &SketchId("creo:model:sketch#917".into()),
            Some(&unresolved_midpoint_geometry),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1] = crate::feature::FeatureSkampItem {
        entity_id: 13,
        sense: 2,
    };
    assert_eq!(
        section_skamp_constraints(
            &midpoint_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    let mut equal_radius_definition = definition.clone();
    let equal_radius_segments = &mut equal_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows;
    equal_radius_segments[1].radius_ref = Some(101);
    equal_radius_segments[4].radius_ref = Some(102);
    equal_radius_definition.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![
            crate::feature::FeatureSectionPoint {
                point_id: 2,
                u: Some(3.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 4,
                u: Some(0.0),
                v: Some(0.0),
            },
        ],
        offset: 89,
    });
    assert_eq!(
        resolved_section_radii(&equal_radius_definition),
        BTreeMap::from([(101, 3.0), (102, 3.0)])
    );
    let mut disabled_equal_radius = equal_radius_definition.clone();
    disabled_equal_radius
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .iter_mut()
        .find(|skamp| skamp.kind == 6)
        .expect("equal-radius incidence")
        .status = 34;
    assert_eq!(
        resolved_section_radii(&disabled_equal_radius),
        BTreeMap::from([(101, 3.0)])
    );
    equal_radius_definition
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .push(crate::feature::FeatureVariableRow {
            variable_type: 3,
            key: 102,
            value: Some(4.0),
            value_body: Vec::new(),
            guess: None,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset: 91,
        });
    equal_radius_definition
        .variables
        .as_mut()
        .expect("variables")
        .declared_count = 1;
    assert!(resolved_section_radii(&equal_radius_definition).is_empty());
    let mut saved_radius_definition = definition.clone();
    saved_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows[1]
        .radius_ref = Some(101);
    saved_radius_definition.order_table = Some(crate::feature::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::FeatureOrderRow {
            external_id: 99,
            internal_id: 20,
            bitmask: 1,
            offset: 92,
        }],
        offset: 91,
    });
    saved_radius_definition.saved_section = Some(crate::feature::FeatureSavedSection {
        entities: vec![crate::feature::FeatureSavedEntity::Circle(
            crate::feature::FeatureSavedCircle {
                entity_id: 20,
                center: [Some(0.0), Some(0.0), Some(0.0)],
                radius: Some(4.0),
                body: Vec::new(),
                offset: 93,
            },
        )],
        offset: 93,
    });
    saved_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 18,
        kind: 6,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
        ],
        offset: 94,
    }];
    saved_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        resolved_section_radii(&saved_radius_definition),
        BTreeMap::from([(101, 4.0)])
    );
    let constraints =
        section_skamp_constraints(&definition, &SketchId("creo:model:sketch#917".into()));
    let mut solver_only = definition.clone();
    solver_only.relations.as_mut().expect("relations").skamps =
        vec![crate::feature::FeatureSkamp {
            id: 20,
            kind: 0,
            flags: 0,
            status: 35,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 3,
                },
            ],
            offset: 95,
        }];
    assert_eq!(
        solver_only_section_entities(&solver_only),
        BTreeMap::from([(99, 95)])
    );
    let mut whole_entity_tangent = definition.clone();
    whole_entity_tangent
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 20,
        kind: 4,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
        ],
        offset: 95,
    }];
    synchronize_skamp_count(&mut whole_entity_tangent);
    assert_eq!(
        section_skamp_constraints(
            &whole_entity_tangent,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Tangent {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
        }
    );
    let mut point_symmetry = definition.clone();
    point_symmetry.relations.as_mut().expect("relations").skamps =
        vec![crate::feature::FeatureSkamp {
            id: 20,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 3,
                },
            ],
            offset: 96,
        }];
    point_symmetry
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 4,
            u: Some(2.0),
            v: Some(3.0),
        });
    synchronize_skamp_count(&mut point_symmetry);
    assert_eq!(
        section_skamp_constraints(&point_symmetry, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::PointSymmetric {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            center: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )),
        }
    );
    assert_eq!(resolved_section_points(&point_symmetry)[&3], [4.0, 4.0]);
    point_symmetry.relations.as_mut().expect("relations").skamps[0].items[1] =
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        };
    point_symmetry.relations.as_mut().expect("relations").skamps[0].items[2] =
        crate::feature::FeatureSkampItem {
            entity_id: 16,
            sense: 4,
        };
    assert_eq!(
        section_skamp_constraints(&point_symmetry, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::PointSymmetric {
            first: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            second: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:16".to_string()
            )),
            center: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )),
        }
    );
    point_symmetry.relations.as_mut().expect("relations").skamps[0].items[2].sense = 1;
    assert!(matches!(
        section_skamp_constraints(&point_symmetry, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));

    assert!(matches!(
        constraints[0].0.definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(matches!(
        constraints[1].0.definition,
        SketchConstraintDefinition::Vertical { .. }
    ));
    let mut incomplete_segments = definition.clone();
    incomplete_segments
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    let incomplete_constraints = section_skamp_constraints(
        &incomplete_segments,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(matches!(
        incomplete_constraints[0].0.definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(matches!(
        incomplete_constraints[1].0.definition,
        SketchConstraintDefinition::Vertical { .. }
    ));
    let mut locus_orientation = definition.clone();
    locus_orientation
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 2;
    assert!(matches!(
        section_skamp_constraints(&locus_orientation, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:skamp:1"
    ));
    let mut duplicate_entity = definition.clone();
    let mut duplicate_line = duplicate_entity.segments.as_ref().expect("segments").rows[0].clone();
    duplicate_line.offset = 500;
    duplicate_entity
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(duplicate_line);
    let unique_ids = unique_section_segment_external_ids(&duplicate_entity);
    assert!(!unique_ids.contains(&12));
    assert_eq!(
        section_segment_identity_suffix(
            &unique_ids,
            duplicate_entity
                .segments
                .as_ref()
                .expect("segments")
                .rows
                .last()
                .expect("duplicate segment")
        ),
        "offset:500"
    );
    assert!(matches!(
        section_skamp_constraints(&duplicate_entity, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:skamp:1"
    ));
    let opaque_segment = crate::feature::FeatureOpaqueSegment {
        kind: 25,
        directions: [None; 3],
        point_ids: [Some(1), Some(2)],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 99,
        body: Vec::new(),
        offset: 600,
    };
    let mut opaque_entity = definition.clone();
    opaque_entity
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .push(opaque_segment.clone());
    assert!(unique_section_segment_external_ids(&opaque_entity).contains(&99));
    assert!(section_entity_external_ids(&opaque_entity).contains(&99));

    let mut opaque_point = definition.clone();
    opaque_point
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    opaque_point
        .segments
        .as_mut()
        .expect("segments")
        .point_rows
        .push(crate::feature::FeaturePointSegment {
            point_id: 1,
            external_id: 99,
            offset: 601,
        });
    let opaque_point_item = crate::feature::FeatureSkampItem {
        entity_id: 99,
        sense: 0,
    };
    assert_eq!(
        section_point_row_geometry(
            &resolved_section_points(&opaque_point),
            &opaque_point.segments.as_ref().expect("segments").point_rows[0],
        ),
        Some(SketchGeometry::Point {
            position: Point2::new(0.0, 2.0),
        })
    );
    assert!(section_skamp_is_point(&opaque_point, &opaque_point_item));
    assert!(matches!(
        section_skamp_locus(
            &opaque_point,
            &SketchId("creo:model:sketch#917".into()),
            &crate::feature::FeatureSkampItem {
                sense: 4,
                ..opaque_point_item.clone()
            },
        ),
        Some(SketchLocus::Entity(entity)) if entity.0.ends_with(":99")
    ));
    assert!(matches!(
        section_skamp_midpoint(
            &opaque_point,
            &SketchId("creo:model:sketch#917".into()),
            &crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            &opaque_point_item,
            None,
        ),
        Some((SketchLocus::Entity(entity), target))
            if entity.0.ends_with(":99") && target.0.ends_with(":12")
    ));
    assert_eq!(
        section_skamp_midpoint(
            &opaque_point,
            &SketchId("creo:model:sketch#917".into()),
            &opaque_point_item,
            &crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
            Some(&BTreeMap::from([(
                SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
                SketchGeometry::Native {
                    native_kind: "point".to_string(),
                },
            )])),
        ),
        None
    );
    let centered_line = crate::feature::FeatureCenteredLineSegment {
        center_id: 2,
        external_id: 100,
        offset: 602,
    };
    assert_eq!(
        section_centered_line_geometry(
            &BTreeMap::from([(0, [3.0, -1.0]), (1, [3.0, 5.0]), (2, [3.0, 2.0]),]),
            &centered_line,
        ),
        Some(SketchGeometry::Line {
            start: Point2::new(3.0, -1.0),
            end: Point2::new(3.0, 5.0),
        })
    );
    assert_eq!(
        section_centered_line_geometry(
            &BTreeMap::from([(0, [3.0, -1.0]), (1, [3.0, 5.0]), (2, [3.0, 2.0]),]),
            &crate::feature::FeatureCenteredLineSegment {
                center_id: 0,
                ..centered_line.clone()
            },
        ),
        None
    );
    let reference_line = crate::feature::FeatureReferenceLineSegment {
        directions: [Some(0), Some(1), Some(0)],
        point_ids: [Some(7), Some(8)],
        vertical_horizontal: Some(1),
        external_id: 101,
        offset: 603,
    };
    assert_eq!(
        section_reference_line_geometry(
            &BTreeMap::from([(7, [-4.0, 2.0]), (8, [6.0, 2.0])]),
            &reference_line,
        ),
        Some(SketchGeometry::ReferenceLine {
            origin: Point2::new(-4.0, 2.0),
            direction: Point2::new(10.0, 0.0),
        })
    );
    assert_eq!(
        section_reference_line_geometry(
            &BTreeMap::from([(7, [-4.0, 2.0]), (8, [-4.0, 2.0])]),
            &reference_line,
        ),
        None
    );
    let axis_reference_line = crate::feature::FeatureReferenceLineSegment {
        directions: [Some(0), Some(1), Some(0)],
        vertical_horizontal: Some(1),
        ..reference_line.clone()
    };
    let mut reference_definition = definition.clone();
    let reference_segments = reference_definition
        .segments
        .as_mut()
        .expect("segment table");
    reference_segments.declared_count += 1;
    reference_segments
        .reference_line_rows
        .push(axis_reference_line.clone());
    assert!(section_skamp_is_line(
        &reference_definition,
        &crate::feature::FeatureSkampItem {
            entity_id: axis_reference_line.external_id,
            sense: 0,
        },
    ));
    assert_eq!(
        resolved_section_reference_line_geometry(
            &reference_definition,
            &BTreeMap::from([(7, [None, Some(2.0)]), (8, [None, Some(2.0)]),]),
            &BTreeMap::new(),
            &axis_reference_line,
        ),
        Some(SketchGeometry::ReferenceLine {
            origin: Point2::new(0.0, 2.0),
            direction: Point2::new(1.0, 0.0),
        })
    );
    assert_eq!(
        resolved_section_reference_line_geometry(
            &reference_definition,
            &BTreeMap::from([(7, [None, Some(2.0)]), (8, [None, Some(3.0)]),]),
            &BTreeMap::new(),
            &axis_reference_line,
        ),
        None
    );
    assert_eq!(
        section_reference_line_geometry(
            &BTreeMap::from([(7, [-4.0, 2.0])]),
            &crate::feature::FeatureReferenceLineSegment {
                point_ids: [Some(7), None],
                ..reference_line
            },
        ),
        None
    );
    let mut endpoint_families = definition.clone();
    let endpoint_segments = endpoint_families
        .segments
        .as_mut()
        .expect("segment table");
    endpoint_segments.declared_count += 2;
    endpoint_segments
        .reference_line_rows
        .push(crate::feature::FeatureReferenceLineSegment {
            point_ids: [Some(1), Some(5)],
            external_id: 101,
            ..reference_line.clone()
        });
    endpoint_segments
        .bounded_curve_rows
        .push(crate::feature::FeatureBoundedCurveSegment {
            point_ids: [1, 5],
            external_id: 102,
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            offset: 604,
        });
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 2,
            },
        ),
        Some(1)
    );
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 3,
            },
        ),
        Some(5)
    );
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::FeatureSkampItem {
                entity_id: 102,
                sense: 2,
            },
        ),
        Some(1)
    );
    assert_eq!(
        section_skamp_selected_point_id(
            &endpoint_families,
            &crate::feature::FeatureSkampItem {
                entity_id: 102,
                sense: 3,
            },
        ),
        Some(5)
    );
    let mut opaque_line = definition.clone();
    opaque_line
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    opaque_line
        .segments
        .as_mut()
        .expect("segments")
        .centered_line_rows
        .push(centered_line);
    let opaque_line_item = crate::feature::FeatureSkampItem {
        entity_id: 100,
        sense: 0,
    };
    assert!(section_skamp_is_line(&opaque_line, &opaque_line_item));
    assert!(matches!(
        section_skamp_locus(
            &opaque_line,
            &SketchId("creo:model:sketch#917".into()),
            &crate::feature::FeatureSkampItem {
                sense: 2,
                ..opaque_line_item
            },
        ),
        Some(SketchLocus::Start(entity)) if entity.0.ends_with(":100")
    ));
    let mut solver_only_point_midpoint = opaque_line.clone();
    let midpoint_relations = solver_only_point_midpoint
        .relations
        .as_mut()
        .expect("relations");
    midpoint_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 102,
        kind: 35,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
        ],
        offset: 604,
    }];
    midpoint_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_only_point_midpoint, 101),
        Some(SectionEntityIncidenceFamily::Point)
    );
    assert_eq!(
        section_skamp_constraints(
            &solver_only_point_midpoint,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:101".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    let mut conflicting_midpoint = solver_only_point_midpoint.clone();
    let conflicting_relations = conflicting_midpoint.relations.as_mut().expect("relations");
    conflicting_relations
        .skamps
        .push(crate::feature::FeatureSkamp {
            id: 103,
            kind: 0,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 101,
                    sense: 4,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
            ],
            offset: 605,
        });
    conflicting_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&conflicting_midpoint, 101),
        None
    );
    let centered_midpoint = crate::feature::FeatureSkamp {
        id: 35,
        kind: 35,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 100,
                sense: 4,
            },
        ],
        offset: 83,
    };
    let centered_midpoint_relations = opaque_line.relations.as_mut().expect("relations");
    centered_midpoint_relations.skamps = vec![centered_midpoint];
    centered_midpoint_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&opaque_line, 101),
        Some(SectionEntityIncidenceFamily::Point)
    );
    assert_eq!(
        section_skamp_constraints_for_geometry(
            &opaque_line,
            &SketchId("creo:model:sketch#917".into()),
            Some(&BTreeMap::from([
                (
                    SketchEntityId("creo:featdefs:sketch_entity#917:100".to_string()),
                    SketchGeometry::Line {
                        start: Point2::new(3.0, -1.0),
                        end: Point2::new(3.0, 5.0),
                    },
                ),
                (
                    SketchEntityId("creo:featdefs:sketch_entity#917:101".to_string()),
                    SketchGeometry::Native {
                        native_kind: "point".to_string(),
                    },
                ),
            ])),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:101".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:100".to_string()),
        }
    );
    opaque_line
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .insert(
            0,
            crate::feature::FeatureSkamp {
                id: 2,
                kind: 2,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 101,
                    sense: 0,
                }],
                offset: 82,
            },
        );
    opaque_line
        .relations
        .as_mut()
        .expect("relations")
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&opaque_line, 101),
        Some(SectionEntityIncidenceFamily::Line)
    );
    opaque_line.relations.as_mut().expect("relations").skamps[0].status = 0;
    assert_eq!(
        solver_only_section_entity_family(&opaque_line, 101),
        Some(SectionEntityIncidenceFamily::Line)
    );
    opaque_line.relations.as_mut().expect("relations").skamps[0].status = 1;
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &opaque_line,
            &SketchId("creo:model:sketch#917".into()),
            Some(&BTreeMap::from([(
                SketchEntityId("creo:featdefs:sketch_entity#917:101".to_string()),
                SketchGeometry::Native {
                    native_kind: "line".to_string(),
                },
            )])),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Vertical { .. }
    ));
    assert!(matches!(
        section_skamp_constraints_for_geometry(
            &opaque_line,
            &SketchId("creo:model:sketch#917".into()),
            Some(&BTreeMap::from([
                (
                    SketchEntityId("creo:featdefs:sketch_entity#917:100".to_string()),
                    SketchGeometry::Line {
                        start: Point2::new(3.0, -1.0),
                        end: Point2::new(3.0, 5.0),
                    },
                ),
                (
                    SketchEntityId("creo:featdefs:sketch_entity#917:101".to_string()),
                    SketchGeometry::Native {
                        native_kind: "line".to_string(),
                    },
                ),
            ])),
        )[1]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut opaque_family_collision = opaque_point.clone();
    opaque_family_collision
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    let colliding_row = crate::feature::FeatureOpaqueSegment {
        kind: 10,
        directions: [Some(0); 3],
        point_ids: [None, Some(1)],
        center_id: Some(1),
        arc_orientation: Some(0),
        vertical_horizontal: Some(0),
        radius_ref: Some(0),
        radius2_ref: None,
        external_id: 99,
        body: Vec::new(),
        offset: 602,
    };
    opaque_family_collision
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .push(colliding_row);
    assert!(!section_skamp_is_point(
        &opaque_family_collision,
        &opaque_point_item,
    ));

    let mut opaque_collision = definition.clone();
    opaque_collision
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .push(crate::feature::FeatureOpaqueSegment {
            external_id: 12,
            ..opaque_segment
        });
    assert!(!unique_section_segment_external_ids(&opaque_collision).contains(&12));
    assert!(ambiguous_section_segment_external_ids(&opaque_collision).contains(&12));
    assert!(!section_entity_external_ids(&opaque_collision).contains(&12));
    let mut equivalent_skamp = definition.clone();
    let mut redundant = equivalent_skamp
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .clone();
    redundant.id = 100;
    redundant.offset = 500;
    equivalent_skamp
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .push(redundant);
    let equivalent_constraints =
        section_skamp_constraints(&equivalent_skamp, &SketchId("creo:model:sketch#917".into()));
    let mut first_definition = equivalent_constraints[0].0.definition.clone();
    let mut redundant_definition = equivalent_constraints
        .last()
        .expect("redundant")
        .0
        .definition
        .clone();
    let SketchConstraintDefinition::Native {
        native_properties: first_properties,
        ..
    } = &mut first_definition
    else {
        panic!("incomplete incidence table must remain native");
    };
    let SketchConstraintDefinition::Native {
        native_properties: redundant_properties,
        ..
    } = &mut redundant_definition
    else {
        panic!("incomplete incidence table must remain native");
    };
    assert_eq!(first_properties.get("id").map(String::as_str), Some("3"));
    assert_eq!(
        redundant_properties.get("id").map(String::as_str),
        Some("100")
    );
    first_properties.clear();
    redundant_properties.clear();
    assert_eq!(first_definition, redundant_definition);
    assert_ne!(
        equivalent_constraints[0].0.id,
        equivalent_constraints.last().expect("redundant").0.id
    );
    let mut duplicate_skamp_id = definition.clone();
    let mut duplicate = duplicate_skamp_id
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .clone();
    duplicate.offset = 500;
    duplicate_skamp_id
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .push(duplicate);
    let duplicate_constraints = section_skamp_constraints(
        &duplicate_skamp_id,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(matches!(
        duplicate_constraints[0].0.definition,
        SketchConstraintDefinition::Native { .. }
    ));
    assert!(matches!(
        duplicate_constraints
            .last()
            .expect("duplicate")
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    assert_eq!(
        duplicate_constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#917:skamp:offset:50"
    );
    assert_eq!(
        duplicate_constraints.last().expect("duplicate").0.id.0,
        "creo:featdefs:sketch_constraint#917:skamp:offset:500"
    );
    for (constraint, _) in [
        &duplicate_constraints[0],
        duplicate_constraints.last().expect("duplicate"),
    ] {
        let SketchConstraintDefinition::Native {
            native_properties, ..
        } = &constraint.definition
        else {
            panic!("duplicate incidence must remain native");
        };
        assert_eq!(native_properties.get("id").map(String::as_str), Some("3"));
    }
    assert_eq!(
        constraints[2].0.definition,
        SketchConstraintDefinition::Native {
            native_kind: "creo:skamp:7".to_string(),
            native_state: Some(1),
            native_flags: Some(0),
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )],
            parameter: None,
            operands: vec![SketchNativeOperand {
                native_kind: "skamp_ptr".to_string(),
                native_field: Some("items.entity_id".to_string()),
                native_role: Some(4),
                object_index: 12,
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }],
        }
    );
    let mut unavailable_skamp_entity = definition.clone();
    unavailable_skamp_entity
        .relations
        .as_mut()
        .expect("relations")
        .skamps[2]
        .items[0]
        .entity_id = 999;
    let unavailable_skamp_constraints = section_skamp_constraints(
        &unavailable_skamp_entity,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(matches!(
        &unavailable_skamp_constraints[2].0.definition,
        SketchConstraintDefinition::Native {
            entities,
            operands,
            ..
        } if entities.is_empty()
            && operands == &[SketchNativeOperand {
                native_kind: "skamp_ptr".to_string(),
                native_field: Some("items.entity_id".to_string()),
                native_role: Some(4),
                object_index: 999,
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }]
    ));
    let mut stored_skamp_state = definition.clone();
    stored_skamp_state
        .relations
        .as_mut()
        .expect("relations")
        .skamps[2]
        .status = 34;
    stored_skamp_state
        .relations
        .as_mut()
        .expect("relations")
        .skamps[2]
        .flags = 0x4000;
    let stored_skamp_state = section_skamp_constraints(
        &stored_skamp_state,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(matches!(
        stored_skamp_state[2].0.definition,
        SketchConstraintDefinition::Native {
            native_state: Some(34),
            native_flags: Some(0x4000),
            ..
        }
    ));
    assert_eq!(stored_skamp_state[2].0.active, Some(false));
    assert!(matches!(
        constraints[3].0.definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:skamp:1"
    ));
    assert!(matches!(
        constraints[4].0.definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:skamp:0"
    ));
    let mut center_coincidence = definition.clone();
    let center_items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 12,
            sense: 2,
        },
    ];
    center_coincidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps[4]
        .items = center_items;
    assert_eq!(
        section_skamp_constraints(
            &center_coincidence,
            &SketchId("creo:model:sketch#917".into())
        )[4]
        .0
        .definition,
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
            ],
        }
    );
    let mut concentric = definition.clone();
    let mut second_arc = concentric
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .iter()
        .find(|segment| segment.external_id == 13)
        .expect("arc")
        .clone();
    second_arc.external_id = 99;
    second_arc.offset = 501;
    concentric
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(second_arc);
    concentric.relations.as_mut().expect("relations").skamps[4].items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 13,
            sense: 4,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 4,
        },
    ];
    assert_eq!(
        section_skamp_constraints(&concentric, &SketchId("creo:model:sketch#917".into()))[4]
            .0
            .definition,
        SketchConstraintDefinition::Concentric {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
        }
    );
    let center_relation = center_coincidence
        .relations
        .as_ref()
        .expect("relations")
        .skamps[4]
        .clone();
    center_coincidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![center_relation];
    synchronize_skamp_count(&mut center_coincidence);
    let variables = center_coincidence.variables.as_mut().expect("variables");
    let first_point = variables
        .points
        .iter_mut()
        .find(|point| point.point_id == 1)
        .expect("line start");
    first_point.u = None;
    first_point.v = None;
    variables.points.push(crate::feature::FeatureSectionPoint {
        point_id: 4,
        u: Some(8.0),
        v: Some(9.0),
    });
    assert_eq!(resolved_section_points(&center_coincidence)[&1], [8.0, 9.0]);
    assert_eq!(
        constraints[5].0.definition,
        SketchConstraintDefinition::TangentLoci {
            first: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
        }
    );
    assert_eq!(
        constraints[6].0.definition,
        SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            axis: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    assert_eq!(
        constraints[7].0.definition,
        SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            second: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            axis: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    assert_eq!(
        constraints[8].0.definition,
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:14".to_string()
                )),
                SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
            ],
        }
    );
    assert_eq!(
        constraints[9].0.definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    let mut reversed_point_on_line = definition.clone();
    reversed_point_on_line
        .relations
        .as_mut()
        .expect("relations")
        .skamps[9]
        .items
        .reverse();
    assert_eq!(
        section_skamp_constraints(
            &reversed_point_on_line,
            &SketchId("creo:model:sketch#917".into())
        )[9]
        .0
        .definition,
        constraints[9].0.definition
    );
    let mut line_type_three = definition.clone();
    line_type_three
        .relations
        .as_mut()
        .expect("relations")
        .skamps[8]
        .items[0]
        .entity_id = 12;
    assert_eq!(
        section_skamp_constraints(&line_type_three, &SketchId("creo:model:sketch#917".into()))[8]
            .0
            .definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Center(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string()
            )),
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    let first = SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string());
    let second = SketchEntityId("creo:featdefs:sketch_entity#917:15".to_string());
    assert_eq!(
        constraints[10].0.definition,
        SketchConstraintDefinition::Perpendicular {
            first: first.clone(),
            second: second.clone(),
        }
    );
    assert_eq!(
        constraints[11].0.definition,
        SketchConstraintDefinition::Parallel {
            first: first.clone(),
            second: second.clone(),
        }
    );
    assert_eq!(
        constraints[12].0.definition,
        SketchConstraintDefinition::Equal { first, second }
    );
    assert_eq!(
        constraints[13].0.definition,
        SketchConstraintDefinition::Equal {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:16".to_string()),
        }
    );
    assert_eq!(
        constraints[14].0.definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:15".to_string()
            )),
            axis: SketchCoordinateAxis::V,
        }
    );
    let mut fixed_y_coordinate = definition.clone();
    fixed_y_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .kind = 30;
    fixed_y_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .flags = 99;
    assert_eq!(
        section_skamp_constraints(
            &fixed_y_coordinate,
            &SketchId("creo:model:sketch#917".into())
        )[14]
            .0
            .definition,
        constraints[14].0.definition
    );
    let mut propagated_same_coordinate = definition.clone();
    propagated_same_coordinate
        .variables
        .as_mut()
        .expect("variables")
        .points
        .iter_mut()
        .find(|point| point.point_id == 5)
        .expect("second same-coordinate point")
        .v = None;
    assert_eq!(
        resolved_section_points(&propagated_same_coordinate).get(&5),
        Some(&[3.0, 2.0])
    );
    let mut unsolved_same_coordinate = definition.clone();
    unsolved_same_coordinate
        .variables
        .as_mut()
        .expect("variables")
        .points
        .clear();
    assert_eq!(
        section_skamp_constraints(
            &unsolved_same_coordinate,
            &SketchId("creo:model:sketch#917".into())
        )[14]
            .0
            .definition,
        constraints[14].0.definition
    );
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .kind = 31;
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .flags = 0;
    assert_eq!(
        section_skamp_constraints(
            &unsolved_same_coordinate,
            &SketchId("creo:model:sketch#917".into())
        )[14]
            .0
            .definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:15".to_string()
            )),
            axis: SketchCoordinateAxis::U,
        }
    );
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .kind = 17;
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .flags = 0;
    assert!(matches!(
        section_skamp_constraints(
            &unsolved_same_coordinate,
            &SketchId("creo:model:sketch#917".into())
        )[14]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut conflicting_same_coordinate = definition.clone();
    conflicting_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps[14]
        .flags = 1;
    assert!(matches!(
        section_skamp_constraints(
            &conflicting_same_coordinate,
            &SketchId("creo:model:sketch#917".into())
        )[14]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut point_coincidence_definition = definition.clone();
    point_coincidence_definition
        .variables
        .as_mut()
        .expect("variables")
        .points = vec![
        crate::feature::FeatureSectionPoint {
            point_id: 4,
            u: Some(1.0),
            v: Some(2.0),
        },
        crate::feature::FeatureSectionPoint {
            point_id: 6,
            u: None,
            v: Some(2.0),
        },
    ];
    let point_segment = point_coincidence_definition
        .segments
        .as_mut()
        .expect("segments");
    point_segment.declared_count = 6;
    point_segment.rows.push(crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Point,
        directions: [None; 3],
        point_ids: [6, 6],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 17,
        body: Vec::new(),
        offset: 45,
    });
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps[8]
        .items = vec![
        crate::feature::FeatureSkampItem {
            entity_id: 14,
            sense: 0,
        },
        crate::feature::FeatureSkampItem {
            entity_id: 17,
            sense: 0,
        },
    ];
    assert_eq!(
        resolved_section_points(&point_coincidence_definition).get(&6),
        Some(&[1.0, 2.0])
    );
    let mut distance_definition = definition.clone();
    let distance_segment = &mut distance_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows[0];
    distance_segment.vertical_horizontal = Some(0);
    let distance_relation = &mut distance_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    distance_relation.relation_type = 0;
    distance_relation.sign = 1;
    distance_relation.dimension_id = 0;
    distance_relation.operand_vectors = Some([
        [Some(1), Some(2), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    let mut unspanned_distance = distance_definition.clone();
    unspanned_distance
        .variables
        .as_mut()
        .expect("variables")
        .points
        .iter_mut()
        .find(|point| point.point_id == 5)
        .expect("point 5")
        .u = None;
    unspanned_distance
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [Some(1), Some(5), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert_eq!(
        resolved_section_points(&unspanned_distance).get(&5),
        Some(&[3.0, 2.0])
    );
    let mut disabled_distance = distance_definition.clone();
    let disabled_relations = disabled_distance.relations.as_mut().expect("relations");
    disabled_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 900,
        kind: 0,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 3,
            },
        ],
        offset: 900,
    }];
    disabled_relations.skamp_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 901,
        offset: 900,
    });
    disabled_relations.triples = vec![crate::feature::FeatureRelationTriple {
        relation_id: Some(8),
        equation_id: None,
        skamp_id: Some(900),
        offset: 902,
    }];
    disabled_relations.triples_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 903,
        offset: 902,
    });
    assert!(!resolved_section_points(&disabled_distance).contains_key(&2));
    distance_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .clear();
    assert_eq!(
        section_dimension_constraints(
            &distance_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut conflicting_spanning_distance = distance_definition.clone();
    conflicting_spanning_distance
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 2,
            u: Some(4.0),
            v: Some(3.0),
        });
    assert!(matches!(
        section_dimension_constraints(
            &conflicting_spanning_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut conflicting_saved_spanning_distance = distance_definition.clone();
    conflicting_saved_spanning_distance.order_table = Some(crate::feature::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::FeatureOrderRow {
            external_id: 12,
            internal_id: 20,
            bitmask: 1,
            offset: 95,
        }],
        offset: 94,
    });
    conflicting_saved_spanning_distance.saved_section = Some(crate::feature::FeatureSavedSection {
        entities: vec![crate::feature::FeatureSavedEntity::Line(
            crate::feature::FeatureSavedLine {
                entity_id: 20,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(2.0), Some(0.0)],
                    [Some(4.0), Some(3.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 96,
            },
        )],
        offset: 96,
    });
    assert!(matches!(
        section_dimension_constraints(
            &conflicting_saved_spanning_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut agreeing_saved_spanning_distance = conflicting_saved_spanning_distance.clone();
    let crate::feature::FeatureSavedEntity::Line(line) = agreeing_saved_spanning_distance
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .first_mut()
        .expect("saved line")
    else {
        panic!("saved line entity");
    };
    line.endpoints[1][0] = Some(0.0);
    assert!(matches!(
        section_dimension_constraints(
            &agreeing_saved_spanning_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
            .0
            .definition,
        SketchConstraintDefinition::VerticalDistance { .. }
    ));
    let mut schema_orientation = distance_definition.clone();
    let schema_dimension = &mut schema_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0];
    schema_dimension.value_unit = crate::feature::DimensionUnit::SchemaDefined;
    schema_dimension.value = Some(0.0);
    let schema_relations = schema_orientation.relations.as_mut().expect("relations");
    let relation_id = schema_relations.rows[0].relation_id;
    schema_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 900,
        kind: 2,
        flags: 0,
        status: 35,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 12,
            sense: 0,
        }],
        offset: 901,
    }];
    schema_relations.skamp_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 902,
        offset: 900,
    });
    schema_relations.triples = vec![crate::feature::FeatureRelationTriple {
        relation_id: Some(relation_id),
        equation_id: None,
        skamp_id: Some(900),
        offset: 902,
    }];
    schema_relations.triples_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 903,
        offset: 902,
    });
    assert_eq!(
        section_dimension_constraints(
            &schema_orientation,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Vertical {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    schema_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value = Some(1.0);
    assert!(matches!(
        section_dimension_constraints(
            &schema_orientation,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut separate_point_distance = distance_definition.clone();
    separate_point_distance
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [Some(1), Some(5), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert!(matches!(
        section_dimension_constraints(
            &separate_point_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut coincident_point_keys = separate_point_distance.clone();
    coincident_point_keys
        .variables
        .as_mut()
        .expect("variables")
        .points
        .iter_mut()
        .find(|point| point.point_id == 5)
        .expect("point 5")
        .u = Some(0.0);
    assert!(matches!(
        section_dimension_constraints(
            &coincident_point_keys,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    separate_point_distance
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .retain(|segment| !segment.point_ids.contains(&5));
    synchronize_segment_count(&mut separate_point_distance);
    assert!(matches!(
        section_dimension_constraints(
            &separate_point_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut incidence_oriented_distance = distance_definition.clone();
    incidence_oriented_distance
        .segments
        .as_mut()
        .expect("segments")
        .rows[0]
        .vertical_horizontal = None;
    incidence_oriented_distance
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .push(crate::feature::FeatureSkamp {
            id: 18,
            kind: 1,
            flags: 0,
            status: 1,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            }],
            offset: 83,
        });
    incidence_oriented_distance
        .relations
        .as_mut()
        .expect("relations")
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        section_dimension_constraints(
            &incidence_oriented_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut incomplete_skamps = incidence_oriented_distance.clone();
    incomplete_skamps
        .relations
        .as_mut()
        .expect("relations")
        .skamp_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 2,
        entity_ref: 1,
        offset: 82,
    });
    assert!(matches!(
        section_dimension_constraints(
            &incomplete_skamps,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut conflicting_orientation = incidence_oriented_distance.clone();
    let mut vertical = conflicting_orientation
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .clone();
    vertical.id = 19;
    vertical.kind = 2;
    vertical.offset = 84;
    conflicting_orientation
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .push(vertical);
    conflicting_orientation
        .relations
        .as_mut()
        .expect("relations")
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert!(matches!(
        section_dimension_constraints(
            &conflicting_orientation,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut solver_definition = distance_definition.clone();
    solver_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .clear();
    assert_eq!(
        resolved_section_points(&solver_definition).get(&2),
        Some(&[0.0, 5.0])
    );
    let mut incomplete_relations = solver_definition.clone();
    incomplete_relations
        .relations
        .as_mut()
        .expect("relations")
        .declared_count = 4;
    assert!(!resolved_section_points(&incomplete_relations).contains_key(&2));
    let mut equivalent_relation = solver_definition.clone();
    let duplicate = equivalent_relation
        .relations
        .as_ref()
        .expect("relations")
        .rows[0]
        .clone();
    equivalent_relation
        .relations
        .as_mut()
        .expect("relations")
        .rows
        .push(duplicate);
    equivalent_relation
        .relations
        .as_mut()
        .expect("relations")
        .declared_count = 4;
    assert_eq!(
        resolved_section_points(&equivalent_relation).get(&2),
        Some(&[0.0, 5.0])
    );
    let conflicting_relation = equivalent_relation
        .relations
        .as_mut()
        .expect("relations")
        .rows
        .last_mut()
        .expect("duplicate relation");
    conflicting_relation.sign = 0xf6;
    assert!(!resolved_section_points(&equivalent_relation).contains_key(&2));
    let mut duplicate_identity = solver_definition.clone();
    let mut duplicate = duplicate_identity.segments.as_ref().expect("segments").rows[0].clone();
    duplicate.offset = 500;
    duplicate_identity
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(duplicate);
    synchronize_segment_count(&mut duplicate_identity);
    assert!(!resolved_section_points(&duplicate_identity).contains_key(&2));
    let mut duplicate_endpoint_segment = solver_definition;
    let mut duplicate = duplicate_endpoint_segment
        .segments
        .as_ref()
        .expect("segments")
        .rows[0]
        .clone();
    duplicate.external_id = 99;
    duplicate.offset = 501;
    duplicate_endpoint_segment
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(duplicate);
    synchronize_segment_count(&mut duplicate_endpoint_segment);
    assert!(!resolved_section_points(&duplicate_endpoint_segment).contains_key(&2));
    let mut shared_vertex_definition = distance_definition.clone();
    let mut incident = shared_vertex_definition
        .segments
        .as_ref()
        .expect("segments")
        .rows[1]
        .clone();
    incident.external_id = 2;
    incident.point_ids = [9, 1];
    shared_vertex_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(incident);
    synchronize_segment_count(&mut shared_vertex_definition);
    assert_eq!(
        section_dimension_constraints(
            &shared_vertex_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut duplicate_relation_id = distance_definition.clone();
    let mut duplicate = duplicate_relation_id
        .relations
        .as_ref()
        .expect("relations")
        .rows[0]
        .clone();
    duplicate.offset = 500;
    duplicate_relation_id
        .relations
        .as_mut()
        .expect("relations")
        .rows
        .push(duplicate);
    let duplicate_constraints = section_dimension_constraints(
        &duplicate_relation_id,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(duplicate_constraints.iter().all(|(constraint, _)| matches!(
        constraint.definition,
        SketchConstraintDefinition::Native { .. }
    )));
    assert_eq!(
        duplicate_constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#917:relation:offset:80"
    );
    assert_eq!(
        duplicate_constraints[1].0.id.0,
        "creo:featdefs:sketch_constraint#917:relation:offset:500"
    );
    for (constraint, _) in &duplicate_constraints {
        let SketchConstraintDefinition::Native {
            native_properties,
            operands,
            ..
        } = &constraint.definition
        else {
            panic!("duplicate relation must remain native");
        };
        assert_eq!(
            native_properties.get("relation_id").map(String::as_str),
            Some("8")
        );
        assert!(operands
            .iter()
            .all(|operand| operand.native_field.is_some()));
    }
    let mut duplicate_measured_segment = distance_definition.clone();
    let duplicate = duplicate_measured_segment
        .segments
        .as_ref()
        .expect("segments")
        .rows[0]
        .clone();
    duplicate_measured_segment
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(duplicate);
    synchronize_segment_count(&mut duplicate_measured_segment);
    assert!(matches!(
        section_dimension_constraints(
            &duplicate_measured_segment,
            &SketchId("creo:model:sketch#917".into())
        )[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:0"
    ));
    duplicate_measured_segment
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .last_mut()
        .expect("duplicate")
        .point_ids = [8, 9];
    assert!(matches!(
        section_dimension_constraints(
            &duplicate_measured_segment,
            &SketchId("creo:model:sketch#917".into())
        )[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:0"
    ));
    let mut angular_distance = distance_definition.clone();
    angular_distance
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value_unit = crate::feature::DimensionUnit::Radians;
    assert!(matches!(
        section_dimension_constraints(&angular_distance, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:0"
    ));
    assert!(!resolved_section_points(&angular_distance).contains_key(&2));
    let mut duplicate_dimension = distance_definition.clone();
    let duplicate = duplicate_dimension
        .dimensions
        .as_ref()
        .expect("dimensions")
        .rows[0]
        .clone();
    duplicate_dimension
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows
        .push(duplicate);
    let duplicate_constraint = section_dimension_constraints(
        &duplicate_dimension,
        &SketchId("creo:model:sketch#917".into()),
    );
    assert!(matches!(
        &duplicate_constraint[0].0.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            parameter: None,
            operands,
            ..
        } if native_kind == "creo:relation:0"
            && operands.first().is_some_and(|operand| operand.object_index == 8)
            && operands.iter().any(|operand| operand.native_field.as_deref() == Some("c[3]"))
    ));
    let mut legacy_radius_definition = definition.clone();
    let legacy_arc = &mut legacy_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows[1];
    legacy_arc.radius_ref = Some(0);
    let [first_point, second_point] = legacy_arc.point_ids;
    let center = legacy_arc.center_id.expect("arc center");
    let legacy_radius_relation = &mut legacy_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    legacy_radius_relation.relation_type = 5;
    legacy_radius_relation.sign = 1;
    legacy_radius_relation.dimension_id = 0;
    legacy_radius_relation.operand_vectors = Some([
        [Some(first_point), Some(0), Some(second_point), Some(0)],
        [Some(center), Some(10), Some(0), Some(1)],
        [Some(16), Some(15), Some(0), Some(0)],
    ]);
    legacy_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 3;
    assert_eq!(
        section_dimension_constraints(
            &legacy_radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    assert_eq!(
        resolved_section_radii(&legacy_radius_definition),
        BTreeMap::from([(0, 3.0)])
    );
    legacy_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        section_dimension_constraints(
            &legacy_radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    assert_eq!(
        resolved_section_radii(&legacy_radius_definition),
        BTreeMap::from([(0, 1.5)])
    );
    for dimension_type in [1, 2, 5] {
        legacy_radius_definition
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0]
            .dimension_type = dimension_type;
        assert_eq!(
            section_dimension_constraints(
                &legacy_radius_definition,
                &SketchId("creo:model:sketch#917".into())
            )[0]
            .0
            .definition,
            SketchConstraintDefinition::Radius {
                entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
            }
        );
        assert_eq!(
            resolved_section_radii(&legacy_radius_definition),
            BTreeMap::from([(0, 3.0)])
        );
    }
    legacy_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [Some(first_point), Some(0), Some(second_point), Some(0)],
        [Some(center), Some(10), Some(0), Some(1)],
        [Some(16), Some(15), Some(0), Some(1)],
    ]);
    assert!(matches!(
        section_dimension_constraints(
            &legacy_radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:5"
    ));

    let mut type6_radius_definition = definition.clone();
    let type6_arc = &mut type6_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows[1];
    type6_arc.radius_ref = Some(0);
    let [type6_first_point, type6_second_point] = type6_arc.point_ids;
    let type6_center = type6_arc.center_id.expect("arc center");
    let type6_relation = &mut type6_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    type6_relation.relation_type = 6;
    type6_relation.sign = 1;
    type6_relation.dimension_id = 0;
    type6_relation.operand_vectors = Some([
        [
            Some(type6_first_point),
            Some(type6_second_point),
            Some(0),
            Some(1),
        ],
        [Some(type6_center), Some(0), Some(0), Some(0)],
        [Some(16), Some(15), Some(0), Some(0)],
    ]);
    type6_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 3;
    assert_eq!(
        section_dimension_constraints(
            &type6_radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    assert_eq!(
        resolved_section_radii(&type6_radius_definition),
        BTreeMap::from([(0, 3.0)])
    );
    type6_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
            [
                Some(type6_first_point),
                Some(type6_second_point),
                Some(0),
                Some(1),
            ],
            [Some(type6_center), Some(0), Some(0), Some(0)],
            [Some(15), Some(16), Some(15), Some(1)],
        ]);
    assert_eq!(
        section_dimension_constraints(
            &type6_radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    type6_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        section_dimension_constraints(
            &type6_radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    assert_eq!(
        resolved_section_radii(&type6_radius_definition),
        BTreeMap::from([(0, 1.5)])
    );
    let mut incomplete_type6 = type6_radius_definition.clone();
    incomplete_type6
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
            [
                Some(type6_first_point),
                Some(type6_second_point),
                Some(0),
                Some(1),
            ],
            [Some(type6_center), Some(0), Some(0), Some(0)],
            [Some(15), Some(16), Some(15), None],
        ]);
    assert!(matches!(
        section_dimension_constraints(
            &incomplete_type6,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:6"
    ));
    let mut ambiguous_type6 = type6_radius_definition.clone();
    let mut duplicate_type6_arc = ambiguous_type6
        .segments
        .as_ref()
        .expect("segments")
        .rows[1]
        .clone();
    duplicate_type6_arc.offset = 500;
    ambiguous_type6
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(duplicate_type6_arc);
    assert!(matches!(
        section_dimension_constraints(
            &ambiguous_type6,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:6"
    ));

    let mut radius_definition = definition.clone();
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 3;
    radius_definition.segments.as_mut().expect("segments").rows[1].radius_ref = Some(101);
    let radius_relation = &mut radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    radius_relation.relation_type = 14;
    radius_relation.sign = 1;
    radius_relation.dimension_id = 0;
    radius_relation.operand_vectors = Some([
        [Some(101), Some(0), Some(0), Some(0)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(0), Some(0), Some(0)],
    ]);
    assert_eq!(
        section_dimension_constraints(
            &radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut noncircular_dimension = radius_definition.clone();
    for dimension_type in [1, 2, 5] {
        noncircular_dimension
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0]
            .dimension_type = dimension_type;
        assert_eq!(
            section_dimension_constraints(
                &noncircular_dimension,
                &SketchId("creo:model:sketch#917".into())
            )[0]
            .0
            .definition,
            SketchConstraintDefinition::Radius {
                entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
            }
        );
        assert_eq!(
            resolved_section_radii(&noncircular_dimension),
            BTreeMap::from([(101, 3.0)])
        );
    }
    let mut opaque_circle_definition = radius_definition.clone();
    let segments = opaque_circle_definition
        .segments
        .as_mut()
        .expect("segments");
    let arc = segments.rows.remove(1);
    segments
        .circle_rows
        .push(crate::feature::FeatureCircleSegment {
            center_id: arc.center_id.expect("arc center"),
            radius_ref: arc.radius_ref.expect("arc radius"),
            external_id: arc.external_id,
            offset: arc.offset,
        });
    assert_eq!(
        section_dimension_constraints(
            &opaque_circle_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut incomplete_radius_segments = radius_definition.clone();
    incomplete_radius_segments
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    assert_eq!(
        section_dimension_constraints(
            &incomplete_radius_segments,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut relation_orientation = definition.clone();
    relation_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value = Some(0.0);
    relation_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value_unit = crate::feature::DimensionUnit::SchemaDefined;
    let relation = relation_orientation
        .relations
        .as_mut()
        .expect("relations");
    relation.rows[0].relation_type = 0;
    relation.rows[0].operand_vectors = Some([
        [Some(1), Some(2), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    relation_orientation
        .segments
        .as_mut()
        .expect("segments")
        .rows[0]
        .vertical_horizontal = Some(1);
    relation.triples = vec![crate::feature::FeatureRelationTriple {
        relation_id: Some(8),
        equation_id: None,
        skamp_id: Some(3),
        offset: 94,
    }];
    relation.triples_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 2,
        offset: 94,
    });
    assert_eq!(
        section_dimension_constraints(
            &relation_orientation,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Horizontal {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    let mut incomplete_relation_orientation = relation_orientation.clone();
    incomplete_relation_orientation
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    assert_eq!(
        section_dimension_constraints(
            &incomplete_relation_orientation,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Horizontal {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
        }
    );
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        section_dimension_constraints(
            &radius_definition,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 2;
    let duplicate = radius_definition.segments.as_ref().expect("segments").rows[1].clone();
    radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(duplicate);
    synchronize_segment_count(&mut radius_definition);
    assert!(matches!(
        section_dimension_constraints(&radius_definition, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:14"
    ));
    radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .pop();
    synchronize_segment_count(&mut radius_definition);
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value_unit = crate::feature::DimensionUnit::Radians;
    assert!(matches!(
        section_dimension_constraints(&radius_definition, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ..
        } if native_kind == "creo:relation:14"
    ));
    assert!(!resolved_section_radii(&radius_definition).contains_key(&101));
    let mut incidence_distance = distance_definition.clone();
    let incidence_relations = incidence_distance.relations.as_mut().expect("relations");
    incidence_relations.rows[0].operand_vectors = None;
    incidence_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 81,
        kind: 0,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
        ],
        offset: 81,
    }];
    incidence_relations.triples = vec![crate::feature::FeatureRelationTriple {
        relation_id: Some(8),
        equation_id: None,
        skamp_id: Some(81),
        offset: 82,
    }];
    incidence_relations.skamp_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 1,
        offset: 80,
    });
    incidence_relations.triples_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 2,
        offset: 82,
    });
    assert_eq!(
        section_dimension_constraints(
            &incidence_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string(),
            )),
            second: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string(),
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    assert_eq!(
        section_dimension_constraints(
            &incidence_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .active,
        Some(true)
    );
    let mut inactive_incidence = incidence_distance.clone();
    inactive_incidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 2;
    assert!(joined_relation_incidence(&inactive_incidence, 8).is_some());
    assert!(relation_incidence(&inactive_incidence, 8).is_none());
    assert_eq!(
        section_dimension_constraints(
            &inactive_incidence,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .active,
        Some(false)
    );
    assert!(matches!(
        section_dimension_constraints(
            &inactive_incidence,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::DistanceLoci { .. }
    ));
    inactive_incidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items
        .push(crate::feature::FeatureSkampItem {
            entity_id: 15,
            sense: 2,
        });
    assert_eq!(
        section_dimension_constraints(
            &inactive_incidence,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Distance {
            entities: [12, 13, 15]
                .map(|entity_id| {
                    SketchEntityId(format!("creo:featdefs:sketch_entity#917:{entity_id}"))
                })
                .to_vec(),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut incomplete_triples = incidence_distance.clone();
    incomplete_triples
        .relations
        .as_mut()
        .expect("relations")
        .triples_header = Some(crate::feature::FeatureSolverTableHeader {
        declared_count: 2,
        entity_ref: 2,
        offset: 82,
    });
    assert!(relation_incidence(&incomplete_triples, 8).is_none());
    let mut duplicate_join = incidence_distance.clone();
    let duplicate_relations = duplicate_join.relations.as_mut().expect("relations");
    duplicate_relations
        .triples
        .push(crate::feature::FeatureRelationTriple {
            offset: 83,
            ..duplicate_relations.triples[0].clone()
        });
    duplicate_relations
        .triples_header
        .as_mut()
        .expect("triples header")
        .declared_count = 2;
    assert!(relation_incidence(&duplicate_join, 8).is_none());
    let mut null_join = incidence_distance.clone();
    let null_relations = null_join.relations.as_mut().expect("relations");
    null_relations
        .triples
        .push(crate::feature::FeatureRelationTriple {
            relation_id: Some(8),
            equation_id: None,
            skamp_id: None,
            offset: 83,
        });
    null_relations
        .triples_header
        .as_mut()
        .expect("triples header")
        .declared_count = 2;
    assert_eq!(
        relation_incidence(&null_join, 8).map(|row| row.id),
        Some(81)
    );
    incidence_distance
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[0]
        .sense = 2;
    assert_eq!(
        section_dimension_constraints(
            &incidence_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string(),
            )),
            second: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:13".to_string(),
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut solver_only_incidence = incidence_distance.clone();
    solver_only_incidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1]
        .entity_id = 999;
    assert_eq!(
        section_dimension_constraints(
            &solver_only_incidence,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string(),
            )),
            second: SketchLocus::Entity(SketchEntityId(
                "creo:featdefs:sketch_entity#917:999".to_string(),
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut angular_dimension = definition.clone();
    let second_angular_line = &mut angular_dimension.segments.as_mut().expect("segments").rows[1];
    second_angular_line.kind = crate::feature::FeatureSegmentKind::Line;
    second_angular_line.center_id = None;
    second_angular_line.radius_ref = None;
    let angle_dimension = &mut angular_dimension
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0];
    angle_dimension.dimension_type = 10;
    angle_dimension.value_unit = crate::feature::DimensionUnit::Radians;
    let angle_relation = &mut angular_dimension
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    angle_relation.relation_type = 1;
    angle_relation.operand_vectors = Some([
        [Some(4), Some(5), None, Some(1)],
        [Some(1), None, Some(1), Some(1)],
        [Some(15), Some(16), Some(15), Some(24)],
    ]);
    angular_dimension.order_table = Some(crate::feature::FeatureOrderTable {
        declared_count: 2,
        has_prototype: false,
        entity_ref: None,
        rows: vec![
            crate::feature::FeatureOrderRow {
                external_id: 12,
                internal_id: 4,
                bitmask: 1,
                offset: 90,
            },
            crate::feature::FeatureOrderRow {
                external_id: 13,
                internal_id: 5,
                bitmask: 1,
                offset: 91,
            },
        ],
        offset: 89,
    });
    assert_eq!(
        section_dimension_constraints(
            &angular_dimension,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Angle {
            first: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            second: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    let mut incomplete_angle_order = angular_dimension.clone();
    incomplete_angle_order
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count = 3;
    assert!(matches!(
        section_dimension_constraints(
            &incomplete_angle_order,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let mut ambiguous_angle = angular_dimension.clone();
    ambiguous_angle
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::FeatureOrderRow {
            external_id: 13,
            internal_id: 5,
            bitmask: 1,
            offset: 92,
        });
    ambiguous_angle
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count = 3;
    assert!(matches!(
        section_dimension_constraints(&ambiguous_angle, &SketchId("creo:model:sketch#917".into()))
            [0]
        .0
        .definition,
        SketchConstraintDefinition::Native { .. }
    ));
    let relations =
        section_dimension_constraints(&definition, &SketchId("creo:model:sketch#917".into()));
    assert_eq!(
        relations[0].0.definition,
        SketchConstraintDefinition::Native {
            native_kind: "creo:relation:99".to_string(),
            native_state: Some(1),
            native_flags: None,
            native_properties: std::collections::BTreeMap::from([
                ("dimension_id".to_string(), "0".to_string()),
                ("sign".to_string(), "1".to_string()),
            ]),
            entities: Vec::new(),
            parameter: Some(ParameterId("creo:featdefs:parameter#917:42".to_string(),)),
            operands: vec![SketchNativeOperand {
                native_kind: "relat_ptr".to_string(),
                native_field: None,
                native_role: None,
                object_index: 8,
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }],
        }
    );
    let mut vector_native = definition.clone();
    vector_native.relations.as_mut().expect("relations").rows[0].operand_vectors = Some([
        [Some(12), None, Some(0), Some(1)],
        [None; 4],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    let vector_relation =
        section_dimension_constraints(&vector_native, &SketchId("creo:model:sketch#917".into()));
    let SketchConstraintDefinition::Native {
        native_properties,
        operands,
        ..
    } = &vector_relation[0].0.definition
    else {
        panic!("native relation");
    };
    assert_eq!(
        native_properties,
        &std::collections::BTreeMap::from([
            ("dimension_id".to_string(), "0".to_string()),
            ("sign".to_string(), "1".to_string()),
        ])
    );
    assert_eq!(
        operands
            .iter()
            .map(|operand| (operand.native_field.as_deref(), operand.object_index))
            .collect::<Vec<_>>(),
        vec![
            (None, 8),
            (Some("a[0]"), 12),
            (Some("a[2]"), 0),
            (Some("a[3]"), 1),
            (Some("c[0]"), 15),
            (Some("c[1]"), 16),
            (Some("c[2]"), 15),
            (Some("c[3]"), 1),
        ]
    );
    let mut special_endpoint_dimension = definition.clone();
    let special_segments = special_endpoint_dimension
        .segments
        .as_mut()
        .expect("segments");
    special_segments.rows.clear();
    special_segments.circle_rows.clear();
    special_segments.point_rows.clear();
    special_segments.centered_line_rows.clear();
    special_segments.reference_line_rows = vec![crate::feature::FeatureReferenceLineSegment {
        directions: [None; 3],
        point_ids: [Some(1), Some(4)],
        vertical_horizontal: None,
        external_id: 101,
        offset: 91,
    }];
    special_segments.bounded_curve_rows = vec![crate::feature::FeatureBoundedCurveSegment {
        directions: [None; 3],
        point_ids: [5, 6],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 102,
        offset: 92,
    }];
    special_segments.conic_rows.clear();
    special_segments.opaque_rows.clear();
    special_segments.declared_count = 2;
    let special_relation = &mut special_endpoint_dimension
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    special_relation.relation_type = 0;
    special_relation.operand_vectors = Some([
        [Some(1), Some(5), None, Some(1)],
        [Some(1), Some(1), Some(0), Some(1)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert_eq!(
        section_dimension_constraints(
            &special_endpoint_dimension,
            &SketchId("creo:model:sketch#917".into()),
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:101".to_string(),
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:102".to_string(),
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
    vector_native.relations.as_mut().expect("relations").rows[0].used = 34;
    let stored_state =
        section_dimension_constraints(&vector_native, &SketchId("creo:model:sketch#917".into()));
    assert!(matches!(
        stored_state[0].0.definition,
        SketchConstraintDefinition::Native {
            native_state: Some(34),
            native_flags: None,
            ..
        }
    ));
    let mut coincident_definition = definition.clone();
    coincident_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .push(crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 8],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 17,
            body: Vec::new(),
            offset: 87,
        });
    synchronize_segment_count(&mut coincident_definition);
    coincident_definition.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![
            crate::feature::FeatureSectionPoint {
                point_id: 2,
                u: Some(3.0),
                v: Some(4.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 5,
                u: None,
                v: None,
            },
            crate::feature::FeatureSectionPoint {
                point_id: 4,
                u: None,
                v: Some(9.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 6,
                u: None,
                v: None,
            },
            crate::feature::FeatureSectionPoint {
                point_id: 7,
                u: Some(2.0),
                v: Some(6.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 8,
                u: None,
                v: None,
            },
        ],
        offset: 0,
    });
    coincident_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![
        crate::feature::FeatureSkamp {
            id: 17,
            kind: 0,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 3,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 2,
                },
            ],
            offset: 83,
        },
        crate::feature::FeatureSkamp {
            id: 18,
            kind: 3,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 3,
                },
            ],
            offset: 84,
        },
        crate::feature::FeatureSkamp {
            id: 19,
            kind: 2,
            flags: 0,
            status: 1,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 15,
                sense: 0,
            }],
            offset: 85,
        },
        crate::feature::FeatureSkamp {
            id: 20,
            kind: 9,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
            ],
            offset: 86,
        },
        crate::feature::FeatureSkamp {
            id: 21,
            kind: 1,
            flags: 0,
            status: 1,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            }],
            offset: 88,
        },
        crate::feature::FeatureSkamp {
            id: 22,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 17,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 17,
                    sense: 3,
                },
            ],
            offset: 89,
        },
        crate::feature::FeatureSkamp {
            id: 23,
            kind: 5,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 17,
                    sense: 0,
                },
            ],
            offset: 90,
        },
    ];
    synchronize_skamp_count(&mut coincident_definition);
    let related_line = unique_section_skamp_segment(&coincident_definition, 17).expect("line");
    assert_eq!(
        section_line_fixed_coordinate(&coincident_definition, related_line),
        Some(0)
    );
    let coincident_points = resolved_section_points(&coincident_definition);
    assert_eq!(coincident_points.get(&5), Some(&[3.0, 4.0]));
    assert_eq!(coincident_points.get(&4), Some(&[3.0, 9.0]));
    assert_eq!(coincident_points.get(&6), Some(&[3.0, 9.0]));
    assert_eq!(coincident_points.get(&8), Some(&[2.0, 2.0]));
    let mut disabled_incidences = coincident_definition.clone();
    for skamp in &mut disabled_incidences
        .relations
        .as_mut()
        .expect("relations")
        .skamps
    {
        skamp.status = 34;
    }
    assert_eq!(
        resolved_section_points(&disabled_incidences),
        BTreeMap::from([(2, [3.0, 4.0]), (7, [2.0, 6.0])])
    );
    let mut ambiguous_definition = coincident_definition.clone();
    let duplicate = ambiguous_definition
        .variables
        .as_ref()
        .and_then(|table| table.points.iter().find(|point| point.point_id == 5))
        .cloned()
        .expect("point 5");
    ambiguous_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(duplicate);
    assert_eq!(
        resolved_section_points(&ambiguous_definition).get(&5),
        Some(&[3.0, 4.0])
    );
    let complementary = ambiguous_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .last_mut()
        .expect("duplicate point");
    complementary.u = Some(3.0);
    assert_eq!(
        resolved_section_points(&ambiguous_definition).get(&5),
        Some(&[3.0, 4.0])
    );
    let mut conflicting_definition = coincident_definition.clone();
    let mut conflicting = conflicting_definition
        .variables
        .as_ref()
        .and_then(|table| table.points.iter().find(|point| point.point_id == 2))
        .cloned()
        .expect("point 2");
    conflicting.u = Some(30.0);
    conflicting_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(conflicting);
    assert!(!resolved_section_points(&conflicting_definition).contains_key(&2));
    let conflicting_record = sketch_section_point_records(&conflicting_definition)
        .into_iter()
        .find(|point| point.point_id == 2)
        .expect("conflicting point record");
    assert_eq!(conflicting_record.state, "conflicting");
    assert_eq!([conflicting_record.u, conflicting_record.v], [None; 2]);
    let mut point_row_type_nine = definition.clone();
    point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .rows[0]
        .vertical_horizontal = Some(1);
    point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .point_rows
        .push(crate::feature::FeaturePointSegment {
            point_id: 7,
            external_id: 17,
            offset: 90,
        });
    point_row_type_nine
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 7,
            u: Some(4.0),
            v: None,
        });
    point_row_type_nine
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 90,
        kind: 9,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 17,
                sense: 0,
            },
        ],
        offset: 91,
    }];
    synchronize_segment_count(&mut point_row_type_nine);
    point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    synchronize_skamp_count(&mut point_row_type_nine);
    let point_row_type_nine_skamp = &point_row_type_nine
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0];
    assert_eq!(
        section_skamp_point_on_line(&point_row_type_nine, point_row_type_nine_skamp),
        Some((1, 7, 1))
    );
    assert_eq!(
        resolved_section_coordinates(&point_row_type_nine).get(&7),
        Some(&[Some(4.0), Some(2.0)])
    );
    let mut saved_definition = definition;
    saved_definition.order_table = Some(crate::feature::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::FeatureOrderRow {
            external_id: 14,
            internal_id: 20,
            bitmask: 1,
            offset: 81,
        }],
        offset: 80,
    });
    saved_definition.saved_section = Some(crate::feature::FeatureSavedSection {
        entities: vec![crate::feature::FeatureSavedEntity::Line(
            crate::feature::FeatureSavedLine {
                entity_id: 20,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(0.0), Some(0.0)],
                    [Some(1.0), Some(0.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 82,
            },
        )],
        offset: 82,
    });
    assert_eq!(
        section_skamp_endpoint(
            &saved_definition,
            &SketchId("creo:model:sketch#917".to_string()),
            &crate::feature::FeatureSkampItem {
                entity_id: 14,
                sense: 3,
            },
        ),
        Some(SketchLocus::End(SketchEntityId(
            "creo:featdefs:sketch_entity#917:14".to_string()
        )))
    );
    saved_definition
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::FeatureOrderRow {
            external_id: 99,
            internal_id: 21,
            bitmask: 1,
            offset: 83,
        });
    saved_definition
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count += 1;
    saved_definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(crate::feature::FeatureSavedEntity::Line(
            crate::feature::FeatureSavedLine {
                entity_id: 21,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(1.0), Some(0.0)],
                    [Some(1.0), Some(1.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 84,
            },
        ));
    saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 30,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        }],
        offset: 85,
    }];
    synchronize_skamp_count(&mut saved_definition);
    assert_eq!(
        section_skamp_constraints(&saved_definition, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Horizontal {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
        }
    );
    saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 31,
        kind: 7,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
        ],
        offset: 86,
    }];
    synchronize_skamp_count(&mut saved_definition);
    let segment = unique_section_skamp_segment(&saved_definition, 12).expect("segment line");
    assert_eq!(
        section_line_fixed_coordinate(&saved_definition, segment),
        Some(1)
    );
    saved_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 4,
            u: Some(3.0),
            v: None,
        });
    saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 32,
        kind: 9,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 14,
                sense: 0,
            },
        ],
        offset: 87,
    }];
    synchronize_skamp_count(&mut saved_definition);
    assert_eq!(
        resolved_section_points(&saved_definition).get(&4),
        Some(&[3.0, 1.0])
    );
    let mut saved_point_row_type_nine = saved_definition.clone();
    saved_point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .point_rows
        .push(crate::feature::FeaturePointSegment {
            point_id: 7,
            external_id: 17,
            offset: 94,
        });
    saved_point_row_type_nine
        .variables
        .as_mut()
        .expect("variables")
        .points
        .push(crate::feature::FeatureSectionPoint {
            point_id: 7,
            u: Some(3.0),
            v: None,
        });
    saved_point_row_type_nine
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .items[1]
        .entity_id = 17;
    synchronize_segment_count(&mut saved_point_row_type_nine);
    saved_point_row_type_nine
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    synchronize_skamp_count(&mut saved_point_row_type_nine);
    let saved_point_row_type_nine_skamp = &saved_point_row_type_nine
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0];
    assert_eq!(
        section_skamp_saved_point_on_line(
            &saved_point_row_type_nine,
            saved_point_row_type_nine_skamp,
        ),
        Some((7, 1, 1.0))
    );
    assert_eq!(
        resolved_section_coordinates(&saved_point_row_type_nine).get(&7),
        Some(&[Some(3.0), Some(1.0)])
    );
    let mut saved_coincident = saved_definition.clone();
    for point in &mut saved_coincident
        .variables
        .as_mut()
        .expect("variables")
        .points
    {
        if matches!(point.point_id, 4 | 5) {
            point.u = None;
            point.v = None;
        }
    }
    saved_coincident
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![
        crate::feature::FeatureSkamp {
            id: 33,
            kind: 0,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 2,
                },
            ],
            offset: 88,
        },
        crate::feature::FeatureSkamp {
            id: 34,
            kind: 3,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 3,
                },
            ],
            offset: 89,
        },
    ];
    synchronize_skamp_count(&mut saved_coincident);
    assert_eq!(
        resolved_section_points(&saved_coincident),
        BTreeMap::from([(1, [0.0, 2.0]), (4, [1.0, 1.0]), (5, [0.0, 1.0])])
    );
    saved_coincident.variables = None;
    assert_eq!(
        resolved_section_points(&saved_coincident),
        BTreeMap::from([(4, [1.0, 1.0]), (5, [0.0, 1.0])])
    );
    let mut saved_symmetric = saved_definition.clone();
    let point = saved_symmetric
        .variables
        .as_mut()
        .expect("variables")
        .points
        .iter_mut()
        .find(|point| point.point_id == 5)
        .expect("point 5");
    point.u = None;
    point.v = None;
    saved_symmetric
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 35,
        kind: 14,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 2,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 15,
                sense: 2,
            },
        ],
        offset: 90,
    }];
    synchronize_skamp_count(&mut saved_symmetric);
    assert_eq!(
        resolved_section_points(&saved_symmetric).get(&5),
        Some(&[0.0, 1.0])
    );
    saved_definition
        .variables
        .as_mut()
        .expect("variables")
        .points
        .iter_mut()
        .find(|point| point.point_id == 5)
        .expect("point 5")
        .v = None;
    saved_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 33,
        kind: 14,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 15,
                sense: 2,
            },
        ],
        offset: 88,
    }];
    synchronize_skamp_count(&mut saved_definition);
    assert_eq!(
        resolved_section_points(&saved_definition).get(&5),
        Some(&[3.0, 0.0])
    );
    let mut saved_same_coordinate = saved_definition.clone();
    saved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps = vec![crate::feature::FeatureSkamp {
        id: 36,
        kind: 17,
        flags: 1,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 2,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 12,
                sense: 2,
            },
        ],
        offset: 91,
    }];
    synchronize_skamp_count(&mut saved_same_coordinate);
    assert_eq!(
        section_skamp_constraints(
            &saved_same_coordinate,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::SameCoordinate {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:99".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            axis: SketchCoordinateAxis::U,
        }
    );
    let mut duplicate_saved = saved_definition
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[1]
        .clone();
    if let crate::feature::FeatureSavedEntity::Line(line) = &mut duplicate_saved {
        line.offset = 86;
    }
    saved_definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(duplicate_saved);
    assert!(matches!(
        section_skamp_constraints(&saved_definition, &SketchId("creo:model:sketch#917".into()))[0]
            .0
            .definition,
        SketchConstraintDefinition::Native { .. }
    ));
}
