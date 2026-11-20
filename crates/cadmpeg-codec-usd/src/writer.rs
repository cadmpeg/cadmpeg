// SPDX-License-Identifier: Apache-2.0
//! The USDA 1.0 writer.
//!
//! The layer is in metres, or in the units that the scene options select,
//! and turned to the up axis that they select. A document with one root occurrence that
//! holds child occurrences or several bodies, and nothing outside it, uses
//! that occurrence as the one root prim; otherwise each root occurrence and
//! each body no definition owns is a root prim. Each occurrence becomes an
//! `Xform` prim with a `matrix4d` transform. A definition that more than one
//! occurrence places is written once, with its bodies and the occurrences it
//! owns, under the abstract `Prototypes` prim, and each placement references
//! it. Bodies become geometry prims: tessellations as
//! `Mesh` prims; for a body with faces and no tessellation, each untrimmed
//! face on a NURBS surface as a `NurbsPatch` prim and the other faces as one
//! `Mesh` prim; and wire edges as one `NurbsCurves` prim. A placement whose content is one geometry prim is
//! written as that prim. Appearances become materials with a
//! `UsdPreviewSurface` shader and a `MaterialX` surface shader in a `Looks`
//! scope under the one root prim, or at the layer root. The scene options
//! can leave out the `BrepArray` prims and the `MaterialX` shaders.

mod brep;
mod brep_array;
mod physics;
mod physx;
mod records;
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
use cadmpeg_ir::products::{Occurrence, ProductDefinition, ProductDefinitionKind, Storage};
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
/// Millimetres per metre. Lengths are divided by the millimetres per scene
/// unit, the inverse of the reader's product, so that a second round trip
/// writes the same values.
pub(super) const MILLIMETRES_PER_METRE: f64 = 1000.0;
/// Name of the transform op that turns a root prim to the selected up axis.
const UP_AXIS_OP: &str = "xformOp:rotateX:upAxis";
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
    use ArenaDisposition::{Ignored, Omitted, Reported, Written};
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
        point_clouds: Omitted,
        history_bodies: Ignored,
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
        declared_measures: Omitted,
        design: cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted),
        process: cadmpeg_ir::codec::write::ProcessArenaDispositions::uniform(Omitted),
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
    let metres = match input.scene.meters_per_unit {
        None => 1.0,
        Some(metres) if metres.is_finite() && metres > 0.0 => metres,
        Some(metres) => {
            return Err(CodecError::InvalidInput(format!(
                "metres per unit must be a finite positive number, not {metres}"
            )))
        }
    };
    let mut writer = Writer::new(input.ir, version, input.scene, metres);
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
        notes: vec![format!("USDA {version} text layer, {}", units_note(metres))],
    };
    Ok((body, members))
}

/// The note text of a layer of `metres` metres per unit.
pub(crate) fn units_note(metres: f64) -> String {
    if metres == 1.0 {
        "metre units".to_owned()
    } else {
        format!("{} metres per unit", real(metres))
    }
}

/// `vector` turned by the up-axis turn `turn` (degrees about X): a quarter
/// turn about X.
fn turned(turn: Option<f64>, vector: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = vector;
    match turn {
        Some(angle) if angle < 0.0 => [x, z, 0.0 - y],
        Some(_) => [x, 0.0 - z, y],
        None => vector,
    }
}

/// Counts of records no prim carries.
#[derive(Default)]
struct Unwritten {
    tessellated_bodies: usize,
    /// Bodies with faces and no `BrepArray` prim, with the reason.
    breps: Vec<String>,
    /// Bodies whose `BrepArray` prim has no trim curves.
    untrimmed_breps: Vec<String>,
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
    /// Triangle groups that name the face whose triangles they hold. No
    /// `GeomSubset` property states the face.
    group_faces: usize,
}

/// The placement of a prim: its up-axis turn, transform, per-axis scale,
/// visibility, and prim metadata.
#[derive(Debug, Clone)]
struct Placement<'a> {
    /// The angle in degrees of a turn about the X axis, applied after the
    /// transform.
    turn: Option<f64>,
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
            turn: None,
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
    /// The exact B-rep of a body.
    Brep(Box<brep_array::BrepPrim>),
    Mesh(&'ir Tessellation),
    /// The mesh of faces of a body with no tessellation, and the triangles
    /// of each face.
    Meshed(Box<Tessellation>, Vec<brep::FaceTriangles>),
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

/// Whether `definition` is a part or an assembly document's definition.
fn is_part_definition(definition: &ProductDefinition) -> bool {
    matches!(
        definition.kind,
        Some(ProductDefinitionKind::Part | ProductDefinitionKind::Assembly)
    )
}

/// Whether `occurrence` places a part definition.
fn is_part(ir: &CadIr, occurrence: &Occurrence) -> bool {
    ir.model
        .product_definitions
        .iter()
        .any(|candidate| candidate.id == occurrence.prototype && is_part_definition(candidate))
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
    /// The identity index of the model.
    index: cadmpeg_ir::index::ModelIndex<'ir>,
    /// The arena position of each region, shell, face, and edge.
    positions: BTreeMap<&'ir str, usize>,
    /// The tessellations of each body, in arena order.
    body_tessellations: BTreeMap<&'ir BodyId, Vec<&'ir Tessellation>>,
    /// Definitions that place a part, directly or through other definitions.
    above_parts: BTreeSet<ProductDefinitionId>,
    /// The members a package holds beside the root layer, or `None` for a
    /// layer that no package holds.
    package: Option<Vec<Member>>,
    /// The source of the files that external assets name.
    assets: Option<&'ir dyn cadmpeg_core::decode::Companions>,
    /// Millimetres per scene unit.
    millimetres_per_unit: f64,
    /// Metres per scene unit.
    metres_per_unit: f64,
    /// The up axis of the layer.
    up_axis: &'static str,
    /// The turn in degrees about the X axis from the up axis of the model
    /// to that of the layer.
    turn: Option<f64>,
    /// Whether bodies with faces get a `BrepArray` prim.
    exact_brep: bool,
    /// Whether materials get a `MaterialX` shader.
    pub(super) materialx: bool,
}

impl<'ir> Writer<'ir> {
    fn new(
        ir: &'ir CadIr,
        version: &'static str,
        scene: cadmpeg_ir::codec::write::SceneOptions,
        metres: f64,
    ) -> Self {
        use cadmpeg_ir::codec::write::UpAxis;
        let millimetres_per_unit = metres * MILLIMETRES_PER_METRE;
        let model_axis = source_up_axis(ir);
        let up_axis = match scene.up_axis {
            None => model_axis,
            Some(UpAxis::Y) => "Y",
            Some(UpAxis::Z) => "Z",
        };
        let turn = match (model_axis, up_axis) {
            ("Z", "Y") => Some(-90.0),
            ("Y", "Z") => Some(90.0),
            _ => None,
        };
        let mut bindings = BTreeMap::new();
        for binding in &ir.model.appearance_bindings {
            let key = match &binding.target.entity {
                AppearanceTarget::Tessellation(id) => id.clone(),
                AppearanceTarget::Body(id) => id.to_string(),
                AppearanceTarget::Face(id) => id.to_string(),
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
            *uses.entry(&occurrence.prototype).or_default() += 1;
        }
        let mut names = Names::default();
        let shared = ir
            .model
            .product_definitions
            .iter()
            .filter(|definition| {
                uses.get(&definition.id).copied().unwrap_or_default() > 1
                    && (!definition.bodies.is_empty() || !definition.occurrences.is_empty())
            })
            .map(|definition| {
                let name = names.unique(definition.source_name.as_deref().unwrap_or("Prototype"));
                (definition.id.clone(), name)
            })
            .collect();
        // The document root's bodies are the layer's own.
        let owned = cadmpeg_ir::products::stated_definitions(&ir.model.product_definitions)
            .flat_map(|definition| definition.bodies.iter().cloned())
            .collect();
        // A definition is above parts when an occurrence it owns places a
        // part, or places a definition above parts.
        let mut above_parts = BTreeSet::new();
        loop {
            let before = above_parts.len();
            for definition in &ir.model.product_definitions {
                if above_parts.contains(&definition.id) {
                    continue;
                }
                let places_part = definition.occurrences.iter().any(|id| {
                    ir.model
                        .occurrences
                        .iter()
                        .find(|occurrence| &occurrence.id == id)
                        .is_some_and(|occurrence| {
                            is_part(ir, occurrence) || above_parts.contains(&occurrence.prototype)
                        })
                });
                if places_part {
                    above_parts.insert(definition.id.clone());
                }
            }
            if above_parts.len() == before {
                break;
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
            physics: physics::Plan::new(ir, millimetres_per_unit),
            index: cadmpeg_ir::index::ModelIndex::new_model_only(ir),
            positions: positions(ir),
            body_tessellations: {
                let mut out: BTreeMap<&'ir BodyId, Vec<&'ir Tessellation>> = BTreeMap::new();
                for tessellation in &ir.model.tessellations {
                    if let Some(body) = &tessellation.body {
                        out.entry(body).or_default().push(tessellation);
                    }
                }
                out
            },
            tessellator: cadmpeg_tessellate::Tessellator::new(
                ir,
                cadmpeg_tessellate::Options::default(),
            ),
            above_parts,
            package: None,
            assets: None,
            millimetres_per_unit,
            metres_per_unit: metres,
            up_axis,
            turn,
            exact_brep: scene.exact_brep,
            materialx: scene.materialx,
        }
    }

    /// The placement of a root prim: the turn to the layer's up axis.
    fn root_placement(&self) -> Placement<'static> {
        Placement {
            turn: self.turn,
            ..Placement::default()
        }
    }

    /// The model kind of the prim of `occurrence` (spec §4.4): `assembly`
    /// for a part above other parts, `component` for another part, and
    /// `group` for any other occurrence above a part.
    fn model_kind(&self, occurrence: &Occurrence) -> Option<&'static str> {
        let part = is_part(self.ir, occurrence);
        match (part, self.above_parts.contains(&occurrence.prototype)) {
            (true, true) => Some("assembly"),
            (true, false) => Some("component"),
            (false, true) => Some("group"),
            (false, false) => None,
        }
    }

    fn layer(&mut self) -> String {
        // A root definition the source names, or one that owns bodies beside
        // stating structure, is written as the root prim; an unnamed one is
        // the layer itself, and its occurrences are the root prims.
        let root_definition = self
            .ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.is_root());
        let root_prim = root_definition
            .filter(|definition| {
                !definition.is_document_root()
                    && (definition.source_name.is_some()
                        || definition.label.is_some()
                        || !definition.bodies.is_empty())
            })
            .and_then(|definition| {
                let id = cadmpeg_ir::ids::OccurrenceId::mint(format!(
                    "{}:root-prim",
                    definition.id.as_str()
                ))
                .ok()?;
                let mut occurrence = Occurrence::new(id, definition.id.clone(), None);
                occurrence.name = definition
                    .label
                    .clone()
                    .or_else(|| definition.source_name.clone());
                Some(occurrence)
            });
        let roots: Vec<&Occurrence> = match (&root_prim, root_definition) {
            (Some(root), _) => vec![root],
            (None, Some(definition)) => self.owned_occurrences(definition),
            (None, None) => Vec::new(),
        };
        let (free_curves, free_points) = self.free_carriers();
        let orphans = self
            .ir
            .geometry
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
                let root = self.root_placement();
                for body in &self.ir.geometry.bodies {
                    if !self.owned.contains(&body.id) {
                        self.body(body, &mut names, &root, None);
                    }
                }
                for tessellation in &self.ir.model.tessellations {
                    if tessellation.body.is_none() {
                        let name = names.unique("Mesh");
                        self.mesh(tessellation, &name, None, &root, &[]);
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
        header.line(&format!(
            "    metersPerUnit = {}",
            real(self.metres_per_unit)
        ));
        if physics || !self.physics.is_empty() {
            header.line("    kilogramsPerUnit = 1");
        }
        header.line(&format!("    upAxis = {}", quoted(self.up_axis)));
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
    fn occurrence(&mut self, occurrence: &Occurrence, names: &mut Names, root: Option<&str>) {
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
        let turn = self.turn.filter(|_| self.stack.len() == 1);
        let (schemas, properties) = self.physics.occurrence(&occurrence.id, turn);
        let mut placement = Placement {
            turn,
            transform: occurrence.transform.as_ref(),
            scale: [1.0; 3],
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
        let definition = self
            .ir
            .model
            .product_definitions
            .iter()
            .find(|candidate| candidate.id == occurrence.prototype);
        match definition.map(|definition| &definition.storage) {
            Some(Storage::External(document)) => {
                if let Some(path) = &document.path {
                    let target = document
                        .object
                        .as_ref()
                        .map(cadmpeg_core::text::NonBlankString::as_str)
                        .filter(|object| object.starts_with('/'))
                        .map(|object| format!("<{object}>"))
                        .unwrap_or_default();
                    placement.metadata.push(format!(
                        "prepend references = {}{target}",
                        asset(path.as_str())
                    ));
                }
            }
            Some(_) => {
                if let Some(prototype) = self.shared.get(&occurrence.prototype) {
                    placement
                        .metadata
                        .push(format!("prepend references = </{PROTOTYPES}/{prototype}>"));
                }
            }
            None => {}
        }
        let shared = definition.is_some_and(|definition| self.shared.contains_key(&definition.id));
        // A shared definition's occurrences are written in its prototype.
        let mut children: Vec<&'ir Occurrence> = if shared {
            Vec::new()
        } else {
            self.children(occurrence)
        };
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
        let bodies: Vec<&'ir Body> = definition
            .filter(|_| !shared)
            .map(|definition| {
                definition
                    .bodies
                    .iter()
                    .filter_map(|id| {
                        self.index
                            .bodies(id.as_str(), cadmpeg_ir::index::StandardIndex)
                    })
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
            placement
                .metadata
                .insert(0, "instanceable = true".to_owned());
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
        self.children(occurrence).is_empty()
            && !self.shared.contains_key(&occurrence.prototype)
            && self
                .ir
                .model
                .product_definitions
                .iter()
                .find(|candidate| candidate.id == occurrence.prototype)
                .is_some_and(|definition| {
                    !matches!(definition.storage, Storage::External(_))
                        && definition.bodies.len() == 1
                })
    }

    /// The occurrences `definition` owns, in its order.
    fn owned_occurrences(&self, definition: &ProductDefinition) -> Vec<&'ir Occurrence> {
        definition
            .occurrences
            .iter()
            .filter_map(|id| {
                self.ir
                    .model
                    .occurrences
                    .iter()
                    .find(|occurrence| &occurrence.id == id)
            })
            .collect()
    }

    /// The child occurrences of `occurrence`: those its prototype owns.
    fn children(&self, occurrence: &Occurrence) -> Vec<&'ir Occurrence> {
        self.ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.id == occurrence.prototype)
            .map(|definition| self.owned_occurrences(definition))
            .unwrap_or_default()
    }

    /// The reference list item that writes `child` on its parent prim:
    /// an external prototype placed at the identity, with no children of its
    /// own.
    fn arc_reference(&self, child: &Occurrence) -> Option<String> {
        let document = self
            .ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.id == child.prototype)
            .and_then(|definition| match &definition.storage {
                Storage::External(document) => Some(document),
                Storage::Embedded | Storage::File { .. } => None,
            })?;
        let path = document.path.as_ref()?;
        let placed = child.transform.is_some_and(|transform| {
            transform.affine_rows() != Transform::identity().affine_rows()
        }) || child.visible.is_some();
        if placed || !self.children(child).is_empty() {
            return None;
        }
        let target = document
            .object
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str)
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
        if let Some(angle) = placement.turn {
            self.out
                .line(&format!("double {UP_AXIS_OP} = {}", real(angle)));
            order.push(format!("\"{UP_AXIS_OP}\""));
        }
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
                        r0[3] / self.millimetres_per_unit,
                        r1[3] / self.millimetres_per_unit,
                        r2[3] / self.millimetres_per_unit,
                        1.0
                    ]),
                );
                self.out
                    .line(&format!("matrix4d xformOp:transform = {matrix}"));
                order.push("\"xformOp:transform\"".to_owned());
            }
        }
        if placement.scale != [1.0; 3] {
            self.out.line(&format!(
                "double3 xformOp:scale = {}",
                tuple(&placement.scale)
            ));
            order.push("\"xformOp:scale\"".to_owned());
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
        let body = self
            .index
            .bodies(id.as_str(), cadmpeg_ir::index::StandardIndex);
        text::identifier(body.and_then(|body| body.name.as_deref()).unwrap_or("Body"))
    }

    /// The tessellations of a body.
    fn tessellations(&self, body: &BodyId) -> Vec<&'ir Tessellation> {
        self.body_tessellations
            .get(body)
            .cloned()
            .unwrap_or_default()
    }

    /// The geometry prims of a body. Records no prim carries are counted
    /// here, once per written body.
    fn geometry(&mut self, body: &'ir Body) -> Vec<Geometry<'ir>> {
        let mut prims = Vec::new();
        let tessellations = self.tessellations(&body.id);
        let faces = self.body_faces(body);
        if !faces.is_empty() && self.exact_brep {
            if let Some(packed) = self.brep_packed(body) {
                prims.push(Geometry::Brep(packed));
            }
        }
        let exact = !prims.is_empty();
        if !tessellations.is_empty() {
            if !faces.is_empty() && !exact {
                self.unwritten.tessellated_bodies += 1;
            }
            prims.extend(tessellations.into_iter().map(Geometry::Mesh));
        } else if !faces.is_empty() {
            let mut meshed = Vec::new();
            for (face, surface) in faces {
                match surface.and_then(nurbs_surface) {
                    Some(nurbs) if face.loops.is_empty() || self.whole_domain(face, nurbs) => {
                        prims.push(Geometry::Patch(face, nurbs));
                    }
                    _ => meshed.push(&face.id),
                }
            }
            if !meshed.is_empty() {
                self.tessellator
                    .set_options(brep::mesh_options(&self.index, body));
                let (mesh, unmeshed) = brep::mesh(body, &meshed, &mut self.tessellator);
                self.unwritten.unmeshed.extend(unmeshed);
                if let Some((mesh, faces)) = mesh {
                    if !exact {
                        self.unwritten.tessellated_bodies += 1;
                    }
                    prims.push(Geometry::Meshed(Box::new(mesh), faces));
                }
            }
        }
        let wires = self.wire_curves(body);
        if !wires.is_empty() {
            prims.push(Geometry::Wires(wires));
        }
        prims
    }

    /// Whether the one loop of `face` runs along the boundary of the domain
    /// of its surface `nurbs`: each pcurve is a segment on one side.
    fn whole_domain(&self, face: &cadmpeg_ir::topology::Face, nurbs: &NurbsSurface) -> bool {
        let [loop_id] = face.loops.iter().collect::<Vec<_>>()[..] else {
            return false;
        };
        let Some(loop_) = self
            .index
            .loops(loop_id.as_str(), cadmpeg_ir::index::StandardIndex)
        else {
            return false;
        };
        let bounds = |knots: &[f64], degree: u32, count: usize| {
            let degree = usize::try_from(degree).ok()?;
            Some([*knots.get(degree)?, *knots.get(count)?])
        };
        let (Some(u), Some(v)) = (
            bounds(
                nurbs.u_knots().as_slice(),
                nurbs.u_degree(),
                nurbs.u_count(),
            ),
            bounds(
                nurbs.v_knots().as_slice(),
                nurbs.v_degree(),
                nurbs.v_count(),
            ),
        ) else {
            return false;
        };
        !loop_.coedges().is_empty()
            && loop_.coedges().iter().all(|coedge| {
                let Some(pcurve) = self
                    .index
                    .coedges(coedge.as_str(), cadmpeg_ir::index::StandardIndex)
                    .and_then(|coedge| coedge.pcurves.first())
                    .and_then(|use_| {
                        self.index
                            .pcurves(use_.pcurve.as_str(), cadmpeg_ir::index::StandardIndex)
                    })
                else {
                    return false;
                };
                let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs: pcurve } =
                    &pcurve.geometry
                else {
                    return false;
                };
                let points = pcurve.control_points();
                let [first, last] = [points.first(), points.last()];
                let (Some(first), Some(last)) = (first, last) else {
                    return false;
                };
                let (a, b) = (first.get(), last.get());
                let on = |value: f64, range: [f64; 2]| value == range[0] || value == range[1];
                points.len() == 2 && ((a.u == b.u && on(a.u, u)) || (a.v == b.v && on(a.v, v)))
            })
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
            let (schemas, properties) = self.physics.body(&body.id, placement.turn);
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
        let brep = prims
            .iter()
            .any(|prim| matches!(prim, Geometry::Brep(_)))
            .then(|| children.unique(brep_array::PRIM_NAME));
        for prim in prims {
            let base = match &prim {
                Geometry::Brep(_) => {
                    let name = brep.clone().unwrap_or_default();
                    self.geometry_prim(prim, &name, body, &inner, &collider);
                    continue;
                }
                Geometry::Mesh(_) | Geometry::Meshed(..) => "Mesh",
                Geometry::Patch(..) => "Patch",
                Geometry::Wires(_) => "Curves",
            };
            let mut placement = inner.clone();
            let fallback = !matches!(prim, Geometry::Wires(_));
            if let Some(brep) = brep.as_ref().filter(|_| fallback) {
                placement.metadata.push(format!(
                    "customData = {{ string {} = {} }}",
                    crate::brep::solid::FALLBACK_KEY,
                    quoted(brep)
                ));
            }
            let child = children.unique(base);
            self.geometry_prim(prim, &child, body, &placement, &collider);
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
            Geometry::Brep(brep) => self.brep_array_prim(&brep, name, body, placement),
            Geometry::Mesh(tessellation) => {
                let mut placement = placement.clone();
                placement.metadata.extend(collider.0.iter().cloned());
                placement.properties.extend(collider.1.iter().cloned());
                self.mesh(tessellation, name, Some(body), &placement, &[]);
            }
            Geometry::Meshed(tessellation, faces) => {
                let mut placement = placement.clone();
                placement.metadata.extend(collider.0.iter().cloned());
                placement.properties.extend(collider.1.iter().cloned());
                self.mesh(&tessellation, name, Some(body), &placement, &faces);
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
        let query = cadmpeg_ir::index::StandardIndex;
        let mut out = Vec::new();
        for region in self.ordered(self.index.ir().geometry.body_regions(&body.id), |region| {
            region.id.as_str()
        }) {
            for shell in self.ordered(
                region
                    .shells
                    .iter()
                    .filter_map(|shell| self.index.shells(shell.as_str(), query)),
                |shell| shell.id.as_str(),
            ) {
                for face in self.ordered(self.index.ir().geometry.shell_faces(&shell.id), |face| {
                    face.id.as_str()
                }) {
                    let surface = self.index.surfaces(face.surface.as_str(), query);
                    out.push((face, surface));
                }
            }
        }
        out
    }

    /// `records` in model arena order, without repeats. Each record is one
    /// of the arena, so each has a position.
    fn ordered<T>(
        &self,
        records: impl Iterator<Item = &'ir T>,
        id: impl Fn(&T) -> &str,
    ) -> Vec<&'ir T> {
        let mut out: Vec<(usize, &'ir T)> = records
            .filter_map(|record| Some((*self.positions.get(id(record))?, record)))
            .collect();
        out.sort_by_key(|(position, _)| *position);
        out.dedup_by_key(|(position, _)| *position);
        out.into_iter().map(|(_, record)| record).collect()
    }

    fn color_primvars(&mut self, body: Option<&Body>) {
        let Some(color) = body.and_then(|body| body.color) else {
            return;
        };
        self.out.line(&format!(
            "color3f[] primvars:displayColor = [{}] (",
            tuple32(&[color.r(), color.g(), color.b()].map(crate::text::value::round_f32))
        ));
        self.out.line("    interpolation = \"constant\"");
        self.out.line(")");
        if color.a() < 1.0 {
            self.out.line(&format!(
                "float[] primvars:displayOpacity = [{}] (",
                real32(crate::text::value::round_f32(color.a()))
            ));
            self.out.line("    interpolation = \"constant\"");
            self.out.line(")");
        }
    }

    fn point_text(&mut self, point: cadmpeg_ir::math::Point3) -> (String, [f32; 3]) {
        let scene = [point.x, point.y, point.z].map(|value| value / self.millimetres_per_unit);
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
        faces: &[brep::FaceTriangles],
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
        // The triangles of the faces that each material binds: the faces of a
        // mesh the writer made, and the faces that triangle groups name. A
        // group bound itself keeps its own material.
        let mut face_materials: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (face, triangles) in faces {
            if let Some(path) = self.binding_path(&face.to_string()) {
                face_materials
                    .entry(path)
                    .or_default()
                    .extend(triangles.clone());
            }
        }
        for (group, own) in tessellation.triangle_groups().iter().zip(&bound) {
            let Some(face) = group.face.as_ref().filter(|_| own.is_none()) else {
                continue;
            };
            if let Some(path) = self.binding_path(&face.to_string()) {
                face_materials.entry(path).or_default().extend(
                    group
                        .triangles
                        .iter()
                        .filter_map(|triangle| usize::try_from(*triangle).ok()),
                );
            }
        }
        for triangles in face_materials.values_mut() {
            triangles.sort_unstable();
            triangles.dedup();
        }
        if bound.iter().any(Option::is_some) || !face_materials.is_empty() {
            self.out
                .line("uniform token subsetFamily:materialBind:familyType = \"nonOverlapping\"");
        }
        let mut subsets = Names::default();
        for (group, binding) in tessellation.triangle_groups().iter().zip(bound) {
            if group.face.is_some() {
                self.unwritten.group_faces += 1;
            }
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
        for (path, triangles) in face_materials {
            let name = subsets.unique(path.rsplit('/').next().unwrap_or("Faces"));
            self.prim_head(
                "def GeomSubset",
                &name,
                &["prepend apiSchemas = [\"MaterialBindingAPI\"]".to_owned()],
            );
            self.out.open();
            self.out.line("uniform token elementType = \"face\"");
            self.out.line("uniform token familyName = \"materialBind\"");
            self.out.array(
                "int[] indices",
                &triangles
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "",
            );
            self.out.line(&format!("rel material:binding = <{path}>"));
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
        let binding = self
            .binding_path(&face.id.to_string())
            .or_else(|| self.binding_path(&body.id.to_string()));
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
        let query = cadmpeg_ir::index::StandardIndex;
        let index = &self.index;
        let mut edges = Vec::new();
        for region in self.ordered(index.ir().geometry.body_regions(&body.id), |region| {
            region.id.as_str()
        }) {
            for shell in self.ordered(
                region
                    .shells
                    .iter()
                    .filter_map(|shell| index.shells(shell.as_str(), query)),
                |shell| shell.id.as_str(),
            ) {
                edges.extend(
                    self.ordered(
                        shell
                            .wire_edges()
                            .filter_map(|edge| index.edges(edge.as_str(), query)),
                        |edge| edge.id.as_str(),
                    ),
                );
            }
        }
        let mut curves = Vec::new();
        for edge in edges {
            match self.edge_nurbs(edge) {
                Some(curve) => curves.push(curve),
                None => self.unwritten.curves += 1,
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
        let geometry = &self.ir.geometry;
        let curve = geometry
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())?;
        let position = |vertex: &cadmpeg_ir::ids::VertexId| {
            let vertex = self
                .index
                .vertices(vertex.as_str(), cadmpeg_ir::index::StandardIndex)?;
            self.index
                .points(vertex.point.as_str(), cadmpeg_ir::index::StandardIndex)
                .map(|point| point.position().get())
        };
        let ends = edge
            .start()
            .and_then(&position)
            .zip(edge.end().and_then(&position));
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
        let geometry = &self.ir.geometry;
        let vertex_points: BTreeSet<_> = geometry
            .vertices
            .iter()
            .map(|vertex| &vertex.point)
            .collect();
        let edge_curves: BTreeSet<_> = geometry
            .edges
            .iter()
            .filter_map(|edge| edge.curve())
            .collect();
        let independent = |source: Option<&cadmpeg_ir::SourceObjectAssociation>| {
            source.is_none_or(|source| {
                source.geometry_role != Some(cadmpeg_ir::SourceGeometryRole::Support)
            })
        };
        let curves = geometry
            .curves
            .iter()
            .filter(|curve| {
                !edge_curves.contains(&curve.id) && independent(curve.source_object.as_ref())
            })
            .collect();
        let points = geometry
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
            self.wire_prim(&written, None, &name, &self.root_placement());
        }
        if !points.is_empty() {
            let name = names.unique("Points");
            self.geometry_head("Points", &name, &self.root_placement(), None);
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
                .find(|candidate| candidate.id == id)
            else {
                continue;
            };
            let bodies: Vec<&'ir Body> = definition
                .bodies
                .iter()
                .filter_map(|id| {
                    self.index
                        .bodies(id.as_str(), cadmpeg_ir::index::StandardIndex)
                })
                .collect();
            let occurrences = self.owned_occurrences(definition);
            let mut names = Names::default();
            // A body named other than its prototype keeps its name on its
            // own prim inside the prototype prim.
            if let ([body], []) = (bodies.as_slice(), occurrences.as_slice()) {
                if body
                    .name
                    .as_deref()
                    .is_none_or(|own| text::identifier(own) == name)
                {
                    self.body(body, &mut names, &Placement::default(), Some(&name));
                    continue;
                }
            }
            self.out.line(&format!("def Xform {}", quoted(&name)));
            self.out.name_next(&name);
            self.out.open();
            self.stack.extend([PROTOTYPES.to_owned(), name.clone()]);
            for body in bodies {
                self.body(body, &mut names, &Placement::default(), None);
            }
            for occurrence in occurrences {
                self.occurrence(occurrence, &mut names, None);
            }
            self.stack.pop();
            self.stack.pop();
            self.out.close();
        }
        self.out.close();
    }

    /// The losses of the write.
    fn losses(&self) -> Vec<LossNote> {
        let unwritten = &self.unwritten;
        let model = &self.ir.model;
        let geometry = &self.ir.geometry;
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
        if !unwritten.breps.is_empty() {
            losses.push(UsdLossCode::ExportBrepNotWritten.note(format!(
                "{} bodies have no BrepArray prim: {}",
                unwritten.breps.len(),
                unwritten.breps.join("; ")
            )));
        }
        if !unwritten.untrimmed_breps.is_empty() {
            losses.push(UsdLossCode::ExportBrepUntrimmed.note(format!(
                "the BrepArray prims of {} bodies have no trim curves, because a pcurve has no \
                 exact image on its NURBS surface: {}",
                unwritten.untrimmed_breps.len(),
                unwritten.untrimmed_breps.join(", ")
            )));
        }
        if unwritten.curves > 0 {
            losses.push(UsdLossCode::ExportCurveNotWritten.note(format!(
                "{} wire edges have no line or NURBS curve and were not written",
                unwritten.curves
            )));
        }
        let face_surfaces: BTreeSet<_> = geometry.faces.iter().map(|face| &face.surface).collect();
        let free_surfaces = geometry
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
        if unwritten.group_faces > 0 {
            losses.push(UsdLossCode::ExportGroupFaceNotWritten.note(format!(
                "{} triangle group faces were not written",
                unwritten.group_faces
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
                    binding.target.entity,
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

/// The arena position of each region, shell, face, and edge of `ir`.
/// The up axis of the model: that of a USD source, or Z.
fn source_up_axis(ir: &CadIr) -> &'static str {
    let axis = ir
        .source
        .as_ref()
        .filter(|source| source.format() == crate::dialect::FORMAT)
        .and_then(|source| {
            source
                .attributes
                .iter()
                .find(|(key, _)| key.as_str() == SOURCE_UP_AXIS)
                .map(|(_, value)| value.as_str())
        });
    match axis {
        Some("Y") => "Y",
        Some("Z") => "Z",
        _ => DEFAULT_UP_AXIS,
    }
}

fn positions(ir: &CadIr) -> BTreeMap<&str, usize> {
    let geometry = &ir.geometry;
    let mut out = BTreeMap::new();
    for (position, region) in geometry.regions.iter().enumerate() {
        out.insert(region.id.as_str(), position);
    }
    for (position, shell) in geometry.shells.iter().enumerate() {
        out.insert(shell.id.as_str(), position);
    }
    for (position, face) in geometry.faces.iter().enumerate() {
        out.insert(face.id.as_str(), position);
    }
    for (position, edge) in geometry.edges.iter().enumerate() {
        out.insert(edge.id.as_str(), position);
    }
    out
}
