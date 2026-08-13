// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn surface_color_search_ignores_curve_style_colors() {
    let (exchange, _) = crate::parse::parse(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=COLOUR_RGB('curve',0.,0.,1.);\
#2=CURVE_STYLE('',#1);\
#3=COLOUR_RGB('surface',1.,0.,0.);\
#4=SURFACE_STYLE_FILL_AREA(#3);\
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));\
ENDSEC;END-ISO-10303-21;",
    )
    .expect("parse style graph");
    let color = find_color(
        5,
        &exchange,
        StyleDomain::Surface,
        &mut BTreeSet::new(),
        &mut BTreeMap::new(),
        &mut Vec::new(),
        0,
    )
    .expect("surface color");
    assert_eq!(color.2.r, 1.0);
    assert_eq!(color.2.b, 0.0);
}
