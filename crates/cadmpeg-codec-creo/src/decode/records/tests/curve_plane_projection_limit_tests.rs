// SPDX-License-Identifier: Apache-2.0
use crate::axis::Axis;
use crate::curve::{
    dummy_curve_prototype, CurvePrototypeTopology, FcCurveCoordinates, PrototypePcurveEndpoints,
};
use crate::datum::{DatumCylinder, DatumPlane, DatumPlaneRecord};
use crate::decode::records::{
    curve_prototype_records, curve_prototype_topology_records, datum_cylinder_records,
    datum_plane_records, fc_curve_coordinate_records, feature_placement_instruction_records,
    feature_section_transform_records, outline_plane_records, plane_envelope_records,
    plane_local_system_records, prototype_pcurve_records,
};
use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition};
use crate::placement::FeatureSectionTransform;
use crate::surface::{
    LocalSystemClassification, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
    PositionalCylinderFrame,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::units::UnitVector3;

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.curves.fc_coordinates.push(FcCurveCoordinates {
        curve_id: 8,
        subtype: 1,
        body: vec![0xfc, 1],
        values_mm: vec![2.0],
        tokens: Vec::new(),
        opaque_spans: Vec::new(),
        offset: 3,
    });
    scan.curves
        .prototype_pcurves
        .push(PrototypePcurveEndpoints {
            curve_id: 8,
            face_0_endpoints: [[0.0, 0.0], [1.0, 0.0]],
            face_1_endpoints: [[0.0, 0.0], [1.0, 0.0]],
            offset: 5,
        });
    scan.curves.prototype_topology.push(CurvePrototypeTopology {
        curve_id: 8,
        faces: [None, None],
        next_edges: [0, 0],
        offset: 7,
    });
    scan.curves.prototypes.push(dummy_curve_prototype());
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 3,
        body: vec![0xe3],
        slots: [None; 12],
        layout: None,
        classification: LocalSystemClassification::Simple,
        row_offset: 13,
        offset: 15,
    });
    scan.planes.envelopes.push(PlaneEnvelopeRecord {
        surface_id: 3,
        body: vec![0xe3],
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [[None; 3]; 2],
        },
        corner_coordinate_equal: [None; 3],
        scalar_tokens: vec![vec![0xf9]],
        row_offset: 13,
        offset: 17,
    });
    scan.planes.outlines.push(OutlinePlane {
        surface_id: 3,
        origin: [0.0, 0.0, 0.0],
        normal: UnitVector3::Z_AXIS,
        u_axis: UnitVector3::X_AXIS,
        offset: 18,
    });
    scan.planes.datums.push(
        DatumPlaneRecord::new(
            3,
            2,
            DatumPlane::new(Axis::X, 1.0).expect("valid datum fixture"),
            1.0,
            [[None; 2]; 2],
            19,
        )
        .expect("valid datum fixture"),
    );
    scan.planes.datum_cylinders.push(DatumCylinder {
        id: 4,
        feature_id: 2,
        reversed: false,
        frame: PositionalCylinderFrame::new(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            2.0,
            None,
        )
        .expect("valid cylinder frame"),
        offset_in_payload: 20,
    });
    scan.features.section_transforms.push(
        FeatureSectionTransform::new(
            2,
            Some(2),
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            21,
        )
        .expect("orthonormal test frame"),
    );
    scan.features.definitions.push(FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: None,
            owner_feature_id: Some(2),
        },
        body: b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
                \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6"
            .to_vec(),
        parameter_frames: Vec::new(),
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
        offset: 1000,
    });
    scan
}

macro_rules! collection_limit_test {
        ($name:ident, $project:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let scan = scan();
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some($operation), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          ($project)(&trial_ctx, &scan).map(|_| ())
      });

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let Err(error) = ($project)(&ctx, &scan) else { panic!("one native record exceeds the collection limit") };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

collection_limit_test!(
    fc_curve_coordinate_record_refuses_limit,
    fc_curve_coordinate_records,
    "creo native FC curve coordinate records"
);
collection_limit_test!(
    prototype_pcurve_record_refuses_limit,
    prototype_pcurve_records,
    "creo native prototype pcurve records"
);
collection_limit_test!(
    curve_prototype_topology_record_refuses_limit,
    curve_prototype_topology_records,
    "creo native curve prototype topology records"
);
collection_limit_test!(
    curve_prototype_record_refuses_limit,
    |ctx, scan| curve_prototype_records(ctx, scan, &scan.curves.prototypes, "creo:curve:prototype"),
    "creo native curve prototype records"
);
collection_limit_test!(
    plane_local_system_record_refuses_limit,
    |ctx, scan| plane_local_system_records(
        ctx,
        scan,
        &scan.planes.local_systems,
        "creo:surface:plane_local_system"
    ),
    "creo native plane local system records"
);
collection_limit_test!(
    plane_envelope_record_refuses_limit,
    |ctx, scan| plane_envelope_records(
        ctx,
        scan,
        &scan.planes.envelopes,
        "creo:surface:plane_envelope"
    ),
    "creo native plane envelope records"
);
collection_limit_test!(
    outline_plane_record_refuses_limit,
    |ctx, scan| outline_plane_records(
        ctx,
        scan,
        &scan.planes.outlines,
        "creo:surface:outline_plane"
    ),
    "creo native outline plane records"
);
collection_limit_test!(
    datum_plane_record_refuses_limit,
    datum_plane_records,
    "creo native datum plane records"
);
collection_limit_test!(
    datum_cylinder_record_refuses_limit,
    datum_cylinder_records,
    "creo native datum cylinder records"
);
collection_limit_test!(
    section_transform_record_refuses_limit,
    feature_section_transform_records,
    "creo native section transform records"
);
collection_limit_test!(
    placement_instruction_record_refuses_limit,
    feature_placement_instruction_records,
    "creo native placement instruction records"
);

#[test]
fn borrowed_curve_and_plane_projection_preserves_json() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let (fc, _fc_storage) = fc_curve_coordinate_records(&ctx, &scan).expect("record is admitted");
    let (plane, _plane_storage) = plane_envelope_records(
        &ctx,
        &scan,
        &scan.planes.envelopes,
        "creo:surface:plane_envelope",
    )
    .expect("record is admitted");
    let fc = serde_json::to_value(&fc[0]).expect("record serializes");
    let plane = serde_json::to_value(&plane[0]).expect("record serializes");
    assert_eq!(fc["body"], serde_json::json!([0xfc, 1]));
    assert_eq!(fc["values_mm"], serde_json::json!([2.0]));
    assert_eq!(plane["scalar_tokens"], serde_json::json!([[0xf9]]));
}
