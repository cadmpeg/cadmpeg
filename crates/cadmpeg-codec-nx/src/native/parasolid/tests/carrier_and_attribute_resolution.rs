use super::*;

#[test]
fn parasolid_attribute_definition_requires_declared_printable_name_and_field_record() {
    let mut bytes = vec![0xaa, 0x00, 0x4f, 0xff];
    bytes.extend_from_slice(&16u32.to_be_bytes());
    bytes.extend_from_slice(&0x012au16.to_be_bytes());
    bytes.extend_from_slice(b"SDL/TYSA_DENSITY");
    bytes.extend_from_slice(&[0x00, 0x50, 0x00, 0x00, 0x00, 0x01]);
    bytes.extend_from_slice(&0x012bu16.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&0x012au16.to_be_bytes());
    bytes.extend_from_slice(&9000u32.to_be_bytes());
    bytes.extend_from_slice(&[0, 1, 2, 3, 4, 5, 6, 0]);
    bytes.extend_from_slice(&0x0030u16.to_be_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]);
    bytes.push(2);
    let definitions = crate::parasolid::attribute_definitions(&bytes);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].offset, 26);
    assert_eq!(definitions[0].xmt, 0x12b);
    assert_eq!(definitions[0].identifier_xmt, 0x12a);
    assert_eq!(definitions[0].identifier_offset, 1);
    assert_eq!(definitions[0].name, "SDL/TYSA_DENSITY");
    assert_eq!(definitions[0].next_definition_xmt, 1);
    assert_eq!(definitions[0].type_id, 9000);
    assert_eq!(definitions[0].action_codes, [0, 1, 2, 3, 4, 5, 6, 0]);
    assert_eq!(definitions[0].field_names_xmt, 0x30);
    assert_eq!(definitions[0].legal_owner_flags[4], 1);
    assert_eq!(definitions[0].legal_owner_flags[12], 1);
    assert_eq!(definitions[0].legal_owner_flag_count, 16);
    assert_eq!(definitions[0].field_count, 1);
    assert_eq!(definitions[0].field_codes, [2]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(crate::parasolid::attribute_definitions(truncated).is_empty());

    let mut duplicate_identifier = bytes.clone();
    duplicate_identifier.splice(26..26, bytes[1..26].iter().copied());
    assert!(crate::parasolid::attribute_definitions(&duplicate_identifier).is_empty());

    bytes[42] = 7;
    assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
    bytes[42] = 0;
    bytes[52] = 2;
    assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
    bytes[52] = 0;
    bytes[20] = 0;
    assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
}

#[test]
fn parasolid_attribute_definition_accepts_fourteen_legal_owner_flags() {
    let mut bytes = vec![0, 0x4f];
    bytes.extend_from_slice(&5u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(b"CLASS");
    bytes.extend_from_slice(&[0, 0x50]);
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&20u16.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&8000u32.to_be_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    bytes.extend_from_slice(&[2, 3]);
    bytes.extend_from_slice(&[0, 0x4f]);

    let definitions = crate::parasolid::attribute_definitions(&bytes);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].xmt, 20);
    assert_eq!(definitions[0].legal_owner_flag_count, 14);
    assert_eq!(
        &definitions[0].legal_owner_flags[..14],
        [0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]
    );
    assert_eq!(&definitions[0].legal_owner_flags[14..], [0, 0]);
    assert_eq!(definitions[0].field_codes, [2, 3]);
}

#[test]
fn decode_preserves_offset_status_without_assigning_parameter_sense() {
    for discriminator in ['V', 'I', 'U'] {
        for true_offset in [false, true] {
            let mut stream = offset_surface_topology_partition_stream();
            let offset_record = stream.len() - 31;
            stream[offset_record + 19] = discriminator as u8;
            stream[offset_record + 20] = u8::from(true_offset);
            let mut cur = Cursor::new(prt_with_partition(&stream));
            let result = NxCodec
                .decode(&mut cur, &DecodeOptions::default())
                .expect("required invariant");

            let procedural = result
                .ir()
                .model
                .procedural_surfaces
                .first()
                .expect("offset surface");
            let ProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense,
                v_sense,
                extension_flags,
                ..
            } = procedural.definition()
            else {
                panic!("offset definition");
            };
            assert_eq!(*distance, 2.5);
            assert_eq!(*u_sense, None);
            assert_eq!(*v_sense, None);
            assert!(extension_flags.is_empty());
            assert_ne!(procedural.surface, *support);
            assert_eq!(result.ir().model.faces[0].surface, procedural.surface);
            let records = result
                .ir()
                .native
                .namespace("nx")
                .expect("required invariant")
                .arena_as::<super::super::ParasolidOffsetSurfaceRecord>(
                    "parasolid_offset_surface_records",
                )
                .expect("required invariant");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].discriminator, discriminator);
            assert_eq!(records[0].true_offset, true_offset);
            assert_eq!(records[0].support_xmt, 6);
            assert_eq!(records[0].distance, 2.5);
            let carrier = result
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == procedural.surface)
                .expect("offset carrier");
            assert_eq!(
                carrier
                    .source_object
                    .as_ref()
                    .map(|source| &source.object_id),
                Some(&records[0].id)
            );
            assert!(matches!(
                &carrier.geometry,
                SurfaceGeometry::Procedural { construction } if construction == &procedural.id
            ));
            assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
        }
    }
}

#[test]
fn decode_resolves_surface_curve_to_its_basis_curve() {
    let stream = surface_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");

    assert_eq!(result.ir().model.edges.len(), 1);
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidSurfaceCurveRecord>("parasolid_surface_curve_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].surface_xmt, 6);
    assert_eq!(records[0].pcurve_xmt, 9);
    assert_eq!(records[0].original_curve_xmt, 9);
    assert_eq!(records[0].tolerance_to_original, 0.000_01);
    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_emits_rolling_ball_blend_surface() {
    let stream = blend_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");

    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .first()
        .expect("blend surface");
    let ProceduralSurfaceDefinition::Blend {
        supports,
        radius,
        cross_section,
        spine,
        native,
    } = procedural.definition()
    else {
        panic!("blend definition");
    };
    assert_eq!(*cross_section, BlendCrossSection::Circular);
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -3.0
        }
    );
    assert_eq!(supports[0].as_ref().map(|side| side.reversed), Some(true));
    assert_eq!(supports[1].as_ref().map(|side| side.reversed), Some(false));
    assert!(spine.is_none());
    assert!(native.is_none());
    assert_eq!(result.ir().model.faces[0].surface, procedural.surface);
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidBlendSurfaceRecord>("parasolid_blend_surface_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].support_xmts, [6, 6]);
    assert_eq!(records[0].spine_xmt, 1);
    assert_eq!(records[0].offsets, [-3.0, 3.0]);
    assert_eq!(records[0].thumb_weights, [1.0, 1.0]);
    let carrier = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .expect("required invariant");
    assert_eq!(
        carrier
            .source_object
            .as_ref()
            .map(|association| association.object_id.as_str()),
        Some(records[0].id.as_str())
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_intersection_curve_as_connected_carrier() {
    let stream = intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");

    let edge_curve = result.ir().model.edges[0]
        .curve
        .as_ref()
        .expect("edge curve");
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == edge_curve)
        .expect("intersection carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidIntersectionRecord>("parasolid_intersection_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert!(!records[0].delta_twin);
    assert_eq!(records[0].header_references[0], 1);
    assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
    assert_eq!(
        curve.source_object.as_ref().map(|source| &source.object_id),
        Some(&records[0].id)
    );
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert_eq!(result.ir().model.procedural_curves[0].curve, curve.id);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == LossCategory::Geometry
            && loss.message.starts_with("1 surface-intersection record(s)")
    }));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_deltas_intersection_data_curve() {
    let mut partition = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = partition
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut partition, record + offset, 12);
    }
    let deltas = deltas_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");

    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidIntersectionRecord>("parasolid_intersection_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert!(records[0].delta_twin);
    assert_eq!(records[0].header_references[0], 1);
    assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_emits_charted_surface_intersection_construction() {
    let stream = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");

    let terms = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidTermUseRecord>("parasolid_term_use_records")
        .expect("required invariant");
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[0].count, 1);
    assert_eq!(terms[0].form, "L?");
    assert_eq!(terms[0].point, [0.0, 0.0, 0.0]);
    assert_eq!(terms[1].point, [10.0, 0.0, 0.0]);
    assert!(terms
        .iter()
        .all(|term| matches!(term.framing, crate::intersection::TermUseFraming::Direct)));
    let support_uv = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidSupportUvRecord>("parasolid_support_uv_records")
        .expect("required invariant");
    assert_eq!(support_uv.len(), 1);
    assert_eq!(support_uv[0].count, 4);
    assert_eq!(support_uv[0].marker, 2);
    assert_eq!(support_uv[0].values, [0.0, 0.0, 0.01, 0.0]);
    assert!(matches!(
        support_uv[0].framing,
        crate::intersection::SupportUvFraming::Direct
    ));
    let charts = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidChartRecord>("parasolid_chart_records")
        .expect("required invariant");
    assert_eq!(charts.len(), 1);
    assert_eq!(charts[0].count, 2);
    assert_eq!(charts[0].base_parameter, 0.0);
    assert_eq!(charts[0].base_scale, 1.0);
    assert_eq!(charts[0].chart_count, 2);
    assert_eq!(charts[0].chordal_error, 0.000_01);
    assert_eq!(charts[0].angular_error, 0.001);
    assert_eq!(charts[0].points, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    assert!(matches!(
        charts[0].point_layout,
        crate::intersection::ChartPointLayout::Xyz3
    ));

    let procedural = result
        .ir()
        .model
        .procedural_curves
        .first()
        .expect("intersection construction");
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .expect("solved chart cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("charted NURBS cache");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.control_points[0].x, 0.0);
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.01));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        procedural.definition()
    else {
        panic!("typed surface intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_none());
    assert_eq!(context.parameter_range, [0.0, 0.01]);
    assert!(result.ir().model.coedges[0].pcurves.is_empty());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code.category() == LossCategory::Geometry
            && loss.message.contains("surface-intersection record(s)")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_resolves_intersection_second_support_through_blend_bound() {
    let stream = blend_bound_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");

    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidBlendBoundRecord>("parasolid_blend_bound_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].header_references, [1; 5]);
    assert!(records[0].sense);
    assert_eq!(records[0].boundary_index, 0);
    assert_eq!(records[0].blend_surface_xmt, 13);
    assert_eq!(
        records[0].framing,
        crate::intersection::BlendBoundFraming::PartitionDirect
    );

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    let second = context.sides[1].surface.as_ref().expect("bridged support");
    assert_ne!(context.sides[0].surface.as_ref(), Some(second));
    assert!(context.sides[1].pcurve.is_some());
}

#[test]
fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
    let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let edge = result.ir().model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir().model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    let records = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::ParasolidTrimmedCurveRecord>("parasolid_trimmed_curve_records")
        .expect("required invariant");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].basis_xmt, 9);
    assert_eq!(records[0].points, [[0.0; 3]; 2]);
    assert_eq!(records[0].parameters, [0.000_25, 0.000_75]);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}
