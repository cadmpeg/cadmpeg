// SPDX-License-Identifier: Apache-2.0
//! STEP writer loss, schema, and strict-mode tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::loss::StepLossCode;
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

use super::round_trips::cylinder_surface_doc;

/// A one-face document whose single edge has no attributed curve, so the writer
/// must omit that edge and record a loss.
fn edgeless_doc() -> CadIr {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("p0".into()),
        position: Point3::new(0.0, 0.0, 0.0),
        source_object: None,
    });
    ir.model.points.push(Point {
        id: PointId("p1".into()),
        position: Point3::new(1.0, 0.0, 0.0),
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: VertexId("v0".into()),
        point: PointId("p0".into()),
        tolerance: None,
    });
    ir.model.vertices.push(Vertex {
        id: VertexId("v1".into()),
        point: PointId("p1".into()),
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("e0".into()),
        curve: None,
        start: VertexId("v0".into()),
        end: VertexId("v1".into()),
        param_range: None,
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId("s0".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.coedges.push(Coedge {
        id: CoedgeId("ce0".into()),
        owner_loop: LoopId("lp0".into()),
        edge: EdgeId("e0".into()),
        next: CoedgeId("ce0".into()),
        previous: CoedgeId("ce0".into()),
        radial_next: CoedgeId("ce0".into()),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
        use_curve_parameter_range: None,
    });
    ir.model.loops.push(Loop {
        id: LoopId("lp0".into()),
        face: FaceId("f0".into()),
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
        coedges: vec![CoedgeId("ce0".into())],
        vertex_uses: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: FaceId("f0".into()),
        shell: ShellId("sh0".into()),
        surface: SurfaceId("s0".into()),
        sense: Sense::Forward,
        loops: vec![LoopId("lp0".into())],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: ShellId("sh0".into()),
        region: RegionId("l0".into()),
        faces: vec![FaceId("f0".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: RegionId("l0".into()),
        body: BodyId("b0".into()),
        shells: vec![ShellId("sh0".into())],
    });
    ir.model.bodies.push(Body {
        id: BodyId("b0".into()),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: vec![RegionId("l0".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn writer_reports_unhandled_neutral_arenas_and_product_metadata() {
    let mut ir = unit_cube();
    ir.model.assets.push(cadmpeg_ir::assets::Asset {
        id: cadmpeg_ir::assets::AssetId("test:asset#texture".into()),
        name: Some("texture".into()),
        media_type: Some("image/png".into()),
        content: cadmpeg_ir::assets::AssetContent::External {
            uri: "urn:test:texture".into(),
        },
        native_ref: None,
    });
    ir.model
        .semantic_annotations
        .push(cadmpeg_ir::semantic_annotations::SemanticAnnotation {
            id: cadmpeg_ir::semantic_annotations::SemanticAnnotationId("test:semantic#note".into()),
            object: "note".into(),
            kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
            runtime_type: "TextNote".into(),
            order: 0,
            text: vec!["inspection note".into()],
            references: std::collections::BTreeMap::new(),
            value: None,
            format: None,
            position: None,
            parameters: std::collections::BTreeMap::new(),
            assets: Vec::new(),
            native_ref: "native-note".into(),
        });
    let mut bom_properties = std::collections::BTreeMap::new();
    bom_properties.insert("stock_code".into(), "A-1".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: "test:product#group".into(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Group,
            source_name: Some("Group".into()),
            label: Some("Group".into()),
            description: None,
            part_number: None,
            bom_properties,
            bodies: Vec::new(),
            native_ref: None,
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::DocumentAssetOmitted.kind()
            && loss.message.contains("1 document asset")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::SemanticAnnotationOmitted.kind()
            && loss.message.contains("1 semantic annotation")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ProductNonPartKind.kind()
            && loss.message.contains("non-part kind")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ProductBomPropertyOmitted.kind()
            && loss.message.contains("1 product BOM property")
    }));
}

#[test]
fn writer_reports_unrepresented_topology_metadata() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode topology metadata fixture")
        .into_parts()
        .0;
    ir.model.faces[0].tolerance = Some(0.01);
    ir.model.edges[0].tolerance = Some(0.02);
    ir.model.vertices[0].tolerance = Some(0.03);
    let edge_curve = ir.model.edges[0].curve.clone().expect("edge curve");
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("pcurve-backed coedge");
    coedge.pcurves[0].isoparametric = Some(true);
    coedge.pcurves[0].parameter_range = Some([0.0, 1.0]);
    coedge.use_curve = Some(edge_curve);
    coedge.use_curve_parameter_range = Some([0.0, 1.0]);

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes topology metadata fixture");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveUseNativeMetadata.kind()
            && loss.message.contains("1 pcurve use")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::CoedgeUseCurveNotRepresented.kind()
            && loss.message.contains("1 coedge-local 3D curve use")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyMetadataNotRepresented.kind()
            && loss.message.contains("topology metadata")
            && loss.message.contains("face tolerance=1")
            && loss.message.contains("edge tolerance=1")
            && loss.message.contains("vertex tolerance=1")
    }));
}

#[test]
fn writer_reports_root_occurrence_scale() {
    let mut ir = unit_cube();
    let product = cadmpeg_ir::ids::ProductDefinitionId("test:product#scaled".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Scaled part".into()),
            label: Some("Scaled part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: "test:occurrence#scaled".into(),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Root,
        ordinal: 0,
        transform: Transform::identity(),
        prototype_transform: Transform::identity(),
        scale: [2.0, 1.0, 1.0],
        name: Some("Scaled root".into()),
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        link_transform: None,
        native_ref: None,
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes unscaled geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RootOccurrencePlacementNotRepresentable.kind()
            && loss.message.contains("placement or scale")
    }));
}

#[test]
fn writer_reports_edge_loop_without_a_continuous_ordering() {
    let mut source = unit_cube();
    let edge_id = source
        .model
        .loops
        .iter()
        .find(|loop_| loop_.coedges.len() >= 3)
        .and_then(|loop_| loop_.coedges.first())
        .and_then(|coedge_id| {
            source
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
        })
        .map(|coedge| coedge.edge.clone())
        .expect("unit cube has a loop edge");
    source
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id == edge_id)
        .expect("loop edge exists")
        .start = cadmpeg_ir::ids::VertexId("missing-loop-vertex".into());

    let report = write_step(
        &source,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode should record the topology loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::LoopNoContinuousOrdering.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("continuous vertex-to-vertex")
    }));
}

#[test]
fn ap242_writer_reports_unrepresented_tessellation_triangle_metadata() {
    use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
    use cadmpeg_ir::tessellation::{TessellationTextureAssignment, TessellationTriangleGroup};

    let mut ir = unit_cube();
    let texture = AssetId("synthetic:test:asset#0".into());
    ir.model.assets.push(Asset {
        id: texture.clone(),
        name: None,
        media_type: Some("image/png".into()),
        content: AssetContent::Embedded { data: vec![0] },
        native_ref: None,
    });
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "synthetic:test:tessellation#triangle-metadata".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: vec![TessellationTriangleGroup {
                source_id: Some("synthetic:test:group#0".into()),
                triangles: vec![0],
            }],
            texture_assignments: vec![TessellationTextureAssignment {
                source_id: Some("synthetic:test:texture-resource#0".into()),
                texture,
                triangles: vec![0],
            }],
            channels: Vec::new(),
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write tessellation geometry");
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| {
                (loss.code == StepLossCode::TessellationTriangleGroups.kind()
                    || loss.code == StepLossCode::TessellationTextureAssignments.kind())
                    && loss.severity == cadmpeg_ir::Severity::Warning
            })
            .count(),
        2
    );
}

#[test]
fn writer_reports_occurrence_with_parent_without_local_product() {
    let mut ir = unit_cube();
    let product = cadmpeg_ir::ids::ProductDefinitionId("product-child".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Child part".into()),
            label: Some("Child part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    let parent = cadmpeg_ir::ids::OccurrenceId("external-parent".into());
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: parent.clone(),
        prototype: cadmpeg_ir::products::PrototypeReference::Unresolved,
        parent: cadmpeg_ir::products::OccurrenceParent::Root,
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        prototype_transform: cadmpeg_ir::transform::Transform::identity(),
        scale: [1.0; 3],
        name: None,
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        link_transform: None,
        native_ref: None,
    });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId("local-child".into()),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence: parent },
        ordinal: 1,
        transform: cadmpeg_ir::transform::Transform::identity(),
        prototype_transform: cadmpeg_ir::transform::Transform::identity(),
        scale: [1.0; 3],
        name: None,
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        link_transform: None,
        native_ref: None,
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the product graph");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AssemblyOccurrenceOmittedNoParentProduct.kind()
            && loss.message.contains("local-child")
            && loss
                .message
                .contains("parent has no local product definition")
    }));
}

#[test]
fn writer_reports_region_without_shells() {
    let mut ir = unit_cube();
    ir.model.regions[0].shells.clear();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RegionNoShellList.kind()
            && loss.message.contains("region(s) have no shell list")
    }));
}

#[test]
fn writer_reports_topology_without_an_emitted_region() {
    let mut ir = unit_cube();
    ir.model.regions.clear();
    ir.model.bodies[0].regions.clear();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the empty shape representation");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyUnreachableFromRegion.kind()
            && loss
                .message
                .contains("topology not reachable from any emitted region shape item")
            && loss.message.contains("face(s)")
            && loss.message.contains("vertex(s)")
    }));
}

#[test]
fn writer_reports_wire_region_without_connected_edges() {
    let mut ir = unit_cube();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.shells[0].faces.clear();
    ir.model.shells[0].wire_edges = vec![cadmpeg_ir::ids::EdgeId("missing-edge".into())];

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireRegionNoConnectedEdgeSet.kind()
            && loss
                .message
                .contains("wire region(s) had no writable connected edge set")
    }));
}

#[test]
fn writer_reports_wire_region_with_missing_shell_record() {
    let mut ir = unit_cube();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.regions[0].shells = vec![cadmpeg_ir::ids::ShellId("missing-shell".into())];

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireRegionMissingShell.kind()
            && loss.message.contains("missing shell records")
            && loss.message.contains("missing-shell")
    }));
}

#[test]
fn writer_reports_hidden_body_without_step_item() {
    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.bodies[0].visible = Some(false);
    ir.model.regions.clear();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::HiddenBodyOmitted.kind() && loss.message.contains(body.as_str())
    }));
}

#[test]
fn writer_reports_dangling_appearance_binding() {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let binding = "test:appearance-binding#dangling";
    let appearance = AppearanceId("test:appearance#missing".into());
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: binding.into(),
        target: AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AppearanceBindingMissingAsset.kind()
            && loss.message.contains(binding)
            && loss.message.contains(appearance.as_str())
    }));
}

#[test]
fn writer_reports_appearance_without_base_color() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let appearance = AppearanceId("test:appearance#colorless".into());
    let binding = "test:appearance-binding#colorless";
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: binding.into(),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AppearanceBindingNoBaseColor.kind()
            && loss.message.contains(binding)
            && loss.message.contains(appearance.as_str())
    }));
}

fn duplicate_target_style_ir(body_target: bool, reverse: bool, same_color: bool) -> CadIr {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let target = if body_target {
        AppearanceTarget::Body(ir.model.bodies[0].id.clone())
    } else {
        AppearanceTarget::Face(ir.model.faces[0].id.clone())
    };
    let red = AppearanceId("test:appearance#red".into());
    let blue = AppearanceId("test:appearance#blue".into());
    for (id, color) in [
        (
            red.clone(),
            cadmpeg_ir::topology::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        ),
        (
            blue.clone(),
            cadmpeg_ir::topology::Color {
                r: if same_color { 1.0 } else { 0.0 },
                g: 0.0,
                b: if same_color { 0.0 } else { 1.0 },
                a: 1.0,
            },
        ),
    ] {
        ir.model.appearances.push(Appearance {
            id,
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(color),
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        });
    }
    let mut bindings = vec![
        AppearanceBinding {
            id: "test:binding#red".into(),
            target: target.clone(),
            appearance: red,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::new(),
        },
        AppearanceBinding {
            id: "test:binding#blue".into(),
            target,
            appearance: blue,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::new(),
        },
    ];
    if reverse {
        bindings.reverse();
    }
    ir.model.appearance_bindings = bindings;
    ir
}

#[test]
fn writer_rejects_order_dependent_duplicate_target_styles() {
    for (body_target, target_kind) in [(true, "body"), (false, "face")] {
        let mut forward_output = Vec::new();
        let forward_report = write_step(
            &duplicate_target_style_ir(body_target, false, false),
            &mut forward_output,
            StepSchema::default(),
            &StepWriteOptions::default(),
        )
        .expect("report mode writes geometry while omitting the conflict");
        let mut reverse_output = Vec::new();
        let reverse_report = write_step(
            &duplicate_target_style_ir(body_target, true, false),
            &mut reverse_output,
            StepSchema::default(),
            &StepWriteOptions::default(),
        )
        .expect("reordered report mode writes geometry while omitting the conflict");
        assert_eq!(forward_output, reverse_output);
        for report in [forward_report, reverse_report] {
            assert_eq!(report.losses.len(), 1, "unexpected losses: {report:?}");
            let loss = &report.losses[0];
            assert_eq!(
                loss.code,
                StepLossCode::AppearanceBindingTargetConflict.kind()
            );
            assert!(loss.message.contains(target_kind));
            assert!(loss.message.contains("test:binding#red"));
            assert!(loss.message.contains("test:binding#blue"));
        }
        assert!(!String::from_utf8_lossy(&forward_output).contains("STYLED_ITEM"));

        let mut strict_output = Vec::new();
        assert!(matches!(
            write_step(
                &duplicate_target_style_ir(body_target, false, false),
                &mut strict_output,
                StepSchema::default(),
                &StepWriteOptions {
                    unsupported: StepUnsupportedPolicy::Reject,
                    ..StepWriteOptions::default()
                }
            ),
            Err(StepError::Unsupported(_))
        ));
        assert!(strict_output.is_empty());
    }

    let mut equivalent_output = Vec::new();
    let equivalent_report = write_step(
        &duplicate_target_style_ir(false, false, true),
        &mut equivalent_output,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("equal target styles are coalesced");
    assert!(equivalent_report.losses.is_empty(), "{equivalent_report:?}");
    assert_eq!(
        String::from_utf8_lossy(&equivalent_output)
            .matches("STYLED_ITEM")
            .count(),
        1
    );
}

#[test]
fn writer_reports_reduced_tessellation_metadata_and_body_links() {
    let mut ir = unit_cube();
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "test:step:tessellation#metadata".into(),
            body: Some(cadmpeg_ir::ids::BodyId("test:missing-body".into())),
            faces: vec![ir.model.faces[0].id.clone()],
            chordal_deflection: Some(0.01),
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: vec![cadmpeg_ir::tessellation::TessellationChannel {
                domain: cadmpeg_ir::tessellation::TessellationChannelDomain::Vertex,
                item_size: 2,
                kind: 1,
                flags: 0,
                count: 3,
                data: vec![0; 6],
                indices: Vec::new(),
            }],
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes reduced tessellation");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationBodyLinkUnwritable.kind()
            && loss
                .message
                .contains("has no writable AP242 tessellation link")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationMetadataReduced.kind()
            && loss.message.contains("face ownership link(s)")
            && loss.message.contains("chordal deflection")
            && loss.message.contains("data channel(s)")
    }));
}

#[test]
fn writer_reports_each_enclosing_topology_reduction_and_strict_mode_rejects() {
    let mut outer_face = unit_cube();
    outer_face.model.faces[0].loops.clear();
    let report = write_step(
        &outer_face,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving faces");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceNoWritableBounds.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("has no writable bounds")
    }));

    let mut inner_loop = unit_cube();
    inner_loop.model.faces[0]
        .loops
        .push(cadmpeg_ir::ids::LoopId(
            "step:data:loop#missing-inner".into(),
        ));
    let report = write_step(
        &inner_loop,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving outer loop");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceOmittedInnerLoop.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains("has no writable topology")
    }));

    let mut missing_edge = unit_cube();
    missing_edge.model.coedges[0].edge = cadmpeg_ir::ids::EdgeId("step:data:edge#missing".into());
    let report = write_step(
        &missing_edge,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving coedges");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::LoopEdgeMissingForOrder.kind()
            && loss.message.contains("loop")
            && loss.message.contains("edge")
    }));

    let mut missing_void = unit_cube();
    missing_void.model.regions[0]
        .shells
        .push(cadmpeg_ir::ids::ShellId(
            "step:data:shell#missing-void".into(),
        ));
    let report = write_step(
        &missing_void,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the outer shell");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RegionOmittedVoidShell.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("omitted void shell")
    }));

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    assert!(matches!(
        write_step(
            &missing_void,
            &mut Vec::new(),
            StepSchema::default(),
            &options
        ),
        Err(StepError::Unsupported(_))
    ));
}

#[test]
fn unsupported_pcurve_family_is_reported_and_strict_export_rejects() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::PcurveGeometry::Harmonic {
        center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        cosine: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        sine: cadmpeg_ir::math::Point2::new(0.0, 1.0),
    };

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable sheet");
    assert!(!String::from_utf8(output).unwrap().contains("PCURVE"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCarrierUnwritable.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains("step:data:pcurve#56")
    }));

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    assert!(matches!(
        write_step(&ir, &mut Vec::new(), StepSchema::default(), &options),
        Err(StepError::Unsupported(message)) if message.contains("pcurve")
    ));
}

#[test]
fn non_similarity_pcurve_replica_is_reported_and_strict_export_rejects() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::PcurveGeometry::Transformed {
        basis: Box::new(cadmpeg_ir::geometry::PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        }),
        transform: cadmpeg_ir::transform::Transform2 {
            rows: [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]],
        },
    };

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable sheet");
    assert!(!String::from_utf8(output).unwrap().contains("PCURVE"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCarrierUnwritable.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains("step:data:pcurve#56")
    }));

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    assert!(matches!(
        write_step(&ir, &mut Vec::new(), StepSchema::default(), &options),
        Err(StepError::Unsupported(message)) if message.contains("pcurve")
    ));
}

#[test]
fn unsupported_standalone_curve_is_reported_and_strict_export_rejects() {
    let mut ir = CadIr::empty(Units::default());
    let curve_id = CurveId("step:test:curve#standalone-unsupported".into());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: ProceduralCurveId("step:test:construction#standalone-unsupported".into()),
        },
        source_object: None,
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable subset");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(curve_id.as_str())
    }));

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    assert!(matches!(
        write_step(&ir, &mut Vec::new(), StepSchema::default(), &options),
        Err(StepError::Unsupported(message)) if message.contains("geometry carrier")
    ));
}

#[test]
fn consumed_unit_and_pmi_wrapper_records_are_strictly_writable() {
    for source in [
        include_bytes!("../../../tests/fixtures/ap242_degree_cone.p21").as_slice(),
        include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21").as_slice(),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode typed STEP wrappers");
        assert!(decoded
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .is_empty());
        let mut bytes = Vec::new();
        write_step(
            decoded.ir(),
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions {
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            },
        )
        .expect("strictly write typed STEP wrappers");
        assert!(!bytes.is_empty());
    }
}

#[test]
fn ap203e1_does_not_emit_invisibility_entities() {
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(false);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap203Edition1,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("hidden body visibility")));
}

#[test]
fn ap203e1_reports_hidden_appearance_visibility_loss() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let appearance = AppearanceId("test:appearance#hidden".into());
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.4,
            g: 0.5,
            b: 0.6,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#hidden-face".into(),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: Some(false),
        channels: std::collections::BTreeMap::new(),
    });

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap203Edition1,
        &StepWriteOptions::default(),
    )
    .expect("report-mode AP203e1 write");
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::HiddenAppearanceVisibilityUnsupported.kind() }));
}

#[test]
fn ap203e1_reports_hidden_presentation_layer_visibility_loss() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#hidden".into()),
        name: "hidden layer".into(),
        description: None,
        visible: Some(false),
        items: vec![PresentationItem::Body { body }],
    });
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap203Edition1,
        &StepWriteOptions::default(),
    )
    .expect("report-mode AP203e1 layer write");
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::HiddenPresentationLayerVisibilityUnsupported.kind()
    }));
}

#[test]
pub(crate) fn rejected_step_write_detects_incomplete_datum_system() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::PmiDefinition;

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .unwrap()
        .into_parts()
        .0;
    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .unwrap();
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    references[0].datum = PmiId("test:model:pmi#missing".into());
    let mut output = Vec::new();
    assert!(matches!(
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions {
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            }
        ),
        Err(StepError::Unsupported(_))
    ));
    assert!(output.is_empty());

    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .unwrap();
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    references.clear();
    assert!(matches!(
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions {
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            }
        ),
        Err(StepError::Unsupported(_))
    ));
    assert!(output.is_empty());
}

#[test]
fn step_writer_rejects_unknown_datum_reference_modifiers() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .expect("datum system");
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    references[0].modifiers.push("unknown_modifier".into());

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("report-mode STEP write");
    assert!(report.losses.iter().any(|loss| loss.code
        == StepLossCode::PmiAnnotationNotWritten.kind()
        || loss.code == StepLossCode::SemanticAnnotationOmitted.kind()));
    assert!(!String::from_utf8_lossy(&output).contains(".UNKNOWN_MODIFIER."));
    assert!(!String::from_utf8_lossy(&output).contains("DATUM_REFERENCE_MODIFIER_WITH_VALUE"));

    let mut strict_output = Vec::new();
    assert!(matches!(
        write_step(
            &ir,
            &mut strict_output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions {
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            }
        ),
        Err(StepError::Unsupported(_))
    ));
    assert!(strict_output.is_empty());
}

/// PMI is dropped whole when the target schema has no semantic PMI. The drop is
/// charged as `pmi.annotation-not-written` for every annotation, and strict mode
/// refuses the write. The schemas are driven by `supports_semantic_pmi`, so this
/// covers every non-AP242 target rather than one sampled schema.
#[test]
fn pmi_dropped_by_schema_without_semantic_pmi_is_charged() {
    let ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    let annotations = ir.model.pmi.len();
    assert!(annotations > 0);

    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        if schema.supports_semantic_pmi() {
            continue;
        }
        let mut output = Vec::new();
        let report = write_step(&ir, &mut output, schema, &StepWriteOptions::default())
            .expect("report-mode STEP write");
        let charged = report
            .losses
            .iter()
            .filter(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind())
            .count();
        assert_eq!(charged, 1, "{}", schema.file_schema());
        assert!(report.losses.iter().any(|loss| {
            loss.code == StepLossCode::PmiAnnotationNotWritten.kind()
                && loss.message.contains(&format!("{annotations} PMI"))
        }));

        let mut strict_output = Vec::new();
        assert!(
            matches!(
                write_step(
                    &ir,
                    &mut strict_output,
                    schema,
                    &StepWriteOptions {
                        unsupported: StepUnsupportedPolicy::Reject,
                        ..StepWriteOptions::default()
                    }
                ),
                Err(StepError::Unsupported(_))
            ),
            "{}",
            schema.file_schema()
        );
        assert!(strict_output.is_empty());
    }
}

#[test]
fn edge_without_curve_is_reported_and_omitted() {
    let _ = cylinder_surface_doc(); // keep helper exercised
    let ir = edgeless_doc();
    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();
    let curve = Curve {
        id: CurveId("unused".into()),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let _ = curve; // silence unused import path
    assert!(report
        .losses
        .iter()
        .any(|l| l.message.contains("edge(s) have no typed 3D curve")));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::EdgeNo3dCurve.kind()
            && loss
                .message
                .contains("was omitted because it has no 3D curve")
    }));
}

#[test]
fn subds_tessellations_and_source_associations_are_reported_as_losses() {
    let source_object = cadmpeg_ir::SourceObjectAssociation {
        format: "test".into(),
        object_id: "object-0".into(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut ir = unit_cube();
    ir.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:step:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        symmetries: Vec::new(),
        source_object: Some(source_object.clone()),
    });
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "test:step:tessellation#0".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: Some(source_object),
            vertices: Vec::new(),
            triangles: Vec::new(),
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("1 subdivision surface(s) were omitted")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("1 tessellation(s) require an AP242 target")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Metadata
            && loss
                .message
                .contains("2 source-object association(s) were not represented")
    }));
}

#[test]
fn face_on_unknown_surface_is_skipped_and_reported() {
    let mut ir = unit_cube();
    let target = ir.model.faces[0].surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == target {
            s.geometry = SurfaceGeometry::Unknown { record: None };
        }
    }
    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();
    let s = String::from_utf8(buf).unwrap();

    assert_eq!(
        s.matches("ADVANCED_FACE").count(),
        5,
        "the unknown-surface face should be omitted"
    );
    let unknown_notes: Vec<_> = report
        .losses
        .iter()
        .filter(|l| l.message.contains("rest on an unknown"))
        .collect();
    assert_eq!(
        unknown_notes.len(),
        1,
        "loss must be aggregated into a single counted note, got: {:?}",
        report.losses
    );
    assert!(unknown_notes[0].message.contains("1 face(s)"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ShellOmittedOuterFace.kind()
            && loss.message.contains("omitted face")
    }));
}

#[test]
fn unsupported_nested_and_polygonal_carriers_are_skipped_without_panicking() {
    let mut polygonal = unit_cube();
    let surface_id = polygonal.model.faces[0].surface.clone();
    polygonal
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = SurfaceGeometry::Polygonal {
        vertices: Vec::new(),
        triangles: Vec::new(),
        chordal_deflection: 0.1,
    };
    let report = write_step(
        &polygonal,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("polygonal face is reported as an export loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("unknown or STEP-unsupported surface")
    }));

    let mut nested_unknown = unit_cube();
    let curve_id = nested_unknown.model.edges[0].curve.clone().unwrap();
    nested_unknown
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Unknown { record: None }),
        transform: cadmpeg_ir::transform::Transform::identity(),
    };
    let report = write_step(
        &nested_unknown,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("transformed unknown curve is reported as an export loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("STEP-unsupported transform")
    }));
}

#[test]
fn procedural_surface_outside_the_writable_set_is_reported_not_panicked() {
    let mut ir = CadIr::empty(Units::default());
    let surface_id = SurfaceId("step:test:surface#unsupported".into());
    let construction_id =
        cadmpeg_ir::ids::ProceduralSurfaceId("step:test:construction:surface#unsupported".into());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction_id.clone(),
        },
        source_object: None,
    });
    ir.model
        .procedural_surfaces
        .push(cadmpeg_ir::geometry::ProceduralSurface {
            id: construction_id,
            surface: surface_id.clone(),
            definition: cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Compound {
                parameters: Vec::new(),
                components: Vec::new(),
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode must not panic on an unwritable procedural surface");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(surface_id.as_str())
    }));
}

#[test]
fn procedural_curve_outside_the_writable_set_is_reported_not_panicked() {
    let mut ir = CadIr::empty(Units::default());
    let curve_id = CurveId("step:test:curve#unsupported".into());
    let construction_id = ProceduralCurveId("step:test:construction:curve#unsupported".into());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction_id.clone(),
        },
        source_object: None,
    });
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: construction_id,
            curve: curve_id.clone(),
            definition: cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact,
            cache_fit_tolerance: None,
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode must not panic on an unwritable procedural curve");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(curve_id.as_str())
    }));
}

#[test]
fn strict_export_rejects_an_unwritable_procedural_carrier() {
    let mut ir = CadIr::empty(Units::default());
    let curve_id = CurveId("step:test:curve#strict-unsupported".into());
    let construction_id =
        ProceduralCurveId("step:test:construction:curve#strict-unsupported".into());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction_id.clone(),
        },
        source_object: None,
    });
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: construction_id,
            curve: curve_id,
            definition: cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact,
            cache_fit_tolerance: None,
        });

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    assert!(matches!(
        write_step(&ir, &mut output, StepSchema::default(), &options),
        Err(StepError::Unsupported(message)) if message.contains("geometry carrier")
    ));
    assert!(output.is_empty());
}

#[test]
fn signed_analytic_radius_normalization_is_reported() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -2.0,
    };

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("normalized to positive STEP radii")
    }));
}

#[test]
fn elliptical_cone_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 0.4,
        half_angle: 0.5,
    };

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("elliptical cone surface(s)")
    }));
}

#[test]
fn procedural_construction_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: ProceduralCurveId("generated_int_cur".into()),
            curve: ir.model.curves[0].id.clone(),
            definition: cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: std::array::from_fn(|_| cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                        pcurve_parameter_range: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: Some(0.01),
        });

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("reduced to their solved STEP carriers")));
}

#[test]
fn source_native_record_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            "asm-history-0",
            Default::default(),
        )],
    );
    ir.finalize();

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("source-native record(s) were not represented in STEP")));
}

#[test]
fn incomplete_nurbs_surface_is_omitted_and_reported() {
    let mut ir = cylinder_surface_doc();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        weights: None,
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    });

    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("report mode omits the invalid carrier");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.message.contains("'cyl'")
    }));
    assert!(!String::from_utf8(bytes)
        .expect("STEP output is UTF-8")
        .contains("B_SPLINE_SURFACE_WITH_KNOTS"));

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    let mut strict_bytes = Vec::new();
    let error = write_step(&ir, &mut strict_bytes, StepSchema::default(), &options)
        .expect_err("strict rejection");
    assert!(matches!(error, StepError::Unsupported(_)));
    assert!(strict_bytes.is_empty());
}

#[test]
pub(crate) fn strict_writer_rejects_before_emitting_bytes() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            "asm-history-0",
            Default::default(),
        )],
    );
    ir.finalize();
    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };

    let mut bytes = Vec::new();
    let error =
        write_step(&ir, &mut bytes, StepSchema::default(), &options).expect_err("strict rejection");
    assert!(matches!(error, StepError::Unsupported(_)));
    assert!(bytes.is_empty());
}

#[test]
pub(crate) fn strict_writer_refuses_retained_opaque_step_records_atomically() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap242_minimal.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode opaque STEP records");
    assert_eq!(decoded.ir().native_unknowns("step").unwrap().len(), 2);

    let mut bytes = Vec::new();
    let result = write_step(
        decoded.ir(),
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions {
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        },
    );
    assert!(matches!(result, Err(StepError::Unsupported(_))));
    assert!(bytes.is_empty());
}
