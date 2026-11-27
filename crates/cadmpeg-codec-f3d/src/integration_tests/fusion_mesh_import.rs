// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion saves that insert an STL cube as a mesh body, then move it,
//! remove it, or move and remove it.

use std::io::Cursor;

use cadmpeg_ir::design::refs::{Evaluated, Member, Position, StateRef};
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::tessellation::Meshed;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::loss::F3dLossCode;
use crate::F3dCodec;

const TOLERANCE_MM: f64 = 1.0e-5;

fn decode(bytes: &[u8]) -> cadmpeg_ir::DecodeResult {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    result
}

/// The component-wise low and high corners of the vertices of `mesh`.
fn bounds(mesh: &cadmpeg_ir::tessellation::Tessellation) -> [f64; 6] {
    let mut low = [f64::MAX; 3];
    let mut high = [f64::MIN; 3];
    for vertex in mesh.vertices() {
        let vertex = vertex.get();
        for (axis, value) in [vertex.x, vertex.y, vertex.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    [low[0], low[1], low[2], high[0], high[1], high[2]]
}

fn assert_bounds(found: [f64; 6], expected: [f64; 6]) {
    for (actual, wanted) in found.into_iter().zip(expected) {
        assert!(
            (actual - wanted).abs() < TOLERANCE_MM,
            "{found:?} {expected:?}"
        );
    }
}

#[test]
fn a_moved_mesh_body_is_the_current_mesh_its_move_selects() {
    // Fusion: Insert Mesh of a 10 mm cube from the origin, then Move/Copy of
    // the mesh body by 15 mm along X.
    let result = decode(include_bytes!(
        "../../../../corpus/f3d/mesh_import_move.f3d"
    ));
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
    assert_bounds(bounds(mesh), [15.0, 0.0, 0.0, 25.0, 10.0, 10.0]);

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
        cadmpeg_core::serde_erase::json::to_value(tessellations).unwrap(),
        cadmpeg_core::json!({"kind": "tessellations", "value": [mesh.id.as_str()]})
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
    assert!(charged(F3dLossCode::FeatureMeshImportPlacementCombined));
    assert!(!charged(F3dLossCode::FeatureBodySelectionNative));
}

/// The one mesh of a design whose `MeshRemove` removes its only mesh body,
/// after asserting that the import and the removal both name it and that its
/// state is the state after `before`, the feature tagged `before_tag`.
fn removed_mesh(result: &cadmpeg_ir::DecodeResult, before_tag: &str) -> [f64; 6] {
    let model = &result.ir().model;
    let [mesh] = model.tessellations.as_slice() else {
        panic!("one mesh body");
    };
    assert!(matches!(mesh.of, Meshed::Definition { .. }));
    let feature = |tag: &str| {
        model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(tag))
            .unwrap_or_else(|| panic!("feature {tag}"))
    };
    assert_eq!(
        mesh.state,
        Some(StateRef {
            configuration: None,
            of: Evaluated::State(Position::After(Member::Feature(
                feature(before_tag).id.clone()
            ))),
        }),
        "the stored mesh holds the state at the MeshRemove input"
    );
    let FeatureDefinition::Operation(FeatureOperation::MeshImport { tessellations }) =
        feature("Base Mesh Feature").evaluation.definition()
    else {
        panic!("a mesh import");
    };
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(tessellations).unwrap(),
        cadmpeg_core::json!({"kind": "tessellations", "value": [mesh.id.as_str()]})
    );
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Local { bodies, .. },
        ..
    }) = feature("MeshRemove").evaluation.definition()
    else {
        panic!("the mesh removal selects the mesh body");
    };
    assert_eq!(bodies.as_slice(), [mesh.id.as_str().to_owned()]);
    let charged = |code: F3dLossCode| {
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.kind())
    };
    for code in [
        F3dLossCode::FeatureMeshImportTessellationsUnresolved,
        F3dLossCode::FeatureMeshOperationSourceUnresolved,
        F3dLossCode::FeatureMeshRemovedStateUnresolved,
        F3dLossCode::MeshContainerUnjoined,
    ] {
        assert!(!charged(code), "{code:?}");
    }
    bounds(mesh)
}

#[test]
fn a_removed_mesh_body_holds_the_state_after_its_import() {
    // Fusion: Insert Mesh of a 10 mm cube from the origin, then MeshRemove.
    for bytes in [
        &include_bytes!("../../../../corpus/f3d/mesh_import_remove.f3d")[..],
        &include_bytes!("../../../../corpus/f3d/mesh_import_remove_control.f3d")[..],
    ] {
        let result = decode(bytes);
        assert_bounds(
            removed_mesh(&result, "Base Mesh Feature"),
            [0.0, 0.0, 0.0, 10.0, 10.0, 10.0],
        );
    }
}

#[test]
fn a_moved_then_removed_mesh_body_holds_the_state_after_its_move() {
    // Fusion: Insert Mesh of a 10 mm cube from the origin, Move/Copy of the
    // mesh body by 15 mm along X, then MeshRemove.
    let result = decode(include_bytes!(
        "../../../../corpus/f3d/mesh_import_move_remove.f3d"
    ));
    assert_bounds(
        removed_mesh(&result, "Modify"),
        [15.0, 0.0, 0.0, 25.0, 10.0, 10.0],
    );
}
