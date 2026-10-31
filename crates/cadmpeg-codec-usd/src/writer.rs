// SPDX-License-Identifier: Apache-2.0
//! The USDA 1.0 writer.
//!
//! The layer has millimetre units. A document with one root occurrence that
//! holds child occurrences or several bodies, and nothing outside it, uses
//! that occurrence as the one root prim; otherwise each root occurrence and
//! each body no definition owns is a root prim. Each occurrence becomes an
//! `Xform` prim with a `matrix4d` transform. A definition placed more than
//! once is written once, under the abstract `Prototypes` prim, and each
//! placement references it. Bodies become geometry prims: tessellations as
//! `Mesh` prims; for a body with faces and no tessellation, each untrimmed
//! face on a NURBS surface as a `NurbsPatch` prim and the other faces as one
//! `Mesh` prim; and wire edges as one `NurbsCurves` prim. A placement whose content is one geometry prim is
//! written as that prim. Appearances become materials with a
//! `UsdPreviewSurface` shader and a `MaterialX` surface shader in a `Looks`
//! scope under the one root prim, or at the layer root.

mod brep;
mod physics;
mod physx;
mod shading;
mod text;
mod texture;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption,
    EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh, TextureCoordinateAddressing};
use cadmpeg_ir::topology::{Body, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::loss::UsdLossCode;
use text::{asset, quoted, real, real32, tuple, tuple32, Emitter, Names};

/// Name of the root prim.
const ROOT: &str = "World";
/// Name of the abstract prim that holds shared definitions.
const PROTOTYPES: &str = "Prototypes";
/// Name of the scope that holds materials.
const LOOKS: &str = "Looks";
/// Metres per scene unit of the written layer.
const METERS_PER_UNIT: &str = "1";
/// Millimetres per scene unit: the layer is in metres. Lengths are divided
/// by it, the inverse of the reader's product, so that a second round trip
/// writes the same values.
pub(super) const MILLIMETRES_PER_UNIT: f64 = 1000.0;
/// Up axis of a layer whose source states none.
const DEFAULT_UP_AXIS: &str = "Z";
/// Family name of the subsets that carry tessellation triangle groups.
const GROUP_FAMILY: &str = "triangleGroup";
/// Source attribute naming the stage up axis of a USD source.
const SOURCE_UP_AXIS: &str = "up_axis";

/// What the writer does with each model arena.
///
/// Topology, geometry carriers, tessellations, and appearances are
/// `Reported`: [`plan`] charges a loss for each record no prim carries.
const COVERAGE: ArenaDispositions = {
    use ArenaDisposition::{Omitted, Reported, Written};
    ArenaDispositions {
        bodies: Reported,
        regions: Reported,
        shells: Reported,
        faces: Reported,
        loops: Reported,
        coedges: Reported,
        edges: Reported,
        vertices: Reported,
        points: Reported,
        surfaces: Reported,
        curves: Reported,
        subds: Omitted,
        pcurves: Reported,
        procedural_surfaces: Omitted,
        procedural_curves: Omitted,
        assets: Written,
        features: Omitted,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: Omitted,
        parameters: Omitted,
        sketches: Omitted,
        sketch_entities: Omitted,
        sketch_constraints: Omitted,
        spatial_sketches: Omitted,
        spatial_sketch_entities: Omitted,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: Written,
        occurrences: Written,
        assembly_joints: Reported,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Reported,
        appearances: Reported,
        appearance_bindings: Reported,
        attributes: Omitted,
        pmi: Omitted,
        presentation_layers: Omitted,
        physical_materials: Written,
        mass_properties: Written,
        rigid_bodies: Written,
        colliders: Written,
        articulations: Written,
        physics_scenes: Written,
        solver_settings: Written,
    }
};

/// A package member: its name and its content.
pub(crate) type Member = (String, Vec<u8>);

/// Plans a USDA write, and returns the members a package holds beside the
/// layer when `packaged`.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
    packaged: bool,
) -> Result<(ExportBody, Vec<Member>), CodecError> {
    let Some(version) = target
        .index()
        .and_then(|index| crate::TARGET_VERSIONS.get(index).copied())
    else {
        return Err(target.unavailable(
            "the source text version is one this writer cannot synthesize, and a USD layer \
             has no byte-replay path",
        ));
    };
    let mut writer = Writer::new(input.ir, version);
    if packaged {
        writer.package = Some(Vec::new());
        writer.assets = input.assets;
    }
    let text = writer.layer();
    let losses = writer.losses();
    let members = writer.package.take().unwrap_or_default();
    let body = ExportBody {
        bytes: text.into_bytes(),
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path: WritePath::Synthesized {
            consumption: Consumption::NotConsumed,
        },
        coverage: ArenaCoverage::Declared(COVERAGE),
        losses,
        notes: vec![format!("USDA {version} text layer, metre units")],
    };
    Ok((body, members))
}

/// Counts of records no prim carries.
#[derive(Default)]
struct Unwritten {
    tessellated_bodies: usize,
    curves: usize,
    channels: usize,
    reduced_appearances: BTreeSet<String>,
    /// Texture paths that the output names but does not hold.
    external_textures: BTreeSet<String>,
    rounded_points: usize,
    free_carriers: usize,
    /// Joints with no `UsdPhysics` form or with bodies that are not written
    /// prims.
    joints: usize,
    /// Joint drives on degrees of freedom the joint type does not name.
    joint_drives: usize,
    /// Joint properties with no `UsdPhysics` or `PhysX` attribute.
    joint_properties: usize,
    /// Solver properties with no prim, attribute, or target to write.
    solver_properties: usize,
    /// Faces of bodies with no tessellation that do not mesh, with the
    /// reason.
    unmeshed: Vec<String>,
}

/// The placement of a prim: its transform, per-axis scale, visibility, and
/// prim metadata.
#[derive(Debug, Clone)]
struct Placement<'a> {
    transform: Option<&'a Transform>,
    scale: [f64; 3],
    hidden: bool,
    metadata: Vec<String>,
    /// Property lines written after the transform ops.
    properties: Vec<String>,
}

impl Default for Placement<'_> {
    fn default() -> Self {
        Self {
            transform: None,
            scale: [1.0; 3],
            hidden: false,
            metadata: Vec::new(),
            properties: Vec::new(),
        }
    }
}

/// One geometry prim of a body.
enum Geometry<'ir> {
    Mesh(&'ir Tessellation),
    /// The mesh of faces of a body with no tessellation.
    Meshed(Box<Tessellation>),
    Patch(&'ir cadmpeg_ir::topology::Face, &'ir NurbsSurface),
    Wires(Vec<WireCurve>),
}

/// The NURBS form of a surface carrier, when it has one.
fn nurbs_surface(surface: &cadmpeg_ir::geometry::Surface) -> Option<&NurbsSurface> {
    match &surface.geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
        | SurfaceGeometry::Procedural {
            cache: Some(SolvedSurfaceGeometry::Nurbs(nurbs)),
            ..
        } => Some(nurbs),
        _ => None,
    }
}

/// The binding key of triangle group `group` of `tessellation`.
fn group_key(tessellation: &str, group: &str) -> String {
    format!("{tessellation}#group:{group}")
}

/// Whether `occurrence` places a part definition.
fn is_part(ir: &CadIr, occurrence: &Occurrence) -> bool {
    let PrototypeReference::Local { definition } = &occurrence.prototype else {
        return false;
    };
    ir.model.product_definitions.iter().any(|candidate| {
        &candidate.id == definition
            && candidate.kind == cadmpeg_ir::products::ProductDefinitionKind::Part
    })
}

/// One wire edge in NURBS form.
struct WireCurve {
    points: Vec<cadmpeg_ir::math::Point3>,
    knots: Vec<f64>,
    weights: Vec<f64>,
    degree: u32,
    range: [f64; 2],
}

struct Writer<'ir> {
    ir: &'ir CadIr,
    /// The header version.
    version: &'static str,
    out: Emitter,
    unwritten: Unwritten,
    /// Material paths by appearance id.
    materials: BTreeMap<String, String>,
    /// Appearance ids bound to each tessellation or body id.
    bindings: BTreeMap<String, String>,
    /// Definitions placed more than once, with their prototype prim names.
    shared: BTreeMap<ProductDefinitionId, String>,
    /// Bodies some definition owns.
    owned: BTreeSet<BodyId>,
    /// Names of the prims that enclose the prim being written.
    stack: Vec<String>,
    /// The prim path of each written occurrence.
    paths: BTreeMap<cadmpeg_ir::ids::OccurrenceId, String>,
    /// The prim path of each relationship-target placeholder.
    target_paths: BTreeMap<String, String>,
    /// Physics schemas and properties of each occurrence and body.
    physics: physics::Plan,
    /// Meshes the faces of bodies with no tessellation.
    tessellator: cadmpeg_tessellate::Tessellator<'ir>,
    /// Occurrences with a part occurrence below them.
    above_parts: BTreeSet<cadmpeg_ir::ids::OccurrenceId>,
    /// The members a package holds beside the root layer, or `None` for a
    /// layer that no package holds.
    package: Option<Vec<Member>>,
    /// The source of the files that external assets name.
    assets: Option<&'ir dyn cadmpeg_core::decode::Companions>,
}

impl<'ir> Writer<'ir> {
    fn new(ir: &'ir CadIr, version: &'static str) -> Self {
        let mut bindings = BTreeMap::new();
        for binding in &ir.model.appearance_bindings {
            let key = match &binding.target {
                AppearanceTarget::Tessellation(id) => id.clone(),
                AppearanceTarget::Body(id) => id.to_string(),
                AppearanceTarget::TriangleGroup {
                    tessellation,
                    group,
                } => group_key(tessellation, group),
                _ => continue,
            };
            bindings
                .entry(key)
                .or_insert_with(|| binding.appearance.to_string());
        }
        let mut uses: BTreeMap<&ProductDefinitionId, usize> = BTreeMap::new();
        for occurrence in &ir.model.occurrences {
            if let PrototypeReference::Local { definition } = &occurrence.prototype {
                *uses.entry(definition).or_default() += 1;
            }
        }
        let mut names = Names::default();
        let shared = ir
            .model
            .product_definitions
            .iter()
            .filter(|definition| {
                uses.get(&definition.id).copied().unwrap_or_default() > 1
                    && !definition.bodies.is_empty()
            })
            .map(|definition| {
                let name = names.unique(definition.source_name.as_deref().unwrap_or("Prototype"));
                (definition.id.clone(), name)
            })
            .collect();
        let owned = ir
            .model
            .product_definitions
            .iter()
            .flat_map(|definition| definition.bodies.iter().cloned())
            .collect();
        let parents: BTreeMap<_, _> = ir
            .model
            .occurrences
            .iter()
            .filter_map(|occurrence| match &occurrence.parent {
                OccurrenceParent::Occurrence { occurrence: parent } => {
                    Some((&occurrence.id, parent))
                }
                OccurrenceParent::Root {} => None,
            })
            .collect();
        let mut above_parts = BTreeSet::new();
        for occurrence in &ir.model.occurrences {
            if !is_part(ir, occurrence) {
                continue;
            }
            let mut at = &occurrence.id;
            while let Some(parent) = parents.get(at) {
                if !above_parts.insert((*parent).clone()) {
                    break;
                }
                at = parent;
            }
        }
        Self {
            ir,
            version,
            out: Emitter::default(),
            unwritten: Unwritten::default(),
            materials: BTreeMap::new(),
            bindings,
            shared,
            owned,
            stack: Vec::new(),
            paths: BTreeMap::new(),
            target_paths: BTreeMap::new(),
            physics: physics::Plan::new(ir),
            tessellator: cadmpeg_tessellate::Tessellator::new(
                ir,
                cadmpeg_tessellate::Options::default(),
            ),
            above_parts,
            package: None,
            assets: None,
        }
    }

    /// The model kind of the prim of `occurrence` (spec §4.4): `assembly`
    /// for a part above other parts, `component` for another part, and
    /// `group` for any other occurrence above a part.
    fn model_kind(&self, occurrence: &Occurrence) -> Option<&'static str> {
        let part = is_part(self.ir, occurrence);
        match (part, self.above_parts.contains(&occurrence.id)) {
            (true, true) => Some("assembly"),
            (true, false) => Some("component"),
            (false, true) => Some("group"),
            (false, false) => None,
        }
    }

    fn up_axis(&self) -> String {
        self.ir
            .source
            .as_ref()
            .filter(|source| source.format() == crate::dialect::FORMAT)
            .and_then(|source| {
                source
                    .attributes
                    .iter()
                    .find(|(key, _)| key.as_str() == SOURCE_UP_AXIS)
                    .map(|(_, value)| value.clone())
            })
            .filter(|axis| axis == "Y" || axis == "Z")
            .unwrap_or_else(|| DEFAULT_UP_AXIS.to_owned())
    }

    fn layer(&mut self) -> String {
        let roots: Vec<&'ir Occurrence> = self
            .ir
            .model
            .occurrences
            .iter()
            .filter(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
            .collect();
        let (free_curves, free_points) = self.free_carriers();
        let orphans = self
            .ir
            .model
            .bodies
            .iter()
            .any(|body| !self.owned.contains(&body.id))
            || self
                .ir
                .model
                .tessellations
                .iter()
                .any(|tessellation| tessellation.body.is_none())
            || !free_curves.is_empty()
            || !free_points.is_empty();
        let mut names = Names::default();
        let physics = !self.ir.model.assembly_joints.is_empty()
            || !self.ir.model.physical_materials.is_empty()
            || !self.ir.model.physics_scenes.is_empty();
        let default_prim = match roots.as_slice() {
            [root] if !orphans && !self.is_leaf(root) => {
                let name = text::identifier(root.name.as_deref().unwrap_or(ROOT));
                self.physics.set_parent(&format!("/{name}"));
                self.occurrence(root, &mut names, Some(&name));
                Some(name)
            }
            _ => {
                if physics {
                    names.reserve(physics::PHYSICS);
                }
                if !self.ir.model.appearances.is_empty() {
                    names.reserve(LOOKS);
                    self.looks(None);
                }
                for occurrence in roots {
                    self.occurrence(occurrence, &mut names, None);
                }
                for body in &self.ir.model.bodies {
                    if !self.owned.contains(&body.id) {
                        self.body(body, &mut names, &Placement::default(), None);
                    }
                }
                for tessellation in &self.ir.model.tessellations {
                    if tessellation.body.is_none() {
                        let name = names.unique("Mesh");
                        self.mesh(tessellation, &name, None, &Placement::default());
                    }
                }
                self.free_geometry(&mut names);
                self.joints(&mut names);
                names.first().map(str::to_owned)
            }
        };
        if !self.shared.is_empty() {
            self.prototypes();
        }
        let prims = std::mem::take(&mut self.out).finish();
        for (occurrence, path) in &self.paths {
            if let Some(key) =
                physx::placeholder(&cadmpeg_ir::physics::solver::SolverTarget::Occurrence {
                    occurrence: occurrence.clone(),
                })
            {
                self.target_paths.insert(key, path.clone());
            }
        }
        let (prims, unresolved) = physx::resolve(&prims, &self.target_paths);
        self.unwritten.solver_properties += unresolved + self.physics.unwritten_settings;
        let mut header = Emitter::default();
        header.line(&format!("#usda {}", self.version));
        header.line("(");
        if let Some(name) = default_prim {
            header.line(&format!("    defaultPrim = {}", quoted(&name)));
        }
        header.line(&format!("    metersPerUnit = {METERS_PER_UNIT}"));
        if physics || !self.physics.is_empty() {
            header.line("    kilogramsPerUnit = 1");
        }
        header.line(&format!("    upAxis = {}", quoted(&self.up_axis())));
        header.line(")");
        header.blank();
        header.finish() + &prims
    }

    /// The `Looks` scope with one material per appearance, under the root
    /// prim `root`, or at the layer root.
    fn looks(&mut self, root: Option<&str>) {
        if self.ir.model.appearances.is_empty() {
            return;
        }
        let scope = match root {
            Some(root) => format!("/{root}/{LOOKS}"),
            None => format!("/{LOOKS}"),
        };
        self.out.line(&format!("def Scope {}", quoted(LOOKS)));
        self.out.open();
        let mut names = Names::default();
        for appearance in &self.ir.model.appearances {
            let name = names.unique(appearance.name.as_deref().unwrap_or("Material"));
            let path = format!("{scope}/{name}");
            self.materials
                .insert(appearance.id.to_string(), path.clone());
            self.material(appearance, &name, &path);
        }
        self.out.close();
    }

    /// The material path bound to triangle group `group` of
    /// `tessellation`.
    fn group_binding_path(&self, tessellation: &str, group: &str) -> Option<String> {
        self.binding_path(&group_key(tessellation, group))
    }

    fn binding_path(&self, key: &str) -> Option<String> {
        self.bindings
            .get(key)
            .and_then(|appearance| self.materials.get(appearance))
            .cloned()
    }

    /// Writes an occurrence and its descendants. The root occurrence names
    /// the root prim `root` and holds the `Looks` scope.
    fn occurrence(&mut self, occurrence: &'ir Occurrence, names: &mut Names, root: Option<&str>) {
        let name = match root {
            Some(root) => {
                names.reserve(root);
                root.to_owned()
            }
            None => names.unique(occurrence.name.as_deref().unwrap_or("Occurrence")),
        };
        self.stack.push(name.clone());
        self.paths
            .insert(occurrence.id.clone(), format!("/{}", self.stack.join("/")));
        let (schemas, properties) = self.physics.occurrence(&occurrence.id);
        let mut placement = Placement {
            transform: Some(&occurrence.transform),
            scale: occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get),
            hidden: occurrence.visible == Some(false),
            metadata: schemas,
            properties,
        };
        if occurrence
            .name
            .as_deref()
            .is_some_and(|original| original != name)
        {
            placement.metadata.push(format!(
                "displayName = {}",
                quoted(occurrence.name.as_deref().unwrap_or_default())
            ));
        }
        let mut definition = None;
        match &occurrence.prototype {
            PrototypeReference::Local { definition: id } => {
                definition = self
                    .ir
                    .model
                    .product_definitions
                    .iter()
                    .find(|candidate| &candidate.id == id);
                if let Some(prototype) = self.shared.get(id) {
                    placement
                        .metadata
                        .push(format!("prepend references = </{PROTOTYPES}/{prototype}>"));
                }
            }
            PrototypeReference::External { document, object } => {
                if let ExternalDocument::Path { path } = document {
                    let target = object
                        .as_deref()
                        .filter(|object| object.starts_with('/'))
                        .map(|object| format!("<{object}>"))
                        .unwrap_or_default();
                    placement.metadata.push(format!(
                        "prepend references = {}{target}",
                        asset(path.as_str())
                    ));
                }
            }
            PrototypeReference::Unresolved {} => {}
        }
        let mut children: Vec<&'ir Occurrence> = self.children(occurrence);
        let mut references = Vec::new();
        children.retain(|child| match self.arc_reference(child) {
            Some(reference) => {
                references.push(reference);
                false
            }
            None => true,
        });
        match references.as_slice() {
            [] => {}
            [reference] => placement
                .metadata
                .push(format!("prepend references = {reference}")),
            _ => placement
                .metadata
                .push(format!("prepend references = [{}]", references.join(", "))),
        }
        let shared = definition.is_some_and(|definition| self.shared.contains_key(&definition.id));
        let bodies: Vec<&'ir Body> = definition
            .filter(|_| !shared)
            .map(|definition| {
                definition
                    .bodies
                    .iter()
                    .filter_map(|id| self.ir.model.bodies.iter().find(|body| &body.id == id))
                    .collect()
            })
            .unwrap_or_default();
        if root.is_none() && children.is_empty() && !shared {
            if let [body] = bodies.as_slice() {
                self.body(body, names, &placement, Some(&name));
                self.stack.pop();
                return;
            }
        }
        if let Some(kind) = self.model_kind(occurrence) {
            placement.metadata.push(format!("kind = {}", quoted(kind)));
        }
        let head = if shared {
            if children.is_empty() {
                placement
                    .metadata
                    .insert(0, "instanceable = true".to_owned());
            }
            "def"
        } else {
            "def Xform"
        };
        self.prim_head(head, &name, &placement.metadata);
        self.out.open();
        self.placement(&placement);
        if root.is_some() {
            self.looks(root);
        }
        let mut child_names = Names::default();
        child_names.reserve(LOOKS);
        if root.is_some() {
            child_names.reserve(physics::PHYSICS);
        }
        if shared {
            for body in definition
                .map(|definition| definition.bodies.as_slice())
                .unwrap_or_default()
            {
                child_names.reserve(&self.body_name(body));
            }
        }
        for body in bodies {
            self.body(body, &mut child_names, &Placement::default(), None);
        }
        for child in children {
            self.occurrence(child, &mut child_names, None);
        }
        if root.is_some() {
            self.joints(&mut child_names);
        }
        self.out.close();
        self.stack.pop();
    }

    /// Whether `occurrence` has no child occurrences and places a definition
    /// of one unshared body: it is written as that body's prim.
    fn is_leaf(&self, occurrence: &Occurrence) -> bool {
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            return false;
        };
        self.children(occurrence).is_empty()
            && !self.shared.contains_key(definition)
            && self
                .ir
                .model
                .product_definitions
                .iter()
                .find(|candidate| &candidate.id == definition)
                .is_some_and(|definition| definition.bodies.len() == 1)
    }

    /// The child occurrences of `occurrence`.
    fn children(&self, occurrence: &Occurrence) -> Vec<&'ir Occurrence> {
        self.ir
            .model
            .occurrences
            .iter()
            .filter(|child| {
                matches!(&child.parent, OccurrenceParent::Occurrence { occurrence: parent }
                    if parent == &occurrence.id)
            })
            .collect()
    }

    /// The reference list item that writes `child` on its parent prim:
    /// an external prototype placed at the identity, with no children of its
    /// own.
    fn arc_reference(&self, child: &Occurrence) -> Option<String> {
        let PrototypeReference::External {
            document: ExternalDocument::Path { path },
            object,
        } = &child.prototype
        else {
            return None;
        };
        let placed = child.transform.affine_rows() != Transform::identity().affine_rows()
            || child.scale.iter().any(|scale| scale.get() != 1.0)
            || child.visible.is_some();
        if placed || !self.children(child).is_empty() {
            return None;
        }
        let target = object
            .as_deref()
            .filter(|object| object.starts_with('/'))
            .map(|object| format!("<{object}>"))
            .unwrap_or_default();
        Some(format!("{}{target}", asset(path.as_str())))
    }

    fn prim_head(&mut self, head: &str, name: &str, metadata: &[String]) {
        self.out.name_next(name);
        let metadata = merged_api_schemas(metadata);
        if metadata.is_empty() {
            self.out.line(&format!("{head} {}", quoted(name)));
            return;
        }
        self.out.line(&format!("{head} {} (", quoted(name)));
        for item in &metadata {
            self.out.line(&format!("    {item}"));
        }
        self.out.line(")");
    }

    /// Writes the visibility and transform ops of a placement.
    fn placement(&mut self, placement: &Placement<'_>) {
        if placement.hidden {
            self.out.line("token visibility = \"invisible\"");
        }
        let mut order = Vec::new();
        if let Some(transform) = placement.transform {
            let rows = transform.affine_rows();
            if rows != Transform::identity().affine_rows() {
                let [r0, r1, r2] = rows;
                let matrix = format!(
                    "({}, {}, {}, {})",
                    tuple(&[r0[0], r1[0], r2[0], 0.0]),
                    tuple(&[r0[1], r1[1], r2[1], 0.0]),
                    tuple(&[r0[2], r1[2], r2[2], 0.0]),
                    tuple(&[
                        r0[3] / MILLIMETRES_PER_UNIT,
                        r1[3] / MILLIMETRES_PER_UNIT,
                        r2[3] / MILLIMETRES_PER_UNIT,
                        1.0
                    ]),
                );
                self.out
                    .line(&format!("matrix4d xformOp:transform = {matrix}"));
                order.push("\"xformOp:transform\"");
            }
        }
        if placement.scale != [1.0; 3] {
            self.out.line(&format!(
                "double3 xformOp:scale = {}",
                tuple(&placement.scale)
            ));
            order.push("\"xformOp:scale\"");
        }
        if !order.is_empty() {
            self.out.line(&format!(
                "uniform token[] xformOpOrder = [{}]",
                order.join(", ")
            ));
        }
        for line in &placement.properties {
            self.out.line(line);
        }
    }

    fn body_name(&self, id: &BodyId) -> String {
        let body = self.ir.model.bodies.iter().find(|body| &body.id == id);
        text::identifier(body.and_then(|body| body.name.as_deref()).unwrap_or("Body"))
    }

    /// The tessellations of a body.
    fn tessellations(&self, body: &BodyId) -> Vec<&'ir Tessellation> {
        self.ir
            .model
            .tessellations
            .iter()
            .filter(|tessellation| tessellation.body.as_ref() == Some(body))
            .collect()
    }

    /// The geometry prims of a body. Records no prim carries are counted
    /// here, once per written body.
    fn geometry(&mut self, body: &'ir Body) -> Vec<Geometry<'ir>> {
        let mut prims = Vec::new();
        let tessellations = self.tessellations(&body.id);
        let faces = self.body_faces(body);
        if !tessellations.is_empty() {
            if !faces.is_empty() {
                self.unwritten.tessellated_bodies += 1;
            }
            prims.extend(tessellations.into_iter().map(Geometry::Mesh));
        } else if !faces.is_empty() {
            let mut meshed = Vec::new();
            for (face, surface) in faces {
                match surface.and_then(nurbs_surface) {
                    Some(nurbs) if face.loops.is_empty() => {
                        prims.push(Geometry::Patch(face, nurbs));
                    }
                    _ => meshed.push(&face.id),
                }
            }
            if !meshed.is_empty() {
                self.tessellator
                    .set_options(brep::mesh_options(self.ir, body));
                let (mesh, unmeshed) = brep::mesh(body, &meshed, &mut self.tessellator);
                self.unwritten.unmeshed.extend(unmeshed);
                if let Some(mesh) = mesh {
                    self.unwritten.tessellated_bodies += 1;
                    prims.push(Geometry::Meshed(Box::new(mesh)));
                }
            }
        }
        let wires = self.wire_curves(body);
        if !wires.is_empty() {
            prims.push(Geometry::Wires(wires));
        }
        prims
    }

    /// Writes one body. A body of one geometry prim is written as that prim
    /// with `placement`; otherwise an `Xform` prim with `placement` holds the
    /// geometry prims.
    fn body(
        &mut self,
        body: &'ir Body,
        names: &mut Names,
        placement: &Placement<'_>,
        name: Option<&str>,
    ) {
        let mut prims = self.geometry(body);
        if prims.is_empty() && name.is_none() {
            return;
        }
        let name = name.map_or_else(
            || names.unique(body.name.as_deref().unwrap_or("Body")),
            str::to_owned,
        );
        let hidden = placement.hidden || body.visible == Some(false);
        let collider = self.physics.collider(&body.id);
        if body.transform.is_none() && prims.len() == 1 {
            let (schemas, properties) = self.physics.body(&body.id);
            let mut placement = Placement {
                hidden,
                ..placement.clone()
            };
            placement.metadata.extend(schemas);
            placement.properties.extend(properties);
            if let Some(prim) = prims.pop() {
                self.geometry_prim(prim, &name, body, &placement, &collider);
            }
            return;
        }
        self.prim_head("def Xform", &name, &placement.metadata);
        self.out.open();
        self.placement(&Placement {
            hidden,
            metadata: Vec::new(),
            ..placement.clone()
        });
        let inner = Placement {
            transform: body.transform.as_ref(),
            ..Placement::default()
        };
        let mut children = Names::default();
        for prim in prims {
            let base = match &prim {
                Geometry::Mesh(_) | Geometry::Meshed(_) => "Mesh",
                Geometry::Patch(..) => "Patch",
                Geometry::Wires(_) => "Curves",
            };
            let child = children.unique(base);
            self.geometry_prim(prim, &child, body, &inner, &collider);
        }
        self.out.close();
    }

    fn geometry_prim(
        &mut self,
        prim: Geometry<'ir>,
        name: &str,
        body: &'ir Body,
        placement: &Placement<'_>,
        collider: &physics::PrimPhysics,
    ) {
        match prim {
            Geometry::Mesh(tessellation) => {
                let mut placement = placement.clone();
                placement.metadata.extend(collider.0.iter().cloned());
                placement.properties.extend(collider.1.iter().cloned());
                self.mesh(tessellation, name, Some(body), &placement);
            }
            Geometry::Meshed(tessellation) => {
                let mut placement = placement.clone();
                placement.metadata.extend(collider.0.iter().cloned());
                placement.properties.extend(collider.1.iter().cloned());
                self.mesh(&tessellation, name, Some(body), &placement);
            }
            Geometry::Patch(face, nurbs) => {
                let mut placement = placement.clone();
                placement.metadata.extend(collider.0.iter().cloned());
                placement.properties.extend(collider.1.iter().cloned());
                self.patch(face, nurbs, body, name, &placement);
            }
            Geometry::Wires(curves) => self.wire_prim(&curves, Some(body), name, placement),
        }
    }

    /// The faces of a body with their surfaces.
    fn body_faces(
        &self,
        body: &Body,
    ) -> Vec<(
        &'ir cadmpeg_ir::topology::Face,
        Option<&'ir cadmpeg_ir::geometry::Surface>,
    )> {
        let model = &self.ir.model;
        let mut out = Vec::new();
        for region in model
            .regions
            .iter()
            .filter(|region| body.regions.contains(&region.id))
        {
            for shell in model
                .shells
                .iter()
                .filter(|shell| region.shells.contains(&shell.id))
            {
                for face in model
                    .faces
                    .iter()
                    .filter(|face| shell.faces().contains(&face.id))
                {
                    let surface = model
                        .surfaces
                        .iter()
                        .find(|surface| surface.id == face.surface);
                    out.push((face, surface));
                }
            }
        }
        out
    }

    fn color_primvars(&mut self, body: Option<&Body>) {
        let Some(color) = body.and_then(|body| body.color) else {
            return;
        };
        self.out.line(&format!(
            "color3f[] primvars:displayColor = [{}] (",
            tuple32(&[color.r(), color.g(), color.b()])
        ));
        self.out.line("    interpolation = \"constant\"");
        self.out.line(")");
        if color.a() < 1.0 {
            self.out.line(&format!(
                "float[] primvars:displayOpacity = [{}] (",
                real32(color.a())
            ));
            self.out.line("    interpolation = \"constant\"");
            self.out.line(")");
        }
    }

    fn point_text(&mut self, point: cadmpeg_ir::math::Point3) -> (String, [f32; 3]) {
        let scene = [point.x, point.y, point.z].map(|value| value / MILLIMETRES_PER_UNIT);
        let values = scene.map(crate::text::value::round_f32);
        if values
            .iter()
            .zip(scene)
            .any(|(rounded, original)| f64::from(*rounded) != original)
        {
            self.unwritten.rounded_points += 1;
        }
        (tuple32(&values), values)
    }

    fn extent(&mut self, bounds: Option<([f32; 3], [f32; 3])>) {
        if let Some((min, max)) = bounds {
            self.out.line(&format!(
                "float3[] extent = [{}, {}]",
                tuple32(&min),
                tuple32(&max)
            ));
        }
    }

    /// Writes the head of a geometry prim with its placement and its
    /// material binding, and opens its body.
    fn geometry_head(
        &mut self,
        prim_type: &str,
        name: &str,
        placement: &Placement<'_>,
        binding: Option<&str>,
    ) {
        let mut metadata = placement.metadata.clone();
        if binding.is_some() {
            metadata.push("prepend apiSchemas = [\"MaterialBindingAPI\"]".to_owned());
        }
        self.prim_head(&format!("def {prim_type}"), name, &metadata);
        self.out.open();
        self.placement(placement);
        if let Some(path) = binding {
            self.out.line(&format!("rel material:binding = <{path}>"));
        }
    }

    fn mesh(
        &mut self,
        tessellation: &Tessellation,
        name: &str,
        body: Option<&Body>,
        placement: &Placement<'_>,
    ) {
        let binding = self
            .binding_path(&tessellation.id.to_string())
            .or_else(|| body.and_then(|body| self.binding_path(&body.id.to_string())));
        self.geometry_head("Mesh", name, placement, binding.as_deref());
        let mut points = Vec::new();
        let mut bounds: Option<([f32; 3], [f32; 3])> = None;
        for vertex in tessellation.vertices() {
            let (text, values) = self.point_text(vertex.get());
            points.push(text);
            bounds = Some(grow(bounds, values));
        }
        self.extent(bounds);
        let triangles = tessellation.triangles();
        self.out.array(
            "int[] faceVertexCounts",
            &triangles.iter().map(|_| "3".to_owned()).collect::<Vec<_>>(),
            "",
        );
        self.out.array(
            "int[] faceVertexIndices",
            &triangles
                .iter()
                .flatten()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "",
        );
        let normals = match tessellation.mesh() {
            TessellationMesh::ShadedList { .. } | TessellationMesh::ShadedStrips { .. } => {
                Some((tessellation.vertex_normals(), "vertex"))
            }
            TessellationMesh::CornerShadedList { .. } => {
                Some((tessellation.per_corner_normals(), "faceVarying"))
            }
            TessellationMesh::List { .. } | TessellationMesh::Strips { .. } => None,
        };
        if let Some((normals, interpolation)) = normals {
            let normals: Vec<String> = normals
                .iter()
                .map(|normal| {
                    let normal = normal.get();
                    tuple32(&[normal.x, normal.y, normal.z].map(crate::text::value::round_f32))
                })
                .collect();
            self.out.array("normal3f[] normals", &normals, " (");
            self.out
                .line(&format!("    interpolation = {}", quoted(interpolation)));
            self.out.line(")");
        }
        self.out.array("point3f[] points", &points, "");
        self.color_primvars(body);
        for set in tessellation.texture_coordinates() {
            let values: Vec<String> = set
                .values
                .iter()
                .map(|value| {
                    let point = value.get();
                    tuple32(&[point.u, point.v].map(crate::text::value::round_f32))
                })
                .collect();
            let interpolation = match set.addressing {
                TextureCoordinateAddressing::Vertex => "vertex",
                TextureCoordinateAddressing::Corner => "faceVarying",
            };
            self.out.array(
                &format!(
                    "texCoord2f[] primvars:{}",
                    text::identifier(set.name.as_str())
                ),
                &values,
                " (",
            );
            self.out
                .line(&format!("    interpolation = {}", quoted(interpolation)));
            self.out.line(")");
        }
        self.out.line("uniform token subdivisionScheme = \"none\"");
        if !tessellation.channels().is_empty() || !tessellation.texture_assignments().is_empty() {
            self.unwritten.channels += 1;
        }
        let tessellation_id = tessellation.id.to_string();
        let bound: Vec<Option<String>> = tessellation
            .triangle_groups()
            .iter()
            .map(|group| {
                group
                    .source_id
                    .as_deref()
                    .and_then(|id| self.group_binding_path(&tessellation_id, id))
            })
            .collect();
        if bound.iter().any(Option::is_some) {
            self.out
                .line("uniform token subsetFamily:materialBind:familyType = \"nonOverlapping\"");
        }
        let mut subsets = Names::default();
        for (group, binding) in tessellation.triangle_groups().iter().zip(bound) {
            // A group with no source identity holds the triangles that the
            // named groups do not; it needs no subset.
            let Some(source) = group.source_id.as_deref() else {
                continue;
            };
            let name = subsets.unique(source.rsplit('/').next().unwrap_or("Group"));
            let family = if binding.is_some() {
                "materialBind"
            } else {
                GROUP_FAMILY
            };
            let metadata = if binding.is_some() {
                vec!["prepend apiSchemas = [\"MaterialBindingAPI\"]".to_owned()]
            } else {
                Vec::new()
            };
            self.prim_head("def GeomSubset", &name, &metadata);
            self.out.open();
            self.out.line("uniform token elementType = \"face\"");
            self.out
                .line(&format!("uniform token familyName = {}", quoted(family)));
            self.out.array(
                "int[] indices",
                &group
                    .triangles
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "",
            );
            if let Some(path) = binding {
                self.out.line(&format!("rel material:binding = <{path}>"));
            }
            self.out.close();
        }
        self.out.close();
    }

    /// Writes a face on a NURBS surface as a `NurbsPatch` prim.
    fn patch(
        &mut self,
        face: &cadmpeg_ir::topology::Face,
        nurbs: &NurbsSurface,
        body: &Body,
        name: &str,
        placement: &Placement<'_>,
    ) {
        let binding = self.binding_path(&body.id.to_string());
        self.geometry_head("NurbsPatch", name, placement, binding.as_deref());
        let grid = nurbs.control_grid();
        let (u_count, v_count) = (nurbs.u_count(), nurbs.v_count());
        let mut points = Vec::new();
        let mut bounds = None;
        for v in 0..v_count {
            for row in &grid {
                if let Some(point) = row.get(v) {
                    let (text, values) = self.point_text(point.get());
                    points.push(text);
                    bounds = Some(grow(bounds, values));
                }
            }
        }
        self.extent(bounds);
        self.out.line(&format!("int uVertexCount = {u_count}"));
        self.out.line(&format!("int vVertexCount = {v_count}"));
        self.out
            .line(&format!("int uOrder = {}", u64::from(nurbs.u_degree()) + 1));
        self.out
            .line(&format!("int vOrder = {}", u64::from(nurbs.v_degree()) + 1));
        let knots = |values: &[f64]| values.iter().map(|knot| real(*knot)).collect::<Vec<_>>();
        self.out
            .array("double[] uKnots", &knots(nurbs.u_knots().as_slice()), "");
        self.out
            .array("double[] vKnots", &knots(nurbs.v_knots().as_slice()), "");
        if let Some(weights) = nurbs.weights() {
            let mut lane = Vec::new();
            for v in 0..v_count {
                for row in &weights {
                    if let Some(weight) = row.get(v) {
                        lane.push(real(weight.get()));
                    }
                }
            }
            self.out.array("double[] pointWeights", &lane, "");
        }
        self.out.array("point3f[] points", &points, "");
        let reversed = nurbs.normal_reversed() != (face.sense == Sense::Reversed);
        if reversed {
            self.out.line("uniform token orientation = \"leftHanded\"");
        }
        self.color_primvars(Some(body));
        self.out.close();
    }

    /// The wire edges of a body in NURBS form. Edges with no line or NURBS
    /// curve are counted as unwritten.
    fn wire_curves(&mut self, body: &Body) -> Vec<WireCurve> {
        let model = &self.ir.model;
        let mut curves = Vec::new();
        for region in model
            .regions
            .iter()
            .filter(|region| body.regions.contains(&region.id))
        {
            for shell in model
                .shells
                .iter()
                .filter(|shell| region.shells.contains(&shell.id))
            {
                for edge in model
                    .edges
                    .iter()
                    .filter(|edge| shell.wire_edges().contains(&edge.id))
                {
                    match self.edge_nurbs(edge) {
                        Some(curve) => curves.push(curve),
                        None => self.unwritten.curves += 1,
                    }
                }
            }
        }
        curves
    }

    /// Writes curves as one `NurbsCurves` prim.
    fn wire_prim(
        &mut self,
        curves: &[WireCurve],
        body: Option<&Body>,
        name: &str,
        placement: &Placement<'_>,
    ) {
        let mut counts = Vec::new();
        let mut orders = Vec::new();
        let mut knots = Vec::new();
        let mut ranges = Vec::new();
        let mut points = Vec::new();
        let mut weights: Vec<String> = Vec::new();
        let mut rational = false;
        let mut bounds = None;
        for curve in curves {
            counts.push(curve.points.len().to_string());
            orders.push((u64::from(curve.degree) + 1).to_string());
            knots.extend(curve.knots.iter().map(|knot| real(*knot)));
            ranges.push(tuple(&curve.range));
            rational |= curve.weights.iter().any(|weight| *weight != 1.0);
            weights.extend(curve.weights.iter().map(|weight| real(*weight)));
            for point in &curve.points {
                let (text, values) = self.point_text(*point);
                points.push(text);
                bounds = Some(grow(bounds, values));
            }
        }
        self.geometry_head("NurbsCurves", name, placement, None);
        self.extent(bounds);
        self.out.array("int[] curveVertexCounts", &counts, "");
        self.out.array("int[] order", &orders, "");
        self.out.array("double[] knots", &knots, "");
        self.out.array("double2[] ranges", &ranges, "");
        if rational {
            self.out.array("double[] pointWeights", &weights, "");
        }
        self.out.array("point3f[] points", &points, "");
        self.color_primvars(body);
        self.out.close();
    }

    /// The NURBS form of a wire edge.
    fn edge_nurbs(&self, edge: &cadmpeg_ir::topology::Edge) -> Option<WireCurve> {
        let model = &self.ir.model;
        let curve = model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())?;
        let position = |vertex: &cadmpeg_ir::ids::VertexId| {
            let vertex = model
                .vertices
                .iter()
                .find(|candidate| &candidate.id == vertex)?;
            model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .map(|point| point.position().get())
        };
        let ends = position(&edge.start).zip(position(&edge.end));
        curve_nurbs(
            curve,
            edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
            ends,
        )
    }

    /// The curves and points no topology uses and no owner supports.
    fn free_carriers(
        &self,
    ) -> (
        Vec<&'ir cadmpeg_ir::geometry::Curve>,
        Vec<&'ir cadmpeg_ir::topology::Point>,
    ) {
        let model = &self.ir.model;
        let vertex_points: BTreeSet<_> =
            model.vertices.iter().map(|vertex| &vertex.point).collect();
        let edge_curves: BTreeSet<_> = model.edges.iter().filter_map(|edge| edge.curve()).collect();
        let independent = |source: Option<&cadmpeg_ir::SourceObjectAssociation>| {
            source.is_none_or(|source| {
                source.geometry_role != Some(cadmpeg_ir::SourceGeometryRole::Support)
            })
        };
        let curves = model
            .curves
            .iter()
            .filter(|curve| {
                !edge_curves.contains(&curve.id) && independent(curve.source_object.as_ref())
            })
            .collect();
        let points = model
            .points
            .iter()
            .filter(|point| {
                !vertex_points.contains(&point.id) && independent(point.source_object.as_ref())
            })
            .collect();
        (curves, points)
    }

    /// Writes the free curves as one `NurbsCurves` prim and the free points as
    /// one `Points` prim.
    fn free_geometry(&mut self, names: &mut Names) {
        let (curves, points) = self.free_carriers();
        let mut written = Vec::new();
        for curve in curves {
            match curve_nurbs(
                curve,
                curve
                    .parameter_range
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints),
                None,
            ) {
                Some(nurbs) => written.push(nurbs),
                None => self.unwritten.free_carriers += 1,
            }
        }
        if !written.is_empty() {
            let name = names.unique("Curves");
            self.wire_prim(&written, None, &name, &Placement::default());
        }
        if !points.is_empty() {
            let name = names.unique("Points");
            self.geometry_head("Points", &name, &Placement::default(), None);
            let mut texts = Vec::new();
            let mut bounds = None;
            for point in points {
                let (text, values) = self.point_text(point.position().get());
                texts.push(text);
                bounds = Some(grow(bounds, values));
            }
            self.extent(bounds);
            self.out.array("point3f[] points", &texts, "");
            self.out.close();
        }
    }

    /// The abstract `Prototypes` prim with the shared definitions.
    fn prototypes(&mut self) {
        self.out.blank();
        self.out.line(&format!("class {}", quoted(PROTOTYPES)));
        self.out.name_next(PROTOTYPES);
        self.out.open();
        let shared: Vec<(ProductDefinitionId, String)> = self
            .shared
            .iter()
            .map(|(id, name)| (id.clone(), name.clone()))
            .collect();
        for (id, name) in shared {
            let Some(definition) = self
                .ir
                .model
                .product_definitions
                .iter()
                .find(|definition| definition.id == id)
            else {
                continue;
            };
            let bodies: Vec<&'ir Body> = definition
                .bodies
                .iter()
                .filter_map(|id| self.ir.model.bodies.iter().find(|body| &body.id == id))
                .collect();
            let mut names = Names::default();
            if let [body] = bodies.as_slice() {
                self.body(body, &mut names, &Placement::default(), Some(&name));
                continue;
            }
            self.out.line(&format!("def Xform {}", quoted(&name)));
            self.out.name_next(&name);
            self.out.open();
            for body in bodies {
                self.body(body, &mut names, &Placement::default(), None);
            }
            self.out.close();
        }
        self.out.close();
    }

    /// The losses of the write.
    fn losses(&self) -> Vec<LossNote> {
        let unwritten = &self.unwritten;
        let model = &self.ir.model;
        let mut losses = Vec::new();
        if unwritten.solver_properties > 0 {
            losses.push(UsdLossCode::ExportSolverPropertyNotWritten.note(format!(
                "{} solver properties have no PhysX attribute, prim, or relationship target to \
                 write and were not written",
                unwritten.solver_properties
            )));
        }
        if unwritten.joints > 0 {
            losses.push(UsdLossCode::ExportJointNotWritten.note(format!(
                "{} assembly joints have no UsdPhysics joint type, or connect a body that is \
                 not a written prim, and were not written",
                unwritten.joints
            )));
        }
        if unwritten.joint_properties > 0 {
            losses.push(UsdLossCode::ExportJointNotWritten.note(format!(
                "{} joint properties, such as the ratio of a rack and pinion stated only by \
                 its radii, have no attribute and were not written",
                unwritten.joint_properties
            )));
        }
        if unwritten.joint_drives > 0 {
            losses.push(UsdLossCode::ExportJointNotWritten.note(format!(
                "{} joint drives act on a degree of freedom that the joint's UsdPhysics type \
                 does not name and were not written",
                unwritten.joint_drives
            )));
        }
        if !unwritten.unmeshed.is_empty() {
            losses.push(UsdLossCode::ExportFaceNotWritten.note(format!(
                "{} faces of bodies with no tessellation do not mesh and were not written: {}",
                unwritten.unmeshed.len(),
                unwritten.unmeshed.join("; ")
            )));
        }
        if unwritten.tessellated_bodies > 0 {
            losses.push(UsdLossCode::ExportTopologyTessellated.note(format!(
                "{} bodies were written as meshes; their faces, edges, vertices, and carriers \
                 were not written",
                unwritten.tessellated_bodies
            )));
        }
        if unwritten.curves > 0 {
            losses.push(UsdLossCode::ExportCurveNotWritten.note(format!(
                "{} wire edges have no line or NURBS curve and were not written",
                unwritten.curves
            )));
        }
        let face_surfaces: BTreeSet<_> = model.faces.iter().map(|face| &face.surface).collect();
        let free_surfaces = model
            .surfaces
            .iter()
            .filter(|surface| !face_surfaces.contains(&surface.id))
            .count();
        if unwritten.free_carriers + free_surfaces > 0 {
            losses.push(UsdLossCode::ExportCarrierNotWritten.note(format!(
                "{} curves with no line or NURBS form and {free_surfaces} surfaces that no face \
                 uses were not written",
                unwritten.free_carriers
            )));
        }
        if unwritten.channels > 0 {
            losses.push(UsdLossCode::ExportChannelNotWritten.note(format!(
                "channels or texture assignments of {} tessellations were not written",
                unwritten.channels
            )));
        }
        if !unwritten.external_textures.is_empty() {
            losses.push(UsdLossCode::ExportTextureExternal.note(format!(
                "{} texture images are named by a path that the output does not hold: {}",
                unwritten.external_textures.len(),
                unwritten
                    .external_textures
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if !unwritten.reduced_appearances.is_empty() {
            losses.push(UsdLossCode::ExportAppearanceReduced.note(format!(
                "properties or textures of {} appearances have no input of a written shader and \
                 were not written",
                unwritten.reduced_appearances.len()
            )));
        }
        let unbound = model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                !matches!(
                    binding.target,
                    AppearanceTarget::Tessellation(_)
                        | AppearanceTarget::Body(_)
                        | AppearanceTarget::TriangleGroup { .. }
                )
            })
            .count();
        if unbound > 0 {
            losses.push(UsdLossCode::ExportAppearanceReduced.note(format!(
                "{unbound} appearance bindings to faces, edges, or carriers were not written"
            )));
        }
        if unwritten.rounded_points > 0 {
            losses.push(UsdLossCode::ExportVertexPrecision.note(format!(
                "{} points were written as binary32 coordinates, which round them",
                unwritten.rounded_points
            )));
        }
        losses
    }
}

/// The NURBS form of a curve carrier over `range`. A line needs its range
/// or its end points.
fn curve_nurbs(
    curve: &cadmpeg_ir::geometry::Curve,
    range: Option<[f64; 2]>,
    ends: Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Point3)>,
) -> Option<WireCurve> {
    let geometry = match &curve.geometry {
        CurveGeometry::Solved(geometry) => geometry,
        CurveGeometry::Procedural { cache, .. } => cache.as_ref()?,
    };
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let points: Vec<_> = nurbs
                .control_points()
                .iter()
                .map(|point| point.get())
                .collect();
            let weights = nurbs.weights().map_or_else(
                || points.iter().map(|_| 1.0).collect(),
                |weights| weights.iter().map(|weight| weight.get()).collect(),
            );
            let range = range.or_else(|| {
                cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            })?;
            Some(WireCurve {
                points,
                knots: nurbs.knots().as_slice().to_vec(),
                weights,
                degree: nurbs.degree(),
                range,
            })
        }
        SolvedCurveGeometry::Line(line) => {
            let (start, end) = match (ends, range) {
                (Some(ends), _) => ends,
                (None, Some([from, to])) => {
                    let origin = line.origin().get();
                    let direction = cadmpeg_ir::math::Vector3::from(line.direction());
                    let at = |t: f64| {
                        cadmpeg_ir::math::Point3::new(
                            origin.x + t * direction.x,
                            origin.y + t * direction.y,
                            origin.z + t * direction.z,
                        )
                    };
                    (at(from), at(to))
                }
                (None, None) => return None,
            };
            Some(WireCurve {
                points: vec![start, end],
                knots: vec![0.0, 0.0, 1.0, 1.0],
                weights: vec![1.0, 1.0],
                degree: 1,
                range: [0.0, 1.0],
            })
        }
        _ => None,
    }
}

/// Prefix of the metadata item that prepends applied API schemas.
const API_SCHEMAS_PREFIX: &str = "prepend apiSchemas = [";

/// `metadata` with its `prepend apiSchemas` items merged into one list, in
/// first-mention order.
fn merged_api_schemas(metadata: &[String]) -> Vec<String> {
    let mut schemas: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    let mut position = None;
    for item in metadata {
        match item
            .strip_prefix(API_SCHEMAS_PREFIX)
            .and_then(|rest| rest.strip_suffix(']'))
        {
            Some(list) => {
                position.get_or_insert(out.len());
                for schema in list.split(", ").filter(|schema| !schema.is_empty()) {
                    if !schemas.contains(&schema) {
                        schemas.push(schema);
                    }
                }
            }
            None => out.push(item.clone()),
        }
    }
    if let Some(position) = position {
        out.insert(
            position,
            format!("{API_SCHEMAS_PREFIX}{}]", schemas.join(", ")),
        );
    }
    out
}

/// `bounds` grown to hold `point`.
fn grow(bounds: Option<([f32; 3], [f32; 3])>, point: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let Some((mut min, mut max)) = bounds else {
        return (point, point);
    };
    // Strict comparisons keep the bound already held when a coordinate
    // equals it, so the sign of a zero bound does not depend on how the
    // platform's `min` and `max` order `0` and `-0`.
    for axis in 0..3 {
        if point[axis] < min[axis] {
            min[axis] = point[axis];
        }
        if point[axis] > max[axis] {
            max[axis] = point[axis];
        }
    }
    (min, max)
}

#[cfg(test)]
mod tests;
