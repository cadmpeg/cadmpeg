// SPDX-License-Identifier: Apache-2.0
use crate::schema::rewrite::typed::IdentityMap;
use crate::schema::EntitySchema;
use serde::de::DeserializeOwned;

const SOURCE: &str = "test:native:record#geometry";
const TARGET: &str = "other:native:record#renamed";

fn field_contract<T: EntitySchema + DeserializeOwned + Clone>(record: &T) {
    let original = serde_json::to_value(record).unwrap();
    assert!(original.get("native_ref").is_none());
    let mut wire = original.clone();
    wire["native_ref"] = SOURCE.into();
    let linked: T = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(linked.native_reference(), Some(SOURCE));
    assert_eq!(serde_json::to_value(&linked).unwrap(), wire);
    for invalid in [serde_json::Value::Null, serde_json::json!(17)] {
        let mut invalid_wire = wire.clone();
        invalid_wire["native_ref"] = invalid;
        assert!(serde_json::from_value::<T>(invalid_wire).is_err());
    }
    let mut unknown = wire.clone();
    unknown["unknown_projection_field"] = true.into();
    assert!(serde_json::from_value::<T>(unknown).is_err());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut seen = 0;
    linked
        .visit_dependency_references(&ctx, &mut |reference| {
            if reference.target == SOURCE {
                assert_eq!(
                    reference.position,
                    crate::schema::rewrite::typed::prune::DependencyPosition::Required
                );
                assert_eq!(reference.field, Some("native_ref"));
                seen += 1;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, 1);
    let mut rewrite = IdentityMap::new(|id: &str| {
        Ok(if id == SOURCE {
            TARGET.to_owned()
        } else {
            id.to_owned()
        })
    });
    let rewritten = linked.rewrite_identities(&ctx, &mut rewrite).unwrap();
    wire["native_ref"] = TARGET.into();
    assert_eq!(serde_json::to_value(rewritten).unwrap(), wire);
    assert_eq!(serde_json::to_value(record).unwrap(), original);
}

fn geometry_model() -> crate::CadIr {
    use crate::geometry::{CurveGeometry, ProceduralCurve, ProceduralSurface, SurfaceGeometry};
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.bodies[0].name = Some(SOURCE.into());
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve {
        id: "test:model:pcurve#one".try_into().unwrap(),
        native_ref: None,
        geometry: crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        metadata: Default::default(),
    });
    ir.model.subds.push(crate::subd::SubdSurface {
        id: "test:model:subd#one".try_into().unwrap(),
        native_ref: None,
        scheme: crate::subd::SubdScheme::CatmullClark,
        source_object: None,
        cage: crate::subd::SubdCage::new(vec![], vec![], vec![], vec![]).unwrap(),
    });
    let curve_id = "test:model:curve#procedural".try_into().unwrap();
    let procedural_curve = ProceduralCurve::new(
        "test:model:procedural-curve#one".try_into().unwrap(),
        crate::geometry::ProceduralCurveDefinition::Offset(
            crate::geometry::curve_payloads::OffsetCurveConstruction::try_new(
                ir.model.curves[0].id.clone(),
                1.0,
                crate::geometry::OffsetSide::PlaneNormal {
                    normal: crate::math::Vector3::new(0.0, 0.0, 1.0),
                },
                None,
            )
            .unwrap(),
        ),
    );
    ir.model.curves.push(crate::geometry::Curve {
        id: curve_id,
        native_ref: None,
        geometry: CurveGeometry::Procedural {
            construction: procedural_curve.id.clone(),
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });
    let surface_id = "test:model:surface#procedural".try_into().unwrap();
    let procedural_surface = ProceduralSurface::new(
        "test:model:procedural-surface#one".try_into().unwrap(),
        crate::geometry::ProceduralSurfaceDefinition::Compound(
            crate::geometry::surface_payloads::CompoundSurfacePayload::try_new(
                vec![crate::geometry::CompoundComponent {
                    parameter: 1.0,
                    component: ir.model.surfaces[0].id.clone(),
                }],
                None,
            )
            .unwrap(),
        ),
        None,
    );
    ir.model.surfaces.push(crate::geometry::Surface {
        id: surface_id,
        native_ref: None,
        geometry: SurfaceGeometry::Procedural {
            construction: procedural_surface.id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model
        .add_procedural_curves(vec![(
            ir.model.curves.last().unwrap().id.clone(),
            procedural_curve,
        )])
        .unwrap()
        .unwrap();
    ir.model
        .add_procedural_surfaces(vec![(
            ir.model.surfaces.last().unwrap().id.clone(),
            procedural_surface,
        )])
        .unwrap()
        .unwrap();
    ir
}

macro_rules! geometry_arenas {
    ($run:ident) => {
        $run!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            curves,
            surfaces,
            pcurves,
            subds,
            procedural_curves,
            procedural_surfaces
        );
    };
}

#[test]
fn all_fifteen_geometry_arenas_keep_strict_optional_native_reference_fields() {
    let ir = geometry_model();
    let mut arenas = 0;
    macro_rules! check { ($($field:ident),*) => {$(
        assert!(!ir.model.$field.is_empty(), stringify!($field));
        for record in &ir.model.$field { field_contract(record); }
        arenas += 1;
    )*}; }
    geometry_arenas!(check);
    assert_eq!(arenas, 15);
}

#[test]
fn full_registry_and_canonical_wire_preserve_geometry_native_references() {
    let mut ir = geometry_model();
    macro_rules! link { ($($field:ident),*) => {$(
        for record in &mut ir.model.$field { record.native_ref = Some(SOURCE.into()); }
    )*}; }
    geometry_arenas!(link);
    ir.native.namespace_mut("test").arenas_mut().insert(
        "records".into(),
        vec![crate::NativeRecord::new(SOURCE.try_into().unwrap(), serde_json::Map::new()).unwrap()],
    );
    let mut arenas = std::collections::BTreeSet::new();
    crate::schema::native_links::visit_native_references(&ir.model, &mut |arena, _, _, native| {
        if native == Some(SOURCE) {
            arenas.insert(arena);
        }
    });
    assert_eq!(arenas.len(), 15);
    let parsed = crate::CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
    for wire in [
        ir.to_canonical_json().unwrap(),
        serde_json::to_string(&ir).unwrap(),
    ] {
        let parsed: crate::CadIr = serde_json::from_str(&wire).unwrap();
        assert_eq!(parsed, ir);
    }
}

#[test]
fn geometry_snapshot_transports_linked_carriers_and_procedural_rows_losslessly() {
    let mut ir = geometry_model();
    macro_rules! link {
        ($field:ident, $target:literal) => {
            for record in &mut ir.model.$field {
                record.native_ref = Some($target.into());
            }
        };
    }
    link!(curves, "test:native:record#curve");
    link!(surfaces, "test:native:record#surface");
    link!(procedural_curves, "test:native:record#procedural-curve");
    link!(procedural_surfaces, "test:native:record#procedural-surface");
    let targets = [
        ("curves", "test:native:record#curve"),
        ("surfaces", "test:native:record#surface"),
        ("procedural_curves", "test:native:record#procedural-curve"),
        (
            "procedural_surfaces",
            "test:native:record#procedural-surface",
        ),
    ];
    let records: Vec<_> = targets
        .iter()
        .map(|(_, target)| {
            crate::NativeRecord::new((*target).try_into().unwrap(), serde_json::Map::new()).unwrap()
        })
        .collect();
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .insert("records".into(), records);
    ir.finalize();
    let before = ir.clone();
    let snapshot = serde_json::to_value(ir.model.geometry_snapshot("brep").unwrap()).unwrap();
    assert_eq!(snapshot["kind"], "brep");
    let canonical = ir.to_canonical_json().unwrap();
    let mut transported: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    for (arena, target) in targets {
        let rows = snapshot[arena].as_array().unwrap();
        assert!(!rows.is_empty(), "{arena}");
        let mut snapshot_by_id = std::collections::BTreeMap::new();
        for row in rows {
            assert_eq!(row["native_ref"], target);
            let id = row["id"].as_str().unwrap();
            assert!(snapshot_by_id.insert(id, row).is_none());
        }
        let mut canonical_by_id = std::collections::BTreeMap::new();
        for row in transported["model"][arena].as_array().unwrap() {
            let id = row["id"].as_str().unwrap();
            assert!(canonical_by_id.insert(id, row).is_none());
        }
        assert_eq!(snapshot_by_id, canonical_by_id, "{arena}");
        transported["model"][arena] = snapshot[arena].clone();
    }
    let parsed = crate::CadIr::from_json(&serde_json::to_string(&transported).unwrap()).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(parsed.to_canonical_json().unwrap(), canonical);
    assert_eq!(ir, before);
}
