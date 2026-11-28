// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use cadmpeg_core::serde_erase::json;

use crate::document::{ArenaName, EntityRewrite, Model, ScopeViolation, SourceMeta, Topology};
use crate::examples::unit_cube;
use crate::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::procedural_curves::ProceduralCurveDefinition;
use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralSurface, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};

use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;
use crate::{diff, CadIr};

mod append;
mod feature_parents;
mod scope;
mod unknowns;

#[test]
fn procedural_curve_attachment_moves_the_solved_carrier_into_the_cache() {
    let build = || {
        let owner = CurveId::mint("test:model:curve#charged").expect("valid curve identity");
        let mut geometry = Topology::default();
        geometry.curves.push(Curve {
            parameter_range: None,
            id: owner.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
            native_ref: None,
        });
        let procedural = ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:construction#charged")
                .expect("valid construction identity"),
            ProceduralCurveDefinition::Exact { cache: None },
        );
        (geometry, owner, procedural)
    };
    let (mut geometry, owner, procedural) = build();
    assert!(geometry.add_procedural_curve(&owner, procedural).is_ok());
    assert!(matches!(
        geometry.curves[0].geometry,
        CurveGeometry::Procedural {
            cache: Some(SolvedCurveGeometry::Unknown { record: None }),
            ..
        }
    ));
    assert_eq!(geometry.procedural_curves.len(), 1);
}

#[test]
fn procedural_surface_attachment_moves_the_solved_knot_storage() {
    let surface_id = SurfaceId::mint("test:model:surface#move-cache").unwrap();
    let procedural_id =
        ProceduralSurfaceId::mint("test:model:surface-construction#move-cache").unwrap();
    let carrier = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    let original_knot_storage = carrier.u_knots().as_ptr();
    let mut geometry = Topology::default();
    geometry.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(carrier)),
        source_object: None,
        native_ref: None,
    });
    geometry
        .add_procedural_surface(
            &surface_id,
            ProceduralSurface::new(
                procedural_id,
                ProceduralSurfaceDefinition::Unknown {
                    record: None,
                    cache: None,
                },
                None,
            ),
        )
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(cached)) = geometry.surfaces[0].geometry.solved_cache()
    else {
        panic!("expected the attached NURBS cache");
    };
    assert_eq!(cached.u_knots().as_ptr(), original_knot_storage);
}

#[test]
fn procedural_curve_attachment_moves_the_solved_knot_storage() {
    let curve_id = CurveId::mint("test:model:curve#move-cache").unwrap();
    let procedural_id =
        ProceduralCurveId::mint("test:model:curve-construction#move-cache").unwrap();
    let carrier = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let original_knot_storage = carrier.knots().as_ptr();
    let mut geometry = Topology::default();
    geometry.curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(carrier)),
        source_object: None,
        native_ref: None,
    });
    geometry
        .add_procedural_curve(
            &curve_id,
            ProceduralCurve::new(
                procedural_id,
                ProceduralCurveDefinition::Unknown {
                    native_kind: None,
                    record: None,
                    cache: None,
                },
            ),
        )
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(cached)) = geometry.curves[0].geometry.solved_cache()
    else {
        panic!("expected the attached NURBS cache");
    };
    assert_eq!(cached.knots().as_ptr(), original_knot_storage);
}

struct TypedIdentity<'a>(&'a cadmpeg_core::decode::DecodeContext<'a>);

impl EntityRewrite for TypedIdentity<'_> {
    type Error = cadmpeg_core::CodecError;

    fn rewrite<T: crate::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, Self::Error> {
        let mut map =
            crate::schema::rewrite::typed::IdentityMap::new(|source: &str| Ok(source.to_owned()));
        entity.rewrite_identities(self.0, &mut map)
    }
}

#[test]
fn entity_kind_index_is_the_arena_registry_position() {
    use super::*;

    let mut kinds = Vec::new();
    macro_rules! collect_kinds {
        ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(kinds.push(<$ty as crate::schema::ArenaRecord>::KIND);)*
        };
    }
    super::every_arena_registry!(collect_kinds);
    assert_eq!(kinds, crate::schema::EntityKind::ALL);
    for (position, kind) in kinds.into_iter().enumerate() {
        assert_eq!(kind.index(), position, "{kind:?}");
    }
}

#[test]
fn entity_schema_registry_covers_arenas_and_unit_cube_references_resolve() {
    fn collect_ids(value: &serde_json::Value, ids: &mut std::collections::HashSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(serde_json::Value::String(id)) = fields.get("id") {
                    ids.insert(id.clone());
                }
                for value in fields.values() {
                    collect_ids(value, ids);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect_ids(value, ids);
                }
            }
            _ => {}
        }
    }

    assert_eq!(crate::schema::EntityKind::ALL.len(), ArenaName::ALL.len());
    let ir = unit_cube().expect("valid unit cube fixture");
    let mut ids = std::collections::HashSet::new();
    collect_ids(&json::to_value(&ir.geometry).unwrap(), &mut ids);
    collect_ids(&json::to_value(&ir.model).unwrap(), &mut ids);
    let mut missing = Vec::new();
    let mut visit = |reference: &str| -> Result<(), cadmpeg_core::CodecError> {
        if !ids.contains(reference) {
            missing.push(reference.to_owned());
        }
        Ok(())
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    macro_rules! visit_arenas {
        ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for entity in &ir.$home.$field {
                crate::schema::EntitySchema::visit_references(entity, &ctx, &mut visit)
                    .expect("every entity states its typed references");
            })*
        };
    }
    super::arena_registry!(visit_arenas);
    for parent in ir.model.feature_regeneration_parents.0.values() {
        visit(parent.as_str()).unwrap();
    }
    assert!(missing.is_empty(), "unresolved references: {missing:?}");
}

#[test]
fn arena_registry_drives_counts_and_diff_dispatch() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    let diff_kinds = diff(&ir, &ir)
        .per_arena
        .into_iter()
        .map(|arena| arena.kind.to_string())
        .collect::<Vec<_>>();

    let arena_names = ArenaName::ALL
        .iter()
        .map(|arena| arena.as_str())
        .collect::<Vec<_>>();
    assert_eq!(&diff_kinds[..ArenaName::ALL.len()], arena_names);
    for name in &arena_names {
        // The census of a document with no history body omits that arena.
        if *name == "history_bodies" {
            assert!(!report.entity_counts.contains_key(*name));
            continue;
        }
        assert!(
            report.entity_counts.contains_key(*name),
            "entity counts omitted registered arena {name}"
        );
    }
}

/// A tree node `parent` that owns `child`, its one history child.
fn tree_node_model() -> (
    Model,
    crate::features::FeatureId,
    crate::features::FeatureId,
) {
    use crate::features::tree::FeatureTreeNodeRole;
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let parent_id = FeatureId::mint("test:model:feature#1-parent").expect("identity grammar");
    let child_id = FeatureId::mint("test:model:feature#2-child").expect("identity grammar");
    let parent = Feature {
        product_definition: None,
        id: parent_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: crate::features::tree::TreeChildren::new(
                    vec![child_id.clone()],
                    Some(child_id.clone()),
                )
                .unwrap(),
            }),
        ),
        native_ref: None,
    };
    let child = Feature {
        product_definition: None,
        id: child_id.clone(),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    };
    let model = Model {
        features: vec![parent, child],
        ..Model::default()
    };
    (model, parent_id, child_id)
}

/// The structural edge is on the wire once, inside the owning tree node's
/// ordered children, and a tree child states no regeneration parent.
#[test]
fn feature_parent_wire_is_derived_from_its_single_owner() {
    use crate::features::{Feature, FeatureDefinition, FeatureOperation};

    let (model, parent_id, child_id) = tree_node_model();
    let value = json::to_value(model.sorted_omitted()).unwrap();
    assert!(value["features"][1].get("parent").is_none());
    assert!(value["features"][1].get("regeneration_parent").is_none());
    assert_eq!(
        value["features"][0]["definition"]["value"]["value"]["children"]["children"][0],
        child_id.as_str()
    );
    assert_eq!(model.feature_parent(&child_id), Some(&parent_id));

    let mut regeneration = Model {
        features: vec![
            Feature {
                product_definition: None,
                id: parent_id.clone(),
                ordinal: 0,
                name: None,
                suppressed: None,
                dependencies: crate::features::selections::DistinctMembers::default(),
                source_properties: std::collections::BTreeMap::default(),
                source_tag: None,
                source_text: None,
                source_content: crate::features::FeatureContent::default(),
                evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
                ),
                native_ref: None,
            },
            Feature {
                product_definition: None,
                id: child_id.clone(),
                ordinal: 1,
                name: None,
                suppressed: None,
                dependencies: crate::features::selections::DistinctMembers::default(),
                source_properties: std::collections::BTreeMap::default(),
                source_tag: None,
                source_text: None,
                source_content: crate::features::FeatureContent::default(),
                evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
                ),
                native_ref: None,
            },
        ],
        ..Model::default()
    };
    regeneration
        .set_feature_regeneration_parent(&(child_id), &(parent_id.clone()))
        .unwrap();
    let value = json::to_value(regeneration.sorted_omitted()).unwrap();
    assert_eq!(
        value["features"][1]["regeneration_parent"],
        parent_id.as_str()
    );
}

#[test]
fn procedural_carrier_ownership_preserves_the_flat_cadir_wire() {
    let mut ir = CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#cache").expect("valid identity");
    let surface_construction =
        ProceduralSurfaceId::mint("test:model:surface-construction#cache").expect("valid identity");
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    });
    ir.geometry
        .add_procedural_surface(
            &surface,
            ProceduralSurface::new(
                surface_construction,
                ProceduralSurfaceDefinition::Unknown {
                    record: None,
                    cache: None,
                },
                None,
            ),
        )
        .unwrap();

    let curve = CurveId::mint("test:model:curve#direct").expect("valid identity");
    let curve_construction =
        ProceduralCurveId::mint("test:model:curve-construction#direct").expect("valid identity");
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: curve_construction.clone(),
            cache: None,
        },
        source_object: None,
        native_ref: None,
    });
    ir.geometry
        .add_procedural_curve(
            &curve,
            ProceduralCurve::new(
                curve_construction,
                ProceduralCurveDefinition::Exact { cache: None },
            ),
        )
        .unwrap();

    let value = json::to_value(&ir).unwrap();
    let model = value["geometry"].as_object().unwrap();
    assert_eq!(model["surfaces"][0]["geometry"]["kind"], "solved");
    assert_eq!(model["surfaces"][0]["geometry"]["value"]["kind"], "plane");
    assert!(model["surfaces"][0]["geometry"]["value"]
        .get("cache")
        .is_none());
    assert_eq!(model["procedural_surfaces"][0]["surface"], surface.as_str());
    assert_eq!(model["curves"][0]["geometry"]["kind"], "procedural");
    assert!(model["curves"][0]["geometry"]["value"]
        .get("cache")
        .is_none());
    assert_eq!(model["procedural_curves"][0]["curve"], curve.as_str());
    assert_eq!(json::from_value::<CadIr>(value).unwrap(), ir);

    let mut rewritten = Topology::default();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    rewritten
        .extend_rewritten(ir.geometry, &mut TypedIdentity(&ctx))
        .unwrap();
    assert!(rewritten.surfaces[0].geometry.solved_cache().is_some());
}

#[test]
fn procedural_surface_attachment_moves_solved_cache() {
    let owner = SurfaceId::mint("test:model:surface#1").unwrap();
    let construction = ProceduralSurfaceId::mint("test:model:surface-construction#1").unwrap();
    let mut base = CadIr::empty();
    base.geometry.surfaces.push(Surface {
        id: owner.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
        native_ref: None,
    });
    let procedural = ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Unknown {
            record: None,
            cache: None,
        },
        None,
    );
    let mut cached = base.clone();
    cached.geometry.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(crate::ids::UnknownId::mint("test:model:unknown#1").unwrap()),
        });
    let solved = cached.geometry.surfaces[0].geometry.clone();
    cached
        .geometry
        .add_procedural_surface(&owner, procedural.clone())
        .unwrap();
    assert_eq!(cached.geometry.procedural_surfaces.len(), 1);
    assert_eq!(
        cached.geometry.surfaces[0].geometry.solved_cache(),
        solved.solved()
    );
    let mut attached = base.clone();
    attached
        .geometry
        .add_procedural_surface(&owner, procedural.clone())
        .unwrap();
    base.geometry
        .add_procedural_surface(&owner, procedural)
        .unwrap();
    assert_eq!(attached, base);
}

#[test]
fn procedural_curve_attachment_moves_solved_cache() {
    let owner = CurveId::mint("test:model:curve#1").unwrap();
    let construction = ProceduralCurveId::mint("test:model:curve-construction#1").unwrap();
    let mut base = CadIr::empty();
    base.geometry.curves.push(Curve {
        parameter_range: None,
        id: owner.clone(),
        geometry: CurveGeometry::Solved(crate::geometry::SolvedCurveGeometry::Unknown {
            record: None,
        }),
        source_object: None,
        native_ref: None,
    });
    let procedural = ProceduralCurve::new(
        construction,
        ProceduralCurveDefinition::Exact { cache: None },
    );
    let mut cached = base.clone();
    cached.geometry.curves[0].geometry =
        CurveGeometry::Solved(crate::geometry::SolvedCurveGeometry::Unknown {
            record: Some(crate::ids::UnknownId::mint("test:model:unknown#1").unwrap()),
        });
    let solved = cached.geometry.curves[0].geometry.clone();
    cached
        .geometry
        .add_procedural_curve(&owner, procedural.clone())
        .unwrap();
    assert_eq!(cached.geometry.procedural_curves.len(), 1);
    assert_eq!(
        cached.geometry.curves[0].geometry.solved_cache(),
        solved.solved()
    );
    let mut attached = base.clone();
    attached
        .geometry
        .add_procedural_curve(&owner, procedural.clone())
        .unwrap();
    base.geometry
        .add_procedural_curve(&owner, procedural)
        .unwrap();
    assert_eq!(attached, base);
}

#[test]
fn json_round_trips_and_is_deterministic() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let json1 = ir.to_canonical_json().unwrap();
    let json2 = ir.to_canonical_json().unwrap();
    assert_eq!(json1, json2, "serialization must be deterministic");

    let parsed = crate::CadIr::from_json(&json1).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the document");
    assert_eq!(parsed.to_canonical_json().unwrap(), json1);
}

#[test]
fn up_axis_round_trips_and_is_absent_from_the_wire_when_unstated() {
    use crate::up_axis::UpAxis;
    let mut ir = unit_cube().expect("valid unit cube fixture");
    assert_eq!(ir.up_axis, None);
    let absent = ir.to_canonical_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&absent).unwrap();
    assert!(
        value.get("up_axis").is_none(),
        "an unstated axis writes no key"
    );
    for (axis, text) in [(UpAxis::Y, "y"), (UpAxis::Z, "z")] {
        ir.up_axis = Some(axis);
        let json = ir.to_canonical_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["up_axis"], text);
        let parsed = CadIr::from_json(&json).unwrap();
        assert_eq!(parsed.up_axis, Some(axis));
        assert_eq!(parsed, ir);
        assert_eq!(parsed.to_canonical_json().unwrap(), json);
    }
    let mut bad: serde_json::Value = serde_json::from_str(&absent).unwrap();
    bad["up_axis"] = "x".into();
    assert!(CadIr::from_json(&bad.to_string()).is_err());
}

#[test]
fn json_round_trip_preserves_ulp_edge_scalars_exactly() {
    // Byte-backed writers compare parsed documents against fresh decodes with
    // exact f64 equality, so JSON parsing must be correctly rounded. The
    // values one to a few ULPs below 1.0 are the ones a fast non-roundtrip
    // float parser misparses by one ULP.
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let edge_values: Vec<f64> = (1..40)
        .map(|n| 1.0f64 - f64::from(n) * f64::EPSILON / 2.0)
        .collect();
    for (point, value) in ir
        .geometry
        .points
        .iter_mut()
        .zip(edge_values.iter().cycle())
    {
        let moved = point.position().get();
        point.set_position(
            crate::features::geometry::FinitePoint3::new(Point3::new(*value, moved.y, moved.z))
                .expect("a finite position is a point"),
        );
    }
    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    for (before, after) in ir.geometry.points.iter().zip(&parsed.geometry.points) {
        assert_eq!(
            before.position().get().x.to_bits(),
            after.position().get().x.to_bits(),
            "JSON round-trip changed {} by at least one ULP",
            before.position().get().x
        );
    }
}

#[test]
fn parser_rejects_unsupported_missing_and_non_string_versions() {
    let canonical = json::to_value(unit_cube().expect("valid unit cube fixture")).unwrap();
    let expected_version = crate::IR_VERSION;
    for (version, found) in [
        (
            Some(serde_json::Value::String("7".into())),
            "unsupported ir_version \"7\"",
        ),
        (
            Some(serde_json::Value::String("8".into())),
            "unsupported ir_version \"8\"",
        ),
        (
            Some(serde_json::Value::String("9".into())),
            "unsupported ir_version \"9\"",
        ),
        (
            Some(serde_json::Value::String("12".into())),
            "unsupported ir_version \"12\"",
        ),
        (
            Some(serde_json::Value::String("37".into())),
            "unsupported ir_version \"37\"",
        ),
        (
            Some(serde_json::Value::String("36".into())),
            "unsupported ir_version \"36\"",
        ),
        (
            Some(serde_json::Value::String("35".into())),
            "unsupported ir_version \"35\"",
        ),
        (
            Some(serde_json::Value::String("34".into())),
            "unsupported ir_version \"34\"",
        ),
        (
            Some(serde_json::Value::String("33".into())),
            "unsupported ir_version \"33\"",
        ),
        (
            Some(serde_json::Value::String("0".into())),
            "unsupported ir_version \"0\"",
        ),
        (
            Some(serde_json::Value::String("14".into())),
            "unsupported ir_version \"14\"",
        ),
        (
            Some(serde_json::Value::String("15".into())),
            "unsupported ir_version \"15\"",
        ),
        (
            Some(serde_json::Value::String("16".into())),
            "unsupported ir_version \"16\"",
        ),
        (
            Some(serde_json::Value::String("17".into())),
            "unsupported ir_version \"17\"",
        ),
        (
            Some(serde_json::Value::String("29".into())),
            "unsupported ir_version \"29\"",
        ),
        (None, "no `ir_version`"),
        (
            Some(serde_json::Value::Number(1.into())),
            "unsupported ir_version 1",
        ),
    ] {
        let expected = format!("{found}; expected {expected_version}");
        let mut value = canonical.clone();
        let object = value.as_object_mut().unwrap();
        match version {
            Some(version) => {
                object.insert("ir_version".into(), version);
            }
            None => {
                object.remove("ir_version");
            }
        }
        let json = json::to_string(&value).unwrap();
        let error = CadIr::from_json(&json).unwrap_err();
        assert!(!error.is_syntax());
        assert!(error.to_string().contains(&expected), "{error}");
        let direct = serde_json::from_str::<CadIr>(&json).unwrap_err();
        assert!(direct.to_string().contains(&expected), "{direct}");
    }
}

#[test]
fn direct_deserialization_accepts_current_version_and_canonical_round_trip() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let json = ir.to_canonical_json().unwrap();
    let parsed = serde_json::from_str::<CadIr>(&json).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(parsed.to_canonical_json().unwrap(), json);
}

#[test]
fn parser_distinguishes_malformed_json_from_version_rejection() {
    let error = CadIr::from_json("{\"ir_version\":\"1\"").unwrap_err();
    assert!(error.is_syntax() || error.is_eof());
    assert!(!error.to_string().contains("unsupported ir_version"));
}

#[test]
fn current_document_excludes_source_byte_accounting() {
    let ir = CadIr::empty();
    let json = json::to_value(&ir).unwrap();

    assert_eq!(json["ir_version"], crate::IR_VERSION);
    assert!(json.get("byte_ledger").is_none());
}

fn parameter(id: &str) -> crate::features::parameters::DesignParameter {
    crate::features::parameters::DesignParameter {
        product_definition: None,
        id: crate::features::ParameterId::mint(id).unwrap(),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "1mm".into(),
        display: None,
        value: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn with_scope(ir: &CadIr, scope: serde_json::Value) -> String {
    let mut value = json::to_value(ir).unwrap();
    value["scope"] = scope;
    json::to_string(&value).unwrap()
}

#[test]
fn scope_is_a_required_wire_field() {
    let ir = unit_cube().expect("valid unit cube fixture");
    assert_eq!(ir.scope(), ContentScope::full());
    let mut value = json::to_value(&ir).unwrap();
    assert_eq!(
        value["scope"],
        cadmpeg_core::json!({"model": true, "design": true})
    );

    value.as_object_mut().unwrap().remove("scope");
    let json = json::to_string(&value).unwrap();
    let error = CadIr::from_json(&json).unwrap_err();
    assert!(error.to_string().contains("scope"), "{error}");
}

#[test]
fn a_geometry_document_without_design_records_round_trips() {
    let json = with_scope(
        &unit_cube().expect("valid unit cube fixture"),
        cadmpeg_core::json!({"model": true, "design": false}),
    );
    let parsed = CadIr::from_json(&json).unwrap();
    assert_eq!(parsed.scope(), ContentScope::geometry());
    let reparsed = serde_json::from_str::<CadIr>(&parsed.to_canonical_json().unwrap()).unwrap();
    assert_eq!(reparsed, parsed);
}

#[test]
fn a_scope_missing_a_required_class_reads_and_writes_as_stated() {
    let ir = CadIr::empty();
    let (held, required) = (ArenaClass::Design, ArenaClass::Model);
    let scope = ContentScope::full().with(required, false);
    let mut set = ir.clone();
    set.set_scope(scope);
    assert_eq!(set.scope(), scope);
    let violation = ScopeViolation::Unmet { held, required };
    assert_eq!(set.scope_violations(), [violation]);
    assert_eq!(
        violation.to_string(),
        format!("the document's scope holds {held} but excludes {required}, which {held} requires")
    );
    let read = CadIr::from_json(&with_scope(&ir, json::to_value(scope).unwrap())).unwrap();
    assert_eq!(read.scope(), scope);
    assert_eq!(read.scope_violations(), [violation]);
    assert_eq!(
        json::to_value(&read).unwrap()["scope"],
        json::to_value(scope).unwrap()
    );
}

#[test]
fn a_document_with_records_of_an_excluded_class_reads_and_names_each_arena() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.design.scopes.push(crate::design::scope::DesignScope {
        definition: crate::ids::ProductDefinitionId::mint("synthetic:test:product-definition#root")
            .unwrap(),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: std::collections::BTreeSet::new(),
        occurrences: std::collections::BTreeMap::new(),
        current: Vec::new(),
        unproduced: std::collections::BTreeSet::new(),
    });
    assert_eq!(
        ir.populated_arena(ArenaClass::Design)
            .map(ArenaName::as_str),
        Some("design.scopes")
    );
    let bodies = ScopeViolation::Populated {
        arena: ArenaName::parse("bodies").unwrap(),
    };
    let scopes = ScopeViolation::Populated {
        arena: ArenaName::parse("design.scopes").unwrap(),
    };
    assert_eq!(
        bodies.to_string(),
        "the document's scope excludes model but model arena `bodies` holds records"
    );
    assert_eq!(
        scopes.to_string(),
        "the document's scope excludes design but design arena `design.scopes` holds records"
    );
    let mut container: Vec<ScopeViolation> = [
        "bodies",
        "regions",
        "shells",
        "faces",
        "loops",
        "coedges",
        "edges",
        "vertices",
        "points",
        "surfaces",
        "curves",
        "product_definitions",
    ]
    .into_iter()
    .map(|arena| ScopeViolation::Populated {
        arena: ArenaName::parse(arena).unwrap(),
    })
    .collect();
    assert_eq!(container[0], bodies);
    container.push(scopes);
    for (scope, expected) in [
        (ContentScope::container(), container),
        (ContentScope::geometry(), vec![scopes]),
        (ContentScope::design(), vec![]),
    ] {
        let json = with_scope(&ir, json::to_value(scope).unwrap());
        let read = CadIr::from_json(&json).unwrap();
        assert_eq!(read.scope(), scope);
        assert_eq!(serde_json::from_str::<CadIr>(&json).unwrap(), read);
        assert_eq!(read.scope_violations(), expected);
    }
}

#[test]
fn a_design_record_outside_the_scope_is_kept_on_set_and_on_write() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.set_scope(ContentScope::geometry());
    ir.model
        .parameters
        .push(parameter("synthetic:test:parameter#depth"));
    let violation = ScopeViolation::Populated {
        arena: ArenaName::parse("parameters").unwrap(),
    };
    assert_eq!(ir.scope_violations(), [violation]);
    ir.set_scope(ContentScope::geometry());
    assert_eq!(ir.scope(), ContentScope::geometry());
    assert_eq!(
        json::to_value(&ir).unwrap()["scope"],
        json::to_value(ContentScope::geometry()).unwrap()
    );
    ir.set_scope(ContentScope::design());
    assert!(ir.scope_violations().is_empty());
}

#[test]
fn every_design_arena_is_classified_design() {
    let design = ArenaName::ALL
        .iter()
        .filter(|arena| arena.class() == ArenaClass::Design)
        .map(|arena| arena.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        design,
        [
            "features",
            "feature_input_topologies",
            "feature_result_topologies",
            "configurations",
            "parameters",
            "sketches",
            "sketch_entities",
            "sketch_constraints",
            "spatial_sketches",
            "spatial_sketch_entities",
            "spatial_sketch_constraints",
            "spreadsheets",
            "history_bodies",
            "design.scopes",
            "design.containers",
            "design.features",
            "design.sketches",
            "design.sketch_entities",
            "design.sketch_constraints",
            "design.spatial_sketches",
            "design.spatial_sketch_entities",
            "design.spatial_sketch_constraints",
            "design.parameters",
            "design.spreadsheets",
            "design.sheet_metal_rules",
            "design.configurations",
            "design.arrangements",
            "design.evaluations",
        ]
    );
}

/// The format identity is one tagged object, so an unclassified source states
/// its format once and a classified one never restates its payload's format.
#[test]
fn an_unclassified_source_states_its_format_inside_the_identity() {
    let stored = "{\"identity\":{\"kind\":\"unclassified\",\"format\":\"rhino\"},\
                  \"attributes\":{\"object_count\":\"3\"}}";
    let source: SourceMeta = serde_json::from_str(stored).unwrap();

    assert_eq!(source.format(), "rhino");
    assert_eq!(source.dialect(), None);

    let rewritten = json::to_string(&source).unwrap();
    assert_eq!(
        rewritten,
        "{\"identity\":{\"kind\":\"unclassified\",\"format\":\"rhino\"},\
         \"attributes\":{\"object_count\":\"3\"}}"
    );
    assert_eq!(
        serde_json::from_str::<SourceMeta>(&rewritten).unwrap(),
        source
    );
}

#[test]
fn a_classified_source_carries_its_format_once() {
    let matched = cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect_id!(
        "rhino:archive-80"
    ));
    let layers = cadmpeg_core::dialect::DialectLayers::of(matched.clone());
    let source = SourceMeta::classified(
        layers.clone(),
        std::collections::BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("object_count"),
            "3".into(),
        )]),
    );

    assert_eq!(source.format(), "rhino");
    assert_eq!(source.dialect(), Some(&matched));
    assert_eq!(source.dialects(), Some(&layers));
    let rendered = json::to_value(&source).unwrap();
    assert_eq!(rendered["identity"]["kind"], "classified");
    assert!(rendered["identity"].get("format").is_none());
    assert!(rendered["identity"]["dialects"]["primary"]
        .get("format")
        .is_none());
    assert_eq!(
        rendered["identity"]["dialects"]["primary"]["dialect"],
        "rhino:archive-80"
    );
    assert_eq!(
        json::from_value::<SourceMeta>(rendered.clone()).unwrap(),
        source
    );

    let mut restated = rendered;
    restated["identity"]["format"] = cadmpeg_core::json!("step");
    let error = json::from_value::<SourceMeta>(restated)
        .expect_err("a classified identity carries no second format");
    assert!(error.to_string().contains("format"), "{error}");
}

#[test]
fn parent_only_wire_preserves_regeneration_without_tree_membership() {
    use crate::features::tree::FeatureTreeNodeRole;
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let parent_id = FeatureId::mint("test:model:feature#1-parent").expect("identity grammar");
    let child_id = FeatureId::mint("test:model:feature#2-child").expect("identity grammar");
    let mut model = Model {
        features: vec![
            Feature {
                product_definition: None,
                id: parent_id.clone(),
                ordinal: 0,
                name: None,
                suppressed: None,
                dependencies: crate::features::selections::DistinctMembers::default(),
                source_properties: std::collections::BTreeMap::default(),
                source_tag: None,
                source_text: None,
                source_content: crate::features::FeatureContent::default(),
                evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::TreeNode {
                        role: FeatureTreeNodeRole::SolidBodies,
                        children: crate::features::tree::TreeChildren::default(),
                    }),
                ),
                native_ref: None,
            },
            Feature {
                product_definition: None,
                id: child_id.clone(),
                ordinal: 1,
                name: None,
                suppressed: None,
                dependencies: crate::features::selections::DistinctMembers::default(),
                source_properties: std::collections::BTreeMap::default(),
                source_tag: None,
                source_text: None,
                source_content: crate::features::FeatureContent::default(),
                evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
                ),
                native_ref: None,
            },
        ],
        ..Model::default()
    };
    model
        .set_feature_regeneration_parent(&(child_id.clone()), &(parent_id.clone()))
        .unwrap();
    assert_eq!(model.feature_tree_parent(&child_id), None);
    assert_eq!(model.feature_parent(&child_id), Some(&parent_id));
    let wire = json::to_value(model.sorted_omitted()).unwrap();
    assert_eq!(
        wire["features"][0]["definition"]["value"]["value"]["children"],
        cadmpeg_core::json!({"children": []})
    );
    assert_eq!(
        wire["features"][1]["regeneration_parent"],
        parent_id.as_str()
    );
    assert!(wire["features"][1].get("parent").is_none());
}

/// Every optional feature key is left out when it holds no value, so a stated
/// `null` is refused by name.
#[test]
fn a_stated_null_is_refused_on_every_feature_key_written_by_omission() {
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#0").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    let feature = json::to_value(&ir.model.features[0]).unwrap();

    for key in [
        "product_definition",
        "name",
        "suppressed",
        "source_tag",
        "source_text",
        "native_ref",
    ] {
        let mut value = feature.clone();
        value
            .as_object_mut()
            .unwrap()
            .insert(key.to_owned(), serde_json::Value::Null);
        let error = json::from_value::<Feature>(value).unwrap_err().to_string();
        assert!(
            error.contains("this key states a value or is left out; it does not state null"),
            "feature.{key}: {error}"
        );
    }
}

#[test]
fn geometry_snapshot_matches_filtered_model_wire_without_intermediate_tree() {
    let geometry = Topology::default();
    let model = Model::default();
    let mut baseline = json::to_value(&geometry).expect("topology serializes");
    let object = baseline.as_object_mut().expect("topology is an object");
    object.remove("subds");
    object.insert(
        "tessellations".into(),
        json::to_value(&model.tessellations).expect("tessellations serialize"),
    );
    object.insert("kind".into(), cadmpeg_core::json!("brep"));
    assert_eq!(
        json::to_string(&geometry.geometry_snapshot(&model, "brep")).expect("snapshot serializes"),
        json::to_string(&baseline).expect("baseline serializes"),
    );
}

#[test]
fn repeated_finalize_admits_thousands_of_sorted_long_identities() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut geometry = Topology::default();
    let suffix = "x".repeat(128);
    for index in 0..4096 {
        geometry.curves.push(Curve {
            parameter_range: None,
            id: CurveId::mint(format!("test:model:curve#{index:04}{suffix}")).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
            native_ref: None,
        });
    }
    let before = geometry.clone();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    geometry.finalize();
    geometry.finalize();
    assert_eq!(geometry, before);
    ctx.finish_session().unwrap();
    // Public-arena mutation must be observed on the next finalize call.
    geometry.curves.reverse();
    geometry.finalize();
    assert_eq!(geometry, before);
}

#[test]
fn a_model_reference_into_the_design_outside_the_scope_is_refused() {
    use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use crate::references::InstanceTarget;

    let attribute = |target: InstanceTarget<AttributeTarget>, values| SourceAttribute {
        id: crate::ids::AttributeId::mint("synthetic:test:attribute#part-number").unwrap(),
        target,
        name: "Part Number".into(),
        values,
        native_ref: None,
    };
    let configuration =
        crate::features::ConfigurationId::mint("synthetic:test:configuration#large").unwrap();
    let state = crate::design::refs::StateRef {
        configuration: Some(configuration.clone()),
        of: crate::design::refs::Evaluated::Tool(
            crate::features::FeatureId::mint("synthetic:test:feature#cut").unwrap(),
        ),
    };
    let attributes = ArenaName::parse("attributes").unwrap();
    for record in [
        attribute(
            AttributeTarget::Configuration(configuration).into(),
            Vec::new(),
        ),
        attribute(
            AttributeTarget::Document.into(),
            vec![Some(AttributeValue::Parameter(
                crate::features::ParameterId::mint("synthetic:test:parameter#depth").unwrap(),
            ))],
        ),
        attribute(
            InstanceTarget {
                entity: AttributeTarget::Document,
                path: None,
                state: Some(state.clone()),
            },
            Vec::new(),
        ),
    ] {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        ir.set_scope(ContentScope::geometry());
        ir.model.attributes.push(record);
        let violation = ScopeViolation::Referenced {
            class: ArenaClass::Design,
            arena: attributes,
        };
        assert_eq!(ir.scope_violations(), [violation]);
        assert_eq!(
            violation.to_string(),
            "the document's scope excludes design but model arena `attributes` holds a reference into it"
        );
        let json = json::to_string(&ir).unwrap();
        let read = CadIr::from_json(&json).unwrap();
        assert_eq!(read.scope(), ContentScope::geometry());
        assert_eq!(read.scope_violations(), [violation]);
        ir.set_scope(ContentScope::design());
        assert!(ir.scope_violations().is_empty());
    }

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.set_scope(ContentScope::geometry());
    ir.model
        .declared_measures
        .push(crate::declared_measures::DeclaredMeasure::new(
            crate::ids::DeclaredMeasureId::mint("synthetic:test:declared_measure#volume").unwrap(),
            crate::declared_measures::DeclaredTarget::State(state),
            crate::declared_measures::DeclaredQuantity::Volume {
                value: crate::scalar::FiniteReal::new(1.0).unwrap(),
            },
        ));
    assert_eq!(
        ir.scope_violations(),
        [ScopeViolation::Referenced {
            class: ArenaClass::Design,
            arena: ArenaName::parse("declared_measures").unwrap(),
        }]
    );
}

#[test]
fn a_solver_setting_of_a_joint_reads_the_design() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.set_scope(ContentScope::geometry());
    ir.model
        .solver_settings
        .push(crate::physics::solver::SolverSettings {
            id: crate::ids::SolverSettingsId::mint("synthetic:test:solver#joint").unwrap(),
            solver: "physx".to_owned(),
            schema: "PhysxJointAPI".to_owned(),
            target: crate::physics::solver::SolverTarget::Joint {
                joint: crate::features::FeatureId::mint("synthetic:test:feature#hinge").unwrap(),
            }
            .into(),
            values: std::collections::BTreeMap::new(),
            native_ref: None,
        });
    assert_eq!(
        ir.scope_violations(),
        [ScopeViolation::Referenced {
            class: ArenaClass::Design,
            arena: ArenaName::parse("solver_settings").unwrap(),
        }]
    );
    ir.set_scope(ContentScope::design());
    assert!(ir.scope_violations().is_empty());
}

#[test]
fn an_arena_out_of_order_is_written_sorted_and_the_strict_reader_refuses_it() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let sorted = json::to_value(&ir).expect("a sorted document writes");
    assert!(ir.geometry.points.len() >= 2);

    // Serialization sorts an arena out of order, as `finalize` does, and
    // keeps records of one id in their order.
    let mut unsorted = ir.clone();
    unsorted.geometry.points.swap(0, 1);
    let report = validate_neutral(&unsorted, Vec::new()).expect("validate in memory");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == crate::report::check::Check::ArenaOrder));
    assert_eq!(
        json::to_value(&unsorted).expect("an unsorted document writes"),
        sorted
    );
    let mut repeated = ir.clone();
    let mut later = repeated.geometry.points[0].clone();
    later.native_ref = Some("the later copy".into());
    repeated.geometry.points.insert(0, later);
    let wire = json::to_value(&repeated).expect("a repeated id writes");
    let points = wire["geometry"]["points"].as_array().unwrap();
    assert_eq!(points.len(), ir.geometry.points.len() + 1);
    assert_eq!(points[0]["native_ref"], "the later copy");

    // The strict reader refuses an arena out of order and a repeated id.
    let mut swapped = sorted;
    swapped["geometry"]["points"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(CadIr::from_json(&swapped.to_string()).is_err());
    assert!(CadIr::from_json(&wire.to_string()).is_err());
}

/// The wire writes every `model` arena but those the registry marks omitted,
/// and the reader refuses those.
#[test]
fn model_wire_omits_exactly_the_marked_arenas() {
    let value = json::to_value(super::Model::default()).unwrap();
    let written: std::collections::BTreeSet<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected: std::collections::BTreeSet<&str> = super::ArenaName::ALL
        .iter()
        .filter(|arena| arena.home() == super::ArenaHome::Model)
        .filter(|arena| !super::ArenaName::WIRE_OMITTED.contains(arena))
        .map(|arena| arena.key())
        .collect();
    assert_eq!(written, expected);
    assert!(super::ArenaName::WIRE_OMITTED
        .iter()
        .all(|arena| arena.home() == super::ArenaHome::Model));
    assert_eq!(super::ArenaName::WIRE_OMITTED.len(), 14);
}

/// A model read from CADIR JSON says it does not hold the omitted arenas, and
/// equality compares records only.
#[test]
fn cadir_read_marks_omitted_arenas_not_read() {
    let ir = crate::examples::unit_cube().unwrap();
    let read = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert!(ir.model.omitted_arenas_held());
    assert!(!read.model.omitted_arenas_held());
    assert_eq!(read, ir);
}
