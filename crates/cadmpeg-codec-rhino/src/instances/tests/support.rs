// SPDX-License-Identifier: Apache-2.0
use crate::wire::Uuid;

pub(super) fn static_definition(
    id: [u8; 16],
    members: &[[u8; 16]],
) -> crate::instances::InstanceDefinition {
    crate::instances::InstanceDefinition {
        source_range: 0..0,
        id: Uuid::from_wire(id),
        members: members.iter().copied().map(Uuid::from_wire).collect(),
        index: None,
        name: String::new(),
        description: String::new(),
        url: String::new(),
        url_tag: String::new(),
        kind: crate::instances::DefinitionKind::Static,
        units: crate::instances::UnitDetail::new(2, 0.001, String::new())
            .expect("valid standard units"),
        linked_depth: 0,
        linked_appearance: 0,
        link: crate::instances::LinkSource::None,
    }
}

pub(super) fn install_definitions(
    scan: &mut crate::container::Scan<'_>,
    definitions: Vec<crate::instances::InstanceDefinition>,
) {
    scan.definitions.definitions = definitions;
    scan.definitions.member_object_ids = scan
        .definitions
        .definitions
        .iter()
        .flat_map(|definition| definition.members.iter().copied())
        .collect();
}
