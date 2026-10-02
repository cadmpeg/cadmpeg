// SPDX-License-Identifier: Apache-2.0
//! Borrowed owner serialization composes with structural decode admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

fn check_wire<T: Serialize + DeserializeOwned>(wire: serde_json::Value) {
    let value: T = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), wire);
    let expected = serde_value::to_value(&value).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16384;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = crate::schema::structural::project(&ctx, &value, "project borrowed owner").unwrap();
    assert_eq!(*projected, expected);
    drop(projected);
    let storage = ctx.reserve_scoped(16384, "borrowed owner storage released").unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("owner serialization dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(limit) = crate::schema::structural::project(&ctx, &value, "project borrowed owner").unwrap_err() else {
            panic!("borrowed owner must preserve resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
    }
}

macro_rules! wire_case {
    ($name:ident, $owner:ty, $wire:expr) => {
        #[test]
        fn $name() { check_wire::<$owner>($wire); }
    };
}

wire_case!(borrowed_shell_members_wire, crate::topology::Shell, json!({"id":"test:model:shell#s","region":"test:model:region#r","members":[{"kind":"face","id":"test:model:face#f"},{"kind":"wire_edge","id":"test:model:edge#e"},{"kind":"free_vertex","id":"test:model:vertex#v"}]}));
wire_case!(borrowed_edge_carrier_wire, crate::topology::EdgeCarrier, json!({"curve":"test:model:curve#c","param_range":[0.0,1.0]}));
wire_case!(borrowed_feature_members_wire, crate::features::FeatureResultTopology, json!({"id":"test:model:feature-result-topology#r","output_of":"test:model:feature#f","members":[{"kind":"body","id":"local-body"},{"kind":"face","id":"local-face"},{"kind":"edge","id":"local-edge"},{"kind":"vertex","id":"local-vertex"}]}));
wire_case!(borrowed_equation_curve_wire, crate::features::FeatureEquationCurve, json!({"parameter":" t ","x_expression":"t*t","y_expression":"0","z_expression":"-t","start":-2.0,"end":3.0}));
wire_case!(borrowed_sketch_binding_wire, crate::features::SketchFeatureBinding, json!({"space":"planar","sketch":"test:model:sketch#s"}));
wire_case!(borrowed_coil_placement_wire, crate::features::CoilPlacement, json!({"kind":"native","native_ref":"native-scope"}));
wire_case!(borrowed_tspline_construction_wire, crate::geometry::TSplineSurfaceConstruction, json!({"parameter_ranges":[[0.0,1.0],[0.0,1.0]],"type_code":7,"subtransform":{"kind":"inline","program":"v 1 2","separator":null,"values":"e 3"},"trailing_value":4,"discontinuities":[[0.2],[0.3],[],[],[],[]],"discontinuity_flag":false}));
wire_case!(borrowed_compound_curve_wire, crate::geometry::CompoundCurveConstruction, json!({"parameters":[0.5],"components":[{"parameter":1.0,"component":"test:model:curve#c"}]}));
wire_case!(borrowed_composite_pattern_wire, crate::features::patterns::CompositePattern, json!([{"pattern":{"kind":"unresolved"}}]));
wire_case!(borrowed_datum_references_wire, crate::pmi::DatumReferences, json!([{"datum":"test:model:pmi#d","precedence":1,"modifiers":["native-modifier"]}]));
wire_case!(borrowed_dimension_wire, crate::pmi::PmiDimension, json!({"dimension":{"other":"native-dimension"}}));
wire_case!(borrowed_link_state_wire, crate::products::LinkState, json!({"members":[{"kind":"linked_subelement","subelement":"Face1"},{"kind":"element_component","component":"test:model:product#p"},{"kind":"claim_child","claim":true},{"kind":"copy_on_change","state":{"policy":{"policy":"native","native_policy":"source-policy"}}}]}));
wire_case!(borrowed_reference_target_wire, crate::references::ReferenceTarget, json!({"kind":"external","document":"external-document","object":"source-object"}));
wire_case!(borrowed_spreadsheet_cell_wire, crate::spreadsheets::SpreadsheetCell, json!({"address":"B12","parameter":"test:model:parameter#p"}));
wire_case!(borrowed_spreadsheet_range_wire, crate::spreadsheets::SpreadsheetRange, json!({"start":"B12","end":"C14"}));
wire_case!(borrowed_spreadsheet_wire, crate::spreadsheets::Spreadsheet, json!({"id":"test:model:spreadsheet#s","feature":"test:model:feature#f","cells":[{"address":"B12","parameter":"test:model:parameter#p"}],"column_widths":[{"name":"AA","pixels":5}],"row_heights":[{"name":"4294967295","pixels":7}],"merged_ranges":[{"start":"B12","end":"C14"}],"native_ref":"source-sheet"}));
wire_case!(borrowed_symmetry_wire, crate::subd::SubdSymmetry, json!({"kind":{"kind":"correspondence"},"plane":{"origin":{"x":0.0,"y":0.0,"z":0.0},"first_axis":{"x":1.0,"y":0.0,"z":0.0},"second_axis":{"x":0.0,"y":1.0,"z":0.0}},"face_pairs":[[0,1]],"edge_pairs":[[2,3]],"vertex_pairs":[[4,5]]}));
wire_case!(borrowed_tessellation_channel_wire, crate::tessellation::TessellationChannel, json!({"addressing":{"domain":"corner","indices":[0,1,0]},"item_size":1,"kind":2,"flags":3,"data":"AQI="}));
wire_case!(borrowed_tessellation_wire, crate::tessellation::Tessellation, json!({"id":"test:model:tessellation#t","body":"test:model:body#b","faces":["test:model:face#f"],"mesh":{"kind":"list","vertices":[{"x":0.0,"y":0.0,"z":0.0},{"x":1.0,"y":0.0,"z":0.0},{"x":0.0,"y":1.0,"z":0.0}],"triangles":[[0,1,2]]},"channels":[]}));
wire_case!(borrowed_exactness_field_wire, crate::annotations::FieldName, json!("geometry.normal"));
wire_case!(borrowed_exactness_map_wire, crate::annotations::NonEmptyMap, json!({"geometry.normal":"derived"}));
wire_case!(borrowed_stream_name_wire, crate::provenance::StreamName, json!("source-stream"));
wire_case!(borrowed_source_provenance_wire, crate::provenance::SourceProvenance, json!({"format":"test","stream":"source-stream","offset":7,"tag":"source-record"}));
wire_case!(borrowed_annotation_provenance_wire, crate::provenance::AnnotationProvenance, json!({"stream":"source-stream","offset":7,"tag":"source-record"}));
