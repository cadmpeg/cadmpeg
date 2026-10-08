// SPDX-License-Identifier: Apache-2.0
use crate::curve::{
    dummy_depdb_curve_suffix, CurveParameterOpaqueSpan, CurveParameterRecord,
    CurveParameterReference, CurveParameterScalar, CurveTopologyRow, DepdbCurveRow,
};
use crate::decode::records::{
    cross_section_curve_row_records, curve_parameter_records, curve_topology_row_records,
    tabulated_cylinder_curve_replay_records,
};
use crate::surface::TabulatedCylinderCurveReplay;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let scalar = CurveParameterScalar {
        value: 2.0,
        raw: vec![0xf9, 0],
        offset: 1,
    };
    let reference = CurveParameterReference {
        entity_id: 9,
        offset: 3,
        length: 2,
    };
    let opaque = CurveParameterOpaqueSpan {
        raw: vec![0xe3],
        offset: 5,
    };
    scan.curves.parameters.push(CurveParameterRecord {
        curve_id: 8,
        type_byte: 1,
        body: vec![0xf9, 0, 0xe3],
        scalar_tokens: vec![scalar.clone()],
        references: vec![reference.clone()],
        opaque_spans: vec![opaque.clone()],
        reference_geometry: [0, 0],
        offset: 11,
        body_offset: 12,
        suffix_offset: 15,
    });
    scan.curves.cross_section_rows.push(DepdbCurveRow {
        id: 8,
        type_byte: 1,
        feature_id: 2,
        directions: [0, 0],
        suffix: dummy_depdb_curve_suffix(),
        body: vec![0xe3],
        scalar_tokens: vec![scalar],
        references: vec![reference],
        opaque_spans: vec![opaque],
        offset: 17,
    });
    scan.curves.topology_rows.push(CurveTopologyRow {
        id: 8,
        type_byte: 1,
        feature_id: 2,
        directions: [0, 0],
        faces: [None, None],
        next_edges: [0, 0],
        offset: 19,
    });
    scan.curves
        .tabulated_cylinder_replays
        .push(TabulatedCylinderCurveReplay {
            body: vec![0xf9, 0xe3],
            surface_id: 7,
            curve_id: 8,
            curve_type: 1,
            flip: 0,
            tangent_condition: 0,
            degree: 3,
            parameter_body: vec![0xf9],
            control_point_ids: [1, 2, 3, 4],
            successor_reference: 0,
            control_point_bodies: std::array::from_fn(|_| vec![0xe3]),
            control_points: [None; 4],
            terminal_reference: 0,
            offset: 23,
            surface_row_offset: 21,
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
                let Err(error) = ($project)(&ctx, &scan) else { panic!("native curve projection exceeds the collection limit") };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

collection_limit_test!(
    curve_parameter_count_nodes_refuse_limit,
    |ctx, scan| curve_parameter_records(ctx, scan, &scan.curves.parameters, "visibgeom"),
    "creo native curve parameter count nodes"
);
collection_limit_test!(
    curve_parameter_records_refuse_limit,
    |ctx, scan| curve_parameter_records(ctx, scan, &scan.curves.parameters, "visibgeom"),
    "creo native curve parameter records"
);
collection_limit_test!(
    cross_section_curve_count_nodes_refuse_limit,
    cross_section_curve_row_records,
    "creo native cross section curve count nodes"
);
collection_limit_test!(
    cross_section_curve_records_refuse_limit,
    cross_section_curve_row_records,
    "creo native cross section curve records"
);
collection_limit_test!(
    curve_topology_count_nodes_refuse_limit,
    |ctx, scan| curve_topology_row_records(ctx, scan, &scan.curves.topology_rows, "visibgeom"),
    "creo native curve topology count nodes"
);
collection_limit_test!(
    curve_topology_records_refuse_limit,
    |ctx, scan| curve_topology_row_records(ctx, scan, &scan.curves.topology_rows, "visibgeom"),
    "creo native curve topology records"
);
collection_limit_test!(
    tabulated_cylinder_replay_records_refuse_limit,
    tabulated_cylinder_curve_replay_records,
    "creo native tabulated cylinder replay records"
);

#[test]
fn borrowed_curve_projection_preserves_nested_json() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let (parameters, _parameters_storage) =
        curve_parameter_records(&ctx, &scan, &scan.curves.parameters, "visibgeom")
            .expect("parameters are admitted");
    let (replay, _replay_storage) =
        tabulated_cylinder_curve_replay_records(&ctx, &scan).expect("replay is admitted");
    let parameter = serde_json::to_value(&parameters[0]).expect("parameter serializes");
    let replay = serde_json::to_value(&replay[0]).expect("replay serializes");
    assert_eq!(parameter["scalar_values"], serde_json::json!([2.0]));
    assert_eq!(
        parameter["scalar_tokens"],
        serde_json::json!([{"value":2.0,"raw":[249,0],"offset":1,"length":2}])
    );
    assert_eq!(parameter["skipped_references"], serde_json::json!([9]));
    assert_eq!(
        parameter["references"],
        serde_json::json!([{"entity_id":9,"offset":3,"length":2}])
    );
    assert_eq!(
        parameter["opaque_spans"],
        serde_json::json!([{"raw":[227],"offset":5,"length":1}])
    );
    assert_eq!(
        replay["control_point_bodies"],
        serde_json::json!([[227], [227], [227], [227]])
    );
}
