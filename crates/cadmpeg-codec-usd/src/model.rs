// SPDX-License-Identifier: Apache-2.0
//! The neutral model of a composed layer.
//!
//! Each scene prim becomes a product definition and an occurrence of it
//! under its parent's occurrence. Geometry prims add their bodies to their
//! definition, in the prim's local coordinates. The descendants of instance
//! prims that share a prototype share definitions. A reference or payload
//! to another layer is an occurrence of an external prototype.
//!
//! Lengths are scaled from scene units to millimetres (spec §10.1). The up
//! axis is recorded and coordinates are not rotated.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{
    Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
};
use cadmpeg_ir::assets::{Asset, AssetContent, AssetData};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::tessellation::{
    ShadedTriangle, ShadedVertex, Tessellation, TessellationMesh, TessellationTriangleGroup,
    TextureCoordinateAddressing, TextureCoordinates,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Color, Edge, EdgeCarrier, Face, FaceLoops, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::CadIr;

use crate::layers::Resource;
use crate::loss::UsdLossCode;
use crate::schema::curves::Curves;
use crate::schema::mesh::{MeshData, Normals};
use crate::schema::patch::PatchData;
use crate::schema::shading::{Bound, SurfaceShader};
use crate::schema::texture::SlotKind;
use crate::schema::xform::{compose, invert, local_transform, Affine, IDENTITY};
use crate::schema::Issues;
use crate::stage::{Prim, Stage};
use crate::text::syntax::{Layer, MetaValue, Value};

mod brep;
mod physics;
mod physx;

/// Millimetres per metre.
const MILLIMETRES_PER_METRE: f64 = 1000.0;
/// `metersPerUnit` when a layer does not state it (spec §10.1).
const DEFAULT_METERS_PER_UNIT: f64 = 0.01;
/// Operation that charges prim names the model keeps.
const RETAIN_NAME: &str = "usd_prim_name";
/// Operation that charges tessellation arrays the model keeps.
const RETAIN_MESH: &str = "usd_mesh_arrays";
/// Bytes one tessellation vertex row holds.
const MESH_VERTEX_BYTES: usize = 48;
/// Bytes one tessellation triangle row holds.
const MESH_TRIANGLE_BYTES: usize = 12;
/// Bytes one texture-coordinate value holds.
const MESH_COORDINATE_BYTES: usize = 16;

/// The stage metrics of a layer (spec §10.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Metrics {
    /// Millimetres per scene unit.
    pub(crate) scale: f64,
    pub(crate) meters_per_unit: f64,
    /// Kilograms per scene mass unit (spec §16.1).
    pub(crate) kilograms_per_unit: f64,
    pub(crate) up_axis: String,
    pub(crate) default_prim: Option<String>,
    /// `metersPerUnit` is authored and is not a positive finite number.
    pub(crate) malformed_units: bool,
    /// `kilogramsPerUnit` is authored and is not a positive finite number.
    pub(crate) malformed_mass_units: bool,
    /// `upAxis` is authored and is not the token `Y` or `Z`.
    pub(crate) malformed_up_axis: bool,
}

fn layer_value<'l>(layer: &'l Layer, key: &str) -> Option<&'l Value> {
    layer.metadata.iter().rev().find_map(|item| {
        if item.key != key || item.list_op.is_some() {
            return None;
        }
        match &item.value {
            MetaValue::Value(value) => Some(value),
            _ => None,
        }
    })
}

/// The stage metrics `layer` states.
pub(crate) fn metrics(layer: &Layer) -> Metrics {
    let authored = layer_value(layer, "metersPerUnit").map(|value| match value {
        Value::Number(number) => Some(number.float),
        _ => None,
    });
    let (meters_per_unit, malformed_units) = match authored {
        None => (DEFAULT_METERS_PER_UNIT, false),
        Some(Some(value)) if value.is_finite() && value > 0.0 => (value, false),
        Some(_) => (DEFAULT_METERS_PER_UNIT, true),
    };
    let (kilograms_per_unit, malformed_mass_units) = match layer_value(layer, "kilogramsPerUnit") {
        None => (1.0, false),
        Some(Value::Number(number)) if number.float.is_finite() && number.float > 0.0 => {
            (number.float, false)
        }
        Some(_) => (1.0, true),
    };
    let text = |key| match layer_value(layer, key) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    };
    let (up_axis, malformed_up_axis) = match layer_value(layer, "upAxis") {
        None => ("Y".to_owned(), false),
        Some(Value::String(axis)) if axis == "Y" || axis == "Z" => (axis.clone(), false),
        Some(_) => ("Y".to_owned(), true),
    };
    Metrics {
        scale: meters_per_unit * MILLIMETRES_PER_METRE,
        meters_per_unit,
        kilograms_per_unit,
        up_axis,
        default_prim: text("defaultPrim"),
        malformed_units,
        malformed_mass_units,
        malformed_up_axis,
    }
}

/// How a prim joins the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A transformable group.
    Group,
    Mesh,
    BasisCurves,
    NurbsCurves,
    NurbsPatch,
    /// An intrinsic primitive: placed, with no geometry transferred.
    Intrinsic,
    /// A prim the model does not carry: its subtree is skipped.
    Skipped,
    /// A prim another reader consumes: materials, shaders, subsets.
    Consumed,
}

fn role(type_name: Option<&str>) -> Role {
    match type_name {
        None | Some("Xform" | "Scope" | "SkelRoot") => Role::Group,
        Some("Mesh") => Role::Mesh,
        Some("BasisCurves") => Role::BasisCurves,
        Some("NurbsCurves") => Role::NurbsCurves,
        Some("NurbsPatch") => Role::NurbsPatch,
        Some(
            "Cube" | "Sphere" | "Cylinder" | "Cone" | "Capsule" | "Plane" | "Cylinder_1"
            | "Capsule_1",
        ) => Role::Intrinsic,
        Some("Material" | "Shader" | "NodeGraph" | "GeomSubset") => Role::Consumed,
        Some(_) => Role::Skipped,
    }
}

/// Per-decode counts that become loss notes.
#[derive(Default)]
struct Tallies {
    skipped_types: BTreeMap<String, usize>,
    /// Properties of group prims that no reader carries, by name.
    unread_properties: BTreeMap<String, usize>,
    intrinsic: BTreeMap<String, usize>,
    xform: Vec<String>,
    subdivision: usize,
    rejected_faces: usize,
    rejected_meshes: Vec<String>,
    dropped_subsets: usize,
    dropped_normals: usize,
    dropped_primvars: BTreeSet<String>,
    rejected_curves: usize,
    rejected_patches: Vec<String>,
    rejected_joints: Vec<String>,
    joint_properties: Vec<String>,
    /// `PhysX` properties that were not carried, by attribute path.
    solver_properties: Vec<String>,
    /// `UsdPhysics` body properties that were not carried, by attribute
    /// path.
    physics_properties: Vec<String>,
    trims: usize,
    ranges: usize,
    display_colors: usize,
    materials: BTreeSet<String>,
    /// Texture paths that name no package member or companion file.
    unlocated_textures: BTreeSet<String>,
    external: usize,
    metadata: BTreeMap<String, usize>,
    api_schemas: BTreeMap<String, usize>,
}

/// Layer metadata keys the model reads (spec §4.3).
const LAYER_KEYS_READ: &[&str] = &[
    "upAxis",
    "metersPerUnit",
    "kilogramsPerUnit",
    "defaultPrim",
    "subLayers",
];
/// Applied API schemas whose properties the model reads.
const API_SCHEMAS_READ: &[&str] = &[
    "MaterialBindingAPI",
    "PhysicsRigidBodyAPI",
    "PhysicsMassAPI",
    "PhysicsCollisionAPI",
    "PhysicsMeshCollisionAPI",
    "PhysicsArticulationRootAPI",
    "PhysicsMaterialAPI",
];

/// Prefix of the multiple-apply drive schema, whose instances the joint
/// reader reads.
const DRIVE_API_PREFIX: &str = "PhysicsDriveAPI:";

/// Prim metadata keys the model or the stage reads (spec §4.4).
const PRIM_KEYS_READ: &[&str] = &[
    "kind",
    "apiSchemas",
    "active",
    "instanceable",
    "references",
    "payload",
    "inherits",
    "specializes",
    "variants",
    "variantSets",
    "documentation",
    "displayName",
];

struct Builder<'s, 'l, 'c, 'a> {
    ctx: &'c DecodeContext<'a>,
    layers: &'s crate::layers::Layers<'a>,
    stage: &'s Stage<'l>,
    scale: f64,
    kilograms_per_unit: f64,
    ir: &'s mut CadIr,
    issues: Issues,
    tallies: Tallies,
    /// Definitions written, by key.
    definitions: BTreeSet<String>,
    /// Prototype ordinals by instance key.
    prototypes: BTreeMap<String, usize>,
    appearances: BTreeSet<String>,
    /// Material prims written as physical materials.
    physical_materials: BTreeSet<String>,
    /// Definitions that own a body.
    bodied: BTreeSet<String>,
    /// Joint prims, resolved after every occurrence exists.
    pending_joints: Vec<physics::PendingJoint>,
    pending_physx: Vec<physx::PendingSettings>,
    admitted: u64,
    transferred: bool,
}

/// Writes the model of the root layer stack of `layers` into `ir`. Returns
/// whether any geometry was transferred.
pub(crate) fn build<'a>(
    ctx: &DecodeContext<'a>,
    layers: &crate::layers::Layers<'a>,
    metrics: &Metrics,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    let stage = crate::stage::compose(ctx, layers)?;
    let admitted = u64_from_index(ir.model.entity_count());
    let mut builder = Builder {
        ctx,
        layers,
        stage: &stage,
        scale: metrics.scale,
        kilograms_per_unit: metrics.kilograms_per_unit,
        ir,
        issues: Issues::default(),
        tallies: Tallies::default(),
        definitions: BTreeSet::new(),
        prototypes: BTreeMap::new(),
        appearances: BTreeSet::new(),
        physical_materials: BTreeSet::new(),
        bodied: BTreeSet::new(),
        pending_joints: Vec::new(),
        pending_physx: Vec::new(),
        admitted,
        transferred: false,
    };
    let mut ordinal = 0_u32;
    for root in stage.roots.clone() {
        if builder.prim(root, None, &IDENTITY, ordinal)? {
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("root occurrence ordinal exceeds u32"))?;
        }
    }
    let pending = std::mem::take(&mut builder.pending_joints);
    builder.joints(pending)?;
    builder.finish_physx()?;
    let transferred = builder.transferred;
    let issues = std::mem::take(&mut builder.issues);
    let tallies = std::mem::take(&mut builder.tallies);
    builder.ir.finalize();
    let layer_keys: BTreeSet<&str> = layers
        .root()
        .metadata
        .iter()
        .map(|item| item.key.as_str())
        .filter(|key| !LAYER_KEYS_READ.contains(key))
        .collect();
    if !layer_keys.is_empty() {
        losses.push(UsdLossCode::MetadataNotTransferred.note(format!(
            "layer metadata was not transferred: {}",
            joined(layer_keys.into_iter().map(str::to_owned))
        )));
    }
    losses.extend(layers.losses.iter().cloned());
    if !layers.unavailable.is_empty() {
        losses.push(UsdLossCode::CompositionLayerUnavailable.note(format!(
            "{} named layers did not open: {}",
            layers.unavailable.len(),
            joined(
                layers
                    .unavailable
                    .iter()
                    .map(|(asset, reason)| format!("{asset} ({reason})"))
            )
        )));
    }
    losses.extend(composition_losses(&stage));
    losses.extend(tally_losses(&tallies, &issues, metrics));
    Ok(transferred)
}

impl Builder<'_, '_, '_, '_> {
    fn admit(&mut self, operation: &'static str) -> Result<(), CodecError> {
        self.ctx.admit_entities(
            u64_from_index(self.ir.model.entity_count()),
            &mut self.admitted,
            operation,
        )
    }

    /// The definition key of a prim: its path, or for a prim inside an
    /// instance, its prototype and relative path. An instance prim whose own
    /// opinions only place it shares the definition of its prototype.
    fn definition_key(&mut self, prim: &Prim<'_>) -> String {
        let (key, relative) = match (&prim.in_instance, &prim.instance_key) {
            (Some((root, relative)), _) => (
                self.stage
                    .prims
                    .get(*root)
                    .and_then(|root| root.instance_key.clone())
                    .unwrap_or_default(),
                Some(relative.as_str()),
            ),
            (None, Some(key)) if places_only(prim) => (key.clone(), None),
            _ => return prim.path.clone(),
        };
        let next = self.prototypes.len();
        let ordinal = *self.prototypes.entry(key).or_insert(next);
        match relative {
            Some(relative) => format!("prototype{ordinal}/{relative}"),
            None => format!("prototype{ordinal}"),
        }
    }

    /// Adds the occurrence of prim `index` and its descendants. Returns
    /// whether an occurrence was added.
    fn prim(
        &mut self,
        index: usize,
        parent: Option<&str>,
        parent_world: &Affine,
        ordinal: u32,
    ) -> Result<bool, CodecError> {
        let stage = self.stage;
        let Some(prim) = stage.prims.get(index) else {
            return Ok(false);
        };
        if physx::is_scene_type(prim.type_name()) {
            self.scene(prim)?;
            return Ok(false);
        }
        if let Some(kind) = prim
            .type_name()
            .and_then(crate::schema::physics::JointType::of)
        {
            let data = crate::schema::physics::joint(prim, kind, &mut self.issues);
            self.pending_joints.push(physics::PendingJoint {
                path: prim.path.clone(),
                data,
            });
            return Ok(false);
        }
        let role = role(prim.type_name());
        match role {
            Role::Skipped => {
                let name = prim.type_name().unwrap_or("untyped").to_owned();
                *self.tallies.skipped_types.entry(name).or_default() += 1;
                return Ok(false);
            }
            Role::Consumed => return Ok(false),
            Role::Group if holds_only_shading(stage, prim) => {
                for child in prim.children.clone() {
                    self.collect_unplaced(child, parent_world)?;
                }
                return Ok(false);
            }
            _ => {}
        }
        let local = match local_transform(prim, &mut self.issues) {
            Ok(local) => local,
            Err(error) => {
                self.tallies
                    .xform
                    .push(format!("{}: {}", prim.path, error.0));
                crate::schema::xform::LocalTransform {
                    matrix: IDENTITY,
                    resets_stack: false,
                }
            }
        };
        let placement = if local.resets_stack {
            match invert(parent_world) {
                Some(inverse) => compose(&inverse, &local.matrix),
                None => {
                    self.tallies
                        .xform
                        .push(format!("{}: the parent transform is singular", prim.path));
                    local.matrix
                }
            }
        } else {
            local.matrix
        };
        let world = compose(parent_world, &placement);
        let transform = self.transform(&placement).unwrap_or_else(|| {
            self.tallies
                .xform
                .push(format!("{}: the transform is not finite", prim.path));
            Transform::identity()
        });
        for schema in prim.api_schemas() {
            if !API_SCHEMAS_READ.contains(&schema)
                && !schema.starts_with(DRIVE_API_PREFIX)
                && crate::schema::physx::holder(crate::schema::physx::split(schema).0).is_none()
            {
                *self
                    .tallies
                    .api_schemas
                    .entry(schema.to_owned())
                    .or_default() += 1;
            }
        }
        for site in &prim.sites {
            for item in site.metadata {
                if !PRIM_KEYS_READ.contains(&item.key.as_str()) {
                    *self.tallies.metadata.entry(item.key.clone()).or_default() += 1;
                }
            }
        }
        let definition = self.definition_key(prim);
        let visibility = crate::schema::token(prim, "visibility", &mut self.issues);
        let purpose = crate::schema::token(prim, "purpose", &mut self.issues);
        let visible = (visibility.as_deref() == Some("invisible")
            || matches!(purpose.as_deref(), Some("guide" | "proxy")))
        .then_some(false);
        if self.definitions.insert(definition.clone()) {
            self.definition(prim, role, &definition)?;
        }
        self.ctx
            .retain_bytes(u64_from_index(prim.name.len()), RETAIN_NAME)?;
        self.ir.model.occurrences.push(Occurrence {
            id: crate::ids::occurrence(&prim.path),
            prototype: PrototypeReference::Local {
                definition: crate::ids::definition(&definition),
            },
            parent: match parent {
                None => OccurrenceParent::Root {},
                Some(parent) => OccurrenceParent::Occurrence {
                    occurrence: crate::ids::occurrence(parent),
                },
            },
            ordinal,
            transform: Some(transform),
            linked_prototype: None,
            name: Some(display_name(prim).to_owned()),
            visible,
            link: None,
            native_ref: None,
        });
        self.admit("usd_occurrence")?;
        let bodied = self.bodied.contains(&definition);
        self.physics_apis(prim, &definition, bodied)?;
        let mut child_ordinal = 0_u32;
        for (arc, external) in prim.external.iter().enumerate() {
            let document = NonBlankString::new(external.asset.clone())
                .map_or(ExternalDocument::Missing {}, |path| {
                    ExternalDocument::Path { path }
                });
            self.ir.model.occurrences.push(Occurrence {
                id: crate::ids::external_occurrence(&prim.path, arc),
                prototype: PrototypeReference::External {
                    document,
                    object: external.prim_path.clone(),
                },
                parent: OccurrenceParent::Occurrence {
                    occurrence: crate::ids::occurrence(&prim.path),
                },
                ordinal: child_ordinal,
                transform: Some(Transform::identity()),
                linked_prototype: None,
                name: Some(external.asset.clone()),
                visible: None,
                link: None,
                native_ref: None,
            });
            self.tallies.external += 1;
            child_ordinal = child_ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("child occurrence ordinal exceeds u32"))?;
            self.admit("usd_occurrence")?;
        }
        for child in prim.children.clone() {
            if self.prim(child, Some(&prim.path), &world, child_ordinal)? {
                child_ordinal = child_ordinal
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("child occurrence ordinal exceeds u32"))?;
            }
        }
        Ok(true)
    }

    /// An occurrence transform: `matrix` with its translation in millimetres.
    fn transform(&self, matrix: &Affine) -> Option<Transform> {
        let mut rows = *matrix;
        for row in &mut rows {
            row[3] *= self.scale;
        }
        Transform::affine(rows)
    }

    fn point(&self, [x, y, z]: [f64; 3]) -> Option<FinitePoint3> {
        FinitePoint3::new(Point3::new(x * self.scale, y * self.scale, z * self.scale))
    }

    /// Writes the definition of `prim` and its geometry.
    fn definition(&mut self, prim: &Prim<'_>, role: Role, key: &str) -> Result<(), CodecError> {
        let kind = match role {
            Role::Mesh | Role::BasisCurves | Role::NurbsCurves | Role::NurbsPatch => {
                ProductDefinitionKind::Object
            }
            _ => match prim.metadata_text("kind") {
                Some("component" | "assembly" | "subcomponent") => ProductDefinitionKind::Part,
                _ => ProductDefinitionKind::Group,
            },
        };
        let body = match role {
            Role::Mesh => self.mesh(prim, key)?,
            Role::BasisCurves => {
                let curves = crate::schema::curves::basis_curves(prim, &mut self.issues);
                self.curves(prim, key, curves)?
            }
            Role::NurbsCurves => {
                let curves = crate::schema::curves::nurbs_curves(prim, &mut self.issues);
                self.curves(prim, key, curves)?
            }
            Role::NurbsPatch => self.patch(prim, key)?,
            Role::Intrinsic => match crate::schema::intrinsic::records(prim, &mut self.issues) {
                Ok(records) => self.primitive_body(prim, key, &records)?,
                Err(reason) => {
                    let name = format!("{}: {reason}", prim.path);
                    *self.tallies.intrinsic.entry(name).or_default() += 1;
                    None
                }
            },
            Role::Group | Role::Skipped | Role::Consumed => None,
        };
        if matches!(
            role,
            Role::Mesh | Role::BasisCurves | Role::NurbsCurves | Role::NurbsPatch
        ) {
            self.unconsumed_primvars(prim, role);
        }
        if role == Role::Group {
            self.unread_group_properties(prim);
        }
        if body.is_some() {
            self.bodied.insert(key.to_owned());
        }
        self.ir.model.product_definitions.push(ProductDefinition {
            id: crate::ids::definition(key),
            kind,
            source_name: Some(definition_name(prim, key).to_owned()),
            label: None,
            description: prim.metadata_text("documentation").map(str::to_owned),
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies: body.into_iter().collect(),
            sketches: Vec::new(),
            native_ref: None,
        });
        self.admit("usd_definition")
    }

    /// Tallies the properties of group prim `prim` that no reader carries.
    /// Its primvars join the primvar tally.
    fn unread_group_properties(&mut self, prim: &Prim<'_>) {
        for name in prim.property_names() {
            if group_property_read(prim, name) {
                continue;
            }
            if name.starts_with("primvars:") {
                self.tallies.dropped_primvars.insert(name.to_owned());
            } else {
                *self
                    .tallies
                    .unread_properties
                    .entry(name.to_owned())
                    .or_default() += 1;
            }
        }
    }

    /// Records the primvars of a geometry prim that the model does not carry.
    fn unconsumed_primvars(&mut self, prim: &Prim<'_>, role: Role) {
        let coordinates = if role == Role::Mesh {
            crate::schema::mesh::texture_coordinate_primvars(prim)
        } else {
            Vec::new()
        };
        for name in prim.property_names() {
            let Some(primvar) = name.strip_prefix("primvars:") else {
                continue;
            };
            let consumed = matches!(primvar, "displayColor" | "displayOpacity")
                || (role == Role::Mesh && matches!(primvar, "normals" | "normals:indices"))
                || coordinates.contains(&name);
            if !consumed && !primvar.ends_with(":indices") {
                self.tallies.dropped_primvars.insert(name.to_owned());
            }
        }
        if prim.has_attribute("widths") {
            self.tallies.dropped_primvars.insert("widths".to_owned());
        }
    }

    /// The body color a gprim's constant display color states.
    fn display_color(&mut self, prim: &Prim<'_>) -> Option<Color> {
        let (color, dropped) = crate::schema::mesh::display_color(prim, &mut self.issues);
        if dropped {
            self.tallies.display_colors += 1;
        }
        let [r, g, b, a] = color?;
        let channel = |value: f64| crate::text::value::round_f32(value.clamp(0.0, 1.0));
        Color::new(channel(r), channel(g), channel(b), channel(a))
    }

    fn body(
        &mut self,
        prim: &Prim<'_>,
        key: &str,
        kind: BodyKind,
        regions: Vec<cadmpeg_ir::ids::RegionId>,
    ) {
        let color = self.display_color(prim);
        self.ir.model.bodies.push(Body {
            native_ref: None,
            id: crate::ids::body(key),
            kind,
            regions,
            transform: None,
            name: Some(prim.name.clone()),
            color,
            visible: None,
        });
    }

    fn mesh(
        &mut self,
        prim: &Prim<'_>,
        key: &str,
    ) -> Result<Option<cadmpeg_ir::ids::BodyId>, CodecError> {
        let Some(mesh) = crate::schema::mesh::read(self.stage, prim, &mut self.issues) else {
            if prim.external.is_empty() {
                self.tallies.rejected_meshes.push(prim.path.clone());
            }
            return Ok(None);
        };
        let coordinate_values: usize = mesh
            .texture_coordinates
            .iter()
            .map(|set| set.values.len())
            .sum();
        self.ctx.retain_bytes(
            u64_from_index(
                mesh.points.len() * MESH_VERTEX_BYTES
                    + mesh.triangles.len() * MESH_TRIANGLE_BYTES
                    + coordinate_values * MESH_COORDINATE_BYTES,
            ),
            RETAIN_MESH,
        )?;
        for name in &mesh.dropped_texture_coordinates {
            self.tallies
                .dropped_primvars
                .insert(format!("primvars:{name}"));
        }
        self.tallies.rejected_faces += mesh.rejected_faces;
        self.tallies.dropped_subsets += mesh.dropped_subsets.len();
        if mesh.dropped_normals {
            self.tallies.dropped_normals += 1;
        }
        if mesh.subdivision.is_some() {
            self.tallies.subdivision += 1;
        }
        let tessellation = match self.tessellation(key, &mesh) {
            Ok(tessellation) => tessellation,
            Err(reason) => {
                self.tallies
                    .rejected_meshes
                    .push(format!("{}: {reason}", prim.path));
                return Ok(None);
            }
        };
        let body = crate::ids::body(key);
        self.body(prim, key, BodyKind::General, Vec::new());
        self.ir
            .model
            .tessellations
            .push(tessellation.with_body(Some(body.clone())));
        self.material(prim, key)?;
        for subset in &mesh.subsets {
            self.subset_material(&subset.path, key)?;
        }
        self.transferred = true;
        self.admit("usd_mesh")?;
        Ok(Some(body))
    }

    fn tessellation(&self, key: &str, mesh: &MeshData) -> Result<Tessellation, String> {
        let points: Vec<FinitePoint3> = mesh
            .points
            .iter()
            .map(|point| self.point(*point))
            .collect::<Option<_>>()
            .ok_or("a point is not finite")?;
        let normal = |[x, y, z]: [f64; 3]| FiniteVector3::new(Vector3::new(x, y, z));
        let lanes = match &mesh.normals {
            Normals::None => TessellationMesh::List {
                vertices: points,
                triangles: mesh.triangles.clone(),
            },
            Normals::Vertex(normals) => TessellationMesh::ShadedList {
                vertices: points
                    .into_iter()
                    .zip(normals.iter())
                    .map(|(position, value)| {
                        normal(*value).map(|normal| ShadedVertex { position, normal })
                    })
                    .collect::<Option<_>>()
                    .ok_or("a normal is not finite")?,
                triangles: mesh.triangles.clone(),
            },
            Normals::Corner(normals) => TessellationMesh::CornerShadedList {
                vertices: points,
                triangles: mesh
                    .triangles
                    .iter()
                    .zip(normals.chunks_exact(3))
                    .map(|(corners, values)| {
                        Some(ShadedTriangle {
                            corners: *corners,
                            normals: [normal(values[0])?, normal(values[1])?, normal(values[2])?],
                        })
                    })
                    .collect::<Option<_>>()
                    .ok_or("a normal is not finite")?,
            },
        };
        let mut groups: Vec<TessellationTriangleGroup> = mesh
            .subsets
            .iter()
            .map(|subset| TessellationTriangleGroup {
                source_id: Some(subset.path.clone()),
                triangles: subset.triangles.clone(),
            })
            .collect();
        if !groups.is_empty() {
            // The triangles no subset claims are one unnamed group, so that
            // the groups partition the mesh.
            let claimed: BTreeSet<u32> = groups
                .iter()
                .flat_map(|group| group.triangles.iter().copied())
                .collect();
            let rest: Vec<u32> = (0..u32::try_from(mesh.triangles.len())
                .map_err(|_| "the mesh has more than 2^32 triangles")?)
                .filter(|triangle| !claimed.contains(triangle))
                .collect();
            if !rest.is_empty() {
                groups.push(TessellationTriangleGroup {
                    source_id: None,
                    triangles: rest,
                });
            }
        }
        let coordinates = mesh
            .texture_coordinates
            .iter()
            .map(|set| {
                let name = NonBlankString::new(set.name.clone())
                    .ok_or("a texture-coordinate primvar has an empty name")?;
                let values = set
                    .values
                    .iter()
                    .map(|[u, v]| FinitePoint2::new(Point2::new(*u, *v)))
                    .collect::<Option<_>>()
                    .ok_or("a texture coordinate is not finite")?;
                Ok(TextureCoordinates {
                    name,
                    addressing: if set.per_vertex {
                        TextureCoordinateAddressing::Vertex
                    } else {
                        TextureCoordinateAddressing::Corner
                    },
                    values,
                })
            })
            .collect::<Result<Vec<_>, &str>>()?;
        Tessellation::from_parts(crate::ids::tessellation(key), lanes, Vec::new())
            .and_then(|tessellation| tessellation.with_triangle_groups(groups))
            .and_then(|tessellation| tessellation.with_texture_coordinates(coordinates))
            .map_err(|error| error.to_string())
    }

    /// Binds the material of `prim` to the tessellation of definition `key`.
    fn material(&mut self, prim: &Prim<'_>, key: &str) -> Result<(), CodecError> {
        let Some(bound) =
            crate::schema::shading::bound_material(self.stage, prim, &mut self.issues)
        else {
            return Ok(());
        };
        let surface = match bound {
            Bound::Surface(surface) => surface,
            Bound::Other(path) => {
                self.tallies.materials.insert(path);
                return Ok(());
            }
        };
        let appearance = self.appearance(&surface)?;
        self.ir.model.appearance_bindings.push(AppearanceBinding {
            native_ref: None,
            id: crate::ids::appearance_binding(key),
            target: AppearanceTarget::Tessellation(crate::ids::tessellation(key).to_string()),
            appearance,
            source_entity_id: Some(surface.material),
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
        self.admit("usd_appearance")
    }

    /// The appearance of `surface`, added on first use.
    fn appearance(
        &mut self,
        surface: &SurfaceShader,
    ) -> Result<cadmpeg_ir::ids::AppearanceId, CodecError> {
        if !surface.connected.is_empty() {
            self.tallies.materials.insert(surface.material.clone());
        }
        if self.appearances.insert(surface.material.clone()) {
            let mut record = surface_appearance(surface);
            record.textures = self.textures(surface)?;
            record.library_id = self.module(surface)?;
            self.ir.model.appearances.push(record);
        }
        Ok(crate::ids::appearance(&surface.material))
    }

    /// Binds the material that the geometry subset at `path` binds to its
    /// triangle group of the tessellation of definition `key`.
    fn subset_material(&mut self, path: &str, key: &str) -> Result<(), CodecError> {
        let Some(subset) = self.stage.find(path) else {
            return Ok(());
        };
        let Some(target) = subset
            .relationship_targets("material:binding")
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        let surface = match crate::schema::shading::material(self.stage, &target, &mut self.issues)
        {
            Bound::Surface(surface) => surface,
            Bound::Other(material) => {
                self.tallies.materials.insert(material);
                return Ok(());
            }
        };
        let appearance = self.appearance(&surface)?;
        self.ir.model.appearance_bindings.push(AppearanceBinding {
            native_ref: None,
            id: crate::ids::subset_binding(key, path),
            target: AppearanceTarget::TriangleGroup {
                tessellation: crate::ids::tessellation(key).to_string(),
                group: path.to_owned(),
            },
            appearance,
            source_entity_id: Some(surface.material),
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
        self.admit("usd_appearance")
    }

    /// The MDL module of `surface`: the identity of the file it names, or
    /// the path as written for a module that the MDL search path finds.
    fn module(&self, surface: &SurfaceShader) -> Result<Option<String>, CodecError> {
        let Some((path, layer)) = &surface.module else {
            return Ok(None);
        };
        Ok(Some(match self.layers.texture(self.ctx, *layer, path)? {
            Resource::Located(id) => id,
            Resource::Embedded { .. } | Resource::Unlocated => path.clone(),
        }))
    }

    /// The texture references of `surface`, with an asset for each image
    /// file that the model does not hold yet.
    fn textures(&mut self, surface: &SurfaceShader) -> Result<Vec<TextureRef>, CodecError> {
        let mut out = Vec::new();
        for slot in &surface.textures {
            let texture = &slot.texture;
            let resource = self
                .layers
                .texture(self.ctx, texture.layer, &texture.file)?;
            let (identity, content) = match resource {
                Resource::Embedded { id, bytes } => (
                    id,
                    AssetData::new(bytes).map(|data| AssetContent::Embedded { data }),
                ),
                Resource::Located(id) => {
                    let uri = NonBlankString::new(id.clone());
                    (id, uri.map(|uri| AssetContent::External { uri }))
                }
                Resource::Unlocated => {
                    self.tallies.unlocated_textures.insert(texture.file.clone());
                    let uri = NonBlankString::new(texture.file.clone());
                    (
                        texture.file.clone(),
                        uri.map(|uri| AssetContent::External { uri }),
                    )
                }
            };
            let Some(content) = content else {
                self.tallies.materials.insert(surface.material.clone());
                continue;
            };
            let asset = crate::ids::asset(&identity);
            if !self.ir.model.assets.iter().any(|known| known.id == asset) {
                self.ctx.charge_entities(1, "usd_texture_asset")?;
                let name = texture
                    .file
                    .rsplit(['/', '\\'])
                    .next()
                    .and_then(|name| NonBlankString::new(name.to_owned()));
                self.ir.model.assets.push(Asset {
                    id: asset.clone(),
                    name,
                    media_type: image_media_type(&texture.file)
                        .and_then(|media| NonBlankString::new(media.to_owned())),
                    content,
                    native_ref: None,
                });
            }
            let slot_name = match (slot.kind, texture.channel) {
                (SlotKind::Scalar, channel) => format!("{}.{}", slot.slot, channel.unwrap_or('r')),
                _ => slot.slot.to_owned(),
            };
            let real = |value: f64| FiniteReal::new(value).unwrap_or(FiniteReal::ZERO);
            out.push(TextureRef {
                asset_guid: asset.to_string(),
                slot: slot_name,
                schema: texture.node.clone(),
                paths: vec![texture.file.clone()],
                urn: None,
                mapping: TextureMap2d {
                    map_channel: 0,
                    uvw_source: 0,
                    u_offset: real(texture.translation[0]),
                    v_offset: real(texture.translation[1]),
                    u_scale: real(texture.scale[0]),
                    v_scale: real(texture.scale[1]),
                    rotation: Angle::new(texture.rotation.to_radians()).unwrap_or(Angle::ZERO),
                    repeat_u: texture.repeat[0],
                    repeat_v: texture.repeat[1],
                    real_world_offset_x: Length::ZERO,
                    real_world_offset_y: Length::ZERO,
                    real_world_scale_x: Length::ZERO,
                    real_world_scale_y: Length::ZERO,
                    coordinates: texture.coordinates.clone().and_then(NonBlankString::new),
                },
                bump: (slot.kind == SlotKind::Normal).then_some(BumpMap {
                    normal_map: true,
                    depth: Length::ZERO,
                    normal_scale: FiniteReal::ONE,
                }),
            });
        }
        Ok(out)
    }

    fn curves(
        &mut self,
        prim: &Prim<'_>,
        key: &str,
        curves: Option<Curves>,
    ) -> Result<Option<cadmpeg_ir::ids::BodyId>, CodecError> {
        let Some(curves) = curves else {
            self.tallies.rejected_curves += 1;
            return Ok(None);
        };
        self.tallies.rejected_curves += curves.rejected;
        let mut edges = Vec::new();
        for (ordinal, curve) in curves.curves.iter().enumerate() {
            let points: Option<Vec<Point3>> = curve
                .points
                .iter()
                .map(|point| self.point(*point).map(FinitePoint3::get))
                .collect();
            let nurbs = points.ok_or("a point is not finite").and_then(|points| {
                NurbsPoles3::from_lanes(points, curve.weights.clone())
                    .and_then(|poles| {
                        NurbsCurve::new(curve.degree, curve.knots.clone(), poles, false)
                    })
                    .map_err(|_| "the curve is not a valid NURBS curve")
            });
            let Ok(nurbs) = nurbs else {
                self.tallies.rejected_curves += 1;
                continue;
            };
            let Some(domain) = curve.range.or_else(|| {
                cadmpeg_ir::eval::nurbs_curve_parameter_domain(&nurbs)
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            }) else {
                self.tallies.rejected_curves += 1;
                continue;
            };
            let ends = [
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(self.ctx, &nurbs, domain[0]),
                )?,
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(self.ctx, &nurbs, domain[1]),
                )?,
            ];
            let [Some(start), Some(end)] = ends else {
                self.tallies.rejected_curves += 1;
                continue;
            };
            let closed = start == end;
            let curve_id = crate::ids::curve(key, ordinal);
            let carrier = EdgeCarrier::new(Some(curve_id.clone()), Some(domain));
            let Ok(carrier) = carrier else {
                self.tallies.rejected_curves += 1;
                continue;
            };
            let parameter_range = curve
                .range
                .and_then(cadmpeg_ir::topology::IncreasingParameterInterval::new);
            self.ir.model.curves.push(Curve {
                native_ref: None,
                id: curve_id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
                parameter_range,
                source_object: None,
            });
            let start_vertex = crate::ids::vertex(key, ordinal, 0);
            let end_vertex = if closed {
                start_vertex.clone()
            } else {
                crate::ids::vertex(key, ordinal, 1)
            };
            for (end, (vertex, position)) in
                [(start_vertex.clone(), start), (end_vertex.clone(), end)]
                    .into_iter()
                    .enumerate()
                    .take(if closed { 1 } else { 2 })
            {
                let point = crate::ids::point(key, ordinal, end);
                self.ir
                    .model
                    .points
                    .push(Point::new(point.clone(), position, None));
                self.ir.model.vertices.push(Vertex {
                    native_ref: None,
                    id: vertex,
                    point,
                    tolerance: None,
                });
            }
            let edge = crate::ids::edge(key, ordinal);
            self.ir.model.edges.push(Edge {
                native_ref: None,
                id: edge.clone(),
                carrier,
                start: start_vertex,
                end: end_vertex,
                tolerance: None,
            });
            edges.push(edge);
            self.admit("usd_curve")?;
        }
        if edges.is_empty() {
            return Ok(None);
        }
        let region = crate::ids::region(key);
        let shell = Shell::new(
            crate::ids::shell(key),
            region.clone(),
            Vec::new(),
            edges,
            Vec::new(),
        )
        .map_err(|_| CodecError::malformed("a curve shell holds no edge"))?;
        self.ir.model.shells.push(shell);
        self.ir.model.regions.push(Region {
            native_ref: None,
            id: region.clone(),
            body: crate::ids::body(key),
            shells: vec![crate::ids::shell(key)],
        });
        self.body(prim, key, BodyKind::Wire, vec![region]);
        self.transferred = true;
        self.admit("usd_curves")?;
        Ok(Some(crate::ids::body(key)))
    }

    fn patch(
        &mut self,
        prim: &Prim<'_>,
        key: &str,
    ) -> Result<Option<cadmpeg_ir::ids::BodyId>, CodecError> {
        let Some(patch) = crate::schema::patch::read(prim, &mut self.issues) else {
            self.tallies.rejected_patches.push(prim.path.clone());
            return Ok(None);
        };
        if !patch.trims.is_empty() || patch.trims_malformed {
            self.tallies.trims += 1;
        }
        if patch.range_narrowed {
            self.tallies.ranges += 1;
        }
        let Some(surface) = self.patch_surface(&patch) else {
            self.tallies.rejected_patches.push(prim.path.clone());
            return Ok(None);
        };
        let surface_id = crate::ids::surface(key);
        self.ir.model.surfaces.push(Surface {
            native_ref: None,
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: None,
        });
        let face = crate::ids::face(key);
        let region = crate::ids::region(key);
        let shell = crate::ids::shell(key);
        self.ir.model.faces.push(Face {
            native_ref: None,
            id: face.clone(),
            shell: shell.clone(),
            surface: surface_id,
            sense: if patch.left_handed {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        });
        let shell_record = Shell::new(
            shell.clone(),
            region.clone(),
            vec![face],
            Vec::new(),
            Vec::new(),
        )
        .map_err(|_| CodecError::malformed("a patch shell holds no face"))?;
        self.ir.model.shells.push(shell_record);
        self.ir.model.regions.push(Region {
            native_ref: None,
            id: region.clone(),
            body: crate::ids::body(key),
            shells: vec![shell],
        });
        self.body(prim, key, BodyKind::Sheet, vec![region]);
        self.transferred = true;
        self.admit("usd_patch")?;
        Ok(Some(crate::ids::body(key)))
    }

    fn patch_surface(&self, patch: &PatchData) -> Option<NurbsSurface> {
        let rows: Vec<Vec<Point3>> = patch
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|point| self.point(*point).map(FinitePoint3::get))
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<_>>()?;
        let poles = NurbsPoleGrid::from_lanes(rows, patch.weights.clone()).ok()?;
        NurbsSurface::new(
            NurbsSurfaceAxis::new(patch.u_degree, patch.u_knots.clone(), false),
            NurbsSurfaceAxis::new(patch.v_degree, patch.v_knots.clone(), false),
            poles,
            false,
        )
        .ok()
    }
}

/// The source name of a definition: the prim name, or for the shared
/// definition of an instance prim, the name of the prim its arc targets.
fn definition_name<'p>(prim: &'p Prim<'_>, key: &str) -> &'p str {
    if key == prim.path {
        return &prim.name;
    }
    prim.sites
        .iter()
        .find(|site| site.origin == crate::stage::Origin::Arc && site.specifier.is_some())
        .and_then(|site| site.spec_path.rsplit('/').next())
        .unwrap_or(&prim.name)
}

/// The name a prim displays: its `displayName`, or its prim name.
fn display_name<'p>(prim: &'p Prim<'_>) -> &'p str {
    prim.metadata_text("displayName").unwrap_or(&prim.name)
}

/// Whether a group prim holds materials, shaders, and joints only: a
/// library, not scene content.
fn holds_only_shading(stage: &Stage<'_>, prim: &Prim<'_>) -> bool {
    !prim.children.is_empty()
        && prim.children.iter().all(|child| {
            stage.prims.get(*child).is_some_and(|child| {
                child
                    .type_name()
                    .and_then(crate::schema::physics::JointType::of)
                    .is_some()
                    || physx::is_scene_type(child.type_name())
                    || match role(child.type_name()) {
                        Role::Consumed => true,
                        Role::Group => holds_only_shading(stage, child),
                        _ => false,
                    }
            })
        })
}

/// Whether a group prim's property is carried, or reported by another
/// reader: placement, material bindings, the physics schemas, and the
/// carried `PhysX` schemas.
fn group_property_read(prim: &Prim<'_>, name: &str) -> bool {
    name.starts_with("xformOp")
        || matches!(name, "visibility" | "purpose")
        || name.starts_with("material:binding")
        || name.starts_with("physics:")
        || name.starts_with("drive:")
        || name.starts_with("limit:")
        || prim.api_schemas().iter().any(|schema| {
            let (base, instance) = crate::schema::physx::split(schema);
            crate::schema::physx::holder(base).is_some()
                && crate::schema::physx::namespace(base, instance)
                    .is_some_and(|prefix| name.starts_with(&prefix))
        })
}

/// Whether the prim's own specifications state only placement properties:
/// transform ops, visibility, and purpose.
fn places_only(prim: &Prim<'_>) -> bool {
    prim.sites
        .iter()
        .filter(|site| site.origin == crate::stage::Origin::Local && site.specifier.is_some())
        .flat_map(|site| site.body.properties.iter())
        .all(|property| {
            let name = property.name();
            name.starts_with("xformOp") || matches!(name, "visibility" | "purpose")
        })
}

/// The base color and opacity of a surface shader's inputs, with the
/// shader's unauthored values (spec §15.3 to §15.5).
fn base_color(surface: &SurfaceShader) -> ([f64; 3], f64) {
    let color = |name: &str, fallback: [f64; 3]| match surface.input(name) {
        Some([r, g, b]) => [*r, *g, *b],
        _ => fallback,
    };
    let scalar = |name: &str, fallback: f64| match surface.input(name) {
        Some([value]) => *value,
        _ => fallback,
    };
    let scaled = |color: [f64; 3], weight: f64| color.map(|channel| channel * weight);
    let shader = if surface.column == Some(3) {
        "OmniPBR"
    } else {
        surface.shader.as_str()
    };
    match shader {
        "ND_open_pbr_surface_surfaceshader" => (
            scaled(color("base_color", [0.8; 3]), scalar("base_weight", 1.0)),
            scalar("geometry_opacity", 1.0),
        ),
        "ND_standard_surface_surfaceshader" => (
            scaled(color("base_color", [0.8; 3]), scalar("base", 1.0)),
            color("opacity", [1.0; 3])[0],
        ),
        "ND_standard_surface_surfaceshader_100" => (
            scaled(color("base_color", [1.0; 3]), scalar("base", 0.8)),
            color("opacity", [1.0; 3])[0],
        ),
        "OmniPBR" | "OmniPBR_ClearCoat" => {
            let tint = color("diffuse_tint", [1.0; 3]);
            let base = color("diffuse_color_constant", [0.2; 3]);
            let opacity = if surface.input("enable_opacity") == Some(&[1.0][..]) {
                scalar("opacity_constant", 1.0)
            } else {
                1.0
            };
            ([0, 1, 2].map(|k| base[k] * tint[k]), opacity)
        }
        _ => (color("diffuseColor", [0.18; 3]), scalar("opacity", 1.0)),
    }
}

/// The appearance of a surface material.
fn surface_appearance(surface: &SurfaceShader) -> Appearance {
    let ([r, g, b], opacity) = base_color(surface);
    let channel = |value: f64| crate::text::value::round_f32(value.clamp(0.0, 1.0));
    let base_color = Color::new(channel(r), channel(g), channel(b), channel(opacity));
    let mut properties = BTreeMap::new();
    for (input, value) in &surface.inputs {
        let suffixes: &[&str] = match value.len() {
            1 => &[""],
            3 => &[".r", ".g", ".b"],
            _ => continue,
        };
        for (suffix, component) in suffixes.iter().zip(value) {
            if let (Some(key), Some(component)) = (
                NonBlankString::new(format!("{input}{suffix}")),
                FiniteReal::new(*component),
            ) {
                properties.insert(key, component);
            }
        }
    }
    Appearance {
        native_ref: None,
        id: crate::ids::appearance(&surface.material),
        name: Some(surface.name.clone()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(surface.shader.clone()),
        category: None,
        base_color,
        properties,
        textures: Vec::new(),
    }
}

/// The media type of an image file, by its extension.
fn image_media_type(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "exr" => "image/x-exr",
        "hdr" => "image/vnd.radiance",
        "tif" | "tiff" => "image/tiff",
        "tga" => "image/x-tga",
        "bmp" => "image/bmp",
        "dds" => "image/vnd-ms.dds",
        "ktx2" => "image/ktx2",
        "webp" => "image/webp",
        _ => return None,
    })
}

fn joined(items: impl IntoIterator<Item = String>) -> String {
    items.into_iter().collect::<Vec<_>>().join(", ")
}

fn composition_losses(stage: &Stage<'_>) -> Vec<LossNote> {
    let tally = &stage.tally;
    let mut losses = Vec::new();
    if !tally.sublayers.is_empty() {
        losses.push(UsdLossCode::CompositionSublayersNotComposed.note(format!(
            "{} sublayers were not composed: {}",
            tally.sublayers.len(),
            joined(tally.sublayers.iter().cloned())
        )));
    }
    if !tally.missing_targets.is_empty() {
        losses.push(UsdLossCode::CompositionTargetMissing.note(format!(
            "internal arcs name {} paths that no prim specification holds: {}",
            tally.missing_targets.len(),
            joined(tally.missing_targets.iter().cloned())
        )));
    }
    if tally.unselected_variants > 0 {
        losses.push(UsdLossCode::CompositionVariantNotSelected.note(format!(
            "{} variants that no selection names were not composed",
            tally.unselected_variants
        )));
    }
    if !tally.undefined_overs.is_empty() {
        losses.push(UsdLossCode::CompositionOverUndefined.note(format!(
            "{} prims have only `over` specifications in this layer and were not composed: {}",
            tally.undefined_overs.len(),
            joined(tally.undefined_overs.iter().cloned())
        )));
    }
    if !tally.cycles.is_empty() {
        losses.push(UsdLossCode::CompositionCycle.note(format!(
            "arcs of {} prims form a cycle and were not expanded: {}",
            tally.cycles.len(),
            joined(tally.cycles.iter().cloned())
        )));
    }
    if !tally.instance_overrides.is_empty() {
        losses.push(UsdLossCode::CompositionOverrideNotApplied.note(format!(
            "opinions authored under {} instance prims have no effect and were not applied: {}",
            tally.instance_overrides.len(),
            joined(tally.instance_overrides.iter().cloned())
        )));
    }
    losses
}

fn counted(map: &BTreeMap<String, usize>) -> String {
    joined(map.iter().map(|(name, count)| format!("{name} {count}")))
}

fn tally_losses(tallies: &Tallies, issues: &Issues, metrics: &Metrics) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut push = |code: UsdLossCode, condition: bool, message: String| {
        if condition {
            losses.push(code.note(message));
        }
    };
    push(
        UsdLossCode::AttributeValueMalformed,
        metrics.malformed_units,
        "`metersPerUnit` is not a positive number; the layer was read in centimetres".to_owned(),
    );
    push(
        UsdLossCode::AttributeValueMalformed,
        metrics.malformed_mass_units,
        "`kilogramsPerUnit` is not a positive number; masses were read in kilograms".to_owned(),
    );
    push(
        UsdLossCode::AttributeValueMalformed,
        metrics.malformed_up_axis,
        "`upAxis` is not the token `Y` or `Z`; the layer was read with the `Y` axis up".to_owned(),
    );
    push(
        UsdLossCode::CompositionExternalArc,
        tallies.external > 0,
        format!(
            "{} references and payloads to other layers were kept as external prototypes; \
             their content was not loaded",
            tallies.external
        ),
    );
    push(
        UsdLossCode::MetadataNotTransferred,
        !tallies.metadata.is_empty(),
        format!(
            "prim metadata was not transferred: {}",
            counted(&tallies.metadata)
        ),
    );
    push(
        UsdLossCode::ApiSchemaNotTransferred,
        !tallies.api_schemas.is_empty(),
        format!(
            "properties of these applied API schemas were not transferred: {}",
            counted(&tallies.api_schemas)
        ),
    );
    push(
        UsdLossCode::PropertyNotTransferred,
        !tallies.unread_properties.is_empty(),
        format!(
            "properties of group prims were not transferred: {}",
            counted(&tallies.unread_properties)
        ),
    );
    push(
        UsdLossCode::PrimTypeNotTransferred,
        !tallies.skipped_types.is_empty(),
        format!(
            "prims of these types and their descendants were not transferred: {}",
            counted(&tallies.skipped_types)
        ),
    );
    push(
        UsdLossCode::PrimIntrinsicNotTransferred,
        !tallies.intrinsic.is_empty(),
        format!(
            "intrinsic primitives were placed without geometry: {}",
            counted(&tallies.intrinsic)
        ),
    );
    push(
        UsdLossCode::XformMalformed,
        !tallies.xform.is_empty(),
        format!(
            "{} transform-op stacks do not resolve; each prim was placed at its parent: {}",
            tallies.xform.len(),
            joined(tallies.xform.iter().cloned())
        ),
    );
    push(
        UsdLossCode::MeshSubdivisionNotApplied,
        tallies.subdivision > 0,
        format!(
            "{} subdivision meshes were transferred as their control cages",
            tallies.subdivision
        ),
    );
    push(
        UsdLossCode::MeshFaceRejected,
        tallies.rejected_faces > 0,
        format!(
            "{} mesh faces below three vertices or indexing no point were not transferred",
            tallies.rejected_faces
        ),
    );
    push(
        UsdLossCode::MeshRejected,
        !tallies.rejected_meshes.is_empty(),
        format!(
            "{} meshes were not transferred: {}",
            tallies.rejected_meshes.len(),
            joined(tallies.rejected_meshes.iter().cloned())
        ),
    );
    push(
        UsdLossCode::MeshNormalsNotTransferred,
        tallies.dropped_normals > 0,
        format!(
            "normals of {} meshes have an interpolation or count that does not map to their \
             triangles and were not transferred",
            tallies.dropped_normals
        ),
    );
    push(
        UsdLossCode::MeshSubsetNotTransferred,
        tallies.dropped_subsets > 0,
        format!(
            "{} geometry subsets that are not disjoint face subsets were not transferred",
            tallies.dropped_subsets
        ),
    );
    push(
        UsdLossCode::MeshPrimvarNotTransferred,
        !tallies.dropped_primvars.is_empty(),
        format!(
            "these primvars were not transferred: {}",
            joined(tallies.dropped_primvars.iter().cloned())
        ),
    );
    push(
        UsdLossCode::CurvesRejected,
        tallies.rejected_curves > 0,
        format!(
            "{} curves do not satisfy their topology and were not transferred",
            tallies.rejected_curves
        ),
    );
    push(
        UsdLossCode::NurbsPatchRejected,
        !tallies.rejected_patches.is_empty(),
        format!(
            "{} NURBS patches do not satisfy their counts and were not transferred: {}",
            tallies.rejected_patches.len(),
            joined(tallies.rejected_patches.iter().cloned())
        ),
    );
    push(
        UsdLossCode::JointRejected,
        !tallies.rejected_joints.is_empty(),
        format!(
            "{} joints name no body, or a body that is not a transferred prim, or have a \
             malformed frame, and were not transferred: {}",
            tallies.rejected_joints.len(),
            joined(tallies.rejected_joints.iter().cloned())
        ),
    );
    push(
        UsdLossCode::PhysicsPropertyNotTransferred,
        !tallies.physics_properties.is_empty(),
        format!(
            "{} UsdPhysics body properties were not transferred: {}",
            tallies.physics_properties.len(),
            joined(tallies.physics_properties.iter().cloned())
        ),
    );
    push(
        UsdLossCode::SolverPropertyNotTransferred,
        !tallies.solver_properties.is_empty(),
        format!(
            "{} PhysX properties were not transferred: {}",
            tallies.solver_properties.len(),
            joined(tallies.solver_properties.iter().cloned())
        ),
    );
    push(
        UsdLossCode::JointPropertyNotTransferred,
        !tallies.joint_properties.is_empty(),
        format!(
            "{} joint properties were not transferred: {}",
            tallies.joint_properties.len(),
            joined(tallies.joint_properties.iter().cloned())
        ),
    );
    push(
        UsdLossCode::NurbsTrimNotTransferred,
        tallies.trims > 0,
        format!(
            "trim curves of {} NURBS patches were not transferred; each face covers its \
             whole patch",
            tallies.trims
        ),
    );
    push(
        UsdLossCode::NurbsRangeNotTransferred,
        tallies.ranges > 0,
        format!(
            "{} NURBS patches state a range narrower than their knot domain; each face covers \
             the knot domain",
            tallies.ranges
        ),
    );
    push(
        UsdLossCode::DisplayColorNotTransferred,
        tallies.display_colors > 0,
        format!(
            "{} display colors or opacities are not constant and were not transferred",
            tallies.display_colors
        ),
    );
    push(
        UsdLossCode::MaterialNotTransferred,
        !tallies.materials.is_empty(),
        format!(
            "{} materials that are not constant UsdPreviewSurface, MaterialX, or OmniPBR \
             materials, and material bindings of geometry subsets, were not transferred in \
             full: {}",
            tallies.materials.len(),
            joined(tallies.materials.iter().cloned())
        ),
    );
    push(
        UsdLossCode::TextureUnlocated,
        !tallies.unlocated_textures.is_empty(),
        format!(
            "{} texture paths name no package member or file and were kept as written: {}",
            tallies.unlocated_textures.len(),
            joined(tallies.unlocated_textures.iter().cloned())
        ),
    );
    push(
        UsdLossCode::AttributeValueMalformed,
        !issues.malformed.is_empty(),
        format!(
            "{} attribute values do not have their declared type: {}",
            issues.malformed.len(),
            joined(
                issues
                    .malformed
                    .iter()
                    .map(|(path, reason)| format!("{path} ({reason})"))
            )
        ),
    );
    push(
        UsdLossCode::AttributeAnimationNotTransferred,
        !issues.animated.is_empty(),
        format!(
            "{} attributes have time samples or splines; each was read at its default value, \
             or its first sample: {}",
            issues.animated.len(),
            joined(issues.animated.iter().cloned())
        ),
    );
    losses
}

#[cfg(test)]
mod tests;
