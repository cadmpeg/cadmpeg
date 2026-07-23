use super::*;

fn synchronize_skamp_count(definition: &mut crate::feature::FeatureDefinition) {
    let relations = definition.relations.as_mut().expect("relations");
    relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = u32::try_from(relations.skamps.len()).expect("skamp count");
}

fn synchronize_segment_count(definition: &mut crate::feature::FeatureDefinition) {
    let segments = definition.segments.as_mut().expect("segments");
    segments.declared_count = u32::try_from(segments.rows.len()).expect("segment count");
}

#[test]
fn section_coordinate_system_solves_coupled_equations_and_withholds_derivations_on_conflict() {
    let mut sum = SectionCoordinateEquation::default();
    sum.add_point(1, 0, 1.0);
    sum.add_point(2, 0, 1.0);
    sum.rhs = 10.0;
    let mut difference = SectionCoordinateEquation::default();
    difference.add_point(1, 0, 1.0);
    difference.add_point(2, 0, -1.0);
    difference.rhs = 2.0;
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                sum,
                difference,
                SectionCoordinateEquation::point_value(1, 1, 3.0),
                SectionCoordinateEquation::point_value(2, 1, 4.0),
            ],
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, [Some(6.0), Some(3.0)]), (2, [Some(4.0), Some(4.0)]),])
    );

    let stored = BTreeMap::from([((1, 0), 1.0), ((1, 1), 3.0)]);
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                SectionCoordinateEquation::point_value(1, 0, 1.0),
                SectionCoordinateEquation::point_value(1, 0, 2.0),
                SectionCoordinateEquation::point_value(1, 1, 3.0),
            ],
            &stored,
        ),
        BTreeMap::from([(1, [Some(1.0), Some(3.0)])])
    );
    let stored = BTreeMap::from([((1, 0), 1.0), ((1, 1), 3.0), ((2, 0), 2.0), ((2, 1), 4.0)]);
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                SectionCoordinateEquation::point_value(1, 0, 1.0),
                SectionCoordinateEquation::point_value(1, 1, 3.0),
                SectionCoordinateEquation::point_value(2, 0, 2.0),
                SectionCoordinateEquation::point_value(2, 1, 4.0),
                SectionCoordinateEquation::point_difference(1, 3, 0, 0.0),
                SectionCoordinateEquation::point_difference(2, 3, 0, 0.0),
                SectionCoordinateEquation::point_value(3, 1, 5.0),
            ],
            &stored,
        ),
        BTreeMap::from([
            (1, [Some(1.0), Some(3.0)]),
            (2, [Some(2.0), Some(4.0)]),
            (3, [None, Some(5.0)]),
        ])
    );
    assert_eq!(
        solve_section_coordinate_equations(
            &[
                SectionCoordinateEquation::point_value(3, 0, 1.0e12),
                SectionCoordinateEquation::point_value(3, 1, -1.0e12),
            ],
            &BTreeMap::new(),
        ),
        BTreeMap::from([(3, [Some(1.0e12), Some(-1.0e12)])])
    );
    assert_eq!(
        solve_section_coordinate_equations(
            &[SectionCoordinateEquation::point_value(4, 0, 7.0)],
            &BTreeMap::new(),
        ),
        BTreeMap::from([(4, [Some(7.0), None])])
    );
}

#[test]
fn normalization_rejects_overflowed_finite_vectors() {
    assert_eq!(normalized([f64::MAX, f64::MAX, 0.0]), None);
    assert_eq!(normalized([3.0, 4.0, 0.0]), Some([0.6, 0.8, 0.0]));
}

#[test]
fn dependency_reconciliation_preserves_typed_history_edges() {
    let owner = IrFeatureId("creo:model:feature#40".to_string());
    let sketch = IrFeatureId("creo:model:sketch_feature#917".to_string());
    let parent = IrFeatureId("creo:model:feature#3".to_string());
    let missing = IrFeatureId("creo:model:feature#999".to_string());
    let emitted = [owner.clone(), sketch.clone(), parent.clone()]
        .into_iter()
        .collect();

    assert_eq!(
        reconciled_dependencies(
            &owner,
            &[sketch.clone(), missing],
            [parent.clone(), sketch.clone(), owner.clone()],
            &emitted,
        ),
        vec![sketch, parent]
    );
}

#[test]
fn class_100_entity_reference_depends_on_its_unique_generator() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        prefixed: true,
        offset: 0,
        end_offset: 0,
    };
    let table = |feature_id: u32,
                 table_class_id: u32,
                 entries: Vec<crate::feature::FeatureEntityTableEntry>| {
        crate::feature::FeatureEntityTable {
            feature_id: Some(feature_id),
            table_class_id,
            entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
            entries,
            surface_ids: Vec::new(),
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        }
    };
    let producer = table(175, 67, vec![entry(192, 200, Some(175))]);
    let consumer = table(416, 100, vec![entry(192, 98, None)]);

    assert_eq!(
        feature_entity_dependencies(&[producer.clone(), consumer.clone()], 416),
        [175]
    );
    let conflicting = table(312, 67, vec![entry(192, 200, Some(312))]);
    assert!(feature_entity_dependencies(&[producer, conflicting, consumer], 416).is_empty());
}

#[test]
fn closed_fallback_profile_selects_revolution_segments() {
    let segment = |external_id| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        offset: 0,
    };
    let segments = [segment(9), segment(10), segment(11)];
    let profiles = vec![vec![
        SketchEntityUse {
            entity: SketchEntityId("creo:featdefs:sketch_entity#2:9".to_string()),
            reversed: false,
        },
        SketchEntityUse {
            entity: SketchEntityId("creo:featdefs:sketch_entity#2:11".to_string()),
            reversed: true,
        },
    ]];

    assert_eq!(
        profile_segment_ids(2, &segments, &profiles),
        BTreeSet::from([9, 11])
    );
}

fn parameter_slot(value: f64) -> crate::surface::SurfaceParameterScalar {
    crate::surface::SurfaceParameterScalar {
        value: Some(value),
        raw: vec![],
        offset: 0,
        length: 1,
    }
}

#[test]
fn complementary_split_outlines_establish_a_cylinder_carrier() {
    let bounds = [
        [[-0.3125, 1.3125], [0.3125, 1.625]],
        [[-0.3125, 1.625], [0.3125, 1.9375]],
    ];
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, -1.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert_eq!(
        cylinder_from_complementary_outline_bounds(&plane, bounds),
        Some(SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 1.625, -1.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.3125,
        })
    );
}

#[test]
fn split_outline_carrier_requires_complementary_square_bounds() {
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert!(cylinder_from_complementary_outline_bounds(
        &plane,
        [[[-1.0, 0.0], [1.0, 0.5]], [[-1.0, 0.6], [1.0, 1.0]]],
    )
    .is_none());
    assert!(cylinder_from_complementary_outline_bounds(
        &plane,
        [[[-1.0, 0.0], [1.0, 0.5]], [[-1.0, 0.5], [1.0, 3.0]]],
    )
    .is_none());
}

#[test]
fn tabulated_cylinder_frame_places_a_unique_cubic_chart() {
    let mut replay = crate::surface::TabulatedCylinderCurveReplay {
        surface_id: 7,
        curve_id: 9,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: vec![],
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 5,
        control_point_bodies: std::array::from_fn(|_| vec![]),
        control_points: [
            Some([1.0, 2.0]),
            Some([2.0, 2.5]),
            Some([3.0, 3.5]),
            Some([4.0, 4.0]),
        ],
        terminal_reference: 6,
        offset: 0,
        surface_row_offset: 0,
    };
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 7,
        body: vec![],
        scalar_values: vec![],
        scalar_tokens: vec![],
        opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
            raw: vec![0x00, 0x0c, 0x9a],
            offset: 3,
            length: 3,
        }],
        scalar_frames: vec![
            crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: [0.0, 0.0, 1.0].into_iter().map(parameter_slot).collect(),
            },
            crate::surface::SurfaceParameterScalarFrame {
                offset: 6,
                slots: [13.0, 22.0, 5.0, 10.0, 20.0, 10.0]
                    .into_iter()
                    .map(parameter_slot)
                    .collect(),
            },
        ],
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };

    let (curve, sweep) =
        placed_tabulated_cylinder_directrix(&replay, &parameters).expect("placement");
    assert_eq!(curve.control_points[0], Point3::new(-13.0, -20.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(-10.0, -22.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    let mut broad_signed_frame = parameters;
    broad_signed_frame.scalar_frames.truncate(1);
    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
        prefixes: [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
    });
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame)
        .expect("broad signed-DICT placement");
    assert_eq!(curve.control_points[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.tabulated_cylinder_frame = Some(crate::surface::TabulatedCylinderFrame {
        values: [29.0, 5.0, 2.0, -26.0, 10.0, 4.0],
        prefixes: [0x4a, 0x46, 0x2f, 0x46, 0x46, 0x2e],
    });
    replay.control_points[1] = Some([10.0, -5.0]);
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &broad_signed_frame)
        .expect("independently signed offset placement");
    assert_eq!(curve.control_points[0], Point3::new(-29.0, 5.0, 2.0));
    assert_eq!(curve.control_points[1], Point3::new(-20.0, 5.0, -5.0));
    assert_eq!(curve.control_points[3], Point3::new(-26.0, 5.0, 4.0));
    assert_eq!(sweep, [0.0, 5.0, 0.0]);
}

#[test]
fn tabulated_cylinder_offset_chart_resolves_signed_unit_axes() {
    assert_eq!(
        signed_unit_chart(
            [33.480_874_469_5, 34.047_445_706_6],
            [3.480_874_469_5, 4.047_445_706_6],
            30.0,
        ),
        Some((1.0, -30.0))
    );
    assert_eq!(
        signed_unit_chart(
            [0.576_336_341_1, 0.746_308_064_9],
            [-0.746_308_064_9, -0.576_336_341_1],
            0.0,
        ),
        Some((-1.0, 0.0))
    );
    assert_eq!(
        signed_unit_chart(
            [21.592_186_587_7, 21.604_574_667_3],
            [8.407_813_412_3, -8.395_425_332_7],
            30.0,
        ),
        Some((1.0, -30.0))
    );
    assert_eq!(signed_unit_chart([1.0, 2.0], [4.0, 5.0], 30.0), None);
}

#[test]
fn zero_offset_2d_tabulated_frame_retains_the_stored_span() {
    let replay = crate::surface::TabulatedCylinderCurveReplay {
        surface_id: 815,
        curve_id: 1,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: Vec::new(),
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 0,
        control_point_bodies: std::array::from_fn(|_| Vec::new()),
        control_points: [
            Some([2.603_530_729_189_511_6, -6.634_758_301_120_719]),
            Some([2.486_761_892_214_414, -6.583_162_851_673_087]),
            Some([2.403_937_662_020_322, -6.519_347_555_976_829]),
            Some([2.355_057_866_495_792, -6.440_596_814_034_794]),
        ],
        terminal_reference: 0,
        offset: 0,
        surface_row_offset: 0,
    };
    let body = vec![
        0x18, 0xe4, 0x0f, 0x00, 0x0c, 0x9a, 0x8d, 0xd7, 0x28, 0x94, 0x26, 0x4b, 0xb2, 0x2d, 0x19,
        0xc3, 0x2b, 0xcf, 0xac, 0x01, 0x44, 0x9e, 0x1e, 0xb8, 0x51, 0xeb, 0x85, 0x1f, 0x8f, 0xd4,
        0x07, 0xeb, 0x3f, 0xff, 0xf8, 0x2d, 0x1a, 0x89, 0xfe, 0x14, 0x80, 0xb6, 0x48, 0x9e, 0x85,
        0x1e, 0xb8, 0x51, 0xeb, 0x85,
    ];
    let tabulated_cylinder_frame = crate::surface::decode_tabulated_cylinder_frame(
        &body,
        &crate::scalar::ScalarCache::default(),
    )
    .map(|(frame, _)| frame);
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 815,
        body,
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
            raw: vec![0, 0x0c, 0x9a],
            offset: 3,
            length: 3,
        }],
        scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
            offset: 0,
            slots: vec![
                parameter_slot(0.0),
                parameter_slot(1.0),
                parameter_slot(0.0),
            ],
        }],
        terminal_scalar_frame: None,
        tabulated_cylinder_frame,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &parameters)
        .expect("zero-offset directrix placement");
    assert_eq!(
        curve.control_points[0],
        Point3::new(-2.603_530_729_189_511_6, 6.634_758_301_120_719, 4.78)
    );
    assert_eq!(
        curve.control_points[3],
        Point3::new(-2.355_057_866_495_792, 6.440_596_814_034_794, 4.78)
    );
    assert_eq!(sweep, [0.0, 0.0, 0.099_999_999_999_999_64]);
}

#[test]
fn geometry_signal_excludes_opaque_carriers() {
    let mut ir = CadIr::empty(Units::default());
    let surface_id = SurfaceId("surface".to_string());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("curve".to_string()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });

    assert!(!has_transferred_geometry(&ir));

    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("procedural".to_string()),
        surface: surface_id,
        definition: ProceduralSurfaceDefinition::Exact {
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
            },
            extension: 0,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    assert!(has_transferred_geometry(&ir));
}

#[test]
fn fc05_row_frame_maps_cyclically_onto_each_model_axis() {
    let center = [11.0, 13.0];
    let reference = [0.6, 0.8];
    assert_eq!(
        fc05_model_frame(0, 17.0, center, reference, -1.0),
        ([17.0, 13.0, 11.0], [-1.0, 0.0, 0.0], [0.0, 0.8, 0.6])
    );
    assert_eq!(
        fc05_model_frame(1, 17.0, center, reference, -1.0),
        ([11.0, 17.0, 13.0], [0.0, -1.0, 0.0], [0.6, 0.0, 0.8])
    );
    assert_eq!(
        fc05_model_frame(2, 17.0, center, reference, -1.0),
        ([13.0, 11.0, 17.0], [0.0, 0.0, -1.0], [0.8, 0.6, 0.0])
    );
}

#[test]
fn full_turn_section_carriers_classify_analytic_revolution_surfaces() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 1,
        feature_id: Some(2),
        origin: [0.0, 0.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 0,
    };
    let axis = RevolutionAxis {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(0.0, 1.0, 0.0),
    };
    let line = |start: [f64; 2], end: [f64; 2]| SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    };

    assert!(matches!(
        revolved_section_circle(&transform, [2.0, 3.0], axis),
        Some(CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        }) if center == Point3::new(0.0, 3.0, 0.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && radius == 2.0
    ));
    assert!(revolved_section_circle(&transform, [0.0, 3.0], axis).is_none());
    assert!(matches!(
        extruded_section_line(&transform, [2.0, 3.0]),
        Some(CurveGeometry::Line { origin, direction })
            if origin == Point3::new(2.0, 3.0, 0.0)
                && direction == Vector3::new(0.0, 0.0, 1.0)
    ));

    assert!(matches!(
        revolved_section_surface(&transform, &line([2.0, 0.0], [2.0, 4.0]), axis),
        Some(SurfaceGeometry::Cylinder { radius, .. }) if radius == 2.0
    ));
    assert!(matches!(
        revolved_section_surface(&transform, &line([0.0, 3.0], [4.0, 3.0]), axis),
        Some(SurfaceGeometry::Plane { origin, .. }) if origin.y == 3.0
    ));
    assert!(matches!(
        revolved_section_surface(&transform, &line([2.0, 0.0], [4.0, 2.0]), axis),
        Some(SurfaceGeometry::Cone { radius, half_angle, .. })
            if radius == 2.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    assert!(matches!(
        revolved_section_surface(&transform, &line([4.0, 0.0], [2.0, 2.0]), axis),
        Some(SurfaceGeometry::Cone { axis, radius, half_angle, .. })
            if axis.y == -1.0
                && radius == 4.0
                && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    let centered_arc = SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(0.0, 3.0),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    assert!(matches!(
        revolved_section_surface(&transform, &centered_arc, axis),
        Some(SurfaceGeometry::Sphere { radius, .. }) if radius == 2.0
    ));
    let offset_arc = SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(5.0, 3.0),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    assert!(matches!(
        revolved_section_surface(&transform, &offset_arc, axis),
        Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
            if major_radius == 5.0 && minor_radius == 2.0
    ));
    let offset_circle = SketchGeometry::Circle {
        center: Point2::new(5.0, 3.0),
        radius: Length(2.0),
    };
    assert!(matches!(
        revolved_section_surface(&transform, &offset_circle, axis),
        Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
            if major_radius == 5.0 && minor_radius == 2.0
    ));
}

#[test]
fn spindle_torus_boundary_pcurve_retains_the_signed_ring_branch() {
    let surface = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 5.0,
    };
    let axis = RevolutionAxis {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
    };
    let pcurve =
        revolution_boundary_pcurve(&surface, [-3.0, 0.0, 0.0], axis).expect("spindle boundary");
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, parameter).expect("pcurve point");
        let point = cadmpeg_ir::eval::surface_point(&surface, uv.u, uv.v).expect("surface point");
        assert!((point.x.hypot(point.y) - 3.0).abs() < 1e-12);
        assert!(point.z.abs() < 1e-12);
    }
}

#[test]
fn generated_source_ids_bind_carriers_independently_of_table_position() {
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(17),
        table_class_id: 80,
        entry_ids: vec![42, 41, 43],
        entries: vec![
            crate::feature::FeatureEntityTableEntry {
                entity_id: 42,
                class_id: 200,
                source_entity_id: Some(10),
                prefixed: false,
                offset: 0,
                end_offset: 0,
            },
            crate::feature::FeatureEntityTableEntry {
                entity_id: 41,
                class_id: 200,
                source_entity_id: Some(8),
                prefixed: false,
                offset: 0,
                end_offset: 0,
            },
            crate::feature::FeatureEntityTableEntry {
                entity_id: 43,
                class_id: 200,
                source_entity_id: Some(9),
                prefixed: false,
                offset: 0,
                end_offset: 0,
            },
        ],
        surface_ids: vec![41, 42, 43],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let order = crate::feature::FeatureOrderTable {
        declared_count: 2,
        has_prototype: false,
        entity_ref: Some(3),
        rows: vec![
            crate::feature::FeatureOrderRow {
                external_id: 8,
                internal_id: 1,
                bitmask: 0,
                offset: 0,
            },
            crate::feature::FeatureOrderRow {
                external_id: 9,
                internal_id: 2,
                bitmask: 0,
                offset: 0,
            },
        ],
        offset: 0,
    };
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![
        row(41, crate::surface::SurfaceKind::Cylinder),
        row(42, crate::surface::SurfaceKind::Cone),
        row(43, crate::surface::SurfaceKind::TorusOrSphere),
    ];
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
        ratio: 1.0,
        half_angle: 0.5,
    };
    assert_eq!(
        analytic_surface_id_for_feature(&rows, std::slice::from_ref(&table), 17, 10, &cone,),
        Some(42)
    );
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            10,
            &cone,
        ),
        None
    );
    assert_eq!(
        analytic_surface_id_for_feature(&rows, std::slice::from_ref(&table), 17, 10, &cylinder,),
        None
    );
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            8,
            &cylinder,
        ),
        Some(41)
    );
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            9,
            &cylinder,
        ),
        None
    );
    let mut first_table = table.clone();
    first_table.entry_ids = vec![41];
    first_table.entries = vec![table.entries[1].clone()];
    first_table.surface_ids = vec![41];
    let mut second_table = table.clone();
    second_table.entry_ids = vec![43];
    second_table.entries = vec![table.entries[2].clone()];
    second_table.surface_ids = vec![43];
    assert_eq!(
        generated_surface_id_for_feature(&[first_table.clone(), second_table], 17, 9),
        Some(43)
    );
    first_table.entries[0].source_entity_id = Some(9);
    assert_eq!(
        generated_surface_id_for_feature(&[first_table, table.clone()], 17, 9),
        None
    );
    let torus = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.0,
        minor_radius: 1.0,
    };
    assert_eq!(
        ordered_analytic_surface_id_for_feature(
            &rows,
            std::slice::from_ref(&table),
            17,
            &order,
            9,
            &torus,
        ),
        Some(43)
    );
    assert_eq!(
        ordered_family_surface_bindings_for_feature(
            &rows,
            17,
            std::slice::from_ref(&table),
            &order,
            [9],
            crate::surface::SurfaceKind::TorusOrSphere,
        ),
        BTreeMap::from([(9, 43)])
    );
    assert_eq!(
        section_generated_profile_surface_kinds(&SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(3.0),
        }),
        Some(&[crate::surface::SurfaceKind::Cylinder][..])
    );
    assert!(section_entity_is_generated_profile(
        true,
        Some(17),
        8,
        &[crate::surface::SurfaceKind::Cylinder],
        std::slice::from_ref(&table),
        &rows,
    ));
    let mut extrusion_rows = rows.clone();
    extrusion_rows[2] = row(43, crate::surface::SurfaceKind::Extrusion);
    assert!(section_entity_is_generated_profile(
        true,
        Some(17),
        9,
        &[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ],
        std::slice::from_ref(&table),
        &extrusion_rows,
    ));
    assert!(!section_entity_is_generated_profile(
        true,
        Some(17),
        9,
        &[crate::surface::SurfaceKind::Spline],
        std::slice::from_ref(&table),
        &extrusion_rows,
    ));
    assert!(!section_entity_is_generated_profile(
        false,
        Some(17),
        9,
        &[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ],
        std::slice::from_ref(&table),
        &extrusion_rows,
    ));
    assert!(!section_entity_is_generated_profile(
        true,
        Some(17),
        10,
        &[crate::surface::SurfaceKind::Cylinder],
        &[table],
        &rows,
    ));
}

#[test]
fn paired_cylinder_sources_and_planar_support_identify_counterbore_form() {
    let entry = |entity_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id: 200,
        source_entity_id: Some(source_entity_id),
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let entries = vec![
        entry(11, 4),
        entry(12, 4),
        entry(13, 6),
        entry(14, 6),
        entry(15, 7),
        entry(16, 7),
    ];
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(9),
        table_class_id: 29,
        entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
        entries,
        surface_ids: vec![11, 12, 13, 15, 16],
        non_surface_entity_ids: vec![14],
        offset: 0,
    };
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 9,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut rows = vec![
        row(11, crate::surface::SurfaceKind::Cylinder),
        row(12, crate::surface::SurfaceKind::Cylinder),
        row(13, crate::surface::SurfaceKind::Plane),
        row(15, crate::surface::SurfaceKind::Cylinder),
        row(16, crate::surface::SurfaceKind::Cylinder),
    ];

    assert_eq!(
        stepped_hole_form(9, std::slice::from_ref(&table), &rows),
        Some(HoleForm::Counterbore)
    );

    rows[4].kind = crate::surface::SurfaceKind::Cone;
    assert_eq!(
        stepped_hole_form(9, std::slice::from_ref(&table), &rows),
        None
    );
}

#[test]
fn counterbore_dimensions_require_complete_agreeing_radius_anchored_tables() {
    let table = |depth: f64| crate::feature::FeatureDimensionTable {
        declared_count: 4,
        entity_ref: Some(88),
        rows: [
            (2, 0.098, 0),
            (2, 0.463_628_944_932_919_5, 1),
            (1, depth, 2),
            (2, 0.3125, 3),
        ]
        .into_iter()
        .map(
            |(dimension_type, value, external_id)| crate::feature::FeatureDimension {
                dimension_type,
                value: Some(value),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: Some(0.0),
                external_id,
                offset: 0,
            },
        )
        .collect(),
        offset: 0,
    };
    let first = table(0.15);
    let second = table(0.15);

    assert_eq!(
        counterbore_dimension_values([&first, &second].into_iter(), &[0.3125]),
        Some((0.196, 0.625, 0.15))
    );
    assert_eq!(
        counterbore_dimension_values([&first].into_iter(), &[0.25]),
        None
    );
    let conflicting = table(0.2);
    assert_eq!(
        counterbore_dimension_values([&first, &conflicting].into_iter(), &[0.3125]),
        None
    );
}

#[test]
fn counterbore_bore_patches_inherit_the_unique_larger_cylinder_frame() {
    let carrier = SurfaceGeometry::Cylinder {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 0.3125,
    };
    let mut existing = BTreeMap::from([(30, carrier.clone()), (31, carrier.clone())]);
    let sources = vec![vec![10, 11], vec![30, 31]];

    let patches = counterbore_source_patch_geometries(&sources, &existing, 0.196, 0.625)
        .expect("coaxial patches");

    assert_eq!(patches.len(), 4);
    assert!(patches
        .iter()
        .filter(|(id, _)| *id < 30)
        .all(|(_, geometry)| {
            matches!(geometry, SurfaceGeometry::Cylinder { origin, axis, radius, .. }
                if *origin == Point3::new(1.0, 2.0, 3.0)
                    && *axis == Vector3::new(0.0, 0.0, 1.0)
                    && (*radius - 0.098).abs() < 1e-12)
        }));
    existing.insert(10, carrier);
    assert_eq!(
        counterbore_source_patch_geometries(&sources, &existing, 0.196, 0.625),
        None
    );
}

#[test]
fn surface_coverage_separates_transferred_unique_rows_from_ambiguous_ids() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let rows = vec![
        row(41, crate::surface::SurfaceKind::Plane),
        row(42, crate::surface::SurfaceKind::Cylinder),
        row(44, crate::surface::SurfaceKind::Extrusion),
        row(43, crate::surface::SurfaceKind::Cone),
        row(43, crate::surface::SurfaceKind::Cone),
    ];
    let plane = |id: &str, native_id: u32| Surface {
        id: SurfaceId(id.to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: Some(SourceObjectAssociation {
            format: "creo".to_string(),
            object_id: format!("VisibGeom:{native_id}"),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    };
    let surfaces = vec![
        plane("derived-id-independent-of-native-id", 41),
        plane("wrong-family", 42),
        plane("extrusion-carrier", 44),
    ];
    let procedural_surfaces = vec![ProceduralSurface {
        id: ProceduralSurfaceId("extrusion-construction".to_string()),
        surface: SurfaceId("extrusion-carrier".to_string()),
        definition: ProceduralSurfaceDefinition::Extrusion {
            directrix: CurveId("directrix".to_string()),
            parameter_interval: None,
            direction: Vector3::new(0.0, 0.0, 1.0),
            native_position: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    }];

    let coverage = surface_transfer_coverage(&rows, &surfaces, &procedural_surfaces);

    assert_eq!(coverage.unique_rows, 3);
    assert_eq!(coverage.transferred_rows, 2);
    assert_eq!(coverage.ambiguous_rows, 2);
    assert_eq!(coverage.by_family["plane"], (1, 1));
    assert_eq!(coverage.by_family["cylinder"], (1, 0));
    assert_eq!(coverage.by_family["cone"], (0, 0));
    assert_eq!(coverage.by_family["extrusion"], (1, 1));
}

#[test]
fn curve_coverage_excludes_unknown_carriers_and_ambiguous_ids() {
    let row = |id, type_byte| crate::curve::CurveTopologyRow {
        id,
        type_byte,
        feature_id: 17,
        directions: [0x01, 0xf6],
        faces: [1, 2],
        next_edges: [id, id],
        offset: 0,
    };
    let rows = vec![row(41, 0x05), row(42, 0x13), row(43, 0x05), row(43, 0x05)];
    let source = |native_id| SourceObjectAssociation {
        format: "creo".to_string(),
        object_id: format!("VisibGeom:{native_id}"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let curves = vec![
        Curve {
            id: CurveId("typed".to_string()),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: Some(source(41)),
        },
        Curve {
            id: CurveId("opaque".to_string()),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: Some(source(42)),
        },
    ];

    let coverage = curve_transfer_coverage(&rows, &curves);

    assert_eq!(coverage.unique_rows, 2);
    assert_eq!(coverage.transferred_rows, 1);
    assert_eq!(coverage.ambiguous_rows, 2);
    assert_eq!(coverage.by_type[&0x05], (1, 1));
    assert_eq!(coverage.by_type[&0x13], (1, 0));
}

#[test]
fn design_constraint_coverage_separates_typed_and_native_constraints() {
    let sketch = SketchId("sketch".to_string());
    let constraint = |id: &str, definition| SketchConstraint {
        id: SketchConstraintId(id.to_string()),
        sketch: sketch.clone(),
        definition,
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let entity = SketchEntityId("entity".to_string());
    let mut constraints = vec![
        constraint(
            "sketch:relation:1",
            SketchConstraintDefinition::Fixed {
                entity: entity.clone(),
            },
        ),
        constraint(
            "sketch:relation:2",
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:9".to_string(),
                entities: vec![entity.clone()],
                parameter: None,
                operands: Vec::new(),
                native_state: None,
            },
        ),
        constraint(
            "sketch:skamp:3",
            SketchConstraintDefinition::Fixed { entity },
        ),
    ];
    constraints[0].active = Some(true);
    constraints[1].active = Some(true);
    constraints[2].active = Some(false);

    let coverage =
        design_constraint_transfer_coverage(&constraints, ":relation:", "creo:relation:");

    assert_eq!(coverage.transferred, 2);
    assert_eq!(coverage.native, 1);
    assert_eq!(coverage.typed(), 1);
    assert_eq!(coverage.active, 2);
    assert_eq!(coverage.active_native, 1);
    assert_eq!(coverage.active_typed(), 1);
}

#[test]
fn native_curve_families_accept_only_their_defined_loci() {
    let point = SketchEntityId("point".to_string());
    let bounded = SketchEntityId("bounded".to_string());
    let line = SketchEntityId("line".to_string());
    let circle = SketchEntityId("circle".to_string());
    let geometry = BTreeMap::from([
        (
            point.clone(),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
        (
            bounded.clone(),
            SketchGeometry::Native {
                native_kind: "bounded_curve".to_string(),
            },
        ),
        (
            line.clone(),
            SketchGeometry::Native {
                native_kind: "line".to_string(),
            },
        ),
        (
            circle.clone(),
            SketchGeometry::Native {
                native_kind: "circle".to_string(),
            },
        ),
    ]);
    let compatible = SketchConstraintDefinition::CoincidentLoci {
        loci: vec![
            SketchLocus::Entity(point),
            SketchLocus::Start(bounded),
            SketchLocus::Center(circle.clone()),
        ],
    };
    assert!(sketch_constraint_loci_compatible(&compatible, &geometry));
    let incompatible = SketchConstraintDefinition::CoincidentLoci {
        loci: vec![SketchLocus::Start(line), SketchLocus::Start(circle)],
    };
    assert!(!sketch_constraint_loci_compatible(&incompatible, &geometry));
}

#[test]
fn incidence_family_lattice_narrows_endpoint_evidence() {
    let mut line = BTreeSet::from([
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Line,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut line);
    assert_eq!(line, BTreeSet::from([SectionEntityIncidenceFamily::Line]));

    let mut arc = BTreeSet::from([
        SectionEntityIncidenceFamily::BoundedCurve,
        SectionEntityIncidenceFamily::Circular,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut arc);
    assert_eq!(arc, BTreeSet::from([SectionEntityIncidenceFamily::Arc]));

    let mut conflicting = BTreeSet::from([
        SectionEntityIncidenceFamily::Line,
        SectionEntityIncidenceFamily::Circular,
    ]);
    normalize_section_incidence_curve_family_evidence(&mut conflicting);
    assert_eq!(conflicting.len(), 2);
}

#[test]
fn rowless_round_cylinder_requires_the_four_entry_sibling_layout() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(23),
        table_class_id: 80,
        entry_ids: vec![10, 11, 12, 13],
        entries: Vec::new(),
        surface_ids: vec![10, 11, 13],
        non_surface_entity_ids: vec![12],
        offset: 47,
    };
    assert_eq!(
        rowless_round_cylinder_pairs(&BTreeSet::from([23]), std::slice::from_ref(&table), &rows,),
        vec![(12, 13, 47)]
    );
    assert!(
        rowless_round_cylinder_pairs(&BTreeSet::new(), std::slice::from_ref(&table), &rows,)
            .is_empty()
    );
    rows[2].reversed = true;
    assert_eq!(
        rowless_round_face_orientations(
            &BTreeSet::from([23]),
            std::slice::from_ref(&table),
            &rows,
            &BTreeSet::from([12]),
        ),
        BTreeMap::from([(12, true)])
    );
    assert!(rowless_round_face_orientations(
        &BTreeSet::from([23]),
        std::slice::from_ref(&table),
        &rows,
        &BTreeSet::new(),
    )
    .is_empty());
    let mut materialized_rowless = rows;
    materialized_rowless.push(row(12, crate::surface::SurfaceKind::Cylinder));
    assert!(
        rowless_round_cylinder_pairs(&BTreeSet::from([23]), &[table], &materialized_rowless,)
            .is_empty()
    );
}

#[test]
fn spline_extrusion_preserves_directrix_basis_and_weights() {
    let directrix = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(4.0, 5.0, 6.0),
            Point3::new(7.0, 8.0, 9.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    };
    let surface =
        extruded_nurbs_surface(&directrix, [0.0, 0.0, 4.0]).expect("valid extrusion surface");

    assert_eq!((surface.u_degree, surface.v_degree), (2, 1));
    assert_eq!((surface.u_count, surface.v_count), (3, 2));
    assert_eq!(surface.u_knots, directrix.knots);
    assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        surface.control_points,
        [
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 7.0),
            Point3::new(4.0, 5.0, 6.0),
            Point3::new(4.0, 5.0, 10.0),
            Point3::new(7.0, 8.0, 9.0),
            Point3::new(7.0, 8.0, 13.0),
        ]
    );
    assert_eq!(surface.weights, Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]));
}

#[test]
fn reversed_arc_uses_opposite_axis_and_canonical_increasing_domain() {
    let (axis_sign, range) = oriented_arc_parameterization(
        true,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    );

    assert_eq!(axis_sign, -1.0);
    assert_eq!(
        range,
        [
            3.0 * std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2
        ]
    );
}

#[test]
fn extrusion_arc_pcurve_is_exact_in_both_directions() {
    for (start, end, expected_middle) in [
        (0.0, std::f64::consts::PI, Point2::new(2.0, 5.0)),
        (std::f64::consts::PI, 0.0, Point2::new(2.0, 5.0)),
    ] {
        let pcurve = circular_pcurve([2.0, 2.0], 3.0, start, end);
        let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("first endpoint");
        let middle = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.5).expect("arc midpoint");
        let last = cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("last endpoint");
        assert!((first.u - (2.0 + 3.0 * start.cos())).abs() < 1e-12);
        assert!((first.v - (2.0 + 3.0 * start.sin())).abs() < 1e-12);
        assert!((middle.u - expected_middle.u).abs() < 1e-12);
        assert!((middle.v - expected_middle.v).abs() < 1e-12);
        assert!((last.u - (2.0 + 3.0 * end.cos())).abs() < 1e-12);
        assert!((last.v - (2.0 + 3.0 * end.sin())).abs() < 1e-12);
    }
}

#[test]
fn extrusion_profile_area_includes_oriented_arc_sector() {
    let arc = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let line = SketchGeometry::Line {
        start: Point2::new(-1.0, 0.0),
        end: Point2::new(1.0, 0.0),
    };
    let counterclockwise = vec![
        (arc.clone(), false, [1.0, 0.0], [-1.0, 0.0]),
        (line.clone(), false, [-1.0, 0.0], [1.0, 0.0]),
    ];
    let clockwise = vec![
        (arc, true, [-1.0, 0.0], [1.0, 0.0]),
        (line, true, [1.0, 0.0], [-1.0, 0.0]),
    ];
    assert!(
        (extrusion_profile_signed_area(&counterclockwise).expect("positive area")
            - std::f64::consts::FRAC_PI_2)
            .abs()
            < 1e-12
    );
    assert!(
        (extrusion_profile_signed_area(&clockwise).expect("negative area")
            + std::f64::consts::FRAC_PI_2)
            .abs()
            < 1e-12
    );
}

#[test]
fn extrusion_profiles_require_one_oppositely_oriented_hole() {
    let rectangle = |minimum: [f64; 2], maximum: [f64; 2], clockwise: bool| {
        let mut points = [
            minimum,
            [maximum[0], minimum[1]],
            maximum,
            [minimum[0], maximum[1]],
        ];
        if clockwise {
            points.reverse();
        }
        (0..4)
            .map(|index| {
                let start = points[index];
                let end = points[(index + 1) % 4];
                (
                    SketchGeometry::Line {
                        start: Point2::new(start[0], start[1]),
                        end: Point2::new(end[0], end[1]),
                    },
                    false,
                    start,
                    end,
                )
            })
            .collect::<ExtrusionProfile>()
    };
    let outer = rectangle([-2.0, -2.0], [2.0, 2.0], false);
    let hole = rectangle([-1.0, -1.0], [1.0, 1.0], true);
    let (profiles, outer_area) = ordered_extrusion_profiles(vec![hole.clone(), outer.clone()])
        .expect("strict outer and hole");
    assert_eq!(profiles[0], outer);
    assert!(outer_area > 0.0);
    assert!(extrusion_profile_signed_area(&profiles[1]).expect("hole area") < 0.0);

    assert!(ordered_extrusion_profiles(vec![
        rectangle([-2.0, -2.0], [2.0, 2.0], false),
        rectangle([-1.0, -1.0], [1.0, 1.0], false),
    ])
    .is_none());
    assert!(ordered_extrusion_profiles(vec![
        rectangle([-2.0, -2.0], [2.0, 2.0], false),
        rectangle([1.0, -1.0], [3.0, 1.0], true),
    ])
    .is_none());

    let circular_hole = [
        (std::f64::consts::PI, 0.0, [-0.5, 0.0], [0.5, 0.0]),
        (
            std::f64::consts::TAU,
            std::f64::consts::PI,
            [0.5, 0.0],
            [-0.5, 0.0],
        ),
    ]
    .into_iter()
    .map(|(end_angle, start_angle, start, end)| {
        (
            SketchGeometry::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length(0.5),
                start_angle: Angle(start_angle),
                end_angle: Angle(end_angle),
            },
            true,
            start,
            end,
        )
    })
    .collect::<ExtrusionProfile>();
    let (profiles, _) = ordered_extrusion_profiles(vec![
        circular_hole,
        rectangle([-2.0, -2.0], [2.0, 2.0], false),
    ])
    .expect("arc-bounded hole");
    assert!(matches!(profiles[1][0].0, SketchGeometry::Arc { .. }));
}

#[test]
fn extrusion_profile_intersections_include_analytic_tangency() {
    let full_upper_circle = ([0.0, 0.0], 1.0, 0.0, std::f64::consts::PI);
    assert!(line_arc_intersect(
        [[-2.0, 1.0], [2.0, 1.0]],
        full_upper_circle,
        1e-9,
    ));
    assert!(!line_arc_intersect(
        [[-2.0, 1.1], [2.0, 1.1]],
        full_upper_circle,
        1e-9,
    ));
    assert!(arcs_intersect(
        full_upper_circle,
        ([2.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
        1e-9,
    ));
    assert!(!arcs_intersect(
        full_upper_circle,
        ([3.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
        1e-9,
    ));
}

#[test]
fn equal_opposite_cap_planes_define_symmetric_extent() {
    let extent = extrusion_extent_and_direction(
        [0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [
            ([0.0, 4.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
            ([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ],
    );

    assert_eq!(
        extent,
        Some((
            ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    draft: None,
                    offset: None,
                }
            },
            [0.0, -1.0, 0.0]
        ))
    );
}

#[test]
fn cap_proof_classifies_section_sweeps_without_overriding_revolves() {
    use crate::feature::FeatureRecipeKind::{Extrude, Revolve};

    assert!(section_sweep_allows_linear_extrusion(916, None));
    assert!(section_sweep_allows_linear_extrusion(917, None));
    assert!(section_sweep_allows_linear_extrusion(917, Some(Extrude)));
    assert!(section_sweep_allows_linear_extrusion(0, Some(Extrude)));
    assert!(!section_sweep_allows_linear_extrusion(917, Some(Revolve)));
    assert!(!section_sweep_allows_linear_extrusion(923, None));
}

#[test]
fn numbered_reference_name_selects_only_its_exact_feature_family() {
    assert!(numbered_feature_name_has_family("Thicken 1", "Thicken"));
    assert!(numbered_feature_name_has_family("Thicken 12", "Thicken"));
    assert!(!numbered_feature_name_has_family("Thicken", "Thicken"));
    assert!(!numbered_feature_name_has_family("Thicken A", "Thicken"));
    assert!(!numbered_feature_name_has_family("GThicken 1", "Thicken"));
    assert!(matches!(
        reference_named_feature_definition("Boundary Blend 1"),
        Some(IrFeatureDefinition::BoundarySurfaceUnresolved)
    ));
    assert!(matches!(
        reference_named_feature_definition("Thicken 1"),
        Some(IrFeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        })
    ));
    assert!(matches!(
        reference_named_feature_definition("Fill 1"),
        Some(IrFeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Unresolved),
            support_faces: FaceSelection::Unresolved,
            continuity: None,
            merge_result: None,
        })
    ));
    assert!(matches!(
        reference_named_feature_definition("Merge 2"),
        Some(IrFeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: None,
            create_solid: None,
            gap_tolerance: None,
        })
    ));
    assert!(reference_named_feature_definition("Extrude 2").is_none());
    assert!(matches!(
        unresolved_extrude_feature_definition(42),
        IrFeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            solid: None,
            ..
        }
    ));
}

#[test]
fn boundary_surface_entity_graph_requires_the_complete_generated_chain() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        prefixed: true,
        offset: 0,
        end_offset: 0,
    };
    let table = |table_class_id, entries: Vec<crate::feature::FeatureEntityTableEntry>| {
        crate::feature::FeatureEntityTable {
            feature_id: Some(144),
            table_class_id,
            entry_ids: entries.iter().map(|entry| entry.entity_id).collect(),
            surface_ids: (table_class_id == 29)
                .then_some(vec![145])
                .unwrap_or_default(),
            non_surface_entity_ids: Vec::new(),
            entries,
            offset: 0,
        }
    };
    let tables = vec![
        table(29, vec![entry(145, 200, Some(0))]),
        table(
            94,
            vec![
                entry(146, 221, None),
                entry(147, 222, None),
                entry(148, 220, None),
                entry(149, 220, None),
            ],
        ),
        table(67, vec![entry(150, 200, Some(144))]),
        table(100, vec![entry(150, 145, None)]),
    ];
    let surface = crate::surface::SurfaceRow {
        id: 145,
        type_byte: 0x2a,
        kind: crate::surface::SurfaceKind::Extrusion,
        feature_id: 144,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };

    assert!(class_942_boundary_surface_entity_graph(
        144,
        &tables,
        std::slice::from_ref(&surface),
    ));

    let mut incomplete = tables.clone();
    incomplete[1].entries.pop();
    assert!(!class_942_boundary_surface_entity_graph(
        144,
        &incomplete,
        &[surface],
    ));
}

#[test]
fn stored_section_sweep_family_defines_boolean_operation() {
    use crate::feature::FeatureRecipeEffect::{Cut, Protrude};

    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Körper", false, true),
        BooleanOp::Join
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Cut), "Ausschnitt", false, false),
        BooleanOp::Cut
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Protrusion", true, false),
        BooleanOp::NewBody
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Protrusion", true, true),
        BooleanOp::Join
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Cut), "Cut", true, true),
        BooleanOp::Cut
    );
    assert_eq!(
        section_sweep_boolean_operation(Some(Protrude), "Körper", false, false),
        BooleanOp::NewBody
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Protrusion", false, false),
        BooleanOp::NewBody
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Protrusion", false, true),
        BooleanOp::Join
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Körper", false, true),
        BooleanOp::Unresolved
    );
    assert_eq!(
        section_sweep_boolean_operation(None, "Körper", true, false),
        BooleanOp::NewBody
    );
}

#[test]
fn datum_feature_uses_its_unique_transferred_plane_carrier() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: 1,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#6".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 1.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });

    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, 923, "Datum Plane"),
        IrFeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 1.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        }
    );

    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 5,
        reversed: false,
        boundary_type: 1,
        next_surface: 0,
        offset: 1,
    });
    assert_eq!(
        schema_feature_definition(&scan, &ir, 5, 923, "Datum Plane"),
        IrFeatureDefinition::DatumPlaneUnresolved
    );
}

#[test]
fn datum_feature_uses_its_unique_complete_local_system() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(5),
            body: Vec::new(),
            parameter_frames: vec![
                crate::feature::FeatureParameterFrame {
                    kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                    body: Vec::new(),
                    decoded_values: Some(vec![
                        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 3.0, 4.0, 5.0,
                    ]),
                    offset: 1,
                },
                crate::feature::FeatureParameterFrame {
                    kind: crate::feature::FeatureParameterFrameKind::LocalSystem,
                    body: vec![0xff],
                    decoded_values: None,
                    offset: 2,
                },
            ],
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });

    assert_eq!(
        schema_feature_definition(
            &scan,
            &CadIr::empty(Units::default()),
            5,
            923,
            "Datum Plane"
        ),
        IrFeatureDefinition::DatumPlane {
            origin: Point3::new(3.0, 4.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }
    );
}

#[test]
fn only_body_evidence_or_a_new_body_sweep_establishes_prior_material() {
    let feature = |definition, outputs| Feature {
        id: IrFeatureId("creo:model:feature#1".to_string()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs,
        definition,
        native_ref: None,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(feature(
        IrFeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: EdgeSelection::Unresolved,
                spec: ChamferSpec::Unresolved { form: None },
            }],
            flip_direction: false,
        },
        Vec::new(),
    ));
    assert!(!preceding_features_establish_body(&ir));

    ir.model.features[0].outputs = vec![BodyId("creo:model:body#1".to_string())];
    assert!(preceding_features_establish_body(&ir));

    ir.model.features[0] = feature(
        IrFeatureDefinition::Extrude {
            profile: ProfileRef::Native("creo:section#1".to_string()),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(1.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            direction_source: None,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        Vec::new(),
    );
    assert!(preceding_features_establish_body(&ir));
    ir.model.features[0].suppressed = Some(true);
    assert!(!preceding_features_establish_body(&ir));
    ir.model.features[0].suppressed = Some(false);
    let IrFeatureDefinition::Extrude { op, .. } = &mut ir.model.features[0].definition else {
        unreachable!();
    };
    *op = BooleanOp::Join;
    assert!(!preceding_features_establish_body(&ir));
}

#[test]
fn current_feature_state_controls_recipe_and_parent_projection() {
    let operation = |recipe, parent_feature_id, offset| crate::feature::FeatureOperation {
        feature_id: 6,
        kind: "Sweep".to_string(),
        display_name_stored: false,
        stored_name: None,
        stored_name_bytes: None,
        identifier_keyword: None,
        stored_name_prefix: None,
        recipe: Some(recipe),
        root_schema_class: Some(917),
        parent_feature_id: Some(parent_feature_id),
        offset,
        state_offset: offset,
    };
    let historical = operation(crate::feature::FeatureRecipe::ProtrudeExtrude, 4, 10);
    let current = operation(crate::feature::FeatureRecipe::ProtrudeRevolve, 5, 20);
    let states = [historical, current.clone()];
    assert_ne!(states[0].recipe, states[1].recipe);
    assert_ne!(states[0].parent_feature_id, states[1].parent_feature_id);
    assert_eq!(
        current_feature_recipe(std::slice::from_ref(&current), 6),
        Some(crate::feature::FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(
        current_feature_recipe_parent(std::slice::from_ref(&current), 6),
        Some(5)
    );
    assert_eq!(
        current_additive_feature_recipe(std::slice::from_ref(&current), 6),
        Some(crate::feature::FeatureRecipeKind::Revolve)
    );
    let mut cut = current;
    cut.recipe = Some(crate::feature::FeatureRecipe::CutRevolve);
    assert_eq!(
        current_additive_feature_recipe(std::slice::from_ref(&cut), 6),
        None
    );
}

#[test]
fn circular_sweep_projects_profile_direction_and_extent() {
    let sweep = CircularSweepGeometry {
        cylinder_ids: vec![12, 13],
        section_definition_id: None,
        direction: [0.0, 0.0, -1.0],
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(6.5),
                },
                draft: None,
                offset: None,
            },
        },
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(2.0, 3.0, 4.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.5,
        },
    };

    assert_eq!(
        circular_sweep_feature_definition(
            ProfileRef::Sketch(SketchId("creo:model:sketch#917".to_string())),
            &sweep,
            BooleanOp::Join,
            Some(true),
        ),
        IrFeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(SketchId("creo:model:sketch#917".to_string())),
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(
                0.0, 0.0, -1.0
            )),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.5),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::Join,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            direction_source: None,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }
    );
}

#[test]
fn circular_sweep_cylinder_recovers_its_section_profile() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 917,
        feature_id: Some(40),
        origin: [1.0, 2.0, 3.0],
        u_axis: [0.0, 0.0, -1.0],
        v_axis: [1.0, 0.0, 0.0],
        normal: [0.0, -1.0, 0.0],
        offset: 20,
    };
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(5.0, -14.0, 1.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.5,
    };

    assert_eq!(
        circular_section_profile_from_cylinder(&transform, &cylinder),
        Some(([2.0, 4.0], 4.5))
    );
    let mut off_axis = cylinder.clone();
    let SurfaceGeometry::Cylinder { axis, .. } = &mut off_axis else {
        unreachable!();
    };
    *axis = Vector3::new(1.0, 0.0, 0.0);
    assert_eq!(
        circular_section_profile_from_cylinder(&transform, &off_axis),
        None
    );
}

#[test]
fn typed_center_locus_requires_a_circular_geometry_family() {
    let entity = SketchEntityId("creo:test:entity#1".into());
    let definition = SketchConstraintDefinition::CoincidentLoci {
        loci: vec![SketchLocus::Center(entity.clone())],
    };
    let unresolved = BTreeMap::from([(
        entity.clone(),
        SketchGeometry::Native {
            native_kind: "solver_only_section_entity".into(),
        },
    )]);
    assert!(!sketch_constraint_loci_compatible(&definition, &unresolved));

    let native_arc = BTreeMap::from([(
        entity.clone(),
        SketchGeometry::Native {
            native_kind: "arc".into(),
        },
    )]);
    assert!(sketch_constraint_loci_compatible(&definition, &native_arc));

    let resolved = BTreeMap::from([(
        entity,
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
        },
    )]);
    assert!(sketch_constraint_loci_compatible(&definition, &resolved));
}

#[test]
fn section_profile_prefers_a_resolved_sketch_chain() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.sketches.push(Sketch {
        id: SketchId("creo:model:sketch#offset:40".to_string()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("creo:featdefs:sketch#offset:40".to_string()),
    });
    assert_eq!(
        section_profile_ref(&ir, "creo:featdefs:sketch#offset:40".to_string()),
        ProfileRef::Native("creo:featdefs:sketch#offset:40".to_string())
    );

    ir.model.sketches[0].profiles.push(vec![SketchEntityUse {
        entity: SketchEntityId("creo:featdefs:sketch_entity#offset:40:4".to_string()),
        reversed: false,
    }]);
    assert_eq!(
        section_profile_ref(&ir, "creo:featdefs:sketch#offset:40".to_string()),
        ProfileRef::Sketch(SketchId("creo:model:sketch#offset:40".to_string()))
    );
    assert_eq!(
        section_profile_ref(&ir, "creo:featdefs:sketch#918".to_string()),
        ProfileRef::Native("creo:featdefs:sketch#918".to_string())
    );
}

#[test]
fn connected_profile_vertices_include_open_chain_terminals() {
    let sketch_id = SketchId("creo:model:sketch#917".to_string());
    let entity_id =
        |external_id| SketchEntityId(format!("creo:featdefs:sketch_entity#917:{external_id}"));
    let mut ir = CadIr::empty(Units::default());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved,
        profiles: vec![vec![
            SketchEntityUse {
                entity: entity_id(1),
                reversed: false,
            },
            SketchEntityUse {
                entity: entity_id(2),
                reversed: true,
            },
        ]],
        native_ref: None,
    });
    ir.model.sketch_entities.extend([
        SketchEntity {
            id: entity_id(1),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        },
        SketchEntity {
            id: entity_id(2),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(1.0, 1.0),
                end: Point2::new(1.0, 0.0),
            },
        },
    ]);

    assert_eq!(
        connected_sketch_profile_vertices(&ir, &sketch_id),
        vec![(0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])]
    );

    if let SketchGeometry::Line { start, .. } = &mut ir.model.sketch_entities[1].geometry {
        *start = Point2::new(0.0, 0.0);
    } else {
        unreachable!();
    }
    assert_eq!(
        connected_sketch_profile_vertices(&ir, &sketch_id),
        vec![(0, vec![[0.0, 0.0], [1.0, 0.0]])]
    );

    if let SketchGeometry::Line { end, .. } = &mut ir.model.sketch_entities[1].geometry {
        *end = Point2::new(2.0, 0.0);
    } else {
        unreachable!();
    }
    assert!(connected_sketch_profile_vertices(&ir, &sketch_id).is_empty());
}

#[test]
fn ordered_hole_cap_planes_define_blind_direction_and_depth() {
    assert_eq!(
        hole_extent_and_direction([
            ([2.0, -21.0, -0.75], [1.0, 0.0, 0.0]),
            ([5.0, -22.5, 0.75], [-1.0, 0.0, 0.0]),
        ]),
        Some((
            [1.0, 0.0, 0.0],
            Termination::Blind {
                length: Length(3.0),
            },
        ))
    );
    assert_eq!(
        hole_extent_and_direction([
            ([0.0, 0.5, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, -0.5, 0.0], [0.0, 1.0, 0.0]),
        ]),
        Some((
            [-0.0, -1.0, -0.0],
            Termination::Blind {
                length: Length(1.0),
            },
        ))
    );
    assert_eq!(
        hole_extent_and_direction([
            ([0.0; 3], [1.0, 0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ]),
        None
    );

    assert_eq!(
        hole_placement([
            (902, [0.0, 0.0, 0.85], [0.0, 0.0, 1.0]),
            (905, [0.0, 0.0, 7.35], [0.0, 0.0, -1.0]),
        ]),
        Some((
            902,
            [0.0, 0.0, 1.0],
            Termination::Blind {
                length: Length(6.5),
            },
        ))
    );
    assert_eq!(
        hole_placement([
            (902, [0.0; 3], [0.0, 0.0, 1.0]),
            (905, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]),
            (908, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
        ]),
        None
    );
    assert!(matches!(
        hole_cylinder_from_cap_outlines([
            (
                902,
                [0.0, 0.0, 0.85],
                [0.0, 0.0, 1.0],
                [[-1.5, 17.5, 0.85], [1.5, 20.5, 0.85]],
            ),
            (
                905,
                [0.0, 0.0, 7.35],
                [0.0, 0.0, -1.0],
                [[-1.5, 17.5, 7.35], [1.5, 20.5, 7.35]],
            ),
        ]),
        Some(SurfaceGeometry::Cylinder { origin, axis, radius, .. })
            if origin == Point3::new(0.0, 19.0, 0.85)
                && axis == Vector3::new(0.0, 0.0, 1.0)
                && radius == 1.5
    ));
    assert!(hole_cylinder_from_cap_outlines([
        (
            902,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [[-1.0, -2.0, 0.0], [1.0, 2.0, 0.0]],
        ),
        (
            905,
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [[-1.0, -2.0, 1.0], [1.0, 2.0, 1.0]],
        ),
    ])
    .is_none());
    assert!(circular_sweep_cylinder_from_cap_outlines([
        (
            828,
            [0.0, 4.0, 0.0],
            [0.0, 1.0, 0.0],
            Some([[-13.25, 4.0, -0.75], [-11.75, 4.0, 0.75]]),
        ),
        (831, [0.0, -4.0, 0.0], [0.0, 1.0, 0.0], None,),
    ])
    .is_none());
    assert!(matches!(
        cylinder_from_single_cap_outline((
            46,
            [0.0, 16.0, 0.0],
            [0.0, 1.0, 0.0],
            Some([[-4.45, 16.0, -4.45], [4.45, 16.0, 4.45]]),
        )),
        Some(SurfaceGeometry::Cylinder { origin, axis, radius, .. })
            if origin == Point3::new(0.0, 16.0, 0.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && radius == 4.45
    ));
}

#[test]
fn compact_hole_table_establishes_the_simple_form_without_metric_geometry() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let mut table = crate::feature::FeatureEntityTable {
        feature_id: Some(107),
        table_class_id: 29,
        entry_ids: vec![109, 112, 115, 117],
        entries: vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        surface_ids: vec![117],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let row = crate::surface::SurfaceRow {
        id: 117,
        type_byte: 0x24,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ),
        Some(117)
    );
    table.entries[2].source_entity_id = None;
    assert!(compact_simple_hole_cylinder_id(
        107,
        std::slice::from_ref(&table),
        std::slice::from_ref(&row),
    )
    .is_none());
    table.entries[2].source_entity_id = Some(0);
    table.entries[3].class_id = 201;
    assert!(compact_simple_hole_cylinder_id(107, std::slice::from_ref(&table), &[row]).is_none());
}

#[test]
fn torus_outline_identifies_exactly_one_prototype_radius_delta() {
    let outline = |values| crate::surface::TorusOutlineFrame {
        values,
        selector: 0,
        offset: 0,
    };
    assert!(outline_has_unique_radius_delta(
        outline([-192.5, -5.0, -40.0, -167.5, -3.0, 52.5]),
        2.0
    ));
    assert!(!outline_has_unique_radius_delta(
        outline([-2.0, -2.0, 0.0, 0.0, 0.0, 8.0]),
        2.0
    ));
    assert!(!outline_has_unique_radius_delta(
        outline([-2.0, 0.0, 0.0, 2.0, 0.0, 8.0]),
        2.0
    ));
    let five_coordinate =
        |values| crate::surface::Type26FiveCoordinateEnvelope { values, offset: 0 };
    assert!(five_coordinate_envelope_proves_torus_radii(
        five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
        0.0,
        2.65
    ));
    assert!(!five_coordinate_envelope_proves_torus_radii(
        five_coordinate([-2.65, -15.0, -2.5, 2.65, -17.65]),
        0.0,
        2.65
    ));
    assert!(five_coordinate_envelope_proves_torus_radii(
        five_coordinate([-4.95, 17.24, -4.95, 4.95, 16.74]),
        4.45,
        0.5
    ));
    assert!(coordinate_pair_proves_torus_radii(
        [-4.95, 17.24],
        [16.74, 4.95],
        4.45,
        0.5
    ));
    assert_eq!(
        paired_five_coordinate_sphere_center(
            [
                five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
                five_coordinate([-2.65, -12.35, -2.65, 2.65, -15.0]),
            ],
            2.65,
        ),
        Some([0.0, 0.0, -15.0])
    );
    assert!(paired_five_coordinate_sphere_center(
        [
            five_coordinate([-2.65, -15.0, -2.65, 2.65, -17.65]),
            five_coordinate([-2.65, -12.0, -2.65, 2.65, -15.0]),
        ],
        2.65,
    )
    .is_none());
}

#[test]
fn unique_parallel_round_supports_define_constant_radius() {
    assert_eq!(unique_positive_length(&[0.5, 0.5 + 1e-12]), Some(0.5));
    assert_eq!(unique_positive_length(&[0.5, 0.6]), None);
    assert_eq!(unique_positive_length(&[0.0]), None);
    assert!(!differing_positive_lengths(&[15.0, 15.0 + 1e-12]));
    assert!(differing_positive_lengths(&[15.0, 7.0, 15.0]));
    assert!(!differing_positive_lengths(&[0.0, 1.0]));
    assert_eq!(
        parallel_support_radius([
            ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, -6.1], [0.0, 0.0, 1.0]),
            ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ]),
        Some(0.5)
    );
    assert_eq!(
        parallel_support_radius([
            ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, -8.0], [0.0, 0.0, 1.0]),
        ]),
        None
    );
    assert_eq!(
        parallel_support_radius([
            ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, -7.0], [0.0, 0.0, 1.0]),
        ]),
        Some(0.5)
    );
    let cylinder = slot_fillet_cylinder(
        [
            PlaneEquation {
                origin: [0.0, -2.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 3.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
        &[
            PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 0.0, -7.0],
                normal: [0.0, 0.0, 1.0],
            },
            PlaneEquation {
                origin: [0.0, 0.0, -6.0],
                normal: [0.0, 0.0, 1.0],
            },
        ],
    )
    .expect("fully constrained slot fillet");
    assert_eq!(cylinder.origin, [-8.5, -2.0, -6.5]);
    assert_eq!(cylinder.axis, [0.0, 1.0, 0.0]);
    assert_eq!(cylinder.radius, 0.5);
    assert!(slot_fillet_cylinder(
        [
            PlaneEquation {
                origin: [0.0, -2.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 3.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
        &[
            PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
        ],
    )
    .is_none());
}

#[test]
fn opposite_reference_caps_select_one_round_envelope_axis() {
    let circle = |entity_id, axis, start, end| crate::reference::ReferenceCircle {
        entity_id,
        center: [0.0; 3],
        center_stored: true,
        radius: 2.0,
        axis,
        start,
        end,
        offset: 0,
    };
    let envelope = crate::surface::Type24RoundEnvelope {
        diameter: 2.0,
        extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
    };
    let first = circle(367, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);
    let second = circle(368, [0.0, 0.0, -1.0], [5.5, 10.0, -4.0], [3.5, 8.0, -4.0]);
    let frame =
        reference_cap_bound_round_frame(envelope, &[&first, &second]).expect("opposite Z caps");
    assert_eq!(frame.origin, [4.5, 9.0, -6.0]);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 1.0);
    assert_eq!(frame.length, Some(2.0));
    assert!(reference_cap_bound_round_frame(envelope, &[&first]).is_none());

    let x_first = circle(371, [1.0, 0.0, 0.0], [3.5, 8.0, -6.0], [3.5, 10.0, -4.0]);
    let x_second = circle(372, [-1.0, 0.0, 0.0], [5.5, 10.0, -4.0], [5.5, 8.0, -6.0]);
    assert!(
        reference_cap_bound_round_frame(envelope, &[&first, &second, &x_first, &x_second])
            .is_none()
    );

    let crossed_first = circle(369, [0.0, 0.0, -1.0], [5.5, 8.0, -6.0], [3.5, 10.0, -6.0]);
    let crossed_second = circle(370, [0.0, 0.0, 1.0], [3.5, 10.0, -4.0], [5.5, 8.0, -4.0]);
    assert_eq!(
        reference_cap_bound_round_frame(envelope, &[&crossed_first, &crossed_second]),
        Some(frame)
    );
    assert!(reference_cap_bound_round_frame(envelope, &[&first, &crossed_second]).is_none());
}

#[test]
fn coaxial_reference_circles_define_a_cylinder_frame() {
    let circle = |entity_id, center, axis, start| crate::reference::ReferenceCircle {
        entity_id,
        center,
        center_stored: true,
        radius: 2.0,
        axis,
        start,
        end: [0.0, 0.0, 0.0],
        offset: 0,
    };
    let first = circle(41, [3.0, 5.0, -2.0], [0.0, 0.0, 1.0], [3.0, 7.0, -2.0]);
    let second = circle(42, [3.0, 5.0, 4.0], [0.0, 0.0, -1.0], [1.0, 5.0, 4.0]);

    assert_eq!(
        reference_circle_pair_cylinder_frame(&[&first, &second]),
        Some(crate::surface::PositionalCylinderFrame {
            origin: first.center,
            axis: [0.0, 0.0, 1.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 2.0,
            length: Some(6.0),
        })
    );
    assert!(reference_circle_pair_cylinder_frame(&[&first]).is_none());

    let mut unequal_radius = second.clone();
    unequal_radius.radius = 1.0;
    assert!(reference_circle_pair_cylinder_frame(&[&first, &unequal_radius]).is_none());

    let displaced = circle(43, [3.5, 5.0, 4.0], [0.0, 0.0, 1.0], [3.5, 7.0, 4.0]);
    assert!(reference_circle_pair_cylinder_frame(&[&first, &displaced]).is_none());

    let mut derived_center = second;
    derived_center.center_stored = false;
    assert!(reference_circle_pair_cylinder_frame(&[&first, &derived_center]).is_none());
}

#[test]
fn asymmetric_cap_planes_define_two_sided_extent() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [
                ([0.0, 0.0, -2.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]),
            ],
        ),
        Some((
            ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(3.0),
                    },
                    draft: None,
                    offset: None,
                },
                second: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(2.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );
}

#[test]
fn one_negative_cap_offset_reverses_blind_direction() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, -1.0, 0.0],
            [([0.0, 48.0, 0.0], [0.0, 1.0, 0.0])],
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(48.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [-0.0, 1.0, -0.0],
        ))
    );
}

#[test]
fn zero_offset_support_plane_does_not_obscure_blind_cap() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, 1.0, 0.0],
            [
                ([20.0, 0.0, 6.0], [0.0, 1.0, 0.0]),
                ([0.0, 48.0, 0.0], [0.0, 1.0, 0.0]),
            ],
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(48.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [0.0, 1.0, 0.0],
        ))
    );
}

#[test]
fn interior_axis_normal_planes_do_not_shorten_blind_extent() {
    assert_eq!(
        extrusion_extent_and_direction(
            [0.0; 3],
            [0.0, -1.0, 0.0],
            [
                ([0.0, 38.0, 0.0], [0.0, 1.0, 0.0]),
                ([3.0, 2.5, 7.0], [0.0, -1.0, 0.0]),
                ([-4.0, 5.75, 1.0], [0.0, 1.0, 0.0]),
            ],
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(38.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            [-0.0, 1.0, -0.0],
        ))
    );
}

#[test]
fn agreeing_generated_cylinders_define_blind_extrusion_extent() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 917,
        feature_id: Some(40),
        origin: [0.0, 4.0, 0.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, -1.0],
        normal: [0.0, 1.0, 0.0],
        offset: 100,
    };
    let frame = |origin| crate::surface::PositionalCylinderFrame {
        origin,
        axis: [0.0, 1.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 0.75,
        length: Some(34.0),
    };
    let frames = [frame([-12.5, 4.0, 0.0]), frame([12.5, 4.0, 0.0])];
    assert_eq!(
        agreed_generated_cylinder_extent(&transform, &frames),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(34.0)
                    },
                    draft: None,
                    offset: None,
                }
            },
            [0.0, 1.0, 0.0]
        ))
    );
    assert_eq!(
        directed_blind_extrusion_span(transform.normal, [0.0, 1.0, 0.0], 34.0),
        Some(ExtrusionSpan {
            lower: 0.0,
            upper: 34.0,
        })
    );
    assert_eq!(
        directed_blind_extrusion_span(transform.normal, [0.0, -1.0, 0.0], 34.0),
        Some(ExtrusionSpan {
            lower: -34.0,
            upper: 0.0,
        })
    );
    assert!(directed_blind_extrusion_span(transform.normal, [1.0, 0.0, 0.0], 34.0).is_none());

    let mut inconsistent = frames;
    inconsistent[1].length = Some(33.0);
    assert!(agreed_generated_cylinder_extent(&transform, &inconsistent).is_none());
    inconsistent = frames;
    inconsistent[1].origin[1] = 5.0;
    assert!(agreed_generated_cylinder_extent(&transform, &inconsistent).is_none());

    let diagonal = 0.5_f64.sqrt();
    let diagonal_transform = crate::placement::FeatureSectionTransform {
        normal: [diagonal, diagonal, 0.0],
        ..transform
    };
    let perpendicular = [crate::surface::PositionalCylinderFrame {
        origin: diagonal_transform.origin,
        axis: [diagonal, -diagonal, 0.0],
        ..frames[0]
    }];
    assert!(agreed_generated_cylinder_extent(&diagonal_transform, &perpendicular).is_none());
}

#[test]
fn section_line_requires_two_solved_points() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        offset: 40,
    };
    let mut points = BTreeMap::from([(7, [2.0, 3.0])]);
    assert!(section_line_geometry(&points, &segment).is_none());
    points.insert(9, [5.0, 8.0]);
    assert_eq!(
        section_line_geometry(&points, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(2.0, 3.0),
            end: cadmpeg_ir::math::Point2::new(5.0, 8.0),
        })
    );
}

#[test]
fn sketch_constraints_require_every_neutral_reference_to_be_emitted() {
    let first = SketchEntityId("first".to_string());
    let second = SketchEntityId("second".to_string());
    let emitted = BTreeSet::from([first.clone()]);

    let mut horizontal = SketchConstraintDefinition::Horizontal {
        entity: first.clone(),
    };
    assert!(reconcile_constraint_entity_references(
        &mut horizontal,
        &emitted
    ));
    let mut parallel = SketchConstraintDefinition::Parallel {
        first: first.clone(),
        second: second.clone(),
    };
    assert!(!reconcile_constraint_entity_references(
        &mut parallel,
        &emitted
    ));
    let mut distance = SketchConstraintDefinition::DistanceLoci {
        first: SketchLocus::Start(first.clone()),
        second: SketchLocus::Center(second.clone()),
        parameter: ParameterId("distance".to_string()),
    };
    assert!(!reconcile_constraint_entity_references(
        &mut distance,
        &emitted
    ));
    let mut native = SketchConstraintDefinition::Native {
        native_kind: "creo:test".to_string(),
        entities: vec![first.clone(), second],
        parameter: None,
        operands: Vec::new(),
        native_state: None,
    };
    assert!(reconcile_constraint_entity_references(
        &mut native,
        &emitted
    ));
    assert!(matches!(
        native,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == vec![first]
    ));

    let parameter = ParameterId("distance".to_string());
    let parameters = BTreeSet::from([parameter.clone()]);
    let mut radius = SketchConstraintDefinition::Radius {
        entity: SketchEntityId("first".to_string()),
        parameter: parameter.clone(),
    };
    assert!(reconcile_constraint_parameter_reference(
        &mut radius,
        &parameters
    ));
    let mut missing_distance = SketchConstraintDefinition::Distance {
        entities: Vec::new(),
        parameter: ParameterId("missing".to_string()),
    };
    assert!(!reconcile_constraint_parameter_reference(
        &mut missing_distance,
        &parameters
    ));
    let mut native_parameter = SketchConstraintDefinition::Native {
        native_kind: "creo:test".to_string(),
        entities: Vec::new(),
        parameter: Some(ParameterId("missing".to_string())),
        operands: Vec::new(),
        native_state: None,
    };
    assert!(reconcile_constraint_parameter_reference(
        &mut native_parameter,
        &parameters
    ));
    assert!(matches!(
        native_parameter,
        SketchConstraintDefinition::Native {
            parameter: None,
            ..
        }
    ));
}

#[test]
fn section_point_uses_its_single_solved_position() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Point,
        directions: [None; 3],
        point_ids: [7, 7],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        offset: 40,
    };
    let points = BTreeMap::from([(7, [2.0, 3.0])]);

    assert_eq!(
        section_point_geometry(&points, &segment),
        Some(SketchGeometry::Point {
            position: cadmpeg_ir::math::Point2::new(2.0, 3.0),
        })
    );
}

#[test]
fn section_axis_line_carrier_uses_equal_decoded_ordinates() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [Some(0), None, Some(0)],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 7,
                    u: Some(2.0),
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 9,
                    u: Some(2.0),
                    v: Some(8.0),
                },
            ],
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    assert_eq!(
        section_axis_line_carrier(&definition, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(2.0, -8.0),
            end: cadmpeg_ir::math::Point2::new(2.0, 8.0),
        })
    );
    assert_eq!(
        unique_owned_feature_definition(std::slice::from_ref(&definition), 6)
            .map(|matched| matched.id),
        Some(5)
    );
    assert!(
        unique_owned_feature_definition(&[definition.clone(), definition.clone()], 6).is_none()
    );
    let operation = crate::feature::FeatureOperation {
        feature_id: 6,
        kind: "Extrude".to_string(),
        display_name_stored: true,
        stored_name: Some("Extrude id 6".to_string()),
        stored_name_bytes: Some(b"Extrude id 6".to_vec()),
        identifier_keyword: Some("id".to_string()),
        stored_name_prefix: None,
        recipe: Some(crate::feature::FeatureRecipe::ProtrudeExtrude),
        root_schema_class: Some(917),
        parent_feature_id: None,
        offset: 10,
        state_offset: 10,
    };
    assert_eq!(
        current_feature_operation(std::slice::from_ref(&operation), 6)
            .and_then(|current| current.root_schema_class),
        Some(917)
    );
    assert!(current_feature_operation(&[operation.clone(), operation.clone()], 6).is_none());
    assert_eq!(
        current_feature_recipe(std::slice::from_ref(&operation), 6),
        Some(crate::feature::FeatureRecipe::ProtrudeExtrude)
    );
    let mut conflicting_recipe = operation.clone();
    conflicting_recipe.recipe = Some(crate::feature::FeatureRecipe::ProtrudeRevolve);
    assert_eq!(
        current_feature_recipe(&[operation.clone(), conflicting_recipe], 6),
        None
    );
    let mut parented_operation = operation.clone();
    parented_operation.parent_feature_id = Some(5);
    assert_eq!(
        current_feature_recipe_parent(std::slice::from_ref(&parented_operation), 6),
        Some(5)
    );
    let mut conflicting_parent = parented_operation.clone();
    conflicting_parent.parent_feature_id = Some(4);
    assert_eq!(
        current_feature_recipe_parent(&[parented_operation, conflicting_parent], 6),
        None
    );
    let row = |schema_class, offset| crate::feature::FeatureRow {
        feature_id: 6,
        header: [0xeb, 0x04],
        root_schema_class: Some(schema_class),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: offset + 1,
        offset,
    };
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            &[],
            row_feature_schema_classes(&[row(917, 20), row(917, 30)], 6),
            6,
        ),
        Some(917)
    );
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            &[],
            row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
            6,
        ),
        None
    );
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            std::slice::from_ref(&operation),
            row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
            6,
        ),
        Some(917)
    );
    assert_eq!(
        resolved_feature_schema_class_from_classes(
            std::slice::from_ref(&operation),
            row_feature_schema_classes(&[row(913, 20), row(913, 30)], 6),
            6,
        ),
        Some(917)
    );
    assert_eq!(
        row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
        BTreeSet::from([913, 914])
    );
    let extent = |feature_id, offset| crate::feature::FeatureRevolutionExtent {
        feature_id,
        kind: crate::feature::FeatureRevolutionExtentKind::FullTurn,
        offset,
    };
    assert_eq!(
        unique_feature_revolution_extent_kind(&[extent(6, 40), extent(6, 50)], 6),
        Some(crate::feature::FeatureRevolutionExtentKind::FullTurn)
    );
    assert_eq!(
        unique_feature_revolution_extent_kind(&[extent(7, 40)], 6),
        None
    );
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(6),
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        offset: 40,
    };
    assert_eq!(
        unique_feature_section_transform(std::slice::from_ref(&transform), 5, 40)
            .map(|placed| placed.offset),
        Some(40)
    );
    assert!(
        unique_feature_section_transform(&[transform.clone(), transform.clone()], 5, 40).is_none()
    );
    let repeated_schema = crate::placement::FeatureSectionTransform {
        feature_id: Some(7),
        offset: 50,
        ..transform.clone()
    };
    assert_eq!(
        unique_feature_section_transform(&[transform.clone(), repeated_schema], 5, 40)
            .map(|placed| placed.offset),
        Some(40)
    );
    let competing_definition = crate::placement::FeatureSectionTransform {
        definition_id: 7,
        offset: 50,
        ..transform.clone()
    };
    assert!(unique_feature_section_transform(&[transform, competing_definition], 5, 40).is_none());
    let affected = |ids: &[u32], offset| crate::feature::FeatureAffectedIds {
        feature_id: 6,
        kind: crate::feature::AffectedIdKind::Edges,
        ids: ids.to_vec(),
        offset,
    };
    assert_eq!(
        agreed_feature_affected_ids(
            &[affected(&[7, 8], 60), affected(&[7, 8], 70)],
            6,
            crate::feature::AffectedIdKind::Edges,
        ),
        Some(&[7, 8][..])
    );
    assert_eq!(
        agreed_feature_affected_ids(
            &[affected(&[7, 8], 60), affected(&[8, 7], 70)],
            6,
            crate::feature::AffectedIdKind::Edges,
        ),
        None
    );
    let replay =
        |geometry_ids: &[u32], edge_ids: &[u32], offset| crate::feature::FeatureReplayAffectedIds {
            feature_id: 6,
            geometry_ids: geometry_ids.to_vec(),
            edge_ids: edge_ids.to_vec(),
            geometry_extent: crate::feature::ReplayExtentSource::Explicit,
            edge_extent: crate::feature::ReplayExtentSource::Inherited,
            offset,
        };
    assert_eq!(
        agreed_feature_replay_geometry_ids(
            &[replay(&[1, 2], &[7], 80), replay(&[1, 2], &[7], 90)],
            6,
        ),
        Some(&[1, 2][..])
    );
    assert_eq!(
        agreed_feature_replay_edge_ids(&[replay(&[1], &[7], 80), replay(&[1], &[], 90)], 6,),
        None
    );
}

#[test]
fn intersects_evaluated_section_carriers() {
    let horizontal = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(-2.0, 1.0),
        end: cadmpeg_ir::math::Point2::new(2.0, 1.0),
    };
    let vertical = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(0.5, -3.0),
        end: cadmpeg_ir::math::Point2::new(0.5, 3.0),
    };
    assert_eq!(
        intersect_section_lines(&horizontal, &vertical),
        Some([0.5, 1.0])
    );

    let circle_half = SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let endpoint_line = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(2.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(3.0, 1.0),
    };
    let intersection = intersect_section_line_arc(&endpoint_line, &circle_half)
        .expect("line has one endpoint on the arc");
    assert!((intersection[0] - 2.0).abs() <= 1e-12);
    assert!(intersection[1].abs() <= 1e-12);
    let one_crossing = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(3.0, 0.0),
    };
    assert_eq!(
        intersect_section_line_arc(&one_crossing, &circle_half),
        Some([2.0, 0.0])
    );
    let two_crossings = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(-3.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(3.0, 0.0),
    };
    assert_eq!(
        intersect_section_line_arc(&two_crossings, &circle_half),
        None
    );
    let no_crossing = SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(3.0, 0.0),
        end: cadmpeg_ir::math::Point2::new(4.0, 0.0),
    };
    assert_eq!(intersect_section_line_arc(&no_crossing, &circle_half), None);

    let circle = |center, radius| SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center, 0.0),
        radius: Length(radius),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::TAU),
    };
    assert_eq!(
        intersect_tangent_section_arcs(&circle(0.0, 2.0), &circle(3.0, 1.0)),
        Some([2.0, 0.0])
    );
    assert_eq!(
        intersect_tangent_section_arcs(&circle(0.0, 3.0), &circle(2.0, 1.0)),
        Some([3.0, 0.0])
    );
    assert_eq!(
        intersect_tangent_section_arcs(&circle(0.0, 2.0), &circle(2.0, 2.0)),
        None
    );
}

#[test]
fn saved_line_joins_through_order_table() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 10,
            }],
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 3,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(-8.0), Some(-0.85), Some(0.0)],
                        [Some(8.0), Some(-0.85), None],
                    ],
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };

    assert_eq!(
        saved_section_line_geometry(&definition, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [-8.0, -0.85]), (9, [8.0, -0.85])]),
        &segment,
    )
    .is_some());
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [-8.0, -0.85]), (9, [8.0, 0.85])]),
        &segment,
    )
    .is_none());
    assert_eq!(
        section_entity_external_ids(&definition),
        BTreeSet::from([42])
    );
    assert_eq!(
        materialized_saved_section_external_ids(&definition),
        BTreeSet::from([42])
    );
    let mut incomplete = definition.clone();
    let crate::feature::FeatureSavedEntity::Line(incomplete_line) = &mut incomplete
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities[0]
    else {
        panic!("saved line");
    };
    incomplete_line.endpoints[1][1] = None;
    assert!(saved_section_entity_geometry(
        &incomplete
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0]
    )
    .is_none());
    assert_eq!(
        section_entity_external_ids(&incomplete),
        BTreeSet::from([42])
    );
    assert!(materialized_saved_section_external_ids(&incomplete).is_empty());
    let (native_entity, offset) = unresolved_saved_section_entity(
        &incomplete,
        &SketchId("creo:model:sketch#5".into()),
        &incomplete
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0],
        &unique_saved_section_internal_ids(&incomplete),
        &BTreeSet::new(),
    );
    assert_eq!(offset, 20);
    assert_eq!(native_entity.id.0, "creo:featdefs:sketch_entity#5:42");
    assert!(matches!(
        native_entity.geometry,
        SketchGeometry::Native { ref native_kind } if native_kind == "saved_line"
    ));
    let mut duplicate_order_row = definition.clone();
    duplicate_order_row
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::FeatureOrderRow {
            external_id: 42,
            internal_id: 4,
            bitmask: 0,
            offset: 11,
        });
    assert_eq!(
        saved_section_line_geometry(&duplicate_order_row, &segment),
        None
    );
    let mut duplicate_saved_line = definition.clone();
    let duplicate = duplicate_saved_line
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[0]
        .clone();
    duplicate_saved_line
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(duplicate);
    assert_eq!(
        saved_section_line_geometry(&duplicate_saved_line, &segment),
        None
    );
    assert_eq!(
        saved_section_external_id(
            definition.order_table.as_ref().expect("order table"),
            &unique_saved_section_internal_ids(&definition),
            &ambiguous_section_segment_external_ids(&definition),
            3,
        ),
        Some(42)
    );
    let mut constrained = definition.clone();
    constrained.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 0,
        has_elided_prototype: false,
        entity_ref: None,
        rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: 0,
    });
    constrained.dimensions = Some(crate::feature::FeatureDimensionTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![crate::feature::FeatureDimension {
            dimension_type: 1,
            value: Some(2.0),
            unresolved_value_token: None,
            value_unit: crate::feature::DimensionUnit::Millimeters,
            direction_byte: 0,
            auxiliary_value: None,
            external_id: 4,
            offset: 27,
        }],
        offset: 26,
    });
    constrained.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 3,
        entity_ref: None,
        rows: vec![crate::feature::FeatureRelation {
            relation_id: 7,
            used: 1,
            operands: Vec::new(),
            operand_vectors: Some([
                [Some(42), Some(99), None, Some(1)],
                [Some(0); 4],
                [Some(15), Some(16), Some(15), Some(1)],
            ]),
            sign: 0,
            dimension_id: 0,
            relation_type: 0,
            body: Vec::new(),
            offset: 28,
        }],
        skamps: vec![crate::feature::FeatureSkamp {
            id: 5,
            kind: 99,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 42,
                    sense: 4,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
            ],
            offset: 30,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 1,
            offset: 29,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: Some(7),
            equation_id: None,
            skamp_id: Some(5),
            offset: 31,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 2,
            offset: 31,
        }),
        offset: 28,
    });
    let constraints =
        section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
    assert!(matches!(
        &constraints[0].0.definition,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == &[SketchEntityId(
                "creo:featdefs:sketch_entity#5:42".to_string()
            )]
    ));
    assert_eq!(
        relation_incidence_entities(
            &constrained,
            &SketchId("creo:model:sketch#5".to_string()),
            7,
        ),
        vec![
            SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
            SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
        ]
    );
    let dimension_constraints =
        section_dimension_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
    assert!(
        matches!(
            &dimension_constraints[0].0.definition,
            SketchConstraintDefinition::Distance { entities, .. }
                if entities == &[
                    SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
                    SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
                ]
        ),
        "{:?}",
        dimension_constraints[0].0.definition
    );
    let mut solver_families = constrained.clone();
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 0,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 2,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 100,
                sense: 3,
            },
        ],
        offset: 32,
    }];
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        None
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::BoundedCurve)
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 0,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 42,
                sense: 2,
            },
        ],
        offset: 32,
    }];
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        None
    );
    solver_families
        .relations
        .as_mut()
        .expect("relations")
        .skamps[0]
        .status = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Point)
    );
    let solver_geometry = BTreeMap::from([
        (
            SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
            SketchGeometry::Native {
                native_kind: "line".to_string(),
            },
        ),
        (
            SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
            SketchGeometry::Native {
                native_kind: "point".to_string(),
            },
        ),
    ]);
    let solver_constraints = section_skamp_constraints_for_geometry(
        &solver_families,
        &SketchId("creo:model:sketch#5".to_string()),
        Some(&solver_geometry),
    );
    let point_item = &solver_families
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .items[0];
    let line_item = &solver_families
        .relations
        .as_ref()
        .expect("relations")
        .skamps[0]
        .items[1];
    assert!(section_skamp_point_locus(
        &solver_families,
        &SketchId("creo:model:sketch#5".to_string()),
        point_item
    )
    .is_some());
    assert!(section_skamp_incidence_locus(
        &solver_families,
        &SketchId("creo:model:sketch#5".to_string()),
        line_item,
        Some(&solver_geometry)
    )
    .is_some());
    assert!(
        matches!(
            solver_constraints[0].0.definition,
            SketchConstraintDefinition::CoincidentLoci { .. }
        ),
        "{:?}",
        solver_constraints[0].0.definition
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps = vec![crate::feature::FeatureSkamp {
        id: 6,
        kind: 6,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 100,
                sense: 0,
            },
        ],
        offset: 33,
    }];
    family_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        Some(SectionEntityIncidenceFamily::Circular)
    );
    let family_relations = solver_families.relations.as_mut().expect("relations");
    family_relations.skamps.push(crate::feature::FeatureSkamp {
        id: 7,
        kind: 5,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 101,
                sense: 0,
            },
        ],
        offset: 34,
    });
    family_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert_eq!(
        solver_only_section_entity_family(&solver_families, 99),
        None
    );
    let mut duplicate_incidence = constrained.clone();
    let duplicate_relations = duplicate_incidence.relations.as_mut().expect("relations");
    let mut duplicate = duplicate_relations.skamps[0].clone();
    duplicate.status = 34;
    duplicate.offset = 32;
    duplicate_relations.skamps.push(duplicate);
    duplicate_relations
        .skamp_header
        .as_mut()
        .expect("skamp header")
        .declared_count = 2;
    assert!(relation_incidence_entities(
        &duplicate_incidence,
        &SketchId("creo:model:sketch#5".to_string()),
        7,
    )
    .is_empty());
    constrained.relations.as_mut().expect("relations").skamps[0].status = 34;
    assert!(relation_incidence_entities(
        &constrained,
        &SketchId("creo:model:sketch#5".to_string()),
        7,
    )
    .is_empty());
    assert_eq!(
        joined_relation_incidence_entities(
            &constrained,
            &SketchId("creo:model:sketch#5".to_string()),
            7,
        ),
        vec![
            SketchEntityId("creo:featdefs:sketch_entity#5:42".to_string()),
            SketchEntityId("creo:featdefs:sketch_entity#5:99".to_string()),
        ]
    );
    assert_eq!(
        section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()))[0]
            .0
            .active,
        Some(false)
    );
    constrained.segments = None;
    let constraints =
        section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
    assert!(matches!(
        &constraints[0].0.definition,
        SketchConstraintDefinition::Native { entities, .. }
            if entities == &[SketchEntityId(
                "creo:featdefs:sketch_entity#5:42".to_string()
            )]
    ));

    let mut completed = definition;
    completed
        .order_table
        .as_mut()
        .expect("test definition has an order table")
        .rows
        .clear();
    completed
        .order_table
        .as_mut()
        .expect("test definition has an order table")
        .declared_count = 0;
    completed.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment.clone()],
        opaque_rows: Vec::new(),
        offset: 4,
    });
    completed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::FeatureTrimEntity {
            external_id: 42,
            mode: Some(0),
            vertices: [1, 2],
            center_vertex: None,
            kind: crate::feature::TrimEntityKind::Line,
            offset: 6,
        }],
        solved_external_ids: vec![42],
        offset: 5,
    });
    assert_eq!(
        saved_section_line_geometry(&completed, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );
    let mut replay_mismatched = completed.clone();
    replay_mismatched
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows[0]
        .external_id = 99;
    assert_eq!(
        trim_segment_id(
            &replay_mismatched,
            &replay_mismatched
                .trim_entities
                .as_ref()
                .expect("trim table")
                .rows[0],
        ),
        Some(42)
    );
    assert_eq!(
        saved_section_line_geometry(&replay_mismatched, &segment),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
    );
    let mut incomplete_order = completed.clone();
    incomplete_order
        .order_table
        .as_mut()
        .expect("test definition has an order table")
        .declared_count = 1;
    assert_eq!(
        saved_section_line_geometry(&incomplete_order, &segment),
        None
    );
    let mut incomplete_segments = completed.clone();
    incomplete_segments
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 2;
    assert_eq!(
        saved_section_line_geometry(&incomplete_segments, &segment),
        None
    );
    let trim = completed.trim_entities.as_ref().expect("trim table").rows[0].clone();
    assert_eq!(trim_segment_id(&completed, &trim), Some(42));
    let mut duplicate_segment = completed.clone();
    duplicate_segment
        .segments
        .as_mut()
        .expect("segment table")
        .rows
        .push(segment);
    assert_eq!(trim_segment_id(&duplicate_segment, &trim), None);
    let mut duplicate_trim = completed;
    duplicate_trim
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows
        .push(trim.clone());
    assert_eq!(trim_segment_id(&duplicate_trim, &trim), None);
}

#[test]
fn complete_saved_circle_defines_full_section_geometry() {
    let entity = crate::feature::FeatureSavedEntity::Circle(crate::feature::FeatureSavedCircle {
        entity_id: 7,
        center: [Some(2.0), Some(-3.0), Some(0.0)],
        radius: Some(4.5),
        offset: 19,
    });

    assert_eq!(
        saved_section_entity_geometry(&entity),
        Some((
            7,
            SketchGeometry::Circle {
                center: Point2::new(2.0, -3.0),
                radius: Length(4.5),
            },
            19,
        ))
    );
    let (_, geometry, _) = saved_section_entity_geometry(&entity).expect("complete saved circle");
    assert!(is_full_circle_geometry(&geometry));
}

#[test]
fn generated_saved_geometry_forms_closed_profiles() {
    let line = |external_id: u32, start: (f64, f64), end: (f64, f64)| {
        (
            external_id,
            SketchGeometry::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            },
        )
    };
    let geometries = vec![
        line(12, (0.0, 1.0), (1.0, 1.0)),
        (
            10,
            SketchGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                weights: None,
                periodic: false,
            },
        ),
        line(13, (0.0, 0.0), (0.0, 1.0)),
        line(11, (1.0, 1.0), (1.0, 0.0)),
        line(20, (5.0, 5.0), (6.0, 5.0)),
        (
            30,
            SketchGeometry::Arc {
                center: Point2::new(8.0, 8.0),
                radius: Length(2.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::TAU),
            },
        ),
    ];

    let profiles =
        saved_profile_chains(&SketchId("creo:model:sketch#917".to_string()), &geometries);

    assert_eq!(profiles.len(), 2);
    assert_eq!(
        profiles[0][0].entity.0,
        "creo:featdefs:sketch_entity#917:30"
    );
    assert_eq!(profiles[1].len(), 4);
    assert_eq!(
        profiles[1][0].entity.0,
        "creo:featdefs:sketch_entity#917:10"
    );
    assert!(!profiles[1][0].reversed);
    assert!(profiles[1][1..].iter().all(|entity| entity.reversed));
    assert!(profiles
        .iter()
        .flatten()
        .all(|entity| !entity.entity.0.ends_with(":20")));
}

#[test]
fn saved_arc_joins_through_order_table() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: Some(8),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 10,
            }],
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Arc(
                crate::feature::FeatureSavedArc {
                    entity_id: 3,
                    center: [Some(0.0), Some(0.0), Some(0.0)],
                    radius: Some(2.0),
                    endpoints: [
                        [Some(0.0), Some(-2.0), Some(0.0)],
                        [Some(-2.0), Some(0.0), Some(0.0)],
                    ],
                    parameters: [None; 2],
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };

    assert_eq!(
        saved_section_arc_geometry(&definition, &segment),
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
        })
    );
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [0.0, -2.0]), (8, [0.0, 0.0]), (9, [-2.0, 0.0])]),
        &segment,
    )
    .is_some());
    assert!(resolved_section_segment_geometry(
        &definition,
        &BTreeMap::from([(7, [0.0, -3.0]), (8, [0.0, 0.0]), (9, [-3.0, 0.0])]),
        &segment,
    )
    .is_none());
    let mut duplicate_order_row = definition.clone();
    duplicate_order_row
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::FeatureOrderRow {
            external_id: 42,
            internal_id: 4,
            bitmask: 0,
            offset: 11,
        });
    assert_eq!(
        saved_section_arc_geometry(&duplicate_order_row, &segment),
        None
    );
    let mut duplicate_saved_arc = definition.clone();
    let duplicate = duplicate_saved_arc
        .saved_section
        .as_ref()
        .expect("saved section")
        .entities[0]
        .clone();
    duplicate_saved_arc
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(duplicate);
    assert_eq!(
        saved_section_arc_geometry(&duplicate_saved_arc, &segment),
        None
    );

    let mut trimmed = definition;
    trimmed.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment],
        opaque_rows: Vec::new(),
        offset: 38,
    });
    trimmed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::FeatureTrimEntity {
            external_id: 42,
            mode: Some(0),
            vertices: [1, 2],
            center_vertex: None,
            kind: crate::feature::TrimEntityKind::Arc,
            offset: 30,
        }],
        solved_external_ids: vec![42],
        offset: 28,
    });
    assert_eq!(
        resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
        BTreeMap::from([(1, [0.0, -2.0]), (2, [-2.0, 0.0])])
    );
    let mut conflicting_vertex = trimmed.clone();
    conflicting_vertex.trim_vertices = Some(crate::feature::FeatureTrimVertexTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![
            crate::feature::FeatureTrimVertex {
                vertex_id: 1,
                entities: vec![42, 43],
                section_coordinates: Some([0.0, -2.0]),
                offset: 31,
            },
            crate::feature::FeatureTrimVertex {
                vertex_id: 1,
                entities: vec![42, 44],
                section_coordinates: Some([9.0, 9.0]),
                offset: 32,
            },
        ],
        offset: 30,
    });
    assert_eq!(
        resolved_trim_vertex_coordinates(&conflicting_vertex, &BTreeMap::new()),
        BTreeMap::from([(2, [-2.0, 0.0])])
    );
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.center[1] = None;
        arc.radius = None;
    }
    let segment = &trimmed
        .segments
        .as_ref()
        .expect("test definition has a segment table")
        .rows[0];
    assert_eq!(
        saved_section_arc_carrier(&trimmed, segment),
        Some(([0.0, 0.0], 2.0))
    );
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.center[1] = Some(0.0);
        arc.radius = Some(2.0);
    }
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.endpoints[0] = [None; 3];
    } else {
        panic!("test entity is an arc");
    }
    assert_eq!(
        resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
        BTreeMap::from([(2, [-2.0, 0.0])])
    );
    if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
        .saved_section
        .as_mut()
        .expect("test definition has a saved section")
        .entities[0]
    {
        arc.endpoints[1] = [None; 3];
    }
    let segment = &trimmed
        .segments
        .as_ref()
        .expect("test definition has a segment table")
        .rows[0];
    assert!(saved_section_arc_geometry(&trimmed, segment).is_none());
    assert_eq!(
        section_segment_intersection_carrier(
            &trimmed,
            &resolved_section_radii(&trimmed),
            &BTreeMap::new(),
            segment,
        )
        .map(|carrier| carrier.geometry),
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        })
    );
}

#[test]
fn trimmed_line_reconciles_carrier_and_solver_orientation() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        offset: 40,
    };
    let anchor = crate::feature::FeatureSegment {
        point_ids: [5, 6],
        external_id: 41,
        offset: 39,
        ..segment.clone()
    };
    let horizontal = crate::feature::FeatureSkamp {
        id: 1,
        kind: 1,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 50,
    };
    let parallel = crate::feature::FeatureSkamp {
        id: 2,
        kind: 7,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::FeatureSkampItem {
                entity_id: 41,
                sense: 0,
            },
            crate::feature::FeatureSkampItem {
                entity_id: 42,
                sense: 0,
            },
        ],
        offset: 55,
    };
    let mut definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![anchor, segment.clone()],
            opaque_rows: Vec::new(),
            offset: 20,
        }),
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Line,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: Some(crate::feature::FeatureRelationTable {
            declared_count: 2,
            entity_ref: None,
            rows: Vec::new(),
            skamps: vec![horizontal, parallel],
            skamp_header: Some(crate::feature::FeatureSolverTableHeader {
                declared_count: 2,
                entity_ref: 70,
                offset: 45,
            }),
            triples: Vec::new(),
            triples_header: None,
            offset: 44,
        }),
        saved_section: None,
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 3.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &BTreeMap::new(), &trim_vertices, &segment,),
        Some(SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-2.0, 3.0),
            end: cadmpeg_ir::math::Point2::new(4.0, 3.0),
        })
    );
    let mut disabled_parallel = definition.clone();
    disabled_parallel
        .relations
        .as_mut()
        .expect("solver relations")
        .skamps[1]
        .status = 34;
    assert_eq!(
        trimmed_section_segment_geometry(
            &disabled_parallel,
            &BTreeMap::new(),
            &trim_vertices,
            &segment,
        ),
        None
    );

    let carrier_points = BTreeMap::from([(7, [0.0, 3.0]), (9, [2.0, 3.0])]);
    assert!(trimmed_section_segment_geometry(
        &definition,
        &carrier_points,
        &trim_vertices,
        &segment,
    )
    .is_some());
    let off_carrier_vertices = BTreeMap::from([(1, [-2.0, 3.0]), (2, [4.0, 4.0])]);
    assert!(trimmed_section_segment_geometry(
        &definition,
        &carrier_points,
        &off_carrier_vertices,
        &segment,
    )
    .is_none());

    let relations = definition.relations.as_mut().expect("solver relations");
    relations.skamps.push(crate::feature::FeatureSkamp {
        id: 3,
        kind: 2,
        flags: 0,
        status: 1,
        items: vec![crate::feature::FeatureSkampItem {
            entity_id: 41,
            sense: 0,
        }],
        offset: 60,
    });
    relations
        .skamp_header
        .as_mut()
        .expect("solver header")
        .declared_count = 3;
    assert!(trimmed_section_segment_geometry(
        &definition,
        &BTreeMap::new(),
        &trim_vertices,
        &segment,
    )
    .is_none());
}

#[test]
fn arc_carriers_use_trim_vertices() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: Some(8),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 42,
        offset: 40,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 5,
        owner_feature_id: Some(6),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Arc,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        }),
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 10,
            }],
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Arc(
                crate::feature::FeatureSavedArc {
                    entity_id: 3,
                    center: [Some(0.0), Some(0.0), Some(0.0)],
                    radius: Some(2.0),
                    endpoints: [[None; 3]; 2],
                    parameters: [None; 2],
                    offset: 20,
                },
            )],
            offset: 18,
        }),
        offset: 0,
    };
    let trim_vertices = BTreeMap::from([(1, [-2.0, 0.0]), (2, [0.0, -2.0])]);
    let points = BTreeMap::from([(7, [2.0, 0.0]), (8, [0.0, 0.0]), (9, [0.0, 2.0])]);

    assert_eq!(
        trimmed_section_segment_geometry(&definition, &points, &trim_vertices, &segment),
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(-std::f64::consts::FRAC_PI_2),
            end_angle: Angle(std::f64::consts::PI),
        })
    );

    let mut var_segment = segment.clone();
    var_segment.radius_ref = Some(10);
    let mut var_arc = definition;
    var_arc.variables = Some(crate::feature::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        points: vec![
            crate::feature::FeatureSectionPoint {
                point_id: 7,
                u: Some(2.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 8,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::FeatureSectionPoint {
                point_id: 9,
                u: Some(0.0),
                v: Some(2.0),
            },
        ],
        offset: 5,
    });
    var_arc.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![var_segment.clone()],
        opaque_rows: Vec::new(),
        offset: 6,
    });
    var_arc.order_table = None;
    var_arc.saved_section = None;
    assert_eq!(
        trimmed_section_segment_geometry(
            &var_arc,
            &resolved_section_points(&var_arc),
            &trim_vertices,
            &var_segment,
        ),
        Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(-std::f64::consts::FRAC_PI_2),
            end_angle: Angle(std::f64::consts::PI),
        })
    );
}

#[test]
fn placed_extrusion_line_defines_plane() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(5),
        origin: [10.0, 20.0, 30.0],
        u_axis: [0.0, 1.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [1.0, 0.0, 0.0],
        offset: 7,
    };
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 3,
        offset: 9,
    };
    let points = BTreeMap::from([(1, [2.0, 3.0]), (2, [6.0, 3.0])]);
    assert_eq!(
        extruded_segment_surface(&transform, &points, &segment),
        Some(SurfaceGeometry::Plane {
            origin: Point3::new(10.0, 22.0, 33.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
            u_axis: Vector3::new(0.0, 1.0, 0.0),
        })
    );
    assert_eq!(
        placed_section_curve_geometry(&transform, &points, &segment),
        Some(CurveGeometry::Line {
            origin: Point3::new(10.0, 22.0, 33.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        })
    );
}

#[test]
fn sketch_curve_references_require_a_materialized_curve() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(5),
        origin: [10.0, 20.0, 30.0],
        u_axis: [0.0, 1.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [1.0, 0.0, 0.0],
        offset: 7,
    };
    let sketch = SketchId("creo:model:sketch#5".to_string());
    let line = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(2.0, 0.0),
    };
    let point = SketchGeometry::Point {
        position: Point2::new(1.0, 2.0),
    };

    assert_eq!(
        placed_sketch_curve_ref(Some(&transform), &sketch, 3, &line),
        Some("creo:featdefs:section_curve#5:3".to_string())
    );
    assert_eq!(placed_sketch_curve_ref(None, &sketch, 3, &line), None);
    assert_eq!(
        placed_sketch_curve_ref(Some(&transform), &sketch, 4, &point),
        None
    );
}

#[test]
fn placed_extrusion_arc_defines_cylinder() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 5,
        feature_id: Some(5),
        origin: [10.0, 20.0, 30.0],
        u_axis: [0.0, 1.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [1.0, 0.0, 0.0],
        offset: 7,
    };
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        offset: 9,
    };
    let points = BTreeMap::from([(1, [2.0, 0.0]), (2, [-2.0, 0.0]), (3, [0.0, 0.0])]);
    assert_eq!(
        extruded_segment_surface(&transform, &points, &segment),
        Some(SurfaceGeometry::Cylinder {
            origin: Point3::new(10.0, 20.0, 30.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 2.0,
        })
    );
    assert_eq!(
        placed_section_curve_geometry(&transform, &points, &segment),
        Some(CurveGeometry::Circle {
            center: Point3::new(10.0, 20.0, 30.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 2.0,
        })
    );
    assert_eq!(
        placed_section_geometry_curve(
            &transform,
            &SketchGeometry::Circle {
                center: Point2::new(3.0, -4.0),
                radius: Length(2.0),
            },
        ),
        Some(CurveGeometry::Circle {
            center: Point3::new(10.0, 23.0, 26.0),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 2.0,
        })
    );
}

#[test]
fn line_orientation_selectors_are_closed() {
    let mut segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids: [7, 9],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: Some(0),
        radius_ref: None,
        radius2_ref: None,
        external_id: 12,
        offset: 40,
    };
    let entity = SketchEntityId("entity".into());
    assert_eq!(
        line_orientation_definition(&segment, entity.clone()),
        Some(SketchConstraintDefinition::Vertical {
            entity: entity.clone()
        })
    );
    segment.vertical_horizontal = Some(1);
    assert_eq!(
        line_orientation_definition(&segment, entity.clone()),
        Some(SketchConstraintDefinition::Horizontal {
            entity: entity.clone()
        })
    );
    segment.vertical_horizontal = Some(2);
    assert_eq!(line_orientation_definition(&segment, entity.clone()), None);
    segment.kind = crate::feature::FeatureSegmentKind::Arc;
    segment.vertical_horizontal = Some(0);
    assert_eq!(line_orientation_definition(&segment, entity), None);
}

#[test]
fn skamp_status_low_bit_controls_constraint_activity() {
    assert!(!section_skamp_active(2));
    assert!(section_skamp_active(3));
    assert!(!section_skamp_active(34));
    assert!(section_skamp_active(35));
    assert!(!section_skamp_active(50));
    assert!(!section_skamp_active(65_570));
}

#[test]
fn dimension_identity_includes_its_feature_definition() {
    let sketch_917 = SketchId("creo:model:sketch#917".to_string());
    let sketch_1104 = SketchId("creo:model:sketch#1104".to_string());
    let sketch_1200 = SketchId("creo:model:sketch#1200".to_string());
    assert_ne!(
        feature_dimension_parameter_id(&sketch_917, 3),
        feature_dimension_parameter_id(&sketch_1104, 3)
    );
    assert_eq!(
        feature_dimension_parameter_id(&sketch_917, 3).0,
        "creo:featdefs:parameter#917:3"
    );
    assert_eq!(
        feature_dimension_parameter_layout(&[
            (sketch_917.clone(), 3),
            (sketch_1104.clone(), 3),
            (sketch_1104.clone(), 4),
            (sketch_1200, 3),
        ]),
        Some(vec![
            (0, "d3".to_string(), None),
            (0, "d3".to_string(), None),
            (1, "d4".to_string(), None),
            (0, "d3".to_string(), None),
        ])
    );
    assert_eq!(
        feature_dimension_parameter_layout(&[(sketch_917.clone(), 3), (sketch_917.clone(), 3),]),
        Some(vec![
            (0, "d917_3_1".to_string(), Some(0)),
            (1, "d917_3_2".to_string(), Some(1)),
        ])
    );
    assert_ne!(
        feature_dimension_parameter_row_id(&sketch_917, 3, Some(0)),
        feature_dimension_parameter_row_id(&sketch_917, 3, Some(1))
    );
    let dimension = crate::feature::FeatureDimension {
        dimension_type: 2,
        value: Some(5.0),
        unresolved_value_token: None,
        value_unit: crate::feature::DimensionUnit::Millimeters,
        direction_byte: 0,
        auxiliary_value: None,
        external_id: 3,
        offset: 10,
    };
    let mut table = crate::feature::FeatureDimensionTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![dimension.clone()],
        offset: 9,
    };
    let mut definition = crate::feature::FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(table.clone()),
        relations: None,
        saved_section: None,
        offset: 8,
    };
    assert_eq!(
        resolved_feature_dimension_parameter(
            &sketch_917,
            definition.dimensions.as_ref().expect("dimension table"),
            0,
        ),
        Some((
            &dimension,
            ParameterId("creo:featdefs:parameter#917:3".to_string())
        ))
    );
    definition.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: Vec::new(),
        opaque_rows: vec![crate::feature::FeatureOpaqueSegment {
            kind: 10,
            directions: [None; 3],
            point_ids: [None, Some(1)],
            center_id: Some(7),
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: Some(0),
            radius2_ref: None,
            external_id: 42,
            offset: 20,
        }],
        offset: 19,
    });
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 3;
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(0, 5.0)])
    );
    let radius = section_circle_size_constraints(&definition, &sketch_917);
    assert_eq!(radius.len(), 1);
    assert_eq!(
        radius[0].0.definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:42".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:3".to_string()),
        }
    );
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(0, 2.5)])
    );
    let diameter = section_circle_size_constraints(&definition, &sketch_917);
    assert_eq!(diameter.len(), 1);
    assert_eq!(
        diameter[0].0.definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:42".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:3".to_string()),
        }
    );
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 2;
    assert_eq!(
        resolved_section_radii(&definition),
        BTreeMap::from([(0, 2.5)])
    );
    assert_eq!(
        section_circle_size_constraints(&definition, &sketch_917)[0]
            .0
            .definition,
        SketchConstraintDefinition::Diameter {
            entity: SketchEntityId("creo:featdefs:sketch_entity#917:42".to_string()),
            parameter: ParameterId("creo:featdefs:parameter#917:3".to_string()),
        }
    );
    definition
        .segments
        .as_mut()
        .expect("segment table")
        .declared_count = 1;
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 2;
    assert!(resolved_section_radii(&definition).is_empty());
    assert!(section_circle_size_constraints(&definition, &sketch_917).is_empty());
    definition
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        section_opaque_circle_geometry(
            &BTreeMap::from([(7, [1.0, 2.0])]),
            &resolved_section_radii(&definition),
            &definition.segments.as_ref().expect("segments").opaque_rows[0],
        ),
        Some(SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(2.5),
        })
    );
    let unresolved_dimension = crate::feature::FeatureDimension {
        value: None,
        external_id: 4,
        ..dimension.clone()
    };
    let unresolved_table = crate::feature::FeatureDimensionTable {
        rows: vec![unresolved_dimension.clone()],
        ..table.clone()
    };
    assert_eq!(
        resolved_feature_dimension_parameter(&sketch_917, &unresolved_table, 0),
        Some((
            &unresolved_dimension,
            ParameterId("creo:featdefs:parameter#917:4".to_string())
        ))
    );
    let incomplete_table = crate::feature::FeatureDimensionTable {
        declared_count: 2,
        ..unresolved_table
    };
    assert_eq!(
        resolved_feature_dimension_parameter(&sketch_917, &incomplete_table, 0),
        None
    );
    table.rows.push(dimension);
    definition.dimensions = Some(table);
    assert_eq!(
        resolved_feature_dimension_parameter(
            &sketch_917,
            definition.dimensions.as_ref().expect("dimension table"),
            0,
        ),
        None
    );
    assert_eq!(
        resolved_feature_dimension_parameter(
            &sketch_917,
            definition.dimensions.as_ref().expect("dimension table"),
            1,
        ),
        None
    );
}

#[test]
fn dimension_display_preserves_radius_and_diameter_types() {
    assert_eq!(
        feature_dimension_display(0x03),
        Some(DimensionDisplay::Radius)
    );
    assert_eq!(
        feature_dimension_display(0x04),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(feature_dimension_display(0x02), None);
    assert_eq!(feature_dimension_display(0x0a), None);
}

#[test]
fn evaluated_sweep_bodies_are_feature_outputs() {
    let mut ir = CadIr::empty(Units::default());
    for id in [
        "creo:feature:extrusion#40:body",
        "creo:feature:revolution#40:body",
        "creo:feature:revolution#41:body",
    ] {
        ir.model.bodies.push(Body {
            id: BodyId(id.to_string()),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#43:body".to_string()),
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert_eq!(
        evaluated_sweep_output_bodies(&ir, 40),
        vec![
            BodyId("creo:feature:extrusion#40:body".to_string()),
            BodyId("creo:feature:revolution#40:body".to_string()),
        ]
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "extrusion", 40),
        Some(BodyKind::Solid)
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "revolution", 40),
        Some(BodyKind::Solid)
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "extrusion", 43),
        Some(BodyKind::Sheet)
    );
    assert_eq!(evaluated_sweep_body_kind(&ir, "revolution", 42), None);
}

#[test]
fn section_solver_constraints_require_complete_unique_semantics() {
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
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: None,
                external_id: 42,
                offset: 75,
            }],
            offset: 74,
        }),
        relations: Some(relations),
        saved_section: None,
        offset: 0,
    };
    let point_entity = crate::feature::FeatureSkampItem {
        entity_id: 14,
        sense: 0,
    };
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
            guess: None,
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
        .opaque_rows
        .push(crate::feature::FeatureOpaqueSegment {
            kind: 1,
            directions: [Some(0); 3],
            point_ids: [None, Some(1)],
            center_id: Some(1),
            arc_orientation: Some(0),
            vertical_horizontal: Some(0),
            radius_ref: None,
            radius2_ref: None,
            external_id: 99,
            offset: 601,
        });
    let opaque_point_item = crate::feature::FeatureSkampItem {
        entity_id: 99,
        sense: 0,
    };
    assert_eq!(
        section_opaque_point_geometry(
            &resolved_section_points(&opaque_point),
            &opaque_point
                .segments
                .as_ref()
                .expect("segments")
                .opaque_rows[0],
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
        ),
        Some((SketchLocus::Entity(entity), target))
            if entity.0.ends_with(":99") && target.0.ends_with(":12")
    ));
    let centered_line = crate::feature::FeatureOpaqueSegment {
        kind: 47,
        directions: [Some(0); 3],
        point_ids: [None, Some(1)],
        center_id: Some(2),
        external_id: 100,
        offset: 602,
        ..opaque_point
            .segments
            .as_ref()
            .expect("segments")
            .opaque_rows[0]
            .clone()
    };
    assert_eq!(
        section_opaque_centered_line_geometry(
            &BTreeMap::from([(0, [3.0, -1.0]), (1, [3.0, 5.0]), (2, [3.0, 2.0]),]),
            &centered_line,
        ),
        Some(SketchGeometry::Line {
            start: Point2::new(3.0, -1.0),
            end: Point2::new(3.0, 5.0),
        })
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
        .opaque_rows
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
    let mut opaque_family_collision = opaque_point.clone();
    opaque_family_collision
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    let colliding_row = crate::feature::FeatureOpaqueSegment {
        kind: 10,
        center_id: Some(1),
        radius_ref: Some(0),
        offset: 602,
        ..opaque_family_collision
            .segments
            .as_ref()
            .expect("segments")
            .opaque_rows[0]
            .clone()
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
    assert_eq!(
        equivalent_constraints[0].0.definition,
        equivalent_constraints
            .last()
            .expect("redundant")
            .0
            .definition
    );
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
    assert_eq!(
        constraints[2].0.definition,
        SketchConstraintDefinition::Native {
            native_kind: "creo:skamp:7".to_string(),
            native_state: None,
            entities: vec![SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )],
            parameter: None,
            operands: vec![SketchNativeOperand {
                native_kind: "sense:4".to_string(),
                native_field: None,
                native_role: None,
                object_index: 12,
                native_ref: None,
            }],
        }
    );
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
    assert_eq!(
        section_dimension_constraints(
            &separate_point_distance,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:12".to_string()
            )),
            second: SketchLocus::Start(SketchEntityId(
                "creo:featdefs:sketch_entity#917:15".to_string()
            )),
            parameter: ParameterId("creo:featdefs:parameter#917:42".to_string()),
        }
    );
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
    assert_eq!(
        section_dimension_constraints(
            &duplicate_dimension,
            &SketchId("creo:model:sketch#917".into())
        )[0]
        .0
        .definition,
        SketchConstraintDefinition::Native {
            native_kind: "creo:relation:0".to_string(),
            native_state: None,
            entities: Vec::new(),
            parameter: None,
            operands: vec![SketchNativeOperand {
                native_kind: "relat_ptr".to_string(),
                native_field: None,
                native_role: None,
                object_index: 8,
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }],
        }
    );
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
    legacy_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 2;
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

    let mut radius_definition = definition.clone();
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
            native_state: None,
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

#[test]
fn zero_orientation_arc_runs_clockwise_from_first_endpoint() {
    let segment = crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Arc,
        directions: [None; 3],
        point_ids: [1, 2],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: Some(4),
        radius2_ref: None,
        external_id: 12,
        offset: 40,
    };
    let points = BTreeMap::from([(1, [0.0, -2.0]), (2, [0.0, 2.0]), (3, [0.0, 0.0])]);
    let Some(SketchGeometry::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    }) = section_arc_geometry(&points, &segment)
    else {
        panic!("complete arc");
    };
    assert_eq!(center, cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(radius, Length(2.0));
    assert!((start_angle.0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((end_angle.0 - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn profile_chain_follows_trim_vertex_incidence() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: [(10, [1, 2]), (11, [3, 2]), (12, [3, 4]), (13, [4, 1])]
                .into_iter()
                .map(
                    |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                        external_id,
                        mode: None,
                        vertices,
                        center_vertex: None,
                        kind: crate::feature::TrimEntityKind::Line,
                        offset: external_id as usize,
                    },
                )
                .collect(),
            solved_external_ids: vec![10, 11, 12, 13],
            offset: 5,
        }),
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 1,
    };
    let profiles = resolved_profile_chains(
        &definition,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
    );
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 4);
    assert_eq!(profiles[0][0].entity.0, "creo:featdefs:sketch_entity#40:10");
    assert!(!profiles[0][0].reversed);
    assert!(profiles[0][1].reversed);

    let mut incomplete = definition.clone();
    let table = incomplete.trim_entities.as_mut().expect("trim table");
    table.declared_count = Some(1);
    table.buckets.push(crate::feature::FeatureTrimBucket {
        index: 0,
        declared_entry_count: 4,
        decoded_entry_count: 3,
        offset: 5,
    });
    assert!(resolved_profile_chains(
        &incomplete,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
    )
    .is_empty());
    assert_eq!(
        trim_segment_id(
            &incomplete,
            &incomplete.trim_entities.as_ref().expect("trim table").rows[0],
        ),
        None
    );

    assert!(resolved_profile_chains(
        &definition,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32]),
    )
    .is_empty());

    let mut incomplete_trim_graph = definition.clone();
    incomplete_trim_graph.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 4,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [(10, [1, 2]), (11, [2, 3]), (12, [3, 4]), (13, [4, 1])]
            .into_iter()
            .map(|(external_id, point_ids)| crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids,
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                offset: external_id as usize,
            })
            .collect(),
        opaque_rows: Vec::new(),
        offset: 2,
    });
    incomplete_trim_graph
        .trim_entities
        .as_mut()
        .expect("trim table")
        .rows
        .retain(|row| row.external_id != 13);
    let profiles = resolved_profile_chains(
        &incomplete_trim_graph,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
    );
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 4);

    let mut arcs = definition.clone();
    arcs.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: [(10, [1, 2]), (11, [2, 1])]
            .into_iter()
            .map(
                |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                    external_id,
                    mode: None,
                    vertices,
                    center_vertex: Some(3),
                    kind: crate::feature::TrimEntityKind::Arc,
                    offset: external_id as usize,
                },
            )
            .collect(),
        solved_external_ids: vec![10, 11],
        offset: 5,
    });
    arcs.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [10, 11]
            .into_iter()
            .map(|external_id| crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Arc,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                offset: external_id as usize,
            })
            .collect(),
        opaque_rows: Vec::new(),
        offset: 4,
    });
    let arc_profile = resolved_profile_chains(
        &arcs,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10, 11]),
    );
    assert_eq!(arc_profile.len(), 1);
    assert!(arc_profile[0].iter().all(|entity| entity.reversed));

    let mut segment_graph = definition;
    segment_graph.trim_entities = None;
    segment_graph.segments = Some(crate::feature::FeatureSegmentTable {
        declared_count: 5,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [
            (10, [1, 2]),
            (11, [3, 2]),
            (12, [3, 4]),
            (13, [4, 1]),
            (20, [8, 9]),
        ]
        .into_iter()
        .map(|(external_id, point_ids)| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            offset: external_id as usize,
        })
        .collect(),
        opaque_rows: Vec::new(),
        offset: 4,
    });
    let segment_profile = resolved_profile_chains(
        &segment_graph,
        &SketchId("creo:model:sketch#40".to_string()),
        &BTreeSet::from([10, 11, 12, 13, 20]),
    );
    assert_eq!(segment_profile.len(), 1);
    assert_eq!(segment_profile[0].len(), 4);
    assert!(!segment_profile[0][0].reversed);
    assert!(segment_profile[0][1].reversed);
}

#[test]
fn multi_incident_trim_vertex_requires_one_agreeing_pairwise_intersection() {
    let line = |start: [f64; 2], end: [f64; 2]| SectionIntersectionCarrier {
        geometry: SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
            end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
        },
        line_is_bounded: false,
    };
    let concurrent = [
        line([-1.0, 0.0], [1.0, 0.0]),
        line([0.0, -1.0], [0.0, 1.0]),
        line([-1.0, -1.0], [1.0, 1.0]),
    ];
    assert_eq!(
        intersect_incident_section_carriers(&concurrent),
        Some([0.0, 0.0])
    );

    let inconsistent = [
        line([-1.0, 0.0], [1.0, 0.0]),
        line([0.0, -1.0], [0.0, 1.0]),
        line([-1.0, 2.0], [2.0, -1.0]),
    ];
    assert_eq!(intersect_incident_section_carriers(&inconsistent), None);
}

#[test]
fn revolution_axis_uses_the_unique_complete_section_centerline() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
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
                    v: Some(-2.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(0.0),
                    v: Some(3.0),
                },
            ],
            offset: 1,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: Some(0),
                radius_ref: None,
                radius2_ref: None,
                external_id: 1,
                offset: 2,
            }],
            opaque_rows: Vec::new(),
            offset: 2,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 1,
    };
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 40,
        feature_id: Some(40),
        origin: [5.0, 7.0, 11.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [0.0, -1.0, 0.0],
        offset: 3,
    };

    let axis = resolved_revolution_axis(&definition, &transform).expect("axis");
    assert_eq!(axis.origin, Point3::new(5.0, 7.0, 9.0));
    assert_eq!(axis.direction, Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn saved_spline_collocation_interpolates_points_and_endpoint_derivatives() {
    let spline = crate::feature::FeatureSavedSpline {
        entity_id: Some(7),
        declared_point_count: Some(3),
        interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        endpoint_tangents: Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
        parameters: Some(vec![0.0, 1.0, 2.0]),
        offset: 10,
    };
    let nurbs = saved_spline_nurbs(&spline).expect("clamped interpolation spline");
    for (parameter, expected) in [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)] {
        let point = nurbs.control_points.iter().enumerate().fold(
            [0.0; 3],
            |mut point, (index, control)| {
                let basis = bspline_basis(
                    index,
                    nurbs.degree as usize,
                    parameter,
                    &nurbs.knots,
                    nurbs.control_points.len(),
                );
                point[0] += basis * control.x;
                point[1] += basis * control.y;
                point[2] += basis * control.z;
                point
            },
        );
        assert!((point[0] - expected).abs() < 1e-12);
        assert!(point[1].abs() < 1e-12 && point[2].abs() < 1e-12);
    }
    for parameter in [0.0, 2.0] {
        let derivative = nurbs.control_points.iter().enumerate().fold(
            [0.0; 3],
            |mut derivative, (index, control)| {
                let basis = bspline_basis_derivative(
                    index,
                    nurbs.degree as usize,
                    parameter,
                    &nurbs.knots,
                    nurbs.control_points.len(),
                );
                derivative[0] += basis * control.x;
                derivative[1] += basis * control.y;
                derivative[2] += basis * control.z;
                derivative
            },
        );
        assert!((derivative[0] - 1.0).abs() < 1e-12);
        assert!(derivative[1].abs() < 1e-12 && derivative[2].abs() < 1e-12);
    }
    assert!(matches!(
        saved_spline_sketch_geometry(&spline),
        Some(SketchGeometry::Nurbs { degree: 3, .. })
    ));
    let definition = crate::feature::FeatureDefinition {
        id: 917,
        owner_feature_id: Some(40),
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: Vec::new(),
            opaque_rows: vec![crate::feature::FeatureOpaqueSegment {
                kind: 25,
                directions: [None; 3],
                point_ids: [Some(1), Some(2)],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                offset: 20,
            }],
            offset: 20,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 7,
                bitmask: 0,
                offset: 30,
            }],
            offset: 30,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Spline(spline.clone())],
            offset: 40,
        }),
        offset: 1,
    };
    assert_eq!(
        materialized_saved_section_external_ids(&definition),
        BTreeSet::from([42])
    );

    let mut incomplete = spline;
    incomplete.declared_point_count = Some(4);
    assert!(saved_spline_nurbs(&incomplete).is_none());
    assert!(saved_spline_sketch_geometry(&incomplete).is_none());

    let mut duplicate_saved_id = definition.clone();
    duplicate_saved_id
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities
        .push(crate::feature::FeatureSavedEntity::Spline(incomplete));
    assert!(materialized_saved_section_external_ids(&duplicate_saved_id).is_empty());

    let mut ambiguous_external_id = definition;
    let duplicate_opaque = ambiguous_external_id
        .segments
        .as_ref()
        .expect("segments")
        .opaque_rows[0]
        .clone();
    ambiguous_external_id
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .push(duplicate_opaque);
    ambiguous_external_id
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 2;
    assert!(materialized_saved_section_external_ids(&ambiguous_external_id).is_empty());

    let mut incomplete_segment_table = ambiguous_external_id;
    incomplete_segment_table
        .segments
        .as_mut()
        .expect("segments")
        .opaque_rows
        .pop();
    assert_eq!(
        materialized_saved_section_external_ids(&incomplete_segment_table),
        BTreeSet::from([42])
    );
}

#[test]
fn tensor_product_collocation_preserves_position_and_derivative_order() {
    let points = [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 2.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 3.0],
    ];
    let du = [1.0, 0.0, 1.0];
    let dv = [0.0, 1.0, 2.0];
    let zero = [0.0; 3];
    let nurbs = interpolation_spline_surface(
        &points,
        &[0.0, 1.0],
        &[0.0, 1.0],
        &[du, du, du, du],
        &[dv, dv, dv, dv],
        &[zero, zero, zero, zero],
    )
    .expect("bicubic tensor-product surface");

    assert_eq!((nurbs.u_count, nurbs.v_count), (4, 4));
    assert_eq!(nurbs.u_knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.v_knots, nurbs.u_knots);
    for u in 0..4 {
        for v in 0..4 {
            let point = &nurbs.control_points[u * 4 + v];
            let expected_u = u as f64 / 3.0;
            let expected_v = v as f64 / 3.0;
            assert!((point.x - expected_u).abs() < 1e-12);
            assert!((point.y - expected_v).abs() < 1e-12);
            assert!((point.z - expected_u - 2.0 * expected_v).abs() < 1e-12);
        }
    }
}

#[test]
fn nonplanar_saved_spline_places_as_model_curve() {
    let transform = crate::placement::FeatureSectionTransform {
        definition_id: 917,
        feature_id: Some(40),
        origin: [10.0, 20.0, 30.0],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [0.0, 0.0, 1.0],
        normal: [0.0, -1.0, 0.0],
        offset: 5,
    };
    let local = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        weights: None,
        periodic: false,
    };

    let placed = placed_section_nurbs(&transform, &local);

    assert_eq!(placed.control_points[0], Point3::new(11.0, 17.0, 32.0));
    assert_eq!(placed.control_points[1], Point3::new(14.0, 14.0, 35.0));
}

#[test]
fn transferred_geometry_is_derived_from_ir_arenas() {
    let mut ir = CadIr::empty(Units::default());
    assert!(!has_transferred_geometry(&ir));

    ir.model.points.push(Point {
        id: PointId("point".to_string()),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    assert!(has_transferred_geometry(&ir));
}

#[test]
fn full_revolution_uses_exact_quadratic_circle_poles() {
    let directrix = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
        weights: None,
        periodic: false,
    };
    let surface = revolved_nurbs_surface(
        &directrix,
        RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
    )
    .expect("revolution surface");

    assert_eq!((surface.u_count, surface.v_count), (2, 9));
    assert_eq!(surface.control_points[0], Point3::new(2.0, 0.0, 0.0));
    assert_eq!(surface.control_points[1], Point3::new(2.0, 2.0, 0.0));
    assert_eq!(surface.control_points[2], Point3::new(0.0, 2.0, 0.0));
    assert_eq!(surface.control_points[8], surface.control_points[0]);
    assert_eq!(
        surface.weights.as_ref().expect("rational weights")[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
}

#[test]
fn planar_loop_containment_selects_one_outer_boundary() {
    let make_loop = |face_id: u32, first_curve: u32| crate::topology::Loop {
        face_id,
        half_edges: (0_u32..4)
            .map(|index| HalfEdgeId {
                curve_id: first_curve + index,
                side: 0,
            })
            .collect(),
    };
    let outer = make_loop(9, 1);
    let inner = make_loop(9, 5);
    let incidences = (1..=8)
        .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: vertex,
                side: 0,
            },
            start_vertex_id: vertex,
            end_vertex_id: Some(if vertex % 4 == 0 {
                vertex - 3
            } else {
                vertex + 1
            }),
        })
        .collect::<Vec<_>>();
    let incidence = incidences
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let points = BTreeMap::from([
        (1, [-2.0, -2.0, 0.0]),
        (2, [2.0, -2.0, 0.0]),
        (3, [2.0, 2.0, 0.0]),
        (4, [-2.0, 2.0, 0.0]),
        (5, [-1.0, -1.0, 0.0]),
        (6, [1.0, -1.0, 0.0]),
        (7, [1.0, 1.0, 0.0]),
        (8, [-1.0, 1.0, 0.0]),
    ]);
    let plane = PlaneEquation {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    };

    let ordered = ordered_planar_face_loops(vec![&inner, &outer], plane, &incidence, &points)
        .expect("unique outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 1);
    assert_eq!(ordered[1].half_edges[0].curve_id, 5);

    let disjoint_points = points
        .into_iter()
        .map(|(id, mut point)| {
            if id >= 5 {
                point[0] += 10.0;
            }
            (id, point)
        })
        .collect::<BTreeMap<_, _>>();
    assert!(
        ordered_planar_face_loops(vec![&outer, &inner], plane, &incidence, &disjoint_points,)
            .is_none()
    );
    assert_eq!(
        ordered_face_loops(vec![&outer], None, &incidence, &disjoint_points),
        Some(vec![&outer])
    );
    assert!(
        ordered_face_loops(vec![&outer, &inner], None, &incidence, &disjoint_points,).is_none()
    );
}

#[test]
fn carrier_solver_accepts_two_carrier_tangent_vertices() {
    let plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [0.0, 0.0, 1.0],
    });
    let sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(solve_carriers(&[plane, sphere]), Some([0.0, 0.0, 2.0]));

    let second_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [5.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    assert_eq!(
        solve_carriers(&[sphere, second_sphere]),
        Some([2.0, 0.0, 0.0])
    );

    let secant = CarrierEquation::Sphere(SphereEquation {
        center: [3.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(solve_carriers(&[sphere, secant]), None);
}

#[test]
fn coaxial_cone_torus_components_support_edges_and_vertices() {
    let cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    let secant_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 3.0,
        minor_radius: 2.0,
    });
    let candidates = coaxial_cone_torus_circle_candidates(cone, secant_torus);
    assert_eq!(candidates.len(), 2);
    assert!(resolve_curve_candidates(
        coaxial_cone_torus_circle_candidates(cone, secant_torus),
        None,
    )
    .is_none());
    let upper_parameter = f64::midpoint(1.0, 7.0_f64.sqrt());
    let upper_radius = 2.0 + upper_parameter;
    assert!(matches!(
        select_unique_curve_candidate(
            candidates,
            [
                [upper_radius, 0.0, upper_parameter],
                [0.0, upper_radius, upper_parameter],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_torus_circle"))
            if (center.z - upper_parameter).abs() < 1e-12
                && (radius - upper_radius).abs() < 1e-12
    ));
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [3.0 + 7.0_f64.sqrt(), 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let vertex = solve_carriers(&[cone, secant_torus, tangent_plane])
        .expect("unique cone-torus circle tangent");
    assert!((vertex[0] - upper_radius).abs() < 1e-12);
    assert!(vertex[1].abs() < 1e-12);
    assert!((vertex[2] - upper_parameter).abs() < 1e-12);

    let tangent_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 5.0,
        minor_radius: 3.0 / 2.0_f64.sqrt(),
    });
    let tangent_candidates = coaxial_cone_torus_circle_candidates(cone, tangent_torus);
    assert!(matches!(
        tangent_candidates.as_slice(),
        [(CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_torus_circle")]
            if (center.z - 1.5).abs() < 1e-12 && (radius - 3.5).abs() < 1e-12
    ));
    assert!(matches!(
        resolve_curve_candidates(tangent_candidates, None),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_torus_circle"))
            if (center.z - 1.5).abs() < 1e-12 && (radius - 3.5).abs() < 1e-12
    ));
    assert!(resolve_curve_candidates(
        coaxial_cone_torus_circle_candidates(cone, tangent_torus),
        Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
    )
    .is_none());
    let shifted_torus = CarrierEquation::Torus(TorusEquation {
        center: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 3.0,
        minor_radius: 2.0,
    });
    assert!(coaxial_cone_torus_circle_candidates(cone, shifted_torus).is_empty());
}

#[test]
fn axis_containing_plane_torus_components_support_edges_and_vertices() {
    let plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    let torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 3.0,
        minor_radius: 1.0,
    });
    let candidates = axis_containing_plane_torus_circle_candidates(plane, torus);
    assert_eq!(candidates.len(), 2);
    assert!(resolve_curve_candidates(candidates.clone(), None).is_none());
    assert!(matches!(
        select_unique_curve_candidate(candidates, [[4.0, 0.0, 0.0], [3.0, 0.0, 1.0]]),
        Some((CurveGeometry::Circle { center, radius, .. }, "axis_containing_plane_torus_meridian_circle"))
            if (center.x - 3.0).abs() < 1e-12
                && center.y.abs() < 1e-12
                && center.z.abs() < 1e-12
                && (radius - 1.0).abs() < 1e-12
    ));

    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [4.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[plane, torus, tangent_plane]),
        Some([4.0, 0.0, 0.0])
    );

    let offset_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.5, 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    assert!(axis_containing_plane_torus_circle_candidates(offset_plane, torus).is_empty());
}

#[test]
fn coaxial_cone_components_respect_axis_orientation_and_coincidence() {
    let first = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    let second = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        ratio: 1.0,
        half_angle: 0.5_f64.atan(),
    });
    let candidates = coaxial_cones_section_candidates(first, second);
    assert_eq!(candidates.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(candidates, [[6.0, 0.0, 4.0], [0.0, 6.0, 4.0]]),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cones_circle"))
            if (center.z - 4.0).abs() < 1e-12 && (radius - 6.0).abs() < 1e-12
    ));
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [10.0, 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let vertex = solve_carriers(&[first, second, tangent_plane])
        .expect("unique coaxial-cone circle tangent");
    assert!((vertex[0] - 6.0).abs() < 1e-12);
    assert!(vertex[1].abs() < 1e-12);
    assert!((vertex[2] - 4.0).abs() < 1e-12);

    let reversed = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, -1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        ratio: 1.0,
        half_angle: 0.5_f64.atan(),
    });
    let reversed_candidates = coaxial_cones_section_candidates(first, reversed);
    assert_eq!(reversed_candidates.len(), 2);
    assert!(reversed_candidates.iter().any(|(geometry, _)| matches!(
        geometry,
        CurveGeometry::Circle { center, radius, .. }
            if (center.z - 4.0 / 3.0).abs() < 1e-12
                && (radius - 10.0 / 3.0).abs() < 1e-12
    )));
    assert!(coaxial_cones_section_candidates(first, first).is_empty());
    let shifted = CarrierEquation::Cone(ConeEquation {
        origin: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 4.0,
        ratio: 1.0,
        half_angle: 0.5_f64.atan(),
    });
    assert!(coaxial_cones_section_candidates(first, shifted).is_empty());

    let CarrierEquation::Cone(mut elliptical_first_equation) = first else {
        unreachable!();
    };
    elliptical_first_equation.ratio = 0.5;
    let elliptical_first = CarrierEquation::Cone(elliptical_first_equation);
    let CarrierEquation::Cone(mut elliptical_second_equation) = second else {
        unreachable!();
    };
    elliptical_second_equation.ratio = 0.5;
    let elliptical_second = CarrierEquation::Cone(elliptical_second_equation);
    let candidates = coaxial_cones_section_candidates(elliptical_first, elliptical_second);
    assert_eq!(candidates.len(), 2);
    let selected = select_unique_curve_candidate(candidates, [[6.0, 0.0, 4.0], [0.0, 3.0, 4.0]])
        .expect("selected coaxial elliptical-cone section");
    assert!(matches!(
        &selected,
        (
            CurveGeometry::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            },
            "coaxial_cones_ellipse"
        ) if (center.z - 4.0).abs() < 1e-12
            && (major_radius - 6.0).abs() < 1e-12
            && (minor_radius - 3.0).abs() < 1e-12
    ));
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&selected.0, parameter)
            .expect("coaxial cone ellipse point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, elliptical_first));
        assert!(point_on_carrier(point, elliptical_second));
    }
    elliptical_second_equation.ref_direction = [0.0, 1.0, 0.0];
    let incompatible_frame = CarrierEquation::Cone(elliptical_second_equation);
    assert!(coaxial_cones_section_candidates(elliptical_first, incompatible_frame).is_empty());

    elliptical_second_equation.ratio = 2.0;
    elliptical_second_equation.half_angle = 0.25_f64.atan();
    let reciprocal_swapped = CarrierEquation::Cone(elliptical_second_equation);
    let candidates = coaxial_cones_section_candidates(elliptical_first, reciprocal_swapped);
    assert_eq!(candidates.len(), 2);
    let selected = select_unique_curve_candidate(candidates, [[14.0, 0.0, 12.0], [0.0, 7.0, 12.0]])
        .expect("selected reciprocal-frame cone section");
    assert!(matches!(
        &selected,
        (
            CurveGeometry::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            },
            "coaxial_cones_ellipse"
        ) if (center.z - 12.0).abs() < 1e-12
            && (major_radius - 14.0).abs() < 1e-12
            && (minor_radius - 7.0).abs() < 1e-12
    ));
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&selected.0, parameter)
            .expect("reciprocal-frame section point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, elliptical_first));
        assert!(point_on_carrier(point, reciprocal_swapped));
    }
}

#[test]
fn carrier_solver_accepts_unique_plane_plane_quadric_vertices() {
    let cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    let cap = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 3.0],
        normal: [0.0, 0.0, 1.0],
    });
    let tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [2.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[cylinder, cap, tangent]),
        Some([2.0, 0.0, 3.0])
    );
    let x_axis_cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [1.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 1.0,
    });
    let y_axis_cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 1.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 1.0,
    });
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 1.0],
        normal: [0.0, 0.0, 1.0],
    });
    assert_eq!(
        solve_carriers(&[x_axis_cylinder, y_axis_cylinder, tangent_plane]),
        Some([0.0, 0.0, 1.0])
    );
    let cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 1.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    let offset_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 1.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    let generator_parallel_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [1.0, 0.0, -1.0],
    });
    assert_eq!(
        solve_carriers(&[cone, offset_plane, generator_parallel_plane]),
        Some([0.0, 1.0, 0.0])
    );
    let secant_plane = PlaneEquation {
        origin: [1.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    };
    let mut secant_points =
        intersect_plane_with_two_quadrics(secant_plane, x_axis_cylinder, y_axis_cylinder);
    secant_points.sort_by(|left, right| left[1].total_cmp(&right[1]));
    assert_eq!(secant_points, vec![[1.0, -1.0, 0.0], [1.0, 1.0, 0.0]]);
    assert_eq!(
        solve_carriers(&[
            x_axis_cylinder,
            y_axis_cylinder,
            CarrierEquation::Plane(secant_plane),
        ]),
        None
    );

    let secant = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(solve_carriers(&[cylinder, cap, secant]), None);

    assert!(matches!(
        carrier_intersection_curve(cap, cylinder),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_cylinder_circle"))
            if center.z == 3.0 && radius == 2.0
    ));
    let oblique = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(oblique, cylinder),
        Some((CurveGeometry::Ellipse { major_radius, minor_radius, .. }, "plane_cylinder_ellipse"))
            if (major_radius - 2.0 * 2.0_f64.sqrt()).abs() < 1e-12
                && minor_radius == 2.0
    ));
    assert!(matches!(
        carrier_intersection_curve(tangent, cylinder),
        Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_tangent_line"))
            if origin.x == 2.0 && direction.z == 1.0
    ));
    assert!(carrier_intersection_curve(secant, cylinder).is_none());
    let generators = parallel_plane_cylinder_generator_candidates(secant, cylinder);
    assert_eq!(generators.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            parallel_plane_cylinder_generator_candidates(secant, cylinder),
            [[0.0, 2.0, -1.0], [0.0, 2.0, 4.0]],
        ),
        Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_secant_generator"))
            if (origin.y - 2.0).abs() < 1e-12 && direction.z == 1.0
    ));
    assert!(select_unique_curve_candidate(
        parallel_plane_cylinder_generator_candidates(secant, cylinder),
        [[0.0, 0.0, -1.0], [0.0, 0.0, 4.0]],
    )
    .is_none());

    let parallel_cylinder = |origin: [f64; 3], radius| {
        CarrierEquation::Cylinder(CylinderEquation {
            origin,
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius,
        })
    };
    assert!(matches!(
        carrier_intersection_curve(
            parallel_cylinder([0.0, 0.0, 0.0], 2.0),
            parallel_cylinder([5.0, 0.0, 0.0], 3.0),
        ),
        Some((CurveGeometry::Line { origin, direction }, "parallel_cylinder_tangent_line"))
            if origin.x == 2.0 && direction.z == 1.0
    ));
    assert_eq!(
        solve_carriers(&[
            cap,
            parallel_cylinder([0.0, 0.0, 0.0], 2.0),
            parallel_cylinder([5.0, 0.0, 0.0], 3.0),
        ]),
        Some([2.0, 0.0, 3.0])
    );
    assert!(matches!(
        carrier_intersection_curve(
            parallel_cylinder([0.0, 0.0, 0.0], 5.0),
            parallel_cylinder([3.0, 0.0, 0.0], 2.0),
        ),
        Some((CurveGeometry::Line { origin, .. }, "parallel_cylinder_tangent_line"))
            if origin.x == 5.0
    ));
    assert!(carrier_intersection_curve(
        parallel_cylinder([0.0, 0.0, 0.0], 3.0),
        parallel_cylinder([4.0, 0.0, 0.0], 3.0),
    )
    .is_none());
    let secant_cylinders = [
        parallel_cylinder([0.0, 0.0, 0.0], 3.0),
        parallel_cylinder([4.0, 0.0, 1.0], 3.0),
    ];
    assert_eq!(
        parallel_cylinder_generator_candidates(secant_cylinders[0], secant_cylinders[1]).len(),
        2
    );
    let height = 5.0_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            parallel_cylinder_generator_candidates(
                secant_cylinders[0],
                secant_cylinders[1]
            ),
            [[2.0, height, -2.0], [2.0, height, 4.0]],
        ),
        Some((CurveGeometry::Line { origin, direction }, "parallel_cylinder_secant_generator"))
            if (origin.x - 2.0).abs() < 1e-12
                && (origin.y - height).abs() < 1e-12
                && direction.z == 1.0
    ));
    assert!(select_unique_curve_candidate(
        parallel_cylinder_generator_candidates(secant_cylinders[0], secant_cylinders[1]),
        [[2.0, 0.0, -2.0], [2.0, 0.0, 4.0]],
    )
    .is_none());

    let sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    let equator = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(equator, sphere),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_sphere_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
    ));
    assert_eq!(solve_carriers(&[equator, secant, sphere]), None);
    assert_eq!(
        solve_carriers(&[equator, tangent, sphere]),
        Some([2.0, 0.0, 0.0])
    );
    let second_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [4.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    let first_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    assert!(matches!(
        carrier_intersection_curve(first_sphere, second_sphere),
        Some((CurveGeometry::Circle { center, radius, .. }, "sphere_intersection_circle"))
            if center.x == 2.0 && (radius - 5.0_f64.sqrt()).abs() < 1e-12
    ));
    let sphere_circle_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 5.0_f64.sqrt(), 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    let sphere_circle_point = solve_carriers(&[first_sphere, second_sphere, sphere_circle_tangent])
        .expect("unique sphere-circle tangent point");
    assert!((sphere_circle_point[0] - 2.0).abs() < 1e-12);
    assert!((sphere_circle_point[1] - 5.0_f64.sqrt()).abs() < 1e-12);
    assert!(sphere_circle_point[2].abs() < 1e-12);
    let external_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [5.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    assert_eq!(
        solve_carriers(&[sphere, external_tangent_sphere, equator]),
        Some([2.0, 0.0, 0.0])
    );
    let noncoaxial_cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [1.0, 3.0_f64.sqrt(), 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(
        solve_carriers(&[sphere, tangent, noncoaxial_cylinder]),
        Some([2.0, 0.0, 0.0])
    );
    let enclosing_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 5.0,
    });
    let internally_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [3.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(
        solve_carriers(&[enclosing_sphere, internally_tangent_sphere, equator]),
        Some([5.0, 0.0, 0.0])
    );
    assert!(matches!(
        carrier_intersection_curve(cylinder, sphere),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_sphere_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
    ));
    assert_eq!(
        solve_carriers(&[cylinder, sphere, tangent]),
        Some([2.0, 0.0, 0.0])
    );
    assert!(carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 1.0), sphere,).is_none());
    let coaxial_secant = parallel_cylinder([0.0, 0.0, 0.0], 1.0);
    let sphere_offset = 3.0_f64.sqrt();
    assert_eq!(
        coaxial_cylinder_sphere_circle_candidates(coaxial_secant, sphere).len(),
        2
    );
    assert!(matches!(
        select_unique_curve_candidate(
            coaxial_cylinder_sphere_circle_candidates(coaxial_secant, sphere),
            [[1.0, 0.0, sphere_offset], [-1.0, 0.0, sphere_offset]],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_sphere_secant_circle"))
            if (center.z - sphere_offset).abs() < 1e-12 && radius == 1.0
    ));
    assert!(select_unique_curve_candidate(
        coaxial_cylinder_sphere_circle_candidates(coaxial_secant, sphere),
        [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]],
    )
    .is_none());
    let upper_circle_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [
            f64::midpoint(1.0, sphere_offset),
            0.0,
            f64::midpoint(1.0, sphere_offset),
        ],
        normal: [1.0, 0.0, 1.0],
    });
    let solved = solve_carriers(&[coaxial_secant, sphere, upper_circle_tangent])
        .expect("unique upper-circle tangent");
    assert!((solved[0] - 1.0).abs() < 1e-12);
    assert!(solved[1].abs() < 1e-12);
    assert!((solved[2] - sphere_offset).abs() < 1e-12);
    assert_eq!(
        solve_carriers(&[
            coaxial_secant,
            sphere,
            CarrierEquation::Plane(PlaneEquation {
                origin: [1.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            }),
        ]),
        None
    );

    let cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    assert!(matches!(
        carrier_intersection_curve(cap, cone),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_cone_circle"))
            if center == Point3::new(0.0, 0.0, 3.0) && (radius - 5.0).abs() < 1e-12
    ));
    let elliptical_cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    assert!(matches!(
        carrier_intersection_curve(cap, elliptical_cone),
        Some((
            CurveGeometry::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            },
            "plane_cone_parallel_ellipse"
        )) if center == Point3::new(0.0, 0.0, 3.0)
            && (major_radius - 5.0).abs() < 1e-12
            && (minor_radius - 2.5).abs() < 1e-12
    ));
    let elliptical_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [5.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[elliptical_cone, cap, elliptical_tangent]),
        Some([5.0, 0.0, 3.0])
    );
    let elliptical_secant = CarrierEquation::Plane(PlaneEquation {
        origin: [3.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[elliptical_cone, cap, elliptical_secant]),
        None
    );
    let inverse_sqrt_two = 1.0 / 2.0_f64.sqrt();
    let cone_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, -2.0],
        normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_tangent_plane, cone),
        Some((CurveGeometry::Line { origin, direction }, "plane_cone_tangent_line"))
            if origin.x.abs() < 1e-12
                && origin.y.abs() < 1e-12
                && (origin.z + 2.0).abs() < 1e-12
                && (direction.x + inverse_sqrt_two).abs() < 1e-12
                && (direction.z - inverse_sqrt_two).abs() < 1e-12
    ));
    let (elliptical_tangent_geometry, elliptical_tangent_tag) =
        carrier_intersection_curve(cone_tangent_plane, elliptical_cone)
            .expect("elliptical cone tangent generator");
    assert_eq!(elliptical_tangent_tag, "plane_cone_tangent_line");
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&elliptical_tangent_geometry, parameter)
            .expect("elliptical tangent point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, cone_tangent_plane));
        assert!(point_on_carrier(point, elliptical_cone));
    }
    let cone_ellipse_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [-0.2, 0.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_ellipse_plane, cone),
        Some((
            CurveGeometry::Ellipse {
                major_radius,
                minor_radius,
                ..
            },
            "plane_cone_ellipse"
        )) if major_radius > minor_radius && minor_radius > 0.0
    ));
    let cone_parabola_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_parabola_plane, cone),
        Some((
            CurveGeometry::Parabola { focal_distance, .. },
            "plane_cone_parabola"
        )) if focal_distance > 0.0
    ));
    let cone_hyperbola_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [1.0, 0.0, 0.2],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_hyperbola_plane, cone),
        Some((
            CurveGeometry::Hyperbola {
                major_radius,
                minor_radius,
                ..
            },
            "plane_cone_hyperbola"
        )) if major_radius > 0.0 && minor_radius > 0.0
    ));
    let rotated_ellipse_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [-0.2, -0.3, 1.0],
    });
    for (plane, expected_tag) in [
        (rotated_ellipse_plane, "plane_cone_ellipse"),
        (cone_parabola_plane, "plane_cone_parabola"),
        (cone_hyperbola_plane, "plane_cone_hyperbola"),
    ] {
        let (geometry, tag) =
            carrier_intersection_curve(plane, elliptical_cone).expect("elliptical cone conic");
        assert_eq!(tag, expected_tag);
        for parameter in [-1.0, 0.0, 1.0] {
            let point = cadmpeg_ir::eval::curve_point(&geometry, parameter).expect("conic point");
            let point = [point.x, point.y, point.z];
            assert!(point_on_carrier(point, plane));
            assert!(point_on_carrier(point, elliptical_cone));
        }
    }
    let cone_degenerate_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, -2.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert!(carrier_intersection_curve(cone_degenerate_plane, cone).is_none());
    let cone_generators = apex_plane_cone_generator_candidates(cone_degenerate_plane, cone);
    assert_eq!(cone_generators.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            cone_generators,
            [[0.0, 1.0, -1.0], [0.0, 2.0, 0.0]],
        ),
        Some((CurveGeometry::Line { origin, .. }, "plane_cone_secant_generator"))
            if (origin.z + 2.0).abs() < 1e-12
    ));
    let elliptical_generators =
        apex_plane_cone_generator_candidates(cone_degenerate_plane, elliptical_cone);
    assert_eq!(elliptical_generators.len(), 2);
    let (elliptical_generator, tag) =
        select_unique_curve_candidate(elliptical_generators, [[0.0, 1.0, 0.0], [0.0, 2.0, 2.0]])
            .expect("selected elliptical cone generator");
    assert_eq!(tag, "plane_cone_secant_generator");
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&elliptical_generator, parameter)
            .expect("elliptical generator point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, cone_degenerate_plane));
        assert!(point_on_carrier(point, elliptical_cone));
    }
    assert_eq!(solve_carriers(&[cone, cap, tangent]), None);
    let cone_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [5.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[cone, cap, cone_tangent]),
        Some([5.0, 0.0, 3.0])
    );
    let cone_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0_f64.sqrt(),
    });
    assert!(matches!(
        carrier_intersection_curve(cone_tangent_sphere, cone),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_tangent_circle"))
            if (center.z + 1.0).abs() < 1e-12 && (radius - 1.0).abs() < 1e-12
    ));
    let cone_sphere_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [1.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    let cone_sphere_vertex =
        solve_carriers(&[cone_tangent_sphere, cone, cone_sphere_plane]).expect("unique vertex");
    assert!((cone_sphere_vertex[0] - 1.0).abs() < 1e-12);
    assert!(cone_sphere_vertex[1].abs() < 1e-12);
    assert!((cone_sphere_vertex[2] + 1.0).abs() < 1e-12);
    assert!(carrier_intersection_curve(sphere, cone).is_none());
    let cone_secant_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 5.0,
    });
    let cone_sphere_candidates = coaxial_cone_sphere_circle_candidates(cone, cone_secant_sphere);
    assert_eq!(cone_sphere_candidates.len(), 2);
    let upper_parameter = (-4.0 + 184.0_f64.sqrt()) / 4.0;
    let upper_radius = 2.0 + upper_parameter;
    assert!(matches!(
        select_unique_curve_candidate(
            cone_sphere_candidates,
            [
                [upper_radius, 0.0, upper_parameter],
                [0.0, upper_radius, upper_parameter],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_secant_circle"))
            if (center.z - upper_parameter).abs() < 1e-12
                && (radius - upper_radius).abs() < 1e-12
    ));

    let coaxial_cone_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 3.0);
    assert!(carrier_intersection_curve(cone, coaxial_cone_cylinder).is_none());
    let cone_cylinder_candidates =
        coaxial_cone_cylinder_circle_candidates(cone, coaxial_cone_cylinder);
    assert_eq!(cone_cylinder_candidates.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            cone_cylinder_candidates,
            [[3.0, 0.0, 1.0], [0.0, 3.0, 1.0]],
        ),
        Some((
            CurveGeometry::Circle { center, radius, .. },
            "coaxial_cone_cylinder_secant_circle"
        )) if (center.z - 1.0).abs() < 1e-12 && radius == 3.0
    ));
    let cone_cylinder_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [4.0, 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let cone_cylinder_vertex =
        solve_carriers(&[cone, coaxial_cone_cylinder, cone_cylinder_tangent_plane])
            .expect("unique cone-cylinder circle tangent");
    assert!((cone_cylinder_vertex[0] - 3.0).abs() < 1e-12);
    assert!(cone_cylinder_vertex[1].abs() < 1e-12);
    assert!((cone_cylinder_vertex[2] - 1.0).abs() < 1e-12);
    assert!(
        coaxial_cone_cylinder_circle_candidates(cone, parallel_cylinder([1.0, 0.0, 0.0], 3.0),)
            .is_empty()
    );

    let torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 5.0,
        minor_radius: 2.0,
    });
    let torus_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [0.0, 0.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(torus_tangent, torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 2.0) && radius == 5.0
    ));
    assert!(carrier_intersection_curve(equator, torus).is_none());
    let plane_torus_candidates = axis_normal_plane_torus_circle_candidates(equator, torus);
    assert_eq!(plane_torus_candidates.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            plane_torus_candidates,
            [[7.0, 0.0, 0.0], [0.0, 7.0, 0.0]],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_secant_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
    ));
    let outer_tangent_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 7.0);
    assert!(matches!(
        carrier_intersection_curve(outer_tangent_cylinder, torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
    ));
    let secant_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 6.0);
    let cylinder_torus_candidates =
        coaxial_cylinder_torus_circle_candidates(secant_cylinder, torus);
    assert_eq!(cylinder_torus_candidates.len(), 2);
    let section_height = 3.0_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            cylinder_torus_candidates,
            [
                [6.0, 0.0, section_height],
                [0.0, 6.0, section_height],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_secant_circle"))
            if (center.z - section_height).abs() < 1e-12 && radius == 6.0
    ));
    let outer_circle_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [7.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[outer_tangent_cylinder, torus, outer_circle_tangent]),
        Some([7.0, 0.0, 0.0])
    );
    assert!(carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 6.0), torus).is_none());
    let torus_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 3.0,
    });
    assert!(matches!(
        carrier_intersection_curve(torus_tangent_sphere, torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && (radius - 3.0).abs() < 1e-12
    ));
    let torus_secant_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 5.0,
    });
    let sphere_torus_candidates =
        coaxial_sphere_torus_circle_candidates(torus_secant_sphere, torus);
    assert_eq!(sphere_torus_candidates.len(), 2);
    let sphere_torus_height = 3.84_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            sphere_torus_candidates,
            [
                [4.6, 0.0, sphere_torus_height],
                [0.0, 4.6, sphere_torus_height],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_secant_circle"))
            if (center.z - sphere_torus_height).abs() < 1e-12
                && (radius - 4.6).abs() < 1e-12
    ));
    let torus_sphere_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [3.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[torus_tangent_sphere, torus, torus_sphere_plane]),
        Some([3.0, 0.0, 0.0])
    );
    let outer_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [7.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    let oblique_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, -1.0],
    });
    assert_eq!(
        solve_carriers(&[torus, outer_tangent_plane, oblique_tangent_plane]),
        Some([7.0, 0.0, 0.0])
    );
    let axial_plane = PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let equatorial_plane = PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    };
    let mut secant_points = intersect_two_planes_with_torus(
        axial_plane,
        equatorial_plane,
        match torus {
            CarrierEquation::Torus(torus) => torus,
            _ => unreachable!(),
        },
    );
    secant_points.sort_by(|left, right| left[0].total_cmp(&right[0]));
    assert_eq!(
        secant_points,
        vec![
            [-7.0, 0.0, 0.0],
            [-3.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [7.0, 0.0, 0.0]
        ]
    );
    assert!(carrier_intersection_curve(sphere, torus).is_none());
    let second_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 9.0,
        minor_radius: 2.0,
    });
    assert!(matches!(
        carrier_intersection_curve(torus, second_torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && (radius - 7.0).abs() < 1e-12
    ));
    let secant_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 6.0,
        minor_radius: 2.0,
    });
    let tori_candidates = coaxial_tori_circle_candidates(torus, secant_torus);
    assert_eq!(tori_candidates.len(), 2);
    let tori_height = 3.75_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            tori_candidates,
            [[5.5, 0.0, tori_height], [0.0, 5.5, tori_height]],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_secant_circle"))
            if (center.z - tori_height).abs() < 1e-12
                && (radius - 5.5).abs() < 1e-12
    ));
    let tori_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [7.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[torus, second_torus, tori_plane]),
        Some([7.0, 0.0, 0.0])
    );
    assert!(point_on_carrier([5.0, 0.0, 2.0], torus));
    assert!(!point_on_carrier([5.0, 0.0, 0.0], torus));
    assert_eq!(
        solve_carriers(&[torus, torus_tangent, cone_tangent]),
        Some([5.0, 0.0, 2.0])
    );
}

#[cfg(test)]
fn section_axis_line_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let variable_points = definition.variables.as_ref()?.reconciled_points().0;
    section_axis_line_carrier_with_points(&variable_points, segment)
}

#[cfg(test)]
fn section_segment_intersection_carrier(
    definition: &crate::feature::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SectionIntersectionCarrier> {
    let missing_line = saved_section_missing_line_geometry(definition);
    let variable_points = definition
        .variables
        .as_ref()
        .map(|variables| variables.reconciled_points().0)
        .unwrap_or_default();
    section_segment_intersection_carrier_with_missing_line(
        definition,
        radii,
        points,
        segment,
        missing_line.as_ref(),
        &variable_points,
    )
}

#[cfg(test)]
fn trimmed_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let missing_line = saved_section_missing_line_geometry(definition);
    trimmed_section_segment_geometry_with_missing_line(
        definition,
        points,
        trim_vertices,
        segment,
        missing_line.as_ref(),
    )
}

#[cfg(test)]
fn extruded_segment_surface(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SurfaceGeometry> {
    extruded_geometry_surface(transform, &section_segment_geometry(points, segment)?)
}

#[cfg(test)]
fn placed_section_curve_geometry(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<CurveGeometry> {
    placed_section_geometry_curve(transform, &section_segment_geometry(points, segment)?)
}

#[cfg(test)]
fn section_skamp_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    section_skamp_constraints_for_geometry(definition, sketch, None)
}
