// SPDX-License-Identifier: Apache-2.0
//! The picked-entity rules on the fixture cube's current state: the fixture's
//! extrude takes each operation in turn, its picks resolved to cube entities.

use std::collections::{BTreeMap, BTreeSet};

use super::super::fixture::{self, design_findings};
use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::names::{NameKind, TopoName};
use crate::design::ops::body::Graft;
use crate::design::ops::dressup::{
    BlendControls, Chamfer, ChamferSet, ChamferSize, Corner, EdgeSetback, Setbacks,
};
use crate::design::ops::sheet_metal::{
    BendSet, BendSpec, Convert, ConvertBend, ConvertFrom, Flange, FlangeWidth, SheetMetal,
    SheetMetalKind, StampForm, StampOutline,
};
use crate::design::ops::surface::Unsew;
use crate::design::refs::EntityRef;
use crate::design::selection::{Anchor, Members, PickTarget, Picked, Picks, SetSelection};
use crate::design::selection::{MemberOrder, Selection};
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::CylinderSurface;
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::ids::VertexId;
use crate::math::{Point3, Vector3};
use crate::scalar::{Angle, NonNegativeLength, PositiveLength};
use crate::topology::BodyKind;

/// A pick of `entity`, told apart from the other picks by `key`.
fn pick(key: u32, entity: EntityRef) -> Picked {
    Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind: NameKind::Face,
            feature: fixture::feature("extrude"),
            role: None,
            generator: None,
            index: Some(key),
        }),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: Some(vec![entity]),
    }
}

/// A selection in the current state of `entities`.
fn current<O: MemberOrder>(entities: Vec<EntityRef>) -> Selection<O> {
    let picks = entities
        .into_iter()
        .enumerate()
        .map(|(key, entity)| pick(u32::try_from(key).unwrap(), entity))
        .collect();
    Selection {
        at: Anchor::Current,
        picks: Picks::Named(Members::new(picks).unwrap()),
        native: None,
    }
}

fn face(ir: &CadIr, index: usize) -> EntityRef {
    EntityRef::Face(ir.geometry.faces[index].id.clone())
}

fn edge(ir: &CadIr, index: usize) -> EntityRef {
    EntityRef::Edge(ir.geometry.edges[index].id.clone())
}

fn body(ir: &CadIr) -> EntityRef {
    EntityRef::Body(ir.geometry.bodies[0].id.clone())
}

fn point(x: f64, y: f64, z: f64) -> Construction<kinds::Point> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: FinitePoint3::new(Point3::new(x, y, z)),
        resolved_in: BTreeMap::new(),
    }
}

fn positive(value: f64) -> Dimension<PositiveLength> {
    Dimension::Literal(PositiveLength::new(value).unwrap())
}

fn nonnegative(value: f64) -> Dimension<NonNegativeLength> {
    Dimension::Literal(NonNegativeLength::new(value).unwrap())
}

fn sheet(kind: SheetMetalKind) -> FeatureOperation {
    FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

/// Gives the fixture's extrude the operation `make` builds from the fixture.
fn with(ir: &mut CadIr, make: impl FnOnce(&CadIr) -> FeatureOperation) {
    let operation = make(ir);
    ir.design.features[0].definition.operation = operation;
}

/// The height of a vertex.
fn height(ir: &CadIr, vertex: &VertexId) -> f64 {
    let point = &ir
        .geometry
        .vertices
        .iter()
        .find(|record| record.id == *vertex)
        .unwrap()
        .point;
    let point = ir.geometry.points.iter().find(|record| record.id == *point);
    point.unwrap().position().get().z
}

/// The edges of the cube's bottom face: those with both ends at height 0.
fn bottom_edges(ir: &CadIr) -> Vec<usize> {
    let edges = ir.geometry.edges.iter().enumerate();
    edges
        .filter(|(_, edge)| edge.vertices().all(|end| height(ir, end) == 0.0))
        .map(|(index, _)| index)
        .collect()
}

/// The cube's bottom face: the face that uses the bottom edges.
fn bottom(ir: &CadIr) -> usize {
    let edges: BTreeSet<_> = bottom_edges(ir)
        .into_iter()
        .map(|index| &ir.geometry.edges[index].id)
        .collect();
    let record = ir
        .geometry
        .loops
        .iter()
        .find(|record| {
            ir.geometry
                .coedges
                .iter()
                .filter(|coedge| coedge.owner_loop == record.id)
                .all(|coedge| edges.contains(&coedge.edge))
        })
        .unwrap();
    let mut faces = ir.geometry.faces.iter();
    faces.position(|face| face.id == record.face).unwrap()
}

/// Makes the bottom face's surface the cylinder of radius 5 about the
/// vertical axis through the cube's middle.
fn curve_bottom(ir: &mut CadIr) {
    let surface = ir.geometry.faces[bottom(ir)].surface.clone();
    let cylinder = CylinderSurface::try_new(
        Point3::new(5.0, 5.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .unwrap();
    let record = ir
        .geometry
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap();
    record.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder));
}

fn set_kind(ir: &mut CadIr, kind: BodyKind) {
    ir.geometry.bodies[0].kind = kind;
}

/// The messages of the findings of `rule`.
fn ruled(ir: &CadIr, rule: &str) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.rule.as_str() == rule)
        .map(|finding| finding.message)
        .collect()
}

/// The vertex edge 0 starts at, and the indices of the edges that end at it.
fn corner(ir: &CadIr) -> (EntityRef, Vec<usize>) {
    let vertex = ir.geometry.edges[0].start().unwrap().clone();
    let edges = ir
        .geometry
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.vertices().any(|end| *end == vertex))
        .map(|(index, _)| index)
        .collect();
    (EntityRef::Vertex(vertex), edges)
}

/// An edge that touches no end of edge 0.
fn apart(ir: &CadIr) -> usize {
    let ends: BTreeSet<_> = ir.geometry.edges[0].vertices().collect();
    ir.geometry
        .edges
        .iter()
        .position(|edge| edge.vertices().all(|end| !ends.contains(end)))
        .unwrap()
}

#[test]
fn a_fixed_face_is_planar_and_a_bend_pick_is_not() {
    let unfold = |ir: &CadIr| {
        sheet(SheetMetalKind::Unfold {
            fixed: Some(current(vec![face(ir, bottom(ir))])),
            bends: Some(BendSet::Selected(current(vec![face(ir, bottom(ir))]))),
        })
    };
    let mut ir = fixture::document();
    with(&mut ir, unfold);
    assert!(ruled(&ir, "design.picked-face-not-planar").is_empty());
    assert_eq!(ruled(&ir, "design.bend-pick-planar-face").len(), 1);
    curve_bottom(&mut ir);
    assert_eq!(ruled(&ir, "design.picked-face-not-planar").len(), 1);
    assert!(ruled(&ir, "design.bend-pick-planar-face").is_empty());
}

#[test]
fn a_circle_outline_center_lies_on_the_entry_plane() {
    let hole = |ir: &CadIr, z: f64| {
        sheet(SheetMetalKind::ExtrudedHole {
            bore: StampOutline::Circle {
                center: point(5.0, 5.0, z),
                diameter: positive(2.0),
            },
            entry: Some(current(vec![face(ir, bottom(ir))])),
            height: positive(1.0),
            root_fillet: None,
            rule: None,
        })
    };
    let rule = "design.circle-outline-center-off-entry-plane";
    let mut ir = fixture::document();
    with(&mut ir, |ir| hole(ir, 0.0));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| hole(ir, 1.0));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_stiffening_rib_point_lies_on_its_bend_face() {
    let rib = |ir: &CadIr, x: f64| {
        sheet(SheetMetalKind::Stamp {
            entry: None,
            form: StampForm::StiffeningRib {
                bend: current(vec![face(ir, bottom(ir))]),
                at: point(x, 5.0, 3.0),
                length: positive(1.0),
                bottom_radius: positive(1.0),
                wall_angle: Dimension::Literal(Angle::new(0.5).unwrap()),
                rim_radius: positive(1.0),
            },
            rule: None,
        })
    };
    let rule = "design.stiffening-rib-point-off-bend";
    let mut ir = fixture::document();
    curve_bottom(&mut ir);
    with(&mut ir, |ir| rib(ir, 10.0));
    assert!(ruled(&ir, rule).is_empty());
    assert!(ruled(&ir, "design.bend-pick-planar-face").is_empty());
    with(&mut ir, |ir| rib(ir, 5.0));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_graft_picks_a_solid_body_and_a_sheet() {
    let graft = |ir: &CadIr| {
        FeatureOperation::Graft(Box::new(Graft {
            body: current(vec![body(ir)]),
            sheet: current(vec![body(ir)]),
            flip: None,
            simplify: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let rule = "design.graft-pick-body-wrong-kind";
    let mut ir = fixture::document();
    with(&mut ir, graft);
    let messages = ruled(&ir, rule);
    assert!(
        matches!(messages.as_slice(), [message] if message.starts_with("Graft.sheet")),
        "{messages:?}"
    );
    set_kind(&mut ir, BodyKind::Sheet);
    let messages = ruled(&ir, rule);
    assert!(
        matches!(messages.as_slice(), [message] if message.starts_with("Graft.body")),
        "{messages:?}"
    );
}

#[test]
fn unsew_edges_divide_one_sheet_in_two() {
    let unsew = |ir: &CadIr, edges: &[usize]| {
        FeatureOperation::Unsew(Box::new(Unsew {
            edges: current(edges.iter().map(|index| edge(ir, *index)).collect()),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    };
    let shared = "design.unsew-edge-not-shared-sheet";
    let divide = "design.unsew-edges-do-not-divide-sheet";
    let mut ir = fixture::document();
    with(&mut ir, |ir| unsew(ir, &[0]));
    assert_eq!(ruled(&ir, shared).len(), 1);
    set_kind(&mut ir, BodyKind::Sheet);
    assert!(ruled(&ir, shared).is_empty());
    assert_eq!(ruled(&ir, divide).len(), 1);
    with(&mut ir, |ir| unsew(ir, &bottom_edges(ir)));
    assert!(ruled(&ir, shared).is_empty());
    assert!(ruled(&ir, divide).is_empty());
}

fn chamfer(sets: Vec<ChamferSet>, corners: Vec<Corner>) -> FeatureOperation {
    FeatureOperation::Chamfer(Box::new(Chamfer {
        sets,
        controls: BlendControls {
            corner_style: None,
            corners,
            limits: Vec::new(),
            keep: None,
            transitions: Vec::new(),
            overflow: None,
            keep_features: None,
            round_corners: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn setbacks(ir: &CadIr, edges: &[usize]) -> Vec<EdgeSetback> {
    edges
        .iter()
        .map(|index| EdgeSetback {
            edge: current(vec![edge(ir, *index)]),
            distance: nonnegative(1.0),
        })
        .collect()
}

#[test]
fn a_corner_setback_edge_ends_at_the_corner_vertex() {
    let corner_at = |ir: &CadIr, edges: &[usize]| {
        let (vertex, _) = corner(ir);
        chamfer(
            Vec::new(),
            vec![Corner {
                vertex: current(vec![vertex]),
                setbacks: Some(Setbacks::PerEdge(setbacks(ir, edges))),
            }],
        )
    };
    let rule = "design.edge-setback-edge-not-incident";
    let mut ir = fixture::document();
    let (_, at) = corner(&ir);
    with(&mut ir, |ir| corner_at(ir, &at));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| corner_at(ir, &[at[0], apart(ir)]));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn a_vertex_chamfer_edges_meet_at_one_vertex() {
    let vertex_chamfer = |ir: &CadIr, edges: &[usize]| {
        let set = ChamferSet {
            edges: current(vec![edge(ir, edges[0])]),
            size: Some(ChamferSize::Vertex(setbacks(ir, edges))),
            reference: None,
            flip: None,
            measure: None,
        };
        chamfer(vec![set], Vec::new())
    };
    let rule = "design.chamfer-vertex-edges-meet-no-vertex";
    let mut ir = fixture::document();
    let (_, at) = corner(&ir);
    with(&mut ir, |ir| vertex_chamfer(ir, &at));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| vertex_chamfer(ir, &[at[0], at[1], apart(ir)]));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

#[test]
fn flange_setbacks_from_the_ends_fit_the_edge() {
    let flange = |ir: &CadIr, second: f64| {
        sheet(SheetMetalKind::Flange(Flange {
            edges: current(vec![edge(ir, 0)]),
            height: None,
            angle: None,
            width: Some(FlangeWidth::FromEnds {
                first: nonnegative(4.0),
                second: nonnegative(second),
            }),
            bend: bend(),
            rule: None,
            height_datum: None,
            position: None,
            outline: None,
        }))
    };
    let rule = "design.from-ends-setbacks-exceed-edge";
    let mut ir = fixture::document();
    with(&mut ir, |ir| flange(ir, 6.0));
    assert!(ruled(&ir, rule).is_empty());
    with(&mut ir, |ir| flange(ir, 7.0));
    assert_eq!(ruled(&ir, rule).len(), 1);
}

fn bend() -> BendSpec {
    BendSpec {
        radius: None,
        radius_side: None,
        relief: None,
    }
}

#[test]
fn a_convert_bend_face_is_a_cylindrical_face_of_the_solid() {
    let convert = |ir: &CadIr| {
        sheet(SheetMetalKind::Convert(Convert {
            body: current(vec![body(ir)]),
            fixed: current(vec![face(ir, (bottom(ir) + 1) % 6)]),
            thickness: positive(1.0),
            from: ConvertFrom::Faces {
                side: None,
                bends: vec![ConvertBend::Face {
                    face: current(vec![face(ir, bottom(ir))]),
                }],
            },
            rips: Vec::new(),
            bend: bend(),
            keep_solid: None,
            rule: None,
        }))
    };
    let rule = "design.convert-bend-face-not-cylindrical";
    let mut ir = fixture::document();
    with(&mut ir, convert);
    assert_eq!(ruled(&ir, rule).len(), 1);
    assert!(ruled(&ir, "design.convert-pick-outside-solid").is_empty());
    curve_bottom(&mut ir);
    assert!(ruled(&ir, rule).is_empty());
}

#[test]
fn a_pick_without_resolved_entities_or_a_matching_name_is_not_judged() {
    let mut ir = fixture::document();
    let mut fixed: SetSelection = current(vec![face(&ir, bottom(&ir))]);
    if let Picks::Named(members) = &mut fixed.picks {
        let mut picks = members.as_slice().to_vec();
        picks[0].resolved = None;
        *members = Members::new(picks).unwrap();
    }
    with(&mut ir, |_| {
        sheet(SheetMetalKind::FlatPattern { fixed: Some(fixed) })
    });
    curve_bottom(&mut ir);
    assert!(ruled(&ir, "design.picked-face-not-planar").is_empty());
}
