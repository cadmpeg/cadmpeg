// SPDX-License-Identifier: Apache-2.0
//! Each loss code is emitted by the decode or export path that owns it.

use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{ChannelAddressing, TessellationChannel};

use crate::loss::UsdLossCode;
use crate::test_support::{decode, encode, prim_layer};

const TRIANGLE: &str = "int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\nuniform token subdivisionScheme = \"none\"";
const PATCH: &str = "int uVertexCount = 2\nint vVertexCount = 2\nint uOrder = 2\nint vOrder = 2\ndouble[] uKnots = [0, 0, 1, 1]\ndouble[] vKnots = [0, 0, 1, 1]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]";

fn emitted(losses: &[LossNote], code: UsdLossCode) -> bool {
    losses.iter().any(|loss| loss.code == code.kind())
}

fn assert_decode_emits(text: &str, code: UsdLossCode) {
    let result = decode(text);
    assert!(
        emitted(&result.report().losses, code),
        "{code:?} not emitted by:\n{text}\n{:#?}",
        result.report().losses
    );
}

fn mesh(body: &str) -> String {
    format!("#usda 1.0\ndef Mesh \"M\"\n{{\n{body}\n}}\n")
}

#[test]
fn composition_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\n(\n    subLayers = [@base.usda@]\n)\ndef \"A\" {\n}\n",
        UsdLossCode::CompositionSublayersNotComposed,
    );
    assert_decode_emits(
        "#usda 1.0\ndef \"A\" (\n    references = </Missing>\n)\n{\n}\n",
        UsdLossCode::CompositionTargetMissing,
    );
    assert_decode_emits(
        "#usda 1.0\ndef \"A\"\n{\n    variantSet \"v\" = {\n        \"x\" {\n        }\n    }\n}\n",
        UsdLossCode::CompositionVariantNotSelected,
    );
    assert_decode_emits(
        "#usda 1.0\nover \"A\" {\n}\n",
        UsdLossCode::CompositionOverUndefined,
    );
    assert_decode_emits(
        "#usda 1.0\ndef \"A\" (\n    references = </B>\n)\n{\n}\ndef \"B\" (\n    references = </A>\n)\n{\n}\n",
        UsdLossCode::CompositionCycle,
    );
    assert_decode_emits(
        "#usda 1.0\nclass \"P\" {\n}\ndef \"I\" (\n    instanceable = true\n    references = </P>\n)\n{\n    def \"Local\" {\n    }\n}\n",
        UsdLossCode::CompositionOverrideNotApplied,
    );
}

#[test]
fn prim_attribute_and_transform_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef Capsule_1 \"C\" {\n    double radiusTop = 1\n}\n",
        UsdLossCode::PrimIntrinsicNotTransferred,
    );
    assert_decode_emits(
        &prim_layer("double3 xformOp:translate.timeSamples = {\n    0: (1, 2, 3),\n}\nuniform token[] xformOpOrder = [\"xformOp:translate\"]"),
        UsdLossCode::AttributeAnimationNotTransferred,
    );
    assert_decode_emits(
        "#usda 1.0\n(\n    customLayerData = {\n        int a = 1\n    }\n)\ndef \"A\" {\n}\n",
        UsdLossCode::MetadataNotTransferred,
    );
    assert_decode_emits(
        &prim_layer("uniform token[] xformOpOrder = [\"xformOp:bogus\"]\nfloat xformOp:bogus = 1"),
        UsdLossCode::XformMalformed,
    );
}

#[test]
fn mesh_losses_are_emitted() {
    assert_decode_emits(
        &mesh("int[] faceVertexCounts = [3, 2]\nint[] faceVertexIndices = [0, 1, 2, 0, 1]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]"),
        UsdLossCode::MeshFaceRejected,
    );
    assert_decode_emits(
        &mesh("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]"),
        UsdLossCode::MeshRejected,
    );
    assert_decode_emits(
        &mesh(&format!(
            "{TRIANGLE}\ntexCoord2f[] primvars:st = [(0, 0)] (\n    interpolation = \"vertex\"\n)"
        )),
        UsdLossCode::MeshPrimvarNotTransferred,
    );
    assert_decode_emits(
        &mesh(&format!("{TRIANGLE}\nnormal3f[] normals = [(0, 0, 1)]")),
        UsdLossCode::MeshNormalsNotTransferred,
    );
    assert_decode_emits(
        &mesh(&format!("{TRIANGLE}\ndef GeomSubset \"S\"\n{{\n    uniform token elementType = \"point\"\n    int[] indices = [0]\n}}")),
        UsdLossCode::MeshSubsetNotTransferred,
    );
    assert_decode_emits(
        &mesh(&format!("{TRIANGLE}\ncolor3f[] primvars:displayColor = [(1, 0, 0), (0, 1, 0), (0, 0, 1)] (\n    interpolation = \"vertex\"\n)")),
        UsdLossCode::DisplayColorNotTransferred,
    );
    assert_decode_emits(
        &format!("#usda 1.0\ndef Material \"Mdl\"\n{{\n}}\ndef Mesh \"M\"\n{{\n{TRIANGLE}\nrel material:binding = </Mdl>\n}}\n"),
        UsdLossCode::MaterialNotTransferred,
    );
}

#[test]
fn curve_and_patch_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token basis = \"catmullRom\"\n    int[] curveVertexCounts = [3]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0)]\n}\n",
        UsdLossCode::CurvesRejected,
    );
    assert_decode_emits(
        &format!(
            "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{}\n}}\n",
            PATCH.replace("int uVertexCount = 2", "int uVertexCount = 3")
        ),
        UsdLossCode::NurbsPatchRejected,
    );
    assert_decode_emits(
        &format!("#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{PATCH}\ndouble2 uRange = (0, 0.5)\n}}\n"),
        UsdLossCode::NurbsRangeNotTransferred,
    );
}

#[test]
fn export_losses_are_emitted() {
    let curves = decode("#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0)]\n}\n");
    let (mut ir, _, _) = curves.into_parts();
    let circle = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .unwrap();
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle));
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportCurveNotWritten));

    let free = decode("#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0)]\n}\n");
    let (mut ir, _, _) = free.into_parts();
    ir.model.edges.clear();
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle));
    let (_, report) = encode(&ir);
    assert!(emitted(
        &report.losses,
        UsdLossCode::ExportCarrierNotWritten
    ));

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let cylinder = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .unwrap();
    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder));
    // An edge curve with no shape leaves the face unmeshed.
    let face = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == ir.model.surfaces[0].id)
        .unwrap();
    let loop_ = ir
        .model
        .loops
        .iter()
        .find(|loop_| face.loops.contains(&loop_.id))
        .unwrap();
    let coedge = ir
        .model
        .coedges
        .iter()
        .find(|coedge| loop_.coedges().contains(&coedge.id))
        .unwrap();
    let curve = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == coedge.edge)
        .unwrap()
        .curve()
        .unwrap()
        .clone();
    let slot = ir
        .model
        .curves
        .iter()
        .position(|candidate| candidate.id == curve)
        .unwrap();
    ir.model.curves[slot].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportFaceNotWritten));

    let meshed = decode(&mesh(TRIANGLE));
    let (mesh_ir, _, _) = meshed.into_parts();
    let patch = decode(&format!(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{PATCH}\n}}\n"
    ));
    let (mut ir, _, _) = patch.into_parts();
    let body = ir.model.bodies[0].id.clone();
    let tessellation = mesh_ir.model.tessellations[0].clone().with_body(Some(body));
    ir.model.tessellations.push(tessellation);
    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let (_, report) = encode(&ir);
    assert!(emitted(
        &report.losses,
        UsdLossCode::ExportTopologyTessellated
    ));

    let meshed = decode(&mesh(TRIANGLE));
    let (mut ir, _, _) = meshed.into_parts();
    let tessellation = ir.model.tessellations.remove(0);
    let channel =
        TessellationChannel::new(ChannelAddressing::Vertex {}, 4, 1, 0, vec![0; 12]).unwrap();
    let mesh = tessellation.mesh().clone();
    ir.model
        .tessellations
        .push(tessellation.with_mesh(mesh, vec![channel]).unwrap());
    let (_, report) = encode(&ir);
    assert!(emitted(
        &report.losses,
        UsdLossCode::ExportChannelNotWritten
    ));
}

#[test]
fn unknown_schema_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef CadBrep \"B\"\n{\n}\n",
        UsdLossCode::PrimTypeNotTransferred,
    );
    assert_decode_emits(
        &format!("#usda 1.0\ndef Mesh \"M\" (\n    prepend apiSchemas = [\"PhysxVehicleAPI\"]\n)\n{{\n{TRIANGLE}\n}}\n"),
        UsdLossCode::ApiSchemaNotTransferred,
    );
}

#[test]
fn physics_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body1 = </Missing>\n}\n",
        UsdLossCode::JointRejected,
    );
    assert_decode_emits(
        "#usda 1.0\ndef Xform \"B\"\n{\n}\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body1 = </B>\n    float physics:breakForce = 10\n}\n",
        UsdLossCode::JointPropertyNotTransferred,
    );
    let source = decode("#usda 1.0\ndef Xform \"A\"\n{\n}\ndef Xform \"B\"\n{\n}\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body0 = </A>\n    rel physics:body1 = </B>\n}\n");
    let (mut ir, _, _) = source.into_parts();
    let joint = ir.model.assembly_joints.remove(0);
    let connectors: Vec<_> = joint.connectors().cloned().collect();
    ir.model
        .assembly_joints
        .push(cadmpeg_ir::products::AssemblyJoint::paired(
            joint.id.clone(),
            cadmpeg_ir::products::PairedJointKind::Parallel {},
            [connectors[0].clone(), connectors[1].clone()],
            None,
        ));
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportJointNotWritten));
}

#[test]
fn an_unread_property_of_a_group_prim_is_reported() {
    assert_decode_emits(
        "#usda 1.0\ndef Xform \"A\"\n{\n    float custom:thing = 3\n    double3 xformOp:translate = (1, 0, 0)\n    uniform token[] xformOpOrder = [\"xformOp:translate\"]\n}\n",
        UsdLossCode::PropertyNotTransferred,
    );
}
