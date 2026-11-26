// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::topology::BodyKind;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn written(ir: &CadIr) -> String {
    let mut output = Vec::new();
    let report = write_step(
        ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    for name in ir
        .geometry
        .bodies
        .iter()
        .filter_map(|body| body.name.as_deref())
    {
        assert!(
            report
                .losses
                .iter()
                .all(|loss| !loss.message.contains(name)),
            "{:?}",
            report.losses
        );
    }
    text
}

fn read(text: &str) -> (CadIr, DecodeReport) {
    let (ir, report, _) = StepCodec::default()
        .decode(&mut Cursor::new(text.as_bytes()), &DecodeOptions::default())
        .unwrap()
        .into_parts();
    (ir, report)
}

fn cube(kind: BodyKind, name: Option<&str>) -> CadIr {
    let mut ir = unit_cube().unwrap();
    ir.geometry.bodies[0].kind = kind;
    ir.geometry.bodies[0].name = name.map(str::to_owned);
    ir
}

fn names(ir: &CadIr) -> Vec<Option<&str>> {
    ir.geometry
        .bodies
        .iter()
        .map(|body| body.name.as_deref())
        .collect()
}

/// The `#n = ` record of `text` whose entity is `entity`.
fn record<'a>(text: &'a str, entity: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(&format!(" = {entity}(")))
        .unwrap()
}

#[test]
fn a_solid_and_a_surface_model_carry_their_body_name_both_ways() {
    for (kind, entity) in [
        (BodyKind::Solid, "MANIFOLD_SOLID_BREP"),
        (BodyKind::Sheet, "SHELL_BASED_SURFACE_MODEL"),
    ] {
        for name in ["Bracket", "it's a \\ \u{e9}t\u{e9}"] {
            let text = written(&cube(kind, Some(name)));
            let encoded = crate::strings::encode(name);
            assert!(
                record(&text, entity).contains(&format!("{entity}('{encoded}',")),
                "{text}"
            );
            assert_eq!(names(&read(&text).0), [Some(name)]);
        }
        let text = written(&cube(kind, None));
        assert!(record(&text, entity).contains(&format!("{entity}('',")));
        assert_eq!(names(&read(&text).0), [None]);
    }
}

#[test]
fn a_faceted_or_face_based_root_names_its_body() {
    for (kind, from, to) in [
        (BodyKind::Solid, "MANIFOLD_SOLID_BREP(", "FACETED_BREP("),
        (
            BodyKind::Sheet,
            "SHELL_BASED_SURFACE_MODEL(",
            "FACE_BASED_SURFACE_MODEL(",
        ),
    ] {
        let text = written(&cube(kind, Some("Bracket"))).replace(from, to);
        assert_eq!(names(&read(&text).0), [Some("Bracket")], "{to}");
    }
}

#[test]
fn a_complex_root_names_its_body_from_its_representation_item_or_its_root_partial() {
    let text = written(&cube(BodyKind::Solid, Some("Bracket")));
    let line = record(&text, "MANIFOLD_SOLID_BREP");
    let (number, rest) = line.split_once(" = ").unwrap();
    let shell = rest
        .trim_start_matches("MANIFOLD_SOLID_BREP('Bracket',")
        .trim_end_matches(");");
    for complex in [
        format!(
            "{number} = (GEOMETRIC_REPRESENTATION_ITEM()MANIFOLD_SOLID_BREP({shell})\
             REPRESENTATION_ITEM('Bracket')SOLID_MODEL()TOPOLOGICAL_REPRESENTATION_ITEM());"
        ),
        format!(
            "{number} = (GEOMETRIC_REPRESENTATION_ITEM()MANIFOLD_SOLID_BREP('Bracket',{shell})\
             SOLID_MODEL()TOPOLOGICAL_REPRESENTATION_ITEM());"
        ),
    ] {
        let (ir, _) = read(&text.replace(line, &complex));
        assert_eq!(names(&ir), [Some("Bracket")], "{complex}");
    }
}

#[test]
fn an_undecodable_root_name_is_charged_once_and_names_nothing() {
    let text = written(&cube(BodyKind::Sheet, Some("Bracket"))).replace(
        "SHELL_BASED_SURFACE_MODEL('Bracket',",
        "SHELL_BASED_SURFACE_MODEL('\\X2\\00\\X0\\',",
    );
    let (ir, report) = read(&text);
    assert_eq!(names(&ir), [None]);
    let charged = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code == StepLossCode::MetadataStringInvalid.kind()
                && loss.message.contains("body name")
        })
        .count();
    assert_eq!(charged, 1, "{:?}", report.losses);
}

#[test]
fn alias_roots_share_the_first_name_in_instance_order_and_charge_the_others() {
    let (ir, report) = read(
        std::str::from_utf8(include_bytes!(
            "../reader/topology/tests/data/br01_topology_root_identity.p21"
        ))
        .unwrap(),
    );
    let mut found = names(&ir);
    found.sort_unstable();
    assert_eq!(
        found,
        [
            Some("different root kind"),
            Some("first root"),
            Some("reversed physical order")
        ]
    );
    let dropped = report
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::TopologyAliasRootNameDropped.kind())
        .collect::<Vec<_>>();
    assert_eq!(dropped.len(), 1, "{:?}", report.losses);
    assert!(
        dropped[0].message.contains("#33")
            && dropped[0].message.contains("'alias'")
            && dropped[0].message.contains("'first root'"),
        "{}",
        dropped[0].message
    );
}

#[test]
fn a_decoded_solid_with_voids_keeps_its_name_when_written() {
    let (ir, _) = read(
        std::str::from_utf8(include_bytes!(
            "../reader/topology/tests/data/br02_outer_void_roles.p21"
        ))
        .unwrap(),
    );
    assert_eq!(names(&ir), [Some("outer and voids")]);
    let text = written(&ir);
    assert!(
        text.lines()
            .any(|line| line.contains(" = SHELL_BASED_SURFACE_MODEL('outer and voids',")),
        "{text}"
    );
}
