// SPDX-License-Identifier: Apache-2.0
//! Coordinate source streams, identity qualification and topology records.
//!
//! The graph builder walks each face bridge through its loop and coedge rings,
//! resolves edge and vertex uses, closes emitted loops, and groups faces under
//! explicit body records. It derives one body hierarchy when those records are
//! absent. It also derives supported pcurves and periodic seams.



use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::ProceduralSurface;
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::RegionId;
use cadmpeg_ir::ids::ShellId;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Coedge;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::Face;
use cadmpeg_ir::topology::Loop;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Region;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::topology::Shell;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::unknown::UnknownRecord;
use crate::brep::CurveCarrier;
use crate::brep::LEN_TO_MM;
use crate::brep::attrib;
use crate::brep::entity;
use crate::brep::graph::shells::ShellFaceIndex;
use crate::brep::index::scan::scan_stream_carriers;
use crate::brep::index::stream::StreamCarrierIndex;
use crate::brep::index::view::CarrierView;
use crate::brep::topology;
use crate::brep::typed;
use crate::parasolid::StreamHeader;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use self::emission::decode_graph;

mod shells;
mod cylindrical_radial;
mod construction;
mod emission;
mod pcurves;
mod inverse;
mod nurbs_pcurves;
mod clamp;
mod orientation;
mod seams;





const EPS_NORMAL_NONZERO: f64 = 1.0e-12;
const EPS_PLANAR_DISTANCE: f64 = 1.0e-6;
const EPS_GEOMETRY_RESIDUAL: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_CIRCLE_RADIUS_MATCH: f64 = 1.0e-6;
const EPS_RADIUS_ABSOLUTE: f64 = 1.0e-6;
const EPS_RADIUS_RELATIVE: f64 = 1.0e-9;
const EPS_NURBS_WEIGHT: f64 = 1.0e-12;
const EPS_POINT_DISTANCE: f64 = 1.0e-12;
const EPS_SWEEP_EXTENT_ABSOLUTE_MM: f64 = 1.0e-6;
const EPS_SWEEP_EXTENT_RELATIVE: f64 = 1.0e-3;

/// Decoded B-rep arenas, provenance, and transfer statistics.
#[derive(Default)]
pub(crate) struct Brep {
    /// Source locations for decoded entities.
    pub(crate) annotations: Annotations,
    /// Top-level solid or sheet bodies.
    pub(crate) bodies: Vec<Body>,
    /// Solid regions / sheet regions owned by each body.
    pub(crate) regions: Vec<Region>,
    /// Shells owned by each region.
    pub(crate) shells: Vec<Shell>,
    /// Faces reached through face-use bridge records.
    pub(crate) faces: Vec<Face>,
    /// Loops reached through `00 0f` loop heads.
    pub(crate) loops: Vec<Loop>,
    /// Coedges in loop-ring order.
    pub(crate) coedges: Vec<Coedge>,
    /// Edges resolved from edge-use records.
    pub(crate) edges: Vec<Edge>,
    /// Vertices resolved from vertex-use and world-point records.
    pub(crate) vertices: Vec<Vertex>,
    /// World points converted to millimetres.
    pub(crate) points: Vec<Point>,
    /// Analytic, NURBS, or opaque support surfaces.
    pub(crate) surfaces: Vec<Surface>,
    /// Exact procedural constructions behind emitted support surfaces.
    pub(crate) procedural_surfaces: Vec<ProceduralSurface>,
    /// Analytic, NURBS, or opaque support curves.
    pub(crate) curves: Vec<Curve>,
    /// Pcurves derived for supported analytic and NURBS-boundary cases.
    pub(crate) pcurves: Vec<Pcurve>,
    /// Records whose carrier kind this codec does not type, retained as
    /// opaque payloads.
    pub(crate) unknowns: Vec<UnknownRecord>,
    /// Per-face RGB colors resolved from native entity records.
    pub(crate) face_colors: Vec<OwnedFaceColor>,
    /// Per-face producing-feature identities resolved from Parasolid attributes.
    pub(crate) face_atoms: Vec<attrib::FaceAtom>,
    /// Source-local sequence-to-attribute links carried by face bridge
    /// records. The decode boundary retains this map only for the active
    /// source because SWIFT identifiers resolve in that source namespace.
    pub(crate) face_bridge_sequences: Vec<(u32, u16)>,
    /// Source-local sequence-to-attribute links carried by edge-use records.
    /// The decode boundary retains this map only for the active source because
    /// SWIFT identifiers resolve in that source namespace.
    pub(crate) edge_use_sequences: Vec<(u32, u16)>,
    /// Source-local sequence-to-attribute links carried by vertex-use records.
    /// The decode boundary retains this map only for the active source because
    /// SWIFT identifiers resolve in that source namespace.
    pub(crate) vertex_use_sequences: Vec<(u32, u16)>,
    /// Body-to-history ordinals resolved from Parasolid attributes.
    pub(crate) body_modifiers: Vec<attrib::BodyModifier>,
    /// Refusals charged while building this B-rep, each naming the instance
    /// whose lane the reader refused. The model carries the absence of each
    /// refused carrier, so the decode continues and reports the loss.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Loss accounting for this decode.
    pub(crate) stats: Stats,
}

/// A resolved face color retains the stream that admitted its source row.
///
/// Alternate configuration sites are merged into one B-rep, so the selected
/// site cannot serve as a provenance owner for every color row.
#[derive(Debug, Clone)]
pub(crate) struct OwnedFaceColor {
    pub(crate) value: entity::FaceColor,
    pub(crate) source_stream: cadmpeg_ir::StreamName,
    /// Site qualifier applied when this color came from an unselected site.
    /// A color attribute is only site-local, so an unbound color still needs
    /// this qualifier after alternate sites are merged.
    pub(crate) site_key: Option<String>,
}

impl Brep {
    pub(crate) fn neutral_entity_count(&self) -> Result<u64, cadmpeg_core::CodecError> {
        let counts = [
            self.bodies.len(),
            self.regions.len(),
            self.shells.len(),
            self.faces.len(),
            self.loops.len(),
            self.coedges.len(),
            self.edges.len(),
            self.vertices.len(),
            self.points.len(),
            self.surfaces.len(),
            self.procedural_surfaces.len(),
            self.curves.len(),
            self.pcurves.len(),
        ];
        let total = counts.into_iter().try_fold(0_usize, |total, count| {
            total.checked_add(count).ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(
                    "SLDPRT B-rep entity count exceeds usize".into(),
                )
            })
        })?;
        u64::try_from(total).map_err(|_| {
            cadmpeg_core::CodecError::NotImplemented("SLDPRT B-rep entity count exceeds u64".into())
        })
    }

    /// Qualify every document-arena identity and internal reference by one site key.
    pub(crate) fn qualify_ids(
        &mut self,
        ctx: &DecodeContext<'_>,
        site: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        // The site qualifier is admitted once, here, as a key tail. The tail
        // grammar scan reads every byte of the formatted text.
        let (tail_text, _tail_reservation) =
            ctx.format_scoped(format_args!("@{site}"), "qualify SLDPRT site")?;
        ctx.charge_work(
            u64_from_index(tail_text.len()),
            "admit SLDPRT site qualifier",
        )?;
        let tail = cadmpeg_ir::ids::IdentityKeyTail::try_new(tail_text).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "SLDPRT site qualifier is not identity key text: {error}"
            ))
        })?;
        let qualify = |value: &str| qualified_reference(ctx, value, site);
        for body in ctx.admit_iter(&mut self.bodies, "qualify SLDPRT bodies")? {
            body.id = qualified(ctx, &body.id, &tail)?;
            for id in ctx.admit_iter(&mut body.regions, "qualify SLDPRT body regions")? {
                *id = qualified(ctx, id, &tail)?;
            }
        }
        for region in ctx.admit_iter(&mut self.regions, "qualify SLDPRT regions")? {
            region.id = qualified(ctx, &region.id, &tail)?;
            region.body = qualified(ctx, &region.body, &tail)?;
            for id in ctx.admit_iter(&mut region.shells, "qualify SLDPRT region shells")? {
                *id = qualified(ctx, id, &tail)?;
            }
        }
        for shell in ctx.admit_iter(&mut self.shells, "qualify SLDPRT shells")? {
            let id = qualified(ctx, &shell.id, &tail)?;
            let region = qualified(ctx, &shell.region, &tail)?;
            let faces = qualified_ids(ctx, shell.faces(), &tail)?;
            let wire_edges = qualified_ids(ctx, shell.wire_edges(), &tail)?;
            let free_vertices = qualified_ids(ctx, shell.free_vertices(), &tail)?;
            *shell = Shell::new(id, region, faces, wire_edges, free_vertices).map_err(|error| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "qualified shell topology is invalid: {error}"
                ))
            })?;
        }
        for face in ctx.admit_iter(&mut self.faces, "qualify SLDPRT faces")? {
            face.id = qualified(ctx, &face.id, &tail)?;
            face.shell = qualified(ctx, &face.shell, &tail)?;
            face.surface = qualified(ctx, &face.surface, &tail)?;
            face.loops = match &face.loops {
                cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => {
                    cadmpeg_ir::topology::FaceLoops::unspecified(qualified_ids(ctx, loops, &tail)?)
                }
                cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                    cadmpeg_ir::topology::FaceLoops::classified(
                        qualified(ctx, outer, &tail)?,
                        qualified_ids(ctx, inner, &tail)?,
                    )
                }
            };
        }
        for loop_ in ctx.admit_iter(&mut self.loops, "qualify SLDPRT loops")? {
            loop_.id = qualified(ctx, &loop_.id, &tail)?;
            loop_.face = qualified(ctx, &loop_.face, &tail)?;
            match &mut loop_.boundary {
                cadmpeg_ir::topology::LoopBoundary::Vertex { vertex, pcurves } => {
                    *vertex = qualified(ctx, vertex, &tail)?;
                    for pcurve in ctx.admit_iter(pcurves, "qualify SLDPRT vertex loop pcurves")? {
                        pcurve.pcurve = qualified(ctx, &pcurve.pcurve, &tail)?;
                    }
                }
                cadmpeg_ir::topology::LoopBoundary::Ring(ring) => {
                    let coedges = qualified_ids(ctx, ring.coedges(), &tail)?;
                    let vertex_uses = ctx.try_collect_vec(
                        ring.vertex_uses().iter().map(|vertex_use| {
                            Ok::<_, cadmpeg_core::CodecError>(
                                cadmpeg_ir::topology::AnchoredVertexUse {
                                    vertex: qualified(ctx, &vertex_use.vertex, &tail)?,
                                    after: qualified(ctx, &vertex_use.after, &tail)?,
                                    pcurves: qualified_pcurve_uses(
                                        ctx,
                                        &vertex_use.pcurves,
                                        &tail,
                                    )?,
                                },
                            )
                        }),
                        "collect qualified SLDPRT vertex uses",
                    )?;
                    *ring = cadmpeg_ir::topology::LoopRing::new(ctx, coedges, vertex_uses)
                        .map_err(cadmpeg_core::CodecError::from)?
                        .map_err(|error| {
                            cadmpeg_core::CodecError::malformed(format_args!(
                                "qualified loop ring is invalid: {error}"
                            ))
                        })?;
                }
            }
        }
        for coedge in ctx.admit_iter(&mut self.coedges, "qualify SLDPRT coedges")? {
            coedge.id = qualified(ctx, &coedge.id, &tail)?;
            coedge.owner_loop = qualified(ctx, &coedge.owner_loop, &tail)?;
            coedge.edge = qualified(ctx, &coedge.edge, &tail)?;
            coedge.radial_next = qualified(ctx, &coedge.radial_next, &tail)?;
            for use_ in ctx.admit_iter(&mut coedge.pcurves, "qualify SLDPRT coedge pcurves")? {
                use_.pcurve = qualified(ctx, &use_.pcurve, &tail)?;
            }
        }
        for edge in ctx.admit_iter(&mut self.edges, "qualify SLDPRT edges")? {
            edge.id = qualified(ctx, &edge.id, &tail)?;
            if let Some(curve) = edge.curve() {
                let curve = qualified(ctx, curve, &tail)?;
                edge.map_curve(|_| curve);
            }
            edge.start = qualified(ctx, &edge.start, &tail)?;
            edge.end = qualified(ctx, &edge.end, &tail)?;
        }
        for vertex in ctx.admit_iter(&mut self.vertices, "qualify SLDPRT vertices")? {
            vertex.id = qualified(ctx, &vertex.id, &tail)?;
            vertex.point = qualified(ctx, &vertex.point, &tail)?;
        }
        for point in ctx.admit_iter(&mut self.points, "qualify SLDPRT points")? {
            point.id = qualified(ctx, &point.id, &tail)?;
        }
        for surface in ctx.admit_iter(&mut self.surfaces, "qualify SLDPRT surfaces")? {
            surface.id = qualified(ctx, &surface.id, &tail)?;
            match &mut surface.geometry {
                SurfaceGeometry::Procedural { construction, .. } => {
                    *construction = qualified(ctx, construction, &tail)?;
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: Some(record),
                }) => {
                    *record = qualified(ctx, record, &tail)?;
                }
                SurfaceGeometry::Solved(_) => {}
            }
        }
        for procedural in ctx.admit_iter(
            &mut self.procedural_surfaces,
            "qualify SLDPRT procedural surfaces",
        )? {
            procedural.id = qualified(ctx, &procedural.id, &tail)?;
            procedural.edit_definition(|definition| {
                match definition {
                    ProceduralSurfaceDefinition::Blend(definition_payload) => {
                        let [first, second] =
                            definition_payload.supports().each_ref().map(|support| {
                                support
                                    .as_ref()
                                    .map(|support| {
                                        qualified(ctx, &support.surface, &tail).map(|surface| {
                                            cadmpeg_ir::geometry::BlendSupport {
                                                surface,
                                                reversed: support.reversed,
                                            }
                                        })
                                    })
                                    .transpose()
                            });
                        let spine = definition_payload
                            .spine()
                            .as_ref()
                            .map(|spine| qualified(ctx, spine, &tail))
                            .transpose()?;
                        definition_payload.set_supports([first?, second?]);
                        definition_payload.set_spine(spine);
                    }
                    ProceduralSurfaceDefinition::Offset(definition_payload) => {
                        definition_payload.set_support(qualified(
                            ctx,
                            definition_payload.support(),
                            &tail,
                        )?);
                    }
                    _ => {}
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })?;
        }
        for curve in ctx.admit_iter(&mut self.curves, "qualify SLDPRT curves")? {
            curve.id = qualified(ctx, &curve.id, &tail)?;
            if let CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(record),
            }) = &mut curve.geometry
            {
                *record = qualified(ctx, record, &tail)?;
            }
        }
        for pcurve in ctx.admit_iter(&mut self.pcurves, "qualify SLDPRT pcurves")? {
            pcurve.id = qualified(ctx, &pcurve.id, &tail)?;
        }
        for record in ctx.admit_iter(&mut self.unknowns, "qualify SLDPRT unknown records")? {
            let id = qualified(ctx, record.id(), &tail)?;
            record.set_id(id);
            for link in ctx.admit_iter(record.links_mut(), "qualify SLDPRT unknown record links")? {
                *link = qualify(link)?;
            }
        }
        for color in ctx.admit_iter(&mut self.face_colors, "qualify SLDPRT face colors")? {
            if let Some(target) = &mut color.value.target {
                *target = qualify(target)?;
            }
            color.site_key = Some(ctx.copy_retained_text(site, "copy SLDPRT face color site")?);
        }
        for atom in ctx.admit_iter(&mut self.face_atoms, "qualify SLDPRT face atoms")? {
            atom.face = qualified(ctx, &atom.face, &tail)?;
        }
        for modifier in ctx.admit_iter(&mut self.body_modifiers, "qualify SLDPRT body modifiers")? {
            if let Some(target) = &mut modifier.target {
                *target = qualify(target)?;
            }
        }
        self.annotations
            .map_ids(ctx, qualify, "qualify SLDPRT annotation identities")?
            .map_err(cadmpeg_core::CodecError::from)?;

        Ok(())
    }
}

/// Emit the edge-connected components of one native shell into `shells`.
///
/// The first component keeps the native identity and `native_source`; each
/// later component is a derived `.component-<n>` split. Returns the emitted
/// shell identities in component order.
fn emit_shell_components<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    index: &mut ShellFaceIndex<'_, 'ctx, '_>,
    native_shell_id: &ShellId,
    region_id: &RegionId,
    native_source: Option<(usize, &'static str)>,
    annotate: &mut impl FnMut(&str, Option<(usize, &str)>) -> Result<(), cadmpeg_core::CodecError>,
    shells: &mut Vec<Shell>,
) -> Result<Vec<ShellId>, cadmpeg_core::CodecError> {
    const COPY: &str = "SLDPRT decoded identity copy";
    let mut emitted = Vec::new();
    let components = index.take_components(native_shell_id.as_str())?;
    components.visit(|component, members| {
        let shell_id = if component == 0 {
            native_shell_id.try_clone_for_decode(ctx, COPY)?
        } else {
            shell_component(ctx, native_shell_id, component)?
        };
        annotate(shell_id.as_str(), native_source.filter(|_| component == 0))?;
        let faces = index.faces();
        let face_ids = ctx.try_collect_vec(
            members
                .iter()
                .map(|&face| faces[face].id.try_clone_for_decode(ctx, COPY)),
            "collect Parasolid shell faces",
        )?;
        for &face in ctx.admit_iter(members, "assign Parasolid face shells")? {
            index.assign(face, &shell_id)?;
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(shells, 1, "collect Parasolid shells")?;
        shells.push(
            match Shell::new(
                shell_id.try_clone_for_decode(ctx, COPY)?,
                region_id.try_clone_for_decode(ctx, COPY)?,
                face_ids,
                Vec::new(),
                Vec::new(),
            ) {
                Ok(shell) => shell,
                Err(_) => return Ok(()),
            },
        );
        ctx.push_vec(&mut emitted, shell_id, "collect Parasolid region shells")?;
        Ok(())
    })?;
    Ok(emitted)
}

#[derive(Debug, Clone)]
struct BodyRecord {
    attr: u16,
    kind: BodyKind,
    refs: Vec<u16>,
    offset: usize,
    regions: Vec<RegionRecord>,
}

#[derive(Debug, Clone)]
struct RegionRecord {
    attr: u16,
    offset: usize,
    shells: Vec<ShellRecord>,
}

#[derive(Debug, Clone)]
struct ShellRecord {
    attr: u16,
    offset: usize,
    refs: Vec<u16>,
}

/// Transfer limitations found while building a [`Brep`].
#[derive(Default)]
pub(crate) struct Stats {
    /// Framed top-level model entity records across the selected stream site.
    pub(crate) source_entity_records: usize,
    /// Face-color bindings withheld because current records conflict.
    pub(crate) unresolved_face_colors: usize,
    /// Face owners with multiple non-equivalent bridge uses.
    pub(crate) ambiguous_face_owners: usize,
    /// Canonical faces that no explicit body record claims.
    pub(crate) unclaimed_faces: usize,
    /// Faces on a support surface this codec does not type; emitted with an
    /// unknown-geometry carrier.
    pub(crate) unknown_surface_faces: usize,
    /// Hidden procedural support surfaces whose carrier geometry remains opaque.
    pub(crate) unknown_procedural_supports: usize,
    /// Edges whose support curve is an untyped carrier (emitted with no curve).
    pub(crate) unknown_curve_edges: usize,
    /// Pcurves withheld because geometric inverse selection was ambiguous.
    pub(crate) ambiguous_pcurve_parameters: usize,
    /// NURBS edge carriers whose vertex range is off their bound surface.
    pub(crate) off_surface_nurbs_pcurves: usize,
    /// No explicit body record was available, so one body hierarchy was derived.
    pub(crate) synthetic_body_grouping: bool,
}

/// The `sldprt:brep` shell namespace, which three routes mint under.
fn shell_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "shell")
}

/// The `sldprt:brep` region namespace.
fn region_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "region")
}

/// The `sldprt:brep` body namespace.
fn body_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "body")
}

/// The `sldprt:brep` pcurve namespace.
fn pcurve_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "brep", "pcurve")
}

/// The component split of one native shell: `<shell>.component-<ordinal>`.
fn shell_component(
    ctx: &DecodeContext<'_>,
    shell: &ShellId,
    component: usize,
) -> Result<ShellId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(
        format_args!("{shell}.component-{component}"),
        "SLDPRT shell component identity",
    )?;
    ShellId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

/// Append the site qualifier to one typed identity's key.
///
/// The admitted tail preserves the key grammar. The caller context charges
/// the resulting retained text before the append reserves capacity.
fn qualified<T>(
    ctx: &DecodeContext<'_>,
    id: &T,
    site: &cadmpeg_ir::ids::IdentityKeyTail,
) -> Result<T, cadmpeg_core::CodecError>
where
    T: std::fmt::Display + From<cadmpeg_ir::ids::Identity>,
{
    let text = ctx.format_retained(
        format_args!("{id}{}", site.as_str()),
        "qualify SLDPRT identity",
    )?;
    // The identity grammar scan reads every byte of the qualified text.
    ctx.charge_work(
        u64_from_index(text.len()),
        "admit qualified SLDPRT identity",
    )?;
    let identity =
        cadmpeg_ir::ids::Identity::new(text).map_err(cadmpeg_core::CodecError::malformed)?;
    Ok(T::from(identity))
}

fn qualified_ids<T>(
    ctx: &DecodeContext<'_>,
    ids: &[T],
    site: &cadmpeg_ir::ids::IdentityKeyTail,
) -> Result<Vec<T>, cadmpeg_core::CodecError>
where
    T: std::fmt::Display + From<cadmpeg_ir::ids::Identity>,
{
    ctx.try_collect_vec(
        ids.iter().map(|id| qualified(ctx, id, site)),
        "collect qualified SLDPRT identities",
    )
}

fn qualified_pcurve_uses(
    ctx: &DecodeContext<'_>,
    uses: &[cadmpeg_ir::topology::PcurveUse],
    site: &cadmpeg_ir::ids::IdentityKeyTail,
) -> Result<Vec<cadmpeg_ir::topology::PcurveUse>, cadmpeg_core::CodecError> {
    ctx.try_collect_vec(
        uses.iter().map(|use_| {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::topology::PcurveUse {
                pcurve: qualified(ctx, &use_.pcurve, site)?,
                isoparametric: use_.isoparametric,
                parameter_range: use_.parameter_range,
            })
        }),
        "collect qualified SLDPRT pcurve uses",
    )
}

fn qualified_reference(
    ctx: &DecodeContext<'_>,
    value: &str,
    site: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    if ctx.contains_bytes(
        value.as_bytes(),
        b"#",
        "find SLDPRT reference key separator",
    )? {
        ctx.format_retained(format_args!("{value}@{site}"), "qualify SLDPRT reference")
    } else {
        ctx.copy_retained_text(value, "qualify SLDPRT reference")
    }
}

fn id_face(a: u16) -> FaceId {
    FaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "face"),
        a,
    )
}
fn id_surf(a: u16) -> SurfaceId {
    SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "surf"),
        a,
    )
}
fn id_loop(a: u16) -> LoopId {
    LoopId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "loop"),
        a,
    )
}
fn id_coedge(a: u16) -> CoedgeId {
    CoedgeId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "coedge"),
        a,
    )
}
fn id_edge(a: u16) -> EdgeId {
    EdgeId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "edge"),
        a,
    )
}
fn id_curve(a: u16) -> CurveId {
    CurveId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
        a,
    )
}
fn id_vertex(a: u16) -> VertexId {
    VertexId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "vertex"),
        a,
    )
}
fn id_point(a: u16) -> PointId {
    PointId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "point"),
        a,
    )
}
fn id_closed_point(edge: u16) -> PointId {
    PointId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "point"),
        cadmpeg_ir::identity_key!("closed-circle-").then(edge),
    )
}
fn id_closed_vertex(edge: u16) -> VertexId {
    VertexId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "vertex"),
        cadmpeg_ir::identity_key!("closed-circle-").then(edge),
    )
}

/// One face-use's decoded loops: ordered coedge rings, keyed by loop attr.
#[derive(Clone, PartialEq, Eq)]
struct WalkedFace {
    bridge_attr: u16,
    surface_attr: u16,
    sense: Sense,
    /// `(loop_attr, ordered_coedge_attrs)` in sibling order.
    loops: Vec<(u16, Vec<u16>)>,
}

/// Follow the sibling loop-head chain of a bridge and each loop's coedge ring,
/// returning the ordered structure with cycles guarded.
fn walk_face(
    ctx: &DecodeContext<'_>,
    bridge: &topology::Bridge,
    t: &topology::Tables,
) -> Result<WalkedFace, cadmpeg_core::CodecError> {
    let surface_attr = bridge.refs[4];
    let mut loops = Vec::new();
    let mut loop_ref = bridge.refs[2];
    let mut loop_guard = HashSet::new();
    while loop_ref != 0 {
        ctx.charge_work(1, "walk Parasolid face loops")?;
        if !ctx.insert_hash_set(&mut loop_guard, loop_ref, "walk Parasolid face loops")? {
            break;
        }
        let Some(lp) = ctx.get_btree_map(t.loops(), &loop_ref, "lookup Parasolid loops")? else {
            break;
        };
        let owner_bridge = lp.refs[2];
        let same_face_use = owner_bridge == bridge.attr
            || match bridge.owner {
                Some(owner) => {
                    ctx.get_btree_map(t.bridges(), &owner_bridge, "lookup Parasolid bridges")?
                        .and_then(|candidate| candidate.owner)
                        == Some(owner)
                }
                None => false,
            };
        if !same_face_use {
            break;
        }
        let first = lp.refs[1];
        let mut ring = Vec::new();
        let mut ce_ref = first;
        let mut ce_guard = HashSet::new();
        let mut ring_closed = false;
        while ce_ref != 0 {
            ctx.charge_work(1, "walk Parasolid face coedges")?;
            if !ctx.insert_hash_set(&mut ce_guard, ce_ref, "walk Parasolid face coedges")? {
                break;
            }
            let Some(ce) = ctx.get_btree_map(t.coedges(), &ce_ref, "lookup Parasolid coedges")?
            else {
                break;
            };
            if ce.refs[1] != loop_ref {
                break;
            }
            ctx.reserve_vec(&mut ring, 1, "walk Parasolid face coedges")?;
            ring.push(ce_ref);
            ce_ref = ce.refs[3];
            if ce_ref == first {
                ring_closed = true;
                break;
            }
        }
        if ring_closed {
            ctx.reserve_vec(&mut loops, 1, "walk Parasolid face loops")?;
            loops.push((loop_ref, ring));
        }
        loop_ref = lp.refs[3];
    }
    Ok(WalkedFace {
        bridge_attr: bridge.attr,
        surface_attr,
        sense: bridge.sense,
        loops,
    })
}

fn edge_parameter_range(
    ctx: &DecodeContext<'_>,
    carrier: &CurveCarrier,
    endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
) -> Result<Option<([f64; 2], bool)>, cadmpeg_core::CodecError> {
    const TOLERANCE_MM: f64 = 1.0e-7;

    let Some(range) = carrier.parameter_range else {
        return Ok(None);
    };
    let range = range.get();
    let range = match &carrier.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => {
            range.map(|parameter| parameter * LEN_TO_MM)
        }
        _ => range,
    };
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let Some(endpoints) = endpoints else {
        return Ok(Some((range, false)));
    };
    let geometry = &carrier.geometry;
    let first = crate::brep::evaluation::curve_point(ctx, geometry, range[0])?;
    let second = crate::brep::evaluation::curve_point(ctx, geometry, range[1])?;
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    let distance = |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
        (left.x - right.x)
            .hypot(left.y - right.y)
            .hypot(left.z - right.z)
    };
    if distance(first.get(), endpoints[0]).max(distance(second.get(), endpoints[1])) <= TOLERANCE_MM
    {
        Ok(Some((range, false)))
    } else if distance(first.get(), endpoints[1]).max(distance(second.get(), endpoints[0]))
        <= TOLERANCE_MM
    {
        Ok(Some((range, true)))
    } else {
        Ok(None)
    }
}

/// Resolve the coedge that defines an edge's stored direction.
///
/// Bare records carry an explicit coedge attr in `refs[0]`. Prefixed records
/// carry no such slot. An absent or source-null slot is resolved only when exactly
/// one same-edge forward coedge exists. A non-sentinel explicit reference is
/// authoritative: a dangling, cross-edge, or reversed reference is rejected.
fn canonical_coedge_attr(
    ctx: &DecodeContext<'_>,
    edge_attr: u16,
    edge_use: Option<&topology::EdgeUse>,
    coedges: &BTreeMap<u16, topology::Coedge>,
    forward_coedges: &BTreeMap<u16, Option<u16>>,
) -> Result<Option<u16>, cadmpeg_core::CodecError> {
    if let Some(explicit) = edge_use
        .and_then(|record| record.references.canonical())
        .filter(|attr| *attr > 1)
    {
        let Some(coedge) = ctx.get_btree_map(coedges, &explicit, "lookup Parasolid coedges")?
        else {
            return Ok(None);
        };
        return Ok(
            (coedge.refs[6] == edge_attr && coedge.sense == Sense::Forward).then_some(explicit),
        );
    }
    Ok(ctx
        .get_btree_map(
            forward_coedges,
            &edge_attr,
            "lookup Parasolid forward coedges",
        )?
        .copied()
        .flatten())
}

/// The one forward coedge of each edge attr, or `None` for an edge that has
/// several forward coedges.
fn forward_coedges_by_edge(
    ctx: &DecodeContext<'_>,
    coedges: &BTreeMap<u16, topology::Coedge>,
) -> Result<BTreeMap<u16, Option<u16>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "index Parasolid forward coedges";
    let mut forward = BTreeMap::new();
    for (&attr, coedge) in ctx.admit_iter(coedges, OPERATION)? {
        if coedge.sense != Sense::Forward {
            continue;
        }
        match ctx.entry_btree_map(&mut forward, coedge.refs[6], OPERATION)? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(attr));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    Ok(forward)
}

fn edge_end_vuse(
    ctx: &DecodeContext<'_>,
    canonical: u16,
    ring_end: u16,
    coedges: &BTreeMap<u16, topology::Coedge>,
) -> Result<u16, cadmpeg_core::CodecError> {
    let Some(twin) = ctx
        .get_btree_map(coedges, &canonical, "lookup Parasolid coedges")?
        .map(|coedge| coedge.refs[5])
        .filter(|twin| *twin != canonical)
    else {
        return Ok(ring_end);
    };
    let Some(twin_record) = ctx.get_btree_map(coedges, &twin, "lookup Parasolid coedges")? else {
        return Ok(ring_end);
    };
    if twin_record.refs[5] != canonical {
        return Ok(ring_end);
    }
    Ok(twin_record.refs[4])
}

fn surface_sense(sense: Sense, orientation_reversed: bool) -> Sense {
    match (sense, orientation_reversed) {
        (Sense::Forward, true) => Sense::Reversed,
        (Sense::Reversed, true) => Sense::Forward,
        (sense, false) => sense,
    }
}

/// The body bytes a Parasolid stream header names.
///
/// # Errors
///
/// Refuses a body offset past the payload, naming the offset and the payload
/// length: a header that states its body outside its own stream is a framing
/// error, not an empty body.
fn header_body<'a>(
    payload: &'a [u8],
    header: &StreamHeader,
) -> Result<&'a [u8], cadmpeg_core::CodecError> {
    payload.get(header.body_offset..).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format!(
            "sldprt Parasolid stream header states body offset {} past its {}-byte payload",
            header.body_offset,
            payload.len()
        ))
    })
}

/// Decode one parsed Parasolid stream into B-rep arenas.
///
/// `stream` names the provenance stream recorded in [`Brep::annotations`].
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    header: &StreamHeader,
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    decode_body(ctx, header_body(payload, header)?, stream)
}

fn is_deltas_stream(
    ctx: &DecodeContext<'_>,
    description: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.any_by(
        description.as_bytes().windows(6),
        |window| Ok(window.eq_ignore_ascii_case(b"deltas")),
        "scan Parasolid deltas stream description",
    )
}

fn selected_typed_face_offsets(
    ctx: &DecodeContext<'_>,
    facts: &typed::Facts,
    attrs: Option<&BTreeSet<u16>>,
) -> Result<BTreeSet<usize>, cadmpeg_core::CodecError> {
    let mut offsets = BTreeSet::new();
    if let Some(attrs) = attrs {
        for face in ctx.admit_iter(&facts.faces, "select typed Parasolid face offsets")? {
            if ctx.contains_btree_set(attrs, &face.attr, "select typed Parasolid face offsets")? {
                ctx.insert_btree_set(
                    &mut offsets,
                    face.offset,
                    "index typed Parasolid face offsets",
                )?;
            }
        }
    }
    Ok(offsets)
}

/// Decode related partition and deltas streams as one record source.
///
/// Partition records are the base set. Deltas records fill missing subordinate
/// records and point updates, but do not replace a same-identity partition
/// topology or carrier record. `stream` names the combined provenance source.
pub(crate) fn decode_bodies(
    ctx: &DecodeContext<'_>,
    bodies: &[(&[u8], &StreamHeader)],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "hold Parasolid source indexes")?;
    let mut carriers = StreamCarrierIndex::empty(ctx)?;
    let mut tables = topology::Tables::default();
    let mut facts = entity::FactsSource::empty(ctx)?;
    let mut typed_facts = typed::Facts::default();
    let mut initialized = false;
    enum ScannedTables<'ctx> {
        Primary(topology::Tables),
        MergeSource(topology::MergeSource<'ctx>),
    }
    let mut ordered_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
    let mut ordered = Vec::new();
    ordered_storage.with_storage(|| {
        ctx.reserve_capacity(&mut ordered, bodies.len(), "order Parasolid body streams")
    })?;
    for &(payload, header) in ctx.admit_iter(bodies, "scan Parasolid body stream ordering keys")? {
        ordered_storage.with_storage(|| {
            ctx.push_vec(
                &mut (ordered),
                (payload, header, is_deltas_stream(ctx, &header.description)?),
                "order Parasolid body streams",
            )
        })?;
    }
    ctx.stable_sort_by(
        &mut ordered,
        |value| &value.2,
        bool::cmp,
        "sort Parasolid body streams",
    )?;
    let mut entity_streams_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
    let mut entity_streams = Vec::new();
    entity_streams_storage.with_storage(|| {
        ctx.reserve_capacity(
            &mut entity_streams,
            ordered.len(),
            "index Parasolid body streams",
        )
    })?;
    for &(payload, header, is_deltas) in
        ctx.admit_iter(&ordered, "scan ordered Parasolid body streams")?
    {
        let body = header_body(payload, header)?;
        entity_streams_storage.with_storage(|| {
            ctx.push_vec(
                &mut (entity_streams),
                (body, is_deltas),
                "index Parasolid body streams",
            )
        })?;
    }
    let mut typed_streams_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
    let mut typed_streams = Vec::new();
    typed_streams_storage.with_storage(|| {
        ctx.reserve_capacity(
            &mut typed_streams,
            entity_streams.len(),
            "index typed Parasolid streams",
        )
    })?;
    for (body, _) in ctx.admit_iter(&entity_streams, "scan SLDPRT decode_bodies values")? {
        typed_streams_storage.with_storage(|| {
            let stream_facts = typed::scan(body, ctx)?;
            ctx.push_vec(
                &mut (typed_streams),
                stream_facts,
                "index typed Parasolid streams",
            )
        })?;
    }
    drop((entity_streams, entity_streams_storage));
    for stream_typed_facts in ctx.admit_iter(&typed_streams, "scan SLDPRT decode_bodies values")? {
        index_storage
            .with_storage(|| typed_facts.merge_missing(ctx, stream_typed_facts))?;
    }
    let typed_bridge_attrs =
        index_storage.with_storage(|| typed_facts.valid_ownership_face_attrs(ctx))?;
    let selected_bridge_attrs = typed_bridge_attrs.as_ref();
    for (stream_order, ((payload, header, is_deltas), stream_typed_facts)) in ctx
        .admit_iter(&ordered, "scan Parasolid topology body streams")?
        .copied()
        .zip(ctx.admit_iter(&typed_streams, "scan Parasolid typed stream facts")?)
        .enumerate()
    {
        let body = header_body(payload, header)?;
        let scanned_tables = {
            let mut stream_storage = ctx.reserve_scoped(0, "hold Parasolid stream selectors")?;
            let typed_face_offsets = stream_storage.with_storage(|| {
                selected_typed_face_offsets(ctx, stream_typed_facts, typed_bridge_attrs.as_ref())
            })?;
            carriers.merge_missing(ctx, scan_stream_carriers(ctx, body)?)?;
            let curve_attrs = stream_storage.with_storage(|| CarrierView::Stream(&carriers).curve_attrs(ctx))?;
            let scan_tables = || {
                if is_deltas {
                    topology::scan_deltas_with_curve_attrs_excluding(
                        ctx,
                        body,
                        &curve_attrs,
                        &typed_face_offsets,
                    )
                } else {
                    topology::scan_with_curve_attrs_excluding(
                        ctx,
                        body,
                        &curve_attrs,
                        &typed_face_offsets,
                    )
                }
            };
            if initialized {
                ScannedTables::MergeSource(topology::MergeSource::scan(ctx, scan_tables)?)
            } else {
                ScannedTables::Primary(index_storage.with_storage(scan_tables)?)
            }
        };
        let mut scanned_facts = entity::scan_metadata(ctx, body, is_deltas)?;
        for color in ctx.admit_iter(
            &mut scanned_facts.face_colors,
            "order Parasolid face colors by stream",
        )? {
            color.stream_order = stream_order;
        }
        for version in ctx.admit_iter(
            &mut scanned_facts.face_color_versions,
            "order Parasolid face color versions by stream",
        )? {
            version.stream_order = stream_order;
        }
        match scanned_tables {
            ScannedTables::Primary(scanned_tables) => {
                tables = scanned_tables;
                facts = scanned_facts;
                initialized = true;
            }
            ScannedTables::MergeSource(source) => {
                index_storage.with_storage(|| {
                    source.merge_into(ctx, &mut tables, selected_bridge_attrs)
                })?;
                facts.append(ctx, scanned_facts)?;
            }
        }
    }
    drop((typed_streams, typed_streams_storage, ordered, ordered_storage));
    decode_graph(ctx, &CarrierView::Stream(&carriers), &tables, facts, &typed_facts, stream)
}

fn decode_body(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "hold Parasolid source indexes")?;
    let carriers = scan_stream_carriers(ctx, body)?;
    let curve_attrs = index_storage.with_storage(|| CarrierView::Stream(&carriers).curve_attrs(ctx))?;
    let typed_facts = index_storage.with_storage(|| typed::scan(body, ctx))?;
    let typed_face_attrs =
        index_storage.with_storage(|| typed_facts.valid_ownership_face_attrs(ctx))?;
    let typed_face_offsets = index_storage.with_storage(|| {
        selected_typed_face_offsets(ctx, &typed_facts, typed_face_attrs.as_ref())
    })?;
    let t = index_storage.with_storage(|| {
        topology::scan_with_curve_attrs_excluding(ctx, body, &curve_attrs, &typed_face_offsets)
    })?;
    let entity_facts = entity::scan_metadata(ctx, body, false)?;
    decode_graph(ctx, &CarrierView::Stream(&carriers), &t, entity_facts, &typed_facts, stream)
}

fn admit_brep_entity(ctx: &DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_entities(1, "admit SLDPRT B-rep entity")?;

    Ok(())
}

/// One modifier per body attr, in body attr order. A body whose modifiers
/// disagree on the history ordinal keeps none.
fn unique_body_modifiers(
    ctx: &DecodeContext<'_>,
    modifiers: Vec<attrib::BodyModifier>,
) -> Result<Vec<attrib::BodyModifier>, cadmpeg_core::CodecError> {
    let (by_attr, _by_attr_storage) =
        ctx.with_scoped_storage("index Parasolid body modifiers", || {
            let mut by_attr = BTreeMap::<u16, Option<attrib::BodyModifier>>::new();
            for modifier in ctx.admit_iter(modifiers, "select Parasolid body modifiers")? {
                match ctx.entry_btree_map(
                    &mut by_attr,
                    modifier.body_attr,
                    "index Parasolid body modifiers",
                )? {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Some(modifier));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if entry.get().as_ref().is_some_and(|previous| {
                            previous.history_ordinal != modifier.history_ordinal
                        }) {
                            *entry.get_mut() = None;
                        }
                    }
                }
            }
            Ok::<_, cadmpeg_core::CodecError>(by_attr)
        })?;
    ctx.collect_vec(
        ctx.admit_iter(by_attr, "collect Parasolid body modifiers")?
            .filter_map(|(_, modifier)| modifier),
        "collect Parasolid body modifiers",
    )
}

fn unique_face_colors(
    ctx: &DecodeContext<'_>,
    colors: Vec<entity::FaceColor>,
    versions: Vec<entity::FaceColorVersion>,
) -> Result<(Vec<entity::FaceColor>, usize), cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "select Parasolid face colors")?;
    let current_versions = scratch.with_storage(|| {
        let mut current_versions = BTreeMap::<u16, (u32, usize)>::new();
        for version in ctx.admit_iter(versions, "select current Parasolid face colors")? {
            let stamp = (version.seq, version.stream_order);
            match ctx.entry_btree_map(
                &mut current_versions,
                version.face_attr,
                "index Parasolid face color versions",
            )? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(stamp);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let current = entry.get_mut();
                    *current = (*current).max(stamp);
                }
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(current_versions)
    })?;

    let by_face = scratch.with_storage(|| {
        let mut by_face = BTreeMap::<u16, Vec<entity::FaceColor>>::new();
        for color in ctx.admit_iter(colors, "select Parasolid face colors")? {
            if ctx.get_btree_map(
                &current_versions,
                &color.face_attr,
                "lookup Parasolid current_versions",
            )? == Some(&(color.face_seq, color.stream_order))
            {
                ctx.push_btree_group(
                    &mut by_face,
                    color.face_attr,
                    color,
                    "index Parasolid face colors",
                    "collect Parasolid face color candidates",
                )?;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(by_face)
    })?;

    let mut unresolved = 0;
    let by_color = scratch.with_storage(|| {
        let mut by_color = BTreeMap::<u16, Vec<entity::FaceColor>>::new();
        for (_, mut candidates) in ctx.admit_iter(by_face, "select Parasolid face colors")? {
            let first = &candidates[0];
            if ctx.all_by(
                &candidates,
                |candidate| {
                    Ok(candidate.color_attr == first.color_attr && candidate.color == first.color)
                },
                "compare Parasolid face color candidates",
            )? {
                let color = candidates.swap_remove(0);
                ctx.push_btree_group(
                    &mut by_color,
                    color.color_attr,
                    color,
                    "index Parasolid color identities",
                    "collect Parasolid color identity candidates",
                )?;
            } else {
                unresolved += 1;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(by_color)
    })?;

    let mut out = Vec::new();
    for (_, mut candidates) in ctx.admit_iter(by_color, "resolve Parasolid color identities")? {
        let first = &candidates[0];
        if ctx.all_by(
            &candidates,
            |candidate| Ok(candidate.color == first.color),
            "compare Parasolid color identity candidates",
        )? {
            ctx.append_vec(
                &mut out,
                &mut candidates,
                "collect resolved Parasolid face colors",
            )?;
        } else {
            unresolved += candidates.len();
        }
    }
    ctx.stable_sort_by_key(
        &mut out,
        |value| (value.face_attr, value.offset),
        Ord::cmp,
        "sort Parasolid face colors",
    )?;
    Ok((out, unresolved))
}

fn typed_body_records(
    ctx: &DecodeContext<'_>,
    facts: &typed::Facts,
    tables: &topology::Tables,
) -> Result<Option<Vec<BodyRecord>>, cadmpeg_core::CodecError> {
    let (bridge_attrs, _bridge_attrs_storage) =
        ctx.with_scoped_storage("index Parasolid body bridges", || {
            ctx.collect_btree_set(
                tables.bridges().keys().copied(),
                "index Parasolid body bridges",
            )
        })?;
    let Some(hierarchies) = facts.hierarchies(ctx, &bridge_attrs)? else {
        return Ok(None);
    };
    let mut records = Vec::new();
    for hierarchy in ctx.admit_iter(&hierarchies, "assemble typed Parasolid bodies")? {
        let mut body_refs = ctx.collect_vec(
            hierarchy
                .regions
                .iter()
                .map(|region| region.attr)
                .chain(hierarchy.shells.iter().map(|shell| shell.attr))
                .chain(hierarchy.faces.iter().map(|(face, _)| *face)),
            "collect typed Parasolid body references",
        )?;
        ctx.sort_unstable_by(
            &mut body_refs,
            |value| value,
            Ord::cmp,
            "sort typed Parasolid body references",
        )?;
        ctx.dedup_vec(
            &mut body_refs,
            "deduplicate typed Parasolid body references",
        )?;
        // Shells by owning region and faces by owning shell, indexed once for
        // the region and shell walk below.
        let mut scratch = ctx.reserve_scoped(0, "index typed Parasolid body hierarchy")?;
        let shells_by_region = scratch.with_storage(|| {
            let mut shells_by_region = BTreeMap::<u16, Vec<&typed::ShellNode>>::new();
            for shell in ctx.admit_iter(&hierarchy.shells, "index Parasolid region shells")? {
                if let Ok(region) = u16::try_from(shell.refs[6]) {
                    ctx.push_btree_group(
                        &mut shells_by_region,
                        region,
                        shell,
                        "index Parasolid region shells",
                        "collect Parasolid region shells",
                    )?;
                }
            }
            Ok::<_, cadmpeg_core::CodecError>(shells_by_region)
        })?;
        let faces_by_shell = scratch.with_storage(|| {
            let mut faces_by_shell = BTreeMap::<u16, Vec<u16>>::new();
            for &(face_attr, shell_attr) in
                ctx.admit_iter(&hierarchy.faces, "index Parasolid shell faces")?
            {
                ctx.push_btree_group(
                    &mut faces_by_shell,
                    shell_attr,
                    face_attr,
                    "index Parasolid shell faces",
                    "collect Parasolid shell faces",
                )?;
            }
            Ok::<_, cadmpeg_core::CodecError>(faces_by_shell)
        })?;
        let mut regions = Vec::new();
        for region in ctx.admit_iter(&hierarchy.regions, "assemble typed Parasolid regions")? {
            let mut shells = Vec::new();
            let region_shells = ctx
                .get_btree_map(
                    &shells_by_region,
                    &region.attr,
                    "lookup Parasolid shells_by_region",
                )?
                .map_or(&[][..], Vec::as_slice);
            for shell in ctx.admit_iter(region_shells, "assemble typed Parasolid shells")? {
                let mut refs = match ctx.get_btree_map(
                    &faces_by_shell,
                    &shell.attr,
                    "lookup Parasolid faces_by_shell",
                )? {
                    Some(faces) => ctx.copy_slice(faces, "collect typed Parasolid shell faces")?,
                    None => Vec::new(),
                };
                ctx.sort_unstable_by(
                    &mut refs,
                    |value| value,
                    Ord::cmp,
                    "sort typed Parasolid shell faces",
                )?;
                ctx.dedup_vec(&mut refs, "deduplicate typed Parasolid shell faces")?;
                ctx.push_vec(
                    &mut shells,
                    ShellRecord {
                        attr: shell.attr,
                        offset: shell.offset,
                        refs,
                    },
                    "collect typed Parasolid region shells",
                )?;
            }
            ctx.stable_sort_by(
                &mut shells,
                |value| &value.attr,
                Ord::cmp,
                "sort Parasolid topology",
            )?;
            ctx.push_vec(
                &mut regions,
                RegionRecord {
                    attr: region.attr,
                    offset: region.offset,
                    shells,
                },
                "collect typed Parasolid body regions",
            )?;
        }
        ctx.stable_sort_by(
            &mut regions,
            |value| &value.attr,
            Ord::cmp,
            "sort Parasolid topology",
        )?;
        ctx.push_vec(
            &mut records,
            BodyRecord {
                attr: hierarchy.body.attr,
                kind: hierarchy.body.kind,
                refs: body_refs,
                offset: hierarchy.body.offset,
                regions,
            },
            "collect typed Parasolid body records",
        )?;
    }
    ctx.stable_sort_by(
        &mut records,
        |value| &value.attr,
        Ord::cmp,
        "sort Parasolid topology",
    )?;
    Ok((!records.is_empty()).then_some(records))
}

fn sorted_topology_sequences(
    ctx: &DecodeContext<'_>,
    pairs: impl IntoIterator<Item = (u32, u16)>,
    operation: &'static str,
) -> Result<Vec<(u32, u16)>, cadmpeg_core::CodecError> {
    let mut sequences = ctx.collect_vec(pairs, operation)?;
    ctx.sort_unstable_by(&mut sequences, |value| value, Ord::cmp, operation)?;
    ctx.dedup_vec(&mut sequences, operation)?;
    Ok(sequences)
}

fn copy_graph_stream_name(
    ctx: &DecodeContext<'_>,
    stream: &cadmpeg_ir::StreamName,
) -> Result<cadmpeg_ir::StreamName, cadmpeg_core::CodecError> {
    cadmpeg_ir::StreamName::try_from(
        ctx.copy_retained_text(stream.as_str(), "copy Parasolid graph stream name")?,
    )
    .map_err(|_| cadmpeg_core::CodecError::malformed("empty Parasolid graph stream name"))
}

/// The body group and the shell attribute of each face bridge.
type BridgeBindings = (BTreeMap<u16, usize>, BTreeMap<u16, u16>);

/// Bind each walked face to a body record and a shell.
///
/// A face belongs to the last body record whose references name its bridge or
/// the bridge's owner. Its shell is the first shell, in region and shell order,
/// that names either of them within the last such record that has one; a later
/// record that names the face but none of whose shells do keeps the earlier
/// shell.
fn bind_bridges(
    ctx: &DecodeContext<'_>,
    t: &topology::Tables,
    body_records: &[BodyRecord],
    faces: &[WalkedFace],
) -> Result<BridgeBindings, cadmpeg_core::CodecError> {
    const INDEX: &str = "index Parasolid body references";
    const BIND: &str = "bind Parasolid face bridges";
    // Attr -> body groups whose references name it, in group order.
    let mut groups_by_ref = BTreeMap::<u16, Vec<usize>>::new();
    // (attr, group) -> (ordinal, shell attr) of the group's first shell that
    // names the attr.
    let mut shell_by_ref = BTreeMap::<(u16, usize), (usize, u16)>::new();
    for (group, record) in ctx.admit_iter(body_records, INDEX)?.enumerate() {
        for &attr in ctx.admit_iter(&record.refs, INDEX)? {
            ctx.push_btree_group(&mut groups_by_ref, attr, group, INDEX, INDEX)?;
        }
        let mut ordinal = 0_usize;
        for region in ctx.admit_iter(&record.regions, INDEX)? {
            for shell in ctx.admit_iter(&region.shells, INDEX)? {
                for &attr in ctx.admit_iter(&shell.refs, INDEX)? {
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        ctx.entry_btree_map(&mut shell_by_ref, (attr, group), INDEX)?
                    {
                        slot.insert((ordinal, shell.attr));
                    }
                }
                ordinal += 1;
            }
        }
    }

    let mut bridge_group = BTreeMap::new();
    let mut bridge_shell = BTreeMap::new();
    for face in ctx.admit_iter(faces, BIND)? {
        let owner = ctx
            .get_btree_map(t.bridges(), &face.bridge_attr, "lookup Parasolid bridges")?
            .and_then(|r| r.owner);
        let groups_naming = |attr: Option<u16>| -> Result<&[usize], cadmpeg_core::CodecError> {
            Ok(match attr {
                Some(attr) => ctx
                    .get_btree_map(&groups_by_ref, &attr, BIND)?
                    .map_or(&[][..], Vec::as_slice),
                None => &[],
            })
        };
        let bridge_groups = groups_naming(Some(face.bridge_attr))?;
        let owner_groups = groups_naming(owner)?;
        let Some(group) = bridge_groups.last().max(owner_groups.last()).copied() else {
            continue;
        };
        ctx.insert_btree_map(&mut bridge_group, face.bridge_attr, group, BIND)?;
        // Matching groups from the last one back; the first that selects a
        // shell decides.
        let mut bridge_groups = bridge_groups.iter().rev().copied().peekable();
        let mut owner_groups = owner_groups.iter().rev().copied().peekable();
        loop {
            ctx.charge_work(1, BIND)?;
            let next = match (bridge_groups.peek(), owner_groups.peek()) {
                (None, None) => break,
                (Some(&bridge), None) => bridge,
                (None, Some(&owner)) => owner,
                (Some(&bridge), Some(&owner)) => bridge.max(owner),
            };
            if bridge_groups.peek() == Some(&next) {
                bridge_groups.next();
            }
            if owner_groups.peek() == Some(&next) {
                owner_groups.next();
            }
            let mut selected = None;
            for attr in [Some(face.bridge_attr), owner].into_iter().flatten() {
                if let Some(&candidate) = ctx.get_btree_map(&shell_by_ref, &(attr, next), BIND)? {
                    selected = Some(
                        selected
                            .map_or(candidate, |previous: (usize, u16)| previous.min(candidate)),
                    );
                }
            }
            if let Some((_, shell)) = selected {
                ctx.insert_btree_map(&mut bridge_shell, face.bridge_attr, shell, BIND)?;
                break;
            }
        }
    }
    Ok((bridge_group, bridge_shell))
}


#[cfg(test)]
mod tests;

#[cfg(test)]
mod numerical_range_tests;

#[cfg(test)]
mod evaluation_admission_tests;

#[cfg(test)]
mod inverse_candidate_tests;

#[cfg(test)]
mod sample_parameter_tests;

#[cfg(test)]
mod homogeneous_generation_tests;

#[cfg(test)]
mod coordinate_scale_tests;
