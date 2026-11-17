// SPDX-License-Identifier: Apache-2.0
//! Refusal diagnostics for CADIR wires before the current one.

use super::IR_VERSION;

/// What a wire 10 reader must change to read the current wire.
const WIRE_10_CHANGES: &str = "wire 11 writes each enum value's variant under `kind` and a non-struct payload \
     under `value`, omits a `None` option and refuses `null`, writes every list, map, and set and every \
     default value, writes `units` with `length`, `angle`, `mass`, and `time`, always writes `fragment`, \
     holds the topology arenas under `geometry` instead of `model`, records `scope` instead of \
     `decode_depth`, and writes `design` and `process`";

/// The refusal of a document whose `ir_version` is not [`IR_VERSION`].
pub(super) fn unsupported_version(version: Option<&str>) -> String {
    match version {
        Some("10") => {
            format!("unsupported ir_version \"10\"; expected {IR_VERSION}; {WIRE_10_CHANGES}")
        }
        _ => format!("unsupported ir_version {version:?}; expected {IR_VERSION}"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::super::CadIr;

    /// `examples::unit_cube` as the wire 10 writer wrote it.
    const WIRE_10_UNIT_CUBE: &str = concat!(
        r#"{"ir_version":"10","decode_depth":"full","units":{"length":"millimeter"},"tolerances":{""#,
        r#"linear":1e-6,"angular":1e-10},"model":{"bodies":[{"id":"synthetic:cube:body#0","kind":"s"#,
        r#"olid","regions":["synthetic:cube:region#0"],"name":"unit cube"}],"regions":[{"id":"synth"#,
        r#"etic:cube:region#0","body":"synthetic:cube:body#0","shells":["synthetic:cube:shell#0"]}]"#,
        r#","shells":[{"id":"synthetic:cube:shell#0","region":"synthetic:cube:region#0","members":["#,
        r#"{"kind":"face","id":"synthetic:cube:face#bottom"},{"kind":"face","id":"synthetic:cube:fa"#,
        r#"ce#top"},{"kind":"face","id":"synthetic:cube:face#front"},{"kind":"face","id":"synthetic"#,
        r#":cube:face#right"},{"kind":"face","id":"synthetic:cube:face#back"},{"kind":"face","id":""#,
        r#"synthetic:cube:face#left"}]}],"faces":[{"id":"synthetic:cube:face#back","shell":"synthet"#,
        r#"ic:cube:shell#0","surface":"synthetic:cube:surface#back","sense":"forward","loops":{"cla"#,
        r#"ssification":"unspecified","loops":["synthetic:cube:loop#back"]},"name":"back face"},{"i"#,
        r#"d":"synthetic:cube:face#bottom","shell":"synthetic:cube:shell#0","surface":"synthetic:cu"#,
        r#"be:surface#bottom","sense":"forward","loops":{"classification":"unspecified","loops":["s"#,
        r#"ynthetic:cube:loop#bottom"]},"name":"bottom face"},{"id":"synthetic:cube:face#front","sh"#,
        r#"ell":"synthetic:cube:shell#0","surface":"synthetic:cube:surface#front","sense":"forward""#,
        r#","loops":{"classification":"unspecified","loops":["synthetic:cube:loop#front"]},"name":""#,
        r#"front face"},{"id":"synthetic:cube:face#left","shell":"synthetic:cube:shell#0","surface""#,
        r#":"synthetic:cube:surface#left","sense":"forward","loops":{"classification":"unspecified""#,
        r#","loops":["synthetic:cube:loop#left"]},"name":"left face"},{"id":"synthetic:cube:face#ri"#,
        r#"ght","shell":"synthetic:cube:shell#0","surface":"synthetic:cube:surface#right","sense":""#,
        r#"forward","loops":{"classification":"unspecified","loops":["synthetic:cube:loop#right"]},"#,
        r#""name":"right face"},{"id":"synthetic:cube:face#top","shell":"synthetic:cube:shell#0","s"#,
        r#"urface":"synthetic:cube:surface#top","sense":"forward","loops":{"classification":"unspec"#,
        r#"ified","loops":["synthetic:cube:loop#top"]},"name":"top face"}],"loops":[{"id":"syntheti"#,
        r#"c:cube:loop#back","face":"synthetic:cube:face#back","boundary":{"kind":"ring","coedges":"#,
        r#"["synthetic:cube:coedge#back:0","synthetic:cube:coedge#back:1","synthetic:cube:coedge#ba"#,
        r#"ck:2","synthetic:cube:coedge#back:3"]}},{"id":"synthetic:cube:loop#bottom","face":"synth"#,
        r#"etic:cube:face#bottom","boundary":{"kind":"ring","coedges":["synthetic:cube:coedge#botto"#,
        r#"m:0","synthetic:cube:coedge#bottom:1","synthetic:cube:coedge#bottom:2","synthetic:cube:c"#,
        r#"oedge#bottom:3"]}},{"id":"synthetic:cube:loop#front","face":"synthetic:cube:face#front","#,
        r#""boundary":{"kind":"ring","coedges":["synthetic:cube:coedge#front:0","synthetic:cube:coe"#,
        r#"dge#front:1","synthetic:cube:coedge#front:2","synthetic:cube:coedge#front:3"]}},{"id":"s"#,
        r#"ynthetic:cube:loop#left","face":"synthetic:cube:face#left","boundary":{"kind":"ring","co"#,
        r#"edges":["synthetic:cube:coedge#left:0","synthetic:cube:coedge#left:1","synthetic:cube:co"#,
        r#"edge#left:2","synthetic:cube:coedge#left:3"]}},{"id":"synthetic:cube:loop#right","face":"#,
        r#""synthetic:cube:face#right","boundary":{"kind":"ring","coedges":["synthetic:cube:coedge#"#,
        r#"right:0","synthetic:cube:coedge#right:1","synthetic:cube:coedge#right:2","synthetic:cube"#,
        r#":coedge#right:3"]}},{"id":"synthetic:cube:loop#top","face":"synthetic:cube:face#top","bo"#,
        r#"undary":{"kind":"ring","coedges":["synthetic:cube:coedge#top:0","synthetic:cube:coedge#t"#,
        r#"op:1","synthetic:cube:coedge#top:2","synthetic:cube:coedge#top:3"]}}],"coedges":[{"id":""#,
        r#"synthetic:cube:coedge#back:0","owner_loop":"synthetic:cube:loop#back","edge":"synthetic:"#,
        r#"cube:edge#2","radial_next":"synthetic:cube:coedge#bottom:2","sense":"forward"},{"id":"sy"#,
        r#"nthetic:cube:coedge#back:1","owner_loop":"synthetic:cube:loop#back","edge":"synthetic:cu"#,
        r#"be:edge#11","radial_next":"synthetic:cube:coedge#left:3","sense":"forward"},{"id":"synth"#,
        r#"etic:cube:coedge#back:2","owner_loop":"synthetic:cube:loop#back","edge":"synthetic:cube:"#,
        r#"edge#6","radial_next":"synthetic:cube:coedge#top:3","sense":"reversed"},{"id":"synthetic"#,
        r#":cube:coedge#back:3","owner_loop":"synthetic:cube:loop#back","edge":"synthetic:cube:edge"#,
        r#"#10","radial_next":"synthetic:cube:coedge#right:1","sense":"reversed"},{"id":"synthetic:"#,
        r#"cube:coedge#bottom:0","owner_loop":"synthetic:cube:loop#bottom","edge":"synthetic:cube:e"#,
        r#"dge#0","radial_next":"synthetic:cube:coedge#front:0","sense":"reversed"},{"id":"syntheti"#,
        r#"c:cube:coedge#bottom:1","owner_loop":"synthetic:cube:loop#bottom","edge":"synthetic:cube"#,
        r#":edge#3","radial_next":"synthetic:cube:coedge#left:0","sense":"reversed"},{"id":"synthet"#,
        r#"ic:cube:coedge#bottom:2","owner_loop":"synthetic:cube:loop#bottom","edge":"synthetic:cub"#,
        r#"e:edge#2","radial_next":"synthetic:cube:coedge#back:0","sense":"reversed"},{"id":"synthe"#,
        r#"tic:cube:coedge#bottom:3","owner_loop":"synthetic:cube:loop#bottom","edge":"synthetic:cu"#,
        r#"be:edge#1","radial_next":"synthetic:cube:coedge#right:0","sense":"reversed"},{"id":"synt"#,
        r#"hetic:cube:coedge#front:0","owner_loop":"synthetic:cube:loop#front","edge":"synthetic:cu"#,
        r#"be:edge#0","radial_next":"synthetic:cube:coedge#bottom:0","sense":"forward"},{"id":"synt"#,
        r#"hetic:cube:coedge#front:1","owner_loop":"synthetic:cube:loop#front","edge":"synthetic:cu"#,
        r#"be:edge#9","radial_next":"synthetic:cube:coedge#right:3","sense":"forward"},{"id":"synth"#,
        r#"etic:cube:coedge#front:2","owner_loop":"synthetic:cube:loop#front","edge":"synthetic:cub"#,
        r#"e:edge#4","radial_next":"synthetic:cube:coedge#top:1","sense":"reversed"},{"id":"synthet"#,
        r#"ic:cube:coedge#front:3","owner_loop":"synthetic:cube:loop#front","edge":"synthetic:cube:"#,
        r#"edge#8","radial_next":"synthetic:cube:coedge#left:1","sense":"reversed"},{"id":"syntheti"#,
        r#"c:cube:coedge#left:0","owner_loop":"synthetic:cube:loop#left","edge":"synthetic:cube:edg"#,
        r#"e#3","radial_next":"synthetic:cube:coedge#bottom:1","sense":"forward"},{"id":"synthetic:"#,
        r#"cube:coedge#left:1","owner_loop":"synthetic:cube:loop#left","edge":"synthetic:cube:edge#"#,
        r#"8","radial_next":"synthetic:cube:coedge#front:3","sense":"forward"},{"id":"synthetic:cub"#,
        r#"e:coedge#left:2","owner_loop":"synthetic:cube:loop#left","edge":"synthetic:cube:edge#7","#,
        r#""radial_next":"synthetic:cube:coedge#top:0","sense":"reversed"},{"id":"synthetic:cube:co"#,
        r#"edge#left:3","owner_loop":"synthetic:cube:loop#left","edge":"synthetic:cube:edge#11","ra"#,
        r#"dial_next":"synthetic:cube:coedge#back:1","sense":"reversed"},{"id":"synthetic:cube:coed"#,
        r#"ge#right:0","owner_loop":"synthetic:cube:loop#right","edge":"synthetic:cube:edge#1","rad"#,
        r#"ial_next":"synthetic:cube:coedge#bottom:3","sense":"forward"},{"id":"synthetic:cube:coed"#,
        r#"ge#right:1","owner_loop":"synthetic:cube:loop#right","edge":"synthetic:cube:edge#10","ra"#,
        r#"dial_next":"synthetic:cube:coedge#back:3","sense":"forward"},{"id":"synthetic:cube:coedg"#,
        r#"e#right:2","owner_loop":"synthetic:cube:loop#right","edge":"synthetic:cube:edge#5","radi"#,
        r#"al_next":"synthetic:cube:coedge#top:2","sense":"reversed"},{"id":"synthetic:cube:coedge#"#,
        r#"right:3","owner_loop":"synthetic:cube:loop#right","edge":"synthetic:cube:edge#9","radial"#,
        r#"_next":"synthetic:cube:coedge#front:1","sense":"reversed"},{"id":"synthetic:cube:coedge#"#,
        r#"top:0","owner_loop":"synthetic:cube:loop#top","edge":"synthetic:cube:edge#7","radial_nex"#,
        r#"t":"synthetic:cube:coedge#left:2","sense":"forward"},{"id":"synthetic:cube:coedge#top:1""#,
        r#","owner_loop":"synthetic:cube:loop#top","edge":"synthetic:cube:edge#4","radial_next":"sy"#,
        r#"nthetic:cube:coedge#front:2","sense":"forward"},{"id":"synthetic:cube:coedge#top:2","own"#,
        r#"er_loop":"synthetic:cube:loop#top","edge":"synthetic:cube:edge#5","radial_next":"synthet"#,
        r#"ic:cube:coedge#right:2","sense":"forward"},{"id":"synthetic:cube:coedge#top:3","owner_lo"#,
        r#"op":"synthetic:cube:loop#top","edge":"synthetic:cube:edge#6","radial_next":"synthetic:cu"#,
        r#"be:coedge#back:2","sense":"forward"}],"edges":[{"id":"synthetic:cube:edge#0","carrier":{"#,
        r#""curve":"synthetic:cube:curve#0","param_range":[0.0,10.0]},"start":"synthetic:cube:verte"#,
        r#"x#0","end":"synthetic:cube:vertex#1"},{"id":"synthetic:cube:edge#1","carrier":{"curve":""#,
        r#"synthetic:cube:curve#1","param_range":[0.0,10.0]},"start":"synthetic:cube:vertex#1","end"#,
        r#"":"synthetic:cube:vertex#2"},{"id":"synthetic:cube:edge#10","carrier":{"curve":"syntheti"#,
        r#"c:cube:curve#10","param_range":[0.0,10.0]},"start":"synthetic:cube:vertex#2","end":"synt"#,
        r#"hetic:cube:vertex#6"},{"id":"synthetic:cube:edge#11","carrier":{"curve":"synthetic:cube:"#,
        r#"curve#11","param_range":[0.0,10.0]},"start":"synthetic:cube:vertex#3","end":"synthetic:c"#,
        r#"ube:vertex#7"},{"id":"synthetic:cube:edge#2","carrier":{"curve":"synthetic:cube:curve#2""#,
        r#","param_range":[0.0,10.0]},"start":"synthetic:cube:vertex#2","end":"synthetic:cube:verte"#,
        r#"x#3"},{"id":"synthetic:cube:edge#3","carrier":{"curve":"synthetic:cube:curve#3","param_r"#,
        r#"ange":[0.0,10.0]},"start":"synthetic:cube:vertex#3","end":"synthetic:cube:vertex#0"},{"i"#,
        r#"d":"synthetic:cube:edge#4","carrier":{"curve":"synthetic:cube:curve#4","param_range":[0."#,
        r#"0,10.0]},"start":"synthetic:cube:vertex#4","end":"synthetic:cube:vertex#5"},{"id":"synth"#,
        r#"etic:cube:edge#5","carrier":{"curve":"synthetic:cube:curve#5","param_range":[0.0,10.0]},"#,
        r#""start":"synthetic:cube:vertex#5","end":"synthetic:cube:vertex#6"},{"id":"synthetic:cube"#,
        r#":edge#6","carrier":{"curve":"synthetic:cube:curve#6","param_range":[0.0,10.0]},"start":""#,
        r#"synthetic:cube:vertex#6","end":"synthetic:cube:vertex#7"},{"id":"synthetic:cube:edge#7","#,
        r#""carrier":{"curve":"synthetic:cube:curve#7","param_range":[0.0,10.0]},"start":"synthetic"#,
        r#":cube:vertex#7","end":"synthetic:cube:vertex#4"},{"id":"synthetic:cube:edge#8","carrier""#,
        r#":{"curve":"synthetic:cube:curve#8","param_range":[0.0,10.0]},"start":"synthetic:cube:ver"#,
        r#"tex#0","end":"synthetic:cube:vertex#4"},{"id":"synthetic:cube:edge#9","carrier":{"curve""#,
        r#":"synthetic:cube:curve#9","param_range":[0.0,10.0]},"start":"synthetic:cube:vertex#1","e"#,
        r#"nd":"synthetic:cube:vertex#5"}],"vertices":[{"id":"synthetic:cube:vertex#0","point":"syn"#,
        r#"thetic:cube:point#0"},{"id":"synthetic:cube:vertex#1","point":"synthetic:cube:point#1"},"#,
        r#"{"id":"synthetic:cube:vertex#2","point":"synthetic:cube:point#2"},{"id":"synthetic:cube:"#,
        r#"vertex#3","point":"synthetic:cube:point#3"},{"id":"synthetic:cube:vertex#4","point":"syn"#,
        r#"thetic:cube:point#4"},{"id":"synthetic:cube:vertex#5","point":"synthetic:cube:point#5"},"#,
        r#"{"id":"synthetic:cube:vertex#6","point":"synthetic:cube:point#6"},{"id":"synthetic:cube:"#,
        r#"vertex#7","point":"synthetic:cube:point#7"}],"points":[{"id":"synthetic:cube:point#0","p"#,
        r#"osition":{"x":0.0,"y":0.0,"z":0.0}},{"id":"synthetic:cube:point#1","position":{"x":10.0,"#,
        r#""y":0.0,"z":0.0}},{"id":"synthetic:cube:point#2","position":{"x":10.0,"y":10.0,"z":0.0}}"#,
        r#",{"id":"synthetic:cube:point#3","position":{"x":0.0,"y":10.0,"z":0.0}},{"id":"synthetic:"#,
        r#"cube:point#4","position":{"x":0.0,"y":0.0,"z":10.0}},{"id":"synthetic:cube:point#5","pos"#,
        r#"ition":{"x":10.0,"y":0.0,"z":10.0}},{"id":"synthetic:cube:point#6","position":{"x":10.0,"#,
        r#""y":10.0,"z":10.0}},{"id":"synthetic:cube:point#7","position":{"x":0.0,"y":10.0,"z":10.0"#,
        r#"}}],"surfaces":[{"id":"synthetic:cube:surface#back","geometry":{"kind":"plane","origin":"#,
        r#"{"x":0.0,"y":10.0,"z":0.0},"normal":{"x":0.0,"y":1.0,"z":0.0},"u_axis":{"x":1.0,"y":0.0,"#,
        r#""z":0.0}}},{"id":"synthetic:cube:surface#bottom","geometry":{"kind":"plane","origin":{"x"#,
        r#"":0.0,"y":0.0,"z":0.0},"normal":{"x":0.0,"y":0.0,"z":-1.0},"u_axis":{"x":1.0,"y":0.0,"z""#,
        r#":0.0}}},{"id":"synthetic:cube:surface#front","geometry":{"kind":"plane","origin":{"x":0."#,
        r#"0,"y":0.0,"z":0.0},"normal":{"x":0.0,"y":-1.0,"z":0.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0"#,
        r#"}}},{"id":"synthetic:cube:surface#left","geometry":{"kind":"plane","origin":{"x":0.0,"y""#,
        r#":0.0,"z":0.0},"normal":{"x":-1.0,"y":0.0,"z":0.0},"u_axis":{"x":0.0,"y":1.0,"z":0.0}}},{"#,
        r#""id":"synthetic:cube:surface#right","geometry":{"kind":"plane","origin":{"x":10.0,"y":0."#,
        r#"0,"z":0.0},"normal":{"x":1.0,"y":0.0,"z":0.0},"u_axis":{"x":0.0,"y":1.0,"z":0.0}}},{"id""#,
        r#":"synthetic:cube:surface#top","geometry":{"kind":"plane","origin":{"x":0.0,"y":0.0,"z":1"#,
        r#"0.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}}],"curves":["#,
        r#"{"id":"synthetic:cube:curve#0","geometry":{"kind":"line","origin":{"x":0.0,"y":0.0,"z":0"#,
        r#".0},"direction":{"x":1.0,"y":0.0,"z":0.0}}},{"id":"synthetic:cube:curve#1","geometry":{""#,
        r#"kind":"line","origin":{"x":10.0,"y":0.0,"z":0.0},"direction":{"x":0.0,"y":1.0,"z":0.0}}}"#,
        r#",{"id":"synthetic:cube:curve#10","geometry":{"kind":"line","origin":{"x":10.0,"y":10.0,""#,
        r#"z":0.0},"direction":{"x":0.0,"y":0.0,"z":1.0}}},{"id":"synthetic:cube:curve#11","geometr"#,
        r#"y":{"kind":"line","origin":{"x":0.0,"y":10.0,"z":0.0},"direction":{"x":0.0,"y":0.0,"z":1"#,
        r#".0}}},{"id":"synthetic:cube:curve#2","geometry":{"kind":"line","origin":{"x":10.0,"y":10"#,
        r#".0,"z":0.0},"direction":{"x":-1.0,"y":0.0,"z":0.0}}},{"id":"synthetic:cube:curve#3","geo"#,
        r#"metry":{"kind":"line","origin":{"x":0.0,"y":10.0,"z":0.0},"direction":{"x":0.0,"y":-1.0,"#,
        r#""z":0.0}}},{"id":"synthetic:cube:curve#4","geometry":{"kind":"line","origin":{"x":0.0,"y"#,
        r#"":0.0,"z":10.0},"direction":{"x":1.0,"y":0.0,"z":0.0}}},{"id":"synthetic:cube:curve#5",""#,
        r#"geometry":{"kind":"line","origin":{"x":10.0,"y":0.0,"z":10.0},"direction":{"x":0.0,"y":1"#,
        r#".0,"z":0.0}}},{"id":"synthetic:cube:curve#6","geometry":{"kind":"line","origin":{"x":10."#,
        r#"0,"y":10.0,"z":10.0},"direction":{"x":-1.0,"y":0.0,"z":0.0}}},{"id":"synthetic:cube:curv"#,
        r#"e#7","geometry":{"kind":"line","origin":{"x":0.0,"y":10.0,"z":10.0},"direction":{"x":0.0"#,
        r#","y":-1.0,"z":0.0}}},{"id":"synthetic:cube:curve#8","geometry":{"kind":"line","origin":{"#,
        r#""x":0.0,"y":0.0,"z":0.0},"direction":{"x":0.0,"y":0.0,"z":1.0}}},{"id":"synthetic:cube:c"#,
        r#"urve#9","geometry":{"kind":"line","origin":{"x":10.0,"y":0.0,"z":0.0},"direction":{"x":0"#,
        r#".0,"y":0.0,"z":1.0}}}],"subds":[],"pcurves":[],"procedural_surfaces":[],"procedural_curv"#,
        r#"es":[],"features":[],"configurations":[],"parameters":[],"sketches":[],"sketch_entities""#,
        r#":[],"sketch_constraints":[],"spatial_sketches":[],"spatial_sketch_entities":[],"spreadsh"#,
        r#"eets":[],"product_definitions":[],"occurrences":[],"assembly_joints":[],"drawings":[],"s"#,
        r#"emantic_annotations":[],"presentation_documents":[],"view_presentations":[],"tessellatio"#,
        r#"ns":[],"appearances":[],"appearance_bindings":[],"attributes":[],"pmi":[],"presentation_"#,
        r#"layers":[]},"native":{}}"#,
    );

    #[test]
    fn wire_10_refusal_names_the_wire_11_changes() {
        let error = CadIr::from_json(WIRE_10_UNIT_CUBE).unwrap_err().to_string();
        assert!(
            error.contains("unsupported ir_version \"10\"; expected 11"),
            "{error}"
        );
        for change in [
            "`kind`",
            "`value`",
            "`null`",
            "list, map, and set",
            "default value",
            "`units`",
            "`fragment`",
            "`geometry`",
            "`scope`",
            "`design` and `process`",
        ] {
            assert!(error.contains(change), "{error}");
        }
    }

    #[test]
    fn earlier_wire_refusal_is_generic() {
        let document = WIRE_10_UNIT_CUBE.replacen(r#""ir_version":"10""#, r#""ir_version":"9""#, 1);
        let error = CadIr::from_json(&document).unwrap_err().to_string();
        assert!(
            error.contains("unsupported ir_version Some(\"9\"); expected 11"),
            "{error}"
        );
        assert!(!error.contains("wire 11"), "{error}");
    }
}
