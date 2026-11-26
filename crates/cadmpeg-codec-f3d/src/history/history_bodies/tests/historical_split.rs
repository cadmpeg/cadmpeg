// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::scope::DesignFeatureKind;
use std::io::{Cursor, Write};

#[test]
fn a_base_and_later_historical_split_share_one_history_body() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = Vec::new();
        put_header(&mut bytes, width);
        let flags = match width {
            RefWidth::Four => 27,
            RefWidth::Eight => 39,
        };
        bytes[flags] = 1;
        put_plain(&mut bytes, width, "asmheader", &[]);
        put_sphere_solid(&mut bytes, width, 1, 7, -1, [10.0, 0.0, 0.0]);
        put_state(&mut bytes, width, (14, -1, 1, 0), &[(6, 1), (7, 5)]);
        put_plain(
            &mut bytes,
            width,
            "body",
            &[
                (REF, -1),
                (INT, 7),
                (REF, -1),
                (REF, 2),
                (REF, -1),
                (REF, -1),
            ],
        );
        put_surface(&mut bytes, width, [0.0, 0.0, 0.0]);
        put_state(&mut bytes, width, (13, 0, 2, 1), &[]);
        put_state(
            &mut bytes,
            width,
            (12, 1, -1, 2),
            &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
        );
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let name = "FusionAssetName[Active]/Breps.BlobParts/BREP.a.smbh";
        let mut allowance = u64::MAX;
        let history = crate::history::decode(
            &ctx,
            &bytes,
            name,
            width,
            &mut allowance,
            &WorkBudget::new(1 << 20),
        )
        .unwrap()
        .unwrap();
        let mut base = feature("base");
        let mut split = feature("split");
        split.ordinal = 1;
        let mut scopes = Vec::new();
        for (feature, kind, state, previous) in [
            (&mut base, DesignFeatureKind::BaseFeature, 12, None),
            (&mut split, DesignFeatureKind::Split, 14, Some(13)),
        ] {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{state}"),
                kind,
                state,
            );
            scope
                .try_edit(|draft| {
                    draft.history_state_id = Some(i64::from(state));
                    draft.previous_history_state_id = previous;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            feature.native_ref = Some(scope.id.clone());
            scopes.push(scope);
        }
        base.evaluation.set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: DistinctMembers::try_from(vec![cadmpeg_ir::ids::BodyId::mint(
                        "f3d:brep/a.smbh/brep:entity#1",
                    )
                    .unwrap()])
                    .unwrap(),
                    native: scopes[0].id.clone(),
                },
            },
        ));
        let prefix = crate::ids::feature_input_prefix(&split.id, 13);
        split
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
                extend_tool: None,
                targets: BodySelection::historical(
                    crate::ids::history_input_state_id(&prefix),
                    vec![crate::ids::history_input_body_id(&prefix, 1)],
                    scopes[1].id.clone(),
                )
                .unwrap(),
                tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(
                    cadmpeg_ir::features::selections::FaceSelection::Unresolved,
                ),
            }));
        let bindings = scopes
            .iter()
            .map(|scope| (scope.id.clone(), history.id.clone()))
            .collect();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
        zip.start_file(name, options).unwrap();
        zip.write_all(&bytes).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            assert_eq!(container::history_breps(scan).count(), 1);
            let mut losses = Vec::new();
            let features = [base.clone(), split.clone()];
            let histories = [history.clone()];
            let (bodies, _) = fill(
                &ctx,
                scan,
                Inputs {
                    features: &features,
                    scopes: &scopes,
                    histories: &histories,
                    bindings: &bindings,
                    primary_brep: Some(name),
                    groups: &[],
                    operands: &[],
                    recipes: &[],
                },
                &[],
                Vec::new(),
                &mut losses,
            )
            .unwrap();
            assert!(losses.is_empty(), "{losses:?}");
            let [body] = bodies.as_slice() else {
                panic!("one history body: {width:?}, {bodies:?}")
            };
            assert_eq!(body.produced_by.as_ref(), Some(&base.id));
            assert_eq!(body.consumed_by.as_ref(), Some(&split.id));
            assert_eq!(
                body.state,
                HistoryStateRef {
                    ordinal: 0,
                    identifier: 12
                }
            );
            assert_eq!(body.topology.faces.len(), 1);
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
                &body.topology.surfaces[0].geometry
            else {
                panic!("sphere")
            };
            assert_eq!(
                sphere.center().get(),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
            );
        });
    }
}
