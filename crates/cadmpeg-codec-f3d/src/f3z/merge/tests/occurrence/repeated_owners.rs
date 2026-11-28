// SPDX-License-Identifier: Apache-2.0
//! External member trees below repeated local component placements.

use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::transform::Transform;

use crate::records::xref::XrefReference;

#[test]
fn repeated_local_owners_merge_each_external_child_tree_in_its_composed_frame() {
    check_repeated_owners(2);
}

#[test]
fn three_repeated_local_owners_expand_each_member_root_once() {
    check_repeated_owners(3);
}

fn check_repeated_owners(placements: usize) {
    use crate::test_support::assembly_test::{
        f3d_without_brep_with_xref_placement, f3z_archive, XREF_ROLE,
    };
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};

    let mut child_matrix = Transform::identity().rows();
    child_matrix[1][3] = 0.5;
    let member = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "member.f3d",
        "leaf.f3d",
        XREF_ROLE,
        child_matrix,
    );
    let leaf = f3d_with_smbh(&synthetic_geometry_smbh());
    let archive = f3z_archive(
        "member.f3d",
        &[("member.f3d", &member), ("leaf.f3d", &leaf)],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, bytes) = DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, bytes).unwrap();
    let session = crate::F3zSession::open_scanned(&ctx, scan.fork()).unwrap();
    let archive = &session.archive;
    let reference = XrefReference {
        current_pose: None,
        operation_pose: None,
        contained_poses: Vec::new(),
        owned_poses: Vec::new(),
        id: "f3d:xref:reference#0".to_owned(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: None,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "member.f3d".to_owned().try_into().unwrap(),
        neutron_role: "outer-role".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };
    let table = crate::xref::XrefTable {
        references: vec![reference],
        ..Default::default()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.product_definitions.clear();
    ir.geometry.bodies[0].transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 7.0],
            [0.0, 0.0, 1.0, 11.0],
        ])
        .unwrap(),
    );
    let root_body = ir.geometry.bodies[0].clone();
    let group = cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:model:component#group").unwrap();
    let document = crate::xref::document_definition_id("member.f3d");
    ir.model.product_definitions.extend([
        ProductDefinition::new(group.clone(), Storage::Embedded),
        ProductDefinition::new(
            document.clone(),
            Storage::External(ExternalDocument::path("member.f3d")),
        ),
    ]);
    let mut children = Vec::new();
    for index in 0..placements {
        let parent = OccurrenceId::mint(format!("f3d:model:occurrence#group-{index}")).unwrap();
        let transform = if index == 0 {
            Transform::affine([
                [0.0, -1.0, 0.0, 30.0],
                [1.0, 0.0, 0.0, 40.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .unwrap()
        } else {
            Transform::affine([
                [
                    1.0,
                    0.0,
                    0.0,
                    80.0 * f64::from(u32::try_from(index).unwrap()),
                ],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .unwrap()
        };
        let mut child = Occurrence::new(
            crate::ids::nested_xref_occurrence_id(&parent, 0, 0),
            document.clone(),
            Transform::affine([
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ]),
        );
        children.push(child.id.clone());
        child.native_ref = Some(table.references[0].id.clone());
        let child_id = child.id.clone();
        ir.model.occurrences.extend([
            Occurrence::new(parent.clone(), group.clone(), Some(transform)),
            child,
        ]);
        crate::tree::place_top_level(&mut ir.model, parent.clone());
        crate::tree::place_under(&mut ir.model, Some(&parent), child_id);
    }
    let mut report =
        cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::at(
            cadmpeg_core::decode::DecodeDepth::Full,
            false,
        ));
    let mut fidelity = cadmpeg_ir::SourceFidelity::new(Sha256Digest::digest(b""));
    let mut instance_scopes = crate::f3z::merge::InstanceScopes::new();
    let merged = crate::f3z::merge::MergeSession {
        ctx: &ctx,
        scan: &scan,
        archive,
        session: &session,
        decoded: &mut std::collections::BTreeMap::new(),
        stack: vec!["root.f3d".to_owned()],
        members: std::collections::BTreeMap::new(),
        failed: std::collections::BTreeSet::new(),
        scene_sources: &mut crate::scene::Sources::new(),
        operation_poses: std::collections::BTreeMap::new(),
        operation_protected: std::collections::BTreeSet::new(),
        component_path_tables: Vec::new(),
        instance_scopes: &mut instance_scopes,
        joint_roots: crate::design_v11::joint::RootOperands::new(),
    }
    .merge(&mut ir, &mut report, &mut fidelity, &table)
    .unwrap();
    assert_eq!(merged, 2 * placements);
    let member_definitions = children
        .iter()
        .map(|child| {
            ir.model
                .occurrences
                .iter()
                .find(|item| &item.id == child)
                .unwrap()
                .prototype
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(member_definitions
        .iter()
        .all(|definition| definition == &member_definitions[0]));
    let member_roots = crate::tree::listed(&ir.model, &member_definitions[0]);
    assert_eq!(member_roots.len(), placements);
    let expanded = crate::tree::expand(
        &cadmpeg_core::decode::ResourceLimits {
            max_entities: u64::try_from(ir.model.occurrences.len()).unwrap(),
            ..Default::default()
        },
        &ir.model.product_definitions,
        &ir.model.occurrences,
        &instance_scopes,
    )
    .unwrap();
    for root in member_roots {
        assert_eq!(
            expanded
                .occurrence_of
                .values()
                .filter(|occurrence| *occurrence == root)
                .count(),
            1
        );
    }
    assert_eq!(expanded.occurrences.len(), ir.model.occurrences.len());
    crate::tree::own_once(&ctx, &mut ir, &instance_scopes).unwrap();
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_eq!(ir.geometry.bodies[0], root_body);
    assert!(ir.geometry.bodies[1].transform.is_none());
    let leaf_definition = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| !definition.bodies.is_empty())
        .unwrap();
    assert_eq!(leaf_definition.bodies, [ir.geometry.bodies[1].id.clone()]);
    let external = crate::ids::neutral_xref_occurrence_id(0, 0);
    assert_eq!(crate::tree::listed(&ir.model, &group), [&external]);
    let placing = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id == external)
        .unwrap();
    assert_eq!(placing.transform.unwrap().rows()[0][3], 20.0);
    let imported = crate::tree::listed(&ir.model, &placing.prototype);
    assert_eq!(imported.len(), 1);
    let imported = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| &occurrence.id == imported[0])
        .unwrap();
    assert_eq!(imported.prototype, leaf_definition.id);
    assert_eq!(imported.transform.unwrap().rows()[1][3], 5.0);
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|occurrence| occurrence.overrides.is_empty()));
    let expanded = crate::tree::expand(
        &cadmpeg_core::decode::ResourceLimits::default(),
        &ir.model.product_definitions,
        &ir.model.occurrences,
        &std::collections::BTreeMap::new(),
    )
    .unwrap();
    let poses = crate::tree::world_poses(&ir.model);
    let mut translations = expanded
        .occurrence_of
        .iter()
        .filter(|(_, occurrence)| *occurrence == &imported.id)
        .map(|(instance, _)| {
            let rows = poses[instance]
                .compose(
                    ir.geometry.bodies[1]
                        .transform
                        .unwrap_or_else(Transform::identity),
                )
                .unwrap()
                .rows();
            [rows[0][3], rows[1][3]]
        })
        .collect::<Vec<_>>();
    translations.sort_by(|left, right| left.partial_cmp(right).unwrap());
    let mut expected = vec![[25.0, 60.0]];
    expected.extend(
        (1..placements).map(|index| [80.0 * f64::from(u32::try_from(index).unwrap()) + 20.0, 5.0]),
    );
    assert_eq!(translations, expected);
    let references: Vec<XrefReference> = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as_for_decode(&ctx, "xref_references")
        .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].relative_path.as_str(), "leaf.f3d");
    assert_eq!(references[0].neutron_role.as_str(), XREF_ROLE);
    assert_eq!(references[0].transform.unwrap().rows(), child_matrix);
}
