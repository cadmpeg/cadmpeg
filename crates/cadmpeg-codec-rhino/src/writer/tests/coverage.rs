// SPDX-License-Identifier: Apache-2.0
//! Model arenas the 3DM write does not represent are reported, not dropped.

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

use crate::{RhinoArchiveVersion, RhinoCodec};

/// One point body, one part definition holding it, and two root occurrences
/// placing that part at x=100 and x=200.
fn placed_point_product() -> CadIr {
    let mut ir = CadIr::empty();
    let body: cadmpeg_ir::ids::BodyId = "cadir:model:body#pin".try_into().expect("identity");
    let region: cadmpeg_ir::ids::RegionId = "cadir:model:region#pin".try_into().expect("identity");
    let shell: cadmpeg_ir::ids::ShellId = "cadir:model:shell#pin".try_into().expect("identity");
    let vertex = cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#pin").expect("identity");
    let point = cadmpeg_ir::ids::PointId::mint("cadir:model:point#pin").expect("identity");
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell,
            region,
            Vec::new(),
            Vec::new(),
            vec![vertex.clone()],
        )
        .expect("free-vertex shell"),
    );
    ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
        id: vertex,
        point: point.clone(),
        tolerance: None,
    });
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite position"),
        None,
    ));
    let part: cadmpeg_ir::ids::ProductDefinitionId =
        "cadir:model:product#pin".try_into().expect("identity");
    ir.model.product_definitions.push(ProductDefinition {
        id: part.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: Some("Pin".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec![body],
        native_ref: None,
    });
    for (ordinal, x) in [(0, 100.0), (1, 200.0)] {
        ir.model.occurrences.push(Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint(format!(
                "cadir:model:occurrence#pin.{ordinal}"
            ))
            .expect("identity"),
            prototype: PrototypeReference::Local {
                definition: part.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal,
            transform: Transform::identity().with_translation(
                FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).expect("finite offset"),
            ),
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    ir.finalize(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");
    ir
}

#[test]
fn unrepresented_product_structure_is_an_export_loss() {
    let ir = placed_point_product();
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:?}", validation.findings);

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the point body is writable");
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export")
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        omitted,
        [
            "the rhino write does not represent model arena `product_definitions`: 1 record(s) omitted",
            "the rhino write does not represent model arena `occurrences`: 2 record(s) omitted",
        ]
    );
}
