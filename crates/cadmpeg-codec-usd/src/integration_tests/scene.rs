// SPDX-License-Identifier: Apache-2.0
//! Scene options, and the stage forms that the `OpenUSD` validators check.

use cadmpeg_ir::codec::write::{SceneOptions, UpAxis};

use crate::test_support::{decode, encode, encode_with};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

/// A part of one body named `Plate`, from a definition named `Bracket`,
/// placed twice.
fn twice_placed_body() -> cadmpeg_ir::CadIr {
    let source = decode(
        "#usda 1.0\nclass \"Bracket\"\n{\n    def Mesh \"Plate\"\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n        uniform token subdivisionScheme = \"none\"\n    }\n}\ndef Xform \"Asm\"\n{\n    def \"A\" (\n        instanceable = true\n        references = </Bracket>\n    )\n    {\n    }\n    def \"B\" (\n        instanceable = true\n        references = </Bracket>\n    )\n    {\n    }\n}\n",
    );
    let mut ir = source.ir().clone();
    let model = &mut ir.model;
    let plate = model
        .product_definitions
        .iter()
        .find(|definition| definition.source_name.as_deref() == Some("Plate"))
        .expect("the Plate definition")
        .clone();
    model
        .product_definitions
        .retain(|definition| definition.id != plate.id);
    model
        .occurrences
        .retain(|occurrence| occurrence.prototype != plate.id);
    let bracket = model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.source_name.as_deref() == Some("Bracket"))
        .expect("the Bracket definition");
    bracket.occurrences.clear();
    bracket.bodies.clone_from(&plate.bodies);
    ir
}

const NURBS_PATCH: &str = r#"#usda 1.0
def NurbsPatch "Sheet"
{
    int uVertexCount = 2
    int vVertexCount = 2
    int uOrder = 2
    int vOrder = 2
    double[] uKnots = [0, 0, 1, 1]
    double[] vKnots = [0, 0, 1, 1]
    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]
}
def Mesh "Marker" (
    prepend apiSchemas = ["MaterialBindingAPI"]
)
{
    int[] faceVertexCounts = [3]
    int[] faceVertexIndices = [0, 1, 2]
    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]
    uniform token subdivisionScheme = "none"
    rel material:binding = </Looks/Red>
}
def Scope "Looks"
{
    def Material "Red"
    {
        token outputs:surface.connect = </Looks/Red/S.outputs:surface>
        def Shader "S"
        {
            uniform token info:id = "UsdPreviewSurface"
            color3f inputs:diffuseColor = (1, 0, 0)
            token outputs:surface
        }
    }
}
"#;

/// The text of each `GeomSubset` prim of `text`.
fn subsets(text: &str) -> Vec<&str> {
    text.split("def GeomSubset").skip(1).collect()
}

#[test]
fn every_face_subset_belongs_to_the_material_bind_family() {
    let r#box = decode(&fixture("brep_array_box.usda"));
    let mut untessellated = r#box.ir().clone();
    untessellated.model.tessellations.clear();
    for ir in [r#box.ir(), &untessellated] {
        let (text, _) = encode(ir);
        let written = subsets(&text);
        assert!(!written.is_empty(), "{text}");
        for subset in written {
            let body = subset.split("\n    }").next().unwrap_or_default();
            assert!(
                body.contains("uniform token familyName = \"materialBind\""),
                "{subset}"
            );
        }
    }
}

/// A body with no tessellation of its own is written with a mesh of its
/// faces beside its `BrepArray`. Each face material binds to the triangles of
/// its faces in that mesh, an imageable prim, and the B-rep faces keep their
/// bindings.
#[test]
fn a_fallback_mesh_binds_the_materials_of_its_faces() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let r#box = decode(&fixture("brep_array_box.usda"));
    let mut ir = r#box.ir().clone();
    ir.model.tessellations.clear();
    let (text, _) = encode(&ir);
    let mesh_prim = text.split("def Mesh").nth(1).expect("a fallback mesh");
    assert!(
        mesh_prim.contains("rel material:binding = </Looks/Box_face_1_material>"),
        "{text}"
    );
    let round = decode(&text);
    let model = &round.ir().model;
    let face_bindings = model
        .appearance_bindings
        .iter()
        .filter(|binding| matches!(binding.target.entity, AppearanceTarget::Face(_)))
        .count();
    assert_eq!(face_bindings, 6, "{text}");
    // Every face of the box binds the one material, so its subset holds
    // every triangle of the mesh.
    let [mesh] = model.tessellations.as_slice() else {
        panic!("one fallback mesh: {text}");
    };
    let groups: Vec<(Option<&str>, usize)> = mesh
        .triangle_groups()
        .iter()
        .map(|group| (group.source_id.as_deref(), group.triangles.len()))
        .collect();
    assert_eq!(
        groups,
        [(
            Some("/Occurrence/Mesh/Box_face_1_material"),
            mesh.triangles().len()
        )],
        "{text}"
    );
}

#[test]
fn a_patch_binds_the_material_of_its_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let source = decode(NURBS_PATCH);
    let mut ir = source.ir().clone();
    let [face] = ir.geometry.faces.as_slice() else {
        panic!("one face");
    };
    let face = face.id.clone();
    let [marker] = ir.model.appearance_bindings.as_slice() else {
        panic!("the binding of the marker mesh");
    };
    let mut binding = marker.clone();
    binding.id = crate::ids::appearance_binding("/Sheet:face");
    binding.target.entity = AppearanceTarget::Face(face);
    ir.model.appearance_bindings.push(binding);
    let (text, _) = encode(&ir);
    let patch = text
        .split("def NurbsPatch")
        .nth(1)
        .expect("a NurbsPatch prim");
    assert!(
        patch.contains("rel material:binding = </Looks/Red>"),
        "{text}"
    );
}

#[test]
fn a_twice_placed_body_keeps_its_name_in_its_prototype() {
    let (text, _) = encode(&twice_placed_body());
    let prototypes = text
        .split("class \"Prototypes\"")
        .nth(1)
        .expect("a Prototypes prim");
    assert!(prototypes.contains("def Xform \"Bracket\""), "{text}");
    assert!(prototypes.contains("def Mesh \"Plate\""), "{text}");
    assert_eq!(text.matches("def Mesh").count(), 1, "{text}");
    assert_eq!(
        text.matches("prepend references = </Prototypes/Bracket>")
            .count(),
        2,
        "{text}"
    );
}

#[test]
fn without_materialx_a_material_has_its_preview_surface_only() {
    let materials = decode(&fixture("materials.usda"));
    let (default, _) = encode(materials.ir());
    assert!(default.contains("outputs:mtlx:surface"), "{default}");
    let (text, _) = encode_with(
        materials.ir(),
        SceneOptions {
            materialx: false,
            ..SceneOptions::DEFAULT
        },
    );
    assert!(!text.contains("mtlx"), "{text}");
    assert!(!text.contains("ND_"), "{text}");
    assert_eq!(
        text.matches("info:id = \"UsdPreviewSurface\"").count(),
        default.matches("info:id = \"UsdPreviewSurface\"").count(),
        "{text}"
    );
}

#[test]
fn without_the_exact_brep_a_body_is_its_meshes() {
    let r#box = decode(&fixture("brep_array_box.usda"));
    let (text, report) = encode_with(
        r#box.ir(),
        SceneOptions {
            exact_brep: false,
            ..SceneOptions::DEFAULT
        },
    );
    assert!(!text.contains("BrepArray"), "{text}");
    assert!(text.contains("def Mesh"), "{text}");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::UsdLossCode::ExportTopologyTessellated.kind()));
}

/// A Y-up stage in centimetres: the points are in centimetres, and the
/// root prim turns -90 degrees about X, which takes +Z to +Y.
#[test]
fn a_y_up_centimetre_stage_turns_and_scales_the_model() {
    let r#box = decode(&fixture("brep_array_box.usda"));
    let mut ir = r#box.ir().clone();
    ir.model.tessellations.clear();
    let metres = SceneOptions {
        exact_brep: false,
        ..SceneOptions::DEFAULT
    };
    let centimetres = SceneOptions {
        up_axis: Some(UpAxis::Y),
        meters_per_unit: Some(0.01),
        ..metres
    };
    let (z_up, _) = encode_with(&ir, metres);
    let (y_up, _) = encode_with(&ir, centimetres);
    assert!(y_up.contains("    metersPerUnit = 0.01\n"), "{y_up}");
    assert!(y_up.contains("    upAxis = \"Y\"\n"), "{y_up}");
    assert!(
        y_up.contains("double xformOp:rotateX:upAxis = -90"),
        "{y_up}"
    );
    assert!(
        y_up.contains("uniform token[] xformOpOrder = [\"xformOp:rotateX:upAxis\""),
        "{y_up}"
    );
    let points = |text: &str| -> Vec<f64> {
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with("point3f[] points"))
            .expect("a points line");
        line.split(['(', ')', ',', '[', ']', '='])
            .filter_map(|value| value.trim().parse::<f64>().ok())
            .collect()
    };
    let (metre_points, centimetre_points) = (points(&z_up), points(&y_up));
    assert_eq!(metre_points.len(), centimetre_points.len());
    for (metre, centimetre) in metre_points.iter().zip(&centimetre_points) {
        assert!(
            (metre * 100.0 - centimetre).abs() <= 1e-6 * centimetre.abs().max(1.0),
            "{metre} m, {centimetre} cm"
        );
    }
    // The decoded model keeps its size in millimetres and its points in
    // place, and gains the turn as the transform of the root occurrence.
    let round = decode(&y_up);
    let turned = round
        .ir()
        .model
        .occurrences
        .iter()
        .find_map(|occurrence| occurrence.transform.as_ref())
        .expect("a transform");
    // The rows of the map (x, y, z) -> (x, z, -y).
    let [r0, r1, r2] = turned.affine_rows();
    let expected = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
    ];
    for (row, want) in [r0, r1, r2].iter().zip(expected) {
        for (value, want) in row.iter().zip(want) {
            assert!((value - want).abs() < 1e-12, "{row:?}");
        }
    }
}

#[test]
fn a_turned_stage_turns_its_gravity_and_scales_its_magnitude() {
    let arm = decode(&fixture("physx_arm.usda"));
    let (text, _) = encode_with(
        arm.ir(),
        SceneOptions {
            up_axis: Some(UpAxis::Y),
            meters_per_unit: Some(0.01),
            ..SceneOptions::DEFAULT
        },
    );
    // The source points gravity along -Z at 981 cm/s^2.
    assert!(
        text.contains("vector3f physics:gravityDirection = (0, -1, 0)"),
        "{text}"
    );
    assert!(
        text.contains("float physics:gravityMagnitude = 981\n"),
        "{text}"
    );
}

/// A Z-up rigid body moving along +Z at 10 cm/s and turning about +Z at
/// 90 degrees per second. Velocities are in the space that the prim's
/// transform maps into: on the turned root prim `A` that is the Y-up stage,
/// so both point along +Y; on `B` inside the turned root `Asm` it is the
/// model's own space, so both keep +Z.
#[test]
fn a_turned_stage_turns_the_velocities_of_a_root_prim() {
    let body = |name: &str| {
        format!(
            "def Xform \"{name}\" (\n    prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n)\n{{\n    vector3f physics:velocity = (0, 0, 10)\n    vector3f physics:angularVelocity = (0, 0, 90)\n}}\n"
        )
    };
    let header = "#usda 1.0\n(\n    metersPerUnit = 0.01\n    upAxis = \"Z\"\n)\n";
    let y_up = SceneOptions {
        up_axis: Some(UpAxis::Y),
        meters_per_unit: Some(0.01),
        ..SceneOptions::DEFAULT
    };
    let root = decode(&format!("{header}{}", body("A")));
    let (text, _) = encode_with(root.ir(), y_up);
    assert!(
        text.contains("double xformOp:rotateX:upAxis = -90"),
        "{text}"
    );
    assert!(
        text.contains("vector3f physics:velocity = (0, 10, 0)"),
        "{text}"
    );
    assert!(
        text.contains("vector3f physics:angularVelocity = (0, 90, 0)"),
        "{text}"
    );
    let nested = decode(&format!("{header}def Xform \"Asm\"\n{{\n{}}}\n", body("B")));
    let (text, _) = encode_with(nested.ir(), y_up);
    assert!(
        text.contains("double xformOp:rotateX:upAxis = -90"),
        "{text}"
    );
    assert!(
        text.contains("vector3f physics:velocity = (0, 0, 10)"),
        "{text}"
    );
    assert!(
        text.contains("vector3f physics:angularVelocity = (0, 0, 90)"),
        "{text}"
    );
}

#[test]
fn metres_per_unit_is_a_finite_positive_number() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let r#box = decode(&fixture("brep_array_box.usda"));
    for metres in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let result = crate::UsdCodec.plan(
            EncodeInput::new(r#box.ir(), None).with_scene(SceneOptions {
                meters_per_unit: Some(metres),
                ..SceneOptions::DEFAULT
            }),
            TargetRequest::Explicit("usda-1.0"),
        );
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::InvalidInput(_))),
            "{metres}"
        );
    }
}
