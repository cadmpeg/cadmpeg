// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion saves that insert an STL cube as a mesh body, then move it or remove it.

use std::io::Cursor;

use cadmpeg_ir::features::mesh::MeshSelection;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::tessellation::Meshed;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::loss::F3dLossCode;
use crate::F3dCodec;

const TOLERANCE_MM: f64 = 1.0e-5;

#[test]
fn a_moved_mesh_body_is_the_current_mesh_its_move_selects() {
    // Fusion: Insert Mesh of a 10 mm cube from the origin, then Move/Copy of
    // the mesh body by 15 mm along X.
    let result = F3dCodec
        .decode(
            &mut Cursor::new(&include_bytes!("../../../../corpus/f3d/mesh_import_move.f3d")[..]),
            &DecodeOptions::default(),
        )
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let model = &result.ir().model;

    let [mesh] = model.tessellations.as_slice() else {
        panic!("one mesh body");
    };
    assert!(
        matches!(mesh.of, Meshed::Definition { .. }),
        "a mesh body is geometry of its definition"
    );
    assert_eq!(mesh.state, None, "the stored mesh is the current mesh");
    assert_eq!(mesh.triangle_count(), 12);
    let mut low = [f64::MAX; 3];
    let mut high = [f64::MIN; 3];
    for vertex in mesh.vertices() {
        let vertex = vertex.get();
        for (axis, value) in [vertex.x, vertex.y, vertex.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    for (actual, expected) in low
        .into_iter()
        .chain(high)
        .zip([15.0, 0.0, 0.0, 25.0, 10.0, 10.0])
    {
        assert!((actual - expected).abs() < TOLERANCE_MM, "{low:?} {high:?}");
    }

    let import = model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Base Mesh Feature"))
        .expect("the mesh import");
    let FeatureDefinition::Operation(FeatureOperation::MeshImport { tessellations }) =
        import.evaluation.definition()
    else {
        panic!("a mesh import");
    };
    assert_eq!(
        serde_json::to_value(tessellations).unwrap(),
        serde_json::json!({"kind": "tessellations", "value": [mesh.id.as_str()]})
    );

    let modify = model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Modify"))
        .expect("the move");
    let FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation,
        ..
    }) = modify.evaluation.definition()
    else {
        panic!("a body move");
    };
    let BodySelection::Local { bodies, .. } = bodies else {
        panic!("the move selects the mesh body: {bodies:?}");
    };
    assert_eq!(bodies.as_slice(), [mesh.id.as_str().to_owned()]);
    let translation = translation.get();
    assert!((translation.x - 15.0).abs() < TOLERANCE_MM);
    assert!(translation.y.abs() < TOLERANCE_MM && translation.z.abs() < TOLERANCE_MM);
    assert!(modify.dependencies.contains(&import.id));

    let charged = |code: F3dLossCode| {
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.kind())
    };
    assert!(charged(F3dLossCode::FeatureMeshBodySelectionAssumed));
    assert!(!charged(F3dLossCode::FeatureBodySelectionNative));
}

#[test]
fn a_removed_mesh_body_keeps_its_import_and_removal_native() {
    // Fusion: Insert Mesh of a 10 mm cube, then MeshRemove of the mesh body.
    let result = F3dCodec
        .decode(
            &mut Cursor::new(&include_bytes!("../../../../corpus/f3d/mesh_import_remove.f3d")[..]),
            &DecodeOptions::default(),
        )
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let model = &result.ir().model;
    assert!(model.tessellations.is_empty());
    let definition = |tag: &str| {
        model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(tag))
            .unwrap_or_else(|| panic!("feature {tag}"))
            .evaluation
            .definition()
    };
    let FeatureDefinition::Operation(FeatureOperation::MeshImport {
        tessellations:
            MeshSelection::Native {
                members: Some(imported),
                ..
            },
    }) = definition("Base Mesh Feature")
    else {
        panic!("the mesh import keeps its native source");
    };
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Local {
            bodies: removed, ..
        },
        ..
    }) = definition("MeshRemove")
    else {
        panic!("the mesh removal keeps its source body record");
    };
    assert_eq!(imported.as_slice(), removed.as_slice());
    let charged = |code: F3dLossCode| {
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.kind())
    };
    assert!(charged(
        F3dLossCode::FeatureMeshImportTessellationsUnresolved
    ));
    assert!(charged(F3dLossCode::MeshContainerUnjoined));
}
