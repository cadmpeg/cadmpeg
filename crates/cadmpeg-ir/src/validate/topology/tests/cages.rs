// SPDX-License-Identifier: Apache-2.0
//! A shell that holds a subdivision cage (§5.6, part 10).

use crate::examples::{directed_subd_sum, unit_cube};
use crate::ids::{BodyId, RegionId, ShellId, SubdId};
use crate::report::check::{Check, Finding};
use crate::topology::{Region, Shell, ShellMember};
use crate::validate::validate_neutral;
use crate::CadIr;

/// The unit cube with the cage of the directed `SubD` example, and its id.
fn cube_with_cage() -> (CadIr, SubdId) {
    let mut ir = unit_cube().unwrap();
    let cage = directed_subd_sum().unwrap().geometry.subds.remove(0);
    let id = cage.id.clone();
    ir.geometry.subds.push(cage);
    (ir, id)
}

fn findings(ir: &mut CadIr) -> Vec<Finding> {
    ir.finalize();
    validate_neutral(ir, Vec::new()).unwrap().findings
}

fn cage_findings(findings: &[Finding]) -> Vec<&str> {
    findings
        .iter()
        .filter(|finding| finding.message.contains("subdivision cage"))
        .map(|finding| finding.message.as_str())
        .collect()
}

#[test]
fn a_cage_is_the_only_member_of_its_bodys_only_shell() {
    let (mut ir, cage) = cube_with_cage();
    let mut body = ir.geometry.bodies[0].clone();
    body.id = BodyId::mint("test:model:body#cage").unwrap();
    let region = RegionId::mint("test:model:region#cage").unwrap();
    let shell = ShellId::mint("test:model:shell#cage").unwrap();
    ir.geometry.regions.push(Region::new(
        region.clone(),
        body.id.clone(),
        vec![shell.clone()],
    ));
    let mut cage_shell = Shell::new(shell, region);
    cage_shell.members.push(ShellMember::Subd { id: cage });
    ir.geometry.shells.push(cage_shell);
    ir.geometry.bodies.push(body);
    let found = findings(&mut ir);
    assert!(cage_findings(&found).is_empty(), "{found:#?}");
}

#[test]
fn a_shell_holding_a_cage_holds_nothing_else() {
    let (mut ir, cage) = cube_with_cage();
    // The cube's shell holds faces and now shares its region.
    ir.geometry.shells[0]
        .members
        .push(ShellMember::Subd { id: cage.clone() });
    let missing = SubdId::mint("test:model:subd#missing").unwrap();
    let mut second = Shell::new(
        ShellId::mint("test:model:shell#second").unwrap(),
        ir.geometry.shells[0].region.clone(),
    );
    second.members = vec![
        ShellMember::Subd { id: cage.clone() },
        ShellMember::Subd { id: cage.clone() },
        ShellMember::Subd {
            id: missing.clone(),
        },
    ];
    ir.geometry.regions[0].push_shell(second.id.clone());
    ir.geometry.shells.push(second);
    let found = findings(&mut ir);
    for (check, message) in [
        (
            Check::WireTopology,
            "subdivision cage belongs to more than one shell".to_owned(),
        ),
        (
            Check::WireTopology,
            "shell holding a subdivision cage holds faces".to_owned(),
        ),
        (
            Check::WireTopology,
            "shell holding a subdivision cage holds other members".to_owned(),
        ),
        (
            Check::WireTopology,
            "shell holding a subdivision cage is not the only shell of its body's only region"
                .to_owned(),
        ),
        (
            Check::WireTopology,
            format!("shell lists member `{}` more than once", cage.as_str()),
        ),
        (
            Check::ReferentialIntegrity,
            format!("references missing subdivision cage `{}`", missing.as_str()),
        ),
    ] {
        assert!(
            found
                .iter()
                .any(|finding| finding.check == check && finding.message == message),
            "missing `{message}` in {found:#?}"
        );
    }
}

#[test]
fn a_cage_member_round_trips_through_json() {
    let member = ShellMember::Subd {
        id: SubdId::mint("test:model:subd#0").unwrap(),
    };
    let json = serde_json::to_value(&member).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"kind": "subd", "id": "test:model:subd#0"})
    );
    assert_eq!(serde_json::from_value::<ShellMember>(json).unwrap(), member);
}
