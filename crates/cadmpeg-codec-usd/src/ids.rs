// SPDX-License-Identifier: Apache-2.0
//! Identities of the IR entities a layer decodes to.
//!
//! Every identity is `usd:stage:<kind>#<key>`. Occurrence keys are stage
//! prim paths. Definition keys are stage prim paths, or for the content of
//! an instance prototype, the prototype ordinal and the path relative to the
//! instance. Geometry keys extend the definition key, so one definition
//! names the same entities in every decode.

use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey,
    LoopId, OccurrenceId, PcurveId, PointId, ProceduralSurfaceId, ProductDefinitionId, RegionId,
    ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::tessellation::TessellationId;

fn key(text: &str) -> IdentityKey {
    IdentityKey::encode_segment(text)
}

pub(crate) fn definition(definition: &str) -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "definition"),
        key(definition),
    )
}

pub(crate) fn occurrence(path: &str) -> OccurrenceId {
    OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "occurrence"),
        key(path),
    )
}

/// The occurrence of external arc `ordinal` of the prim at `path`.
pub(crate) fn external_occurrence(path: &str, ordinal: usize) -> OccurrenceId {
    OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "occurrence"),
        key(path).colon(key(&format!("arc{ordinal}"))),
    )
}

pub(crate) fn body(definition: &str) -> BodyId {
    BodyId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "body"),
        key(definition),
    )
}

pub(crate) fn region(definition: &str) -> RegionId {
    RegionId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "region"),
        key(definition),
    )
}

pub(crate) fn shell(definition: &str) -> ShellId {
    ShellId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "shell"),
        key(definition),
    )
}

pub(crate) fn face(definition: &str) -> FaceId {
    FaceId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "face"),
        key(definition),
    )
}

pub(crate) fn surface(definition: &str) -> SurfaceId {
    SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "surface"),
        key(definition),
    )
}

pub(crate) fn tessellation(definition: &str) -> TessellationId {
    TessellationId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "tessellation"),
        key(definition),
    )
}

/// Curve `ordinal` of the batch of `definition`.
pub(crate) fn curve(definition: &str, ordinal: usize) -> CurveId {
    CurveId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "curve"),
        key(definition).colon(ordinal),
    )
}

pub(crate) fn edge(definition: &str, ordinal: usize) -> EdgeId {
    EdgeId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "edge"),
        key(definition).colon(ordinal),
    )
}

/// Vertex `end` (0 or 1) of curve `ordinal`.
pub(crate) fn vertex(definition: &str, ordinal: usize, end: usize) -> VertexId {
    VertexId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "vertex"),
        key(definition).colon(ordinal).colon(end),
    )
}

pub(crate) fn point(definition: &str, ordinal: usize, end: usize) -> PointId {
    PointId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "point"),
        key(definition).colon(ordinal).colon(end),
    )
}

/// The asset of the image with identity `identity`.
pub(crate) fn asset(identity: &str) -> cadmpeg_ir::assets::AssetId {
    cadmpeg_ir::assets::AssetId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "asset"),
        key(identity),
    )
}

/// The appearance of the material prim at `path`.
pub(crate) fn appearance(path: &str) -> AppearanceId {
    AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "appearance"),
        key(path),
    )
}

/// The binding of the material of the geometry subset at `subset` of
/// definition `definition`.
pub(crate) fn subset_binding(definition: &str, subset: &str) -> AppearanceBindingId {
    AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "subset-binding"),
        key(definition).colon(key(subset)),
    )
}

pub(crate) fn appearance_binding(definition: &str) -> AppearanceBindingId {
    AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "appearance-binding"),
        key(definition),
    )
}

/// The key of record `ordinal` of group `group` of the B-rep of a
/// definition.
fn brep_key(definition: &str, group: &str, ordinal: usize) -> IdentityKey {
    key(definition).colon(key(group)).colon(ordinal)
}

/// The identity of each record kind of a B-rep (spec §11.5, §17).
pub(crate) mod brep {
    use super::{
        brep_key, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId,
        ProceduralSurfaceId, RegionId, ShellId, SurfaceId, VertexId,
    };

    pub(crate) fn point(definition: &str, vertex: usize) -> PointId {
        PointId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "point"),
            brep_key(definition, "vertex", vertex),
        )
    }

    pub(crate) fn vertex(definition: &str, vertex: usize) -> VertexId {
        VertexId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "vertex"),
            brep_key(definition, "vertex", vertex),
        )
    }

    pub(crate) fn edge(definition: &str, edge: usize) -> EdgeId {
        EdgeId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "edge"),
            brep_key(definition, "edge", edge),
        )
    }

    /// A 3D curve of catalog `catalog` (`analytic` or `nurbs`).
    pub(crate) fn curve(definition: &str, catalog: &str, curve: usize) -> CurveId {
        CurveId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "curve"),
            brep_key(definition, &format!("curve3d-{catalog}"), curve),
        )
    }

    pub(crate) fn surface(definition: &str, catalog: &str, surface: usize) -> SurfaceId {
        SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "surface"),
            brep_key(definition, &format!("surface-{catalog}"), surface),
        )
    }

    pub(crate) fn pcurve(definition: &str, catalog: &str, pcurve: usize) -> PcurveId {
        PcurveId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "pcurve"),
            brep_key(definition, &format!("pcurve-{catalog}"), pcurve),
        )
    }

    pub(crate) fn construction(definition: &str, surface: usize) -> ProceduralSurfaceId {
        ProceduralSurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "construction"),
            brep_key(definition, "surface-analytic", surface),
        )
    }

    pub(crate) fn coedge(definition: &str, oriented_edge: usize) -> CoedgeId {
        CoedgeId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "coedge"),
            brep_key(definition, "orientedEdge", oriented_edge),
        )
    }

    pub(crate) fn loop_(definition: &str, loop_: usize) -> LoopId {
        LoopId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "loop"),
            brep_key(definition, "loop", loop_),
        )
    }

    pub(crate) fn face(definition: &str, face: usize) -> FaceId {
        FaceId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "face"),
            brep_key(definition, "face", face),
        )
    }

    pub(crate) fn shell(definition: &str, shell: usize) -> ShellId {
        ShellId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "shell"),
            brep_key(definition, "shell", shell),
        )
    }

    pub(crate) fn region(definition: &str, region: usize) -> RegionId {
        RegionId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "region"),
            brep_key(definition, "region", region),
        )
    }
}

/// The joint of the joint prim at `path`.
pub(crate) fn joint(path: &str) -> cadmpeg_ir::products::JointId {
    cadmpeg_ir::products::JointId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "joint"),
        key(path),
    )
}

/// The physical material of the material prim at `path`.
pub(crate) fn physical_material(path: &str) -> cadmpeg_ir::ids::PhysicalMaterialId {
    cadmpeg_ir::ids::PhysicalMaterialId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "physical-material"),
        key(path),
    )
}

/// The mass properties of the prim or definition `key`.
pub(crate) fn mass_properties(path: &str) -> cadmpeg_ir::ids::MassPropertiesId {
    cadmpeg_ir::ids::MassPropertiesId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "mass"),
        key(path),
    )
}

pub(crate) fn rigid_body(path: &str) -> cadmpeg_ir::ids::RigidBodyId {
    cadmpeg_ir::ids::RigidBodyId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "rigid-body"),
        key(path),
    )
}

pub(crate) fn collider(path: &str) -> cadmpeg_ir::ids::ColliderId {
    cadmpeg_ir::ids::ColliderId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "collider"),
        key(path),
    )
}

pub(crate) fn articulation(path: &str) -> cadmpeg_ir::ids::ArticulationId {
    cadmpeg_ir::ids::ArticulationId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "articulation"),
        key(path),
    )
}

pub(crate) fn physics_scene(path: &str) -> cadmpeg_ir::ids::PhysicsSceneId {
    cadmpeg_ir::ids::PhysicsSceneId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "physics-scene"),
        key(path),
    )
}

/// The settings of applied schema `schema` on the prim at `path`.
pub(crate) fn solver_settings(path: &str, schema: &str) -> cadmpeg_ir::ids::SolverSettingsId {
    cadmpeg_ir::ids::SolverSettingsId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "solver-settings"),
        key(&format!("{path}.{schema}")),
    )
}

/// The configuration of the `CadConfiguration` prim at `path`.
pub(crate) fn configuration(path: &str) -> cadmpeg_ir::features::ConfigurationId {
    cadmpeg_ir::features::ConfigurationId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "configuration"),
        key(path),
    )
}

/// The feature of the `CadFeature` prim at `path`.
pub(crate) fn feature(path: &str) -> cadmpeg_ir::features::FeatureId {
    cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("usd", "stage", "feature"),
        key(path),
    )
}

/// The identities of the records of a sketch (spec §18.2).
pub(crate) mod sketch {
    use super::key;
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};

    /// The sketch of the `CadSketch` prim at `path`.
    pub(crate) fn sketch(path: &str) -> SketchId {
        SketchId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "sketch"),
            key(path),
        )
    }

    /// The entity of the point or curve prim at `path`.
    pub(crate) fn entity(path: &str) -> SketchEntityId {
        SketchEntityId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "sketch-entity"),
            key(path),
        )
    }

    /// The constraint of the constraint prim at `path`.
    pub(crate) fn constraint(path: &str) -> SketchConstraintId {
        SketchConstraintId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "sketch-constraint"),
            key(path),
        )
    }

    /// The design parameter of the dimensional constraint or `CadParameter`
    /// prim at `path`.
    pub(crate) fn parameter(path: &str) -> ParameterId {
        ParameterId::compose(
            &cadmpeg_ir::identity_namespace!("usd", "stage", "parameter"),
            key(path),
        )
    }
}
