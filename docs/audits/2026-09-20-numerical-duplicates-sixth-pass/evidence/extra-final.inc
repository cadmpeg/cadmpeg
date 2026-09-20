f=source('cadmpeg-ir/src/math/planar.rs')
s+='mod ir_planar {use super::*;'+fn(f,'scaled_displacement')+fn(f,'line_circle_parameters')+'''
#[test]fn remote_line_invents_tangent(){let a=Point2::new(1e8,2.);let b=Point2::new(1e8+1.,2.);let got=line_circle_parameters(a,b,Point2::new(0.,0.),1.);println!("IR unit-circle vs line y2, startx1e8: {got:?}; expectedNone");assert!(got.is_some());}
#[test]fn remote_secant_inaccurate_roots(){let got=line_circle_parameters(Point2::new(1e8,0.),Point2::new(1e8+1.,0.),Point2::new(0.,0.),1.).unwrap();println!("IR unitcircle secant startx1e8: {got:?}; expected -100000001,-99999999");assert!((got[0]+100000001.).abs()>1e-3); assert!((got[1]+99999999.).abs()>1e-3);}
}
'''
s+='fn multiply_divide(a:f64,b:f64,c:f64)->Option<f64>{cadmpeg_ir::math::multiply_divide(a,b,c)}\n'
f=source('cadmpeg-codec-iges/src/entities/conics.rs')
exprs=re.findall(r'let focal_distance =\s*([^;]+);',fn(f,'project'));assert len(exprs)==2
s+='mod iges {use super::*;\n'
for i,expr in enumerate(exprs):
 s+=f'fn focal_{i}(coefficient:f64)->f64 {{let coeff_a=&coefficient;let coeff_c=&coefficient;let coeff_e=&-coefficient;let coeff_d=&-coefficient;let factor=1.0;let scale_x=1.0;let scale_y=1.0; {expr} }}\n'
s+='''#[test]fn parabola_common_coefficient_scale(){for focal in [focal_0,focal_1]{let ordinary=focal(1.);let large=focal(1e308);println!("IGES parabola focal ordinary={ordinary}, commoncoeff1e308={large}; expected .25 both");assert_eq!(ordinary,0.25);assert_eq!(large,0.);}}}
'''

f=source('cadmpeg-codec-rhino/src/brep.rs')
expr=re.search(r'tolerance = tolerance\s*(\.max\(\(delta\[0\][^;]+);',fn(f,'parse_legacy_major2')).group(1)
s+='mod rhino_tolerance {use super::*;fn measured(delta:[f64;3])->f64 {let tolerance=0.0_f64;tolerance'+expr+'}'+'''
#[test]fn finite_vertex_tolerance_overflow(){let got=measured([1e200,0.,0.]);println!("Rhino legacy gap1e200 yields tolerance {got}; expected finite1e200");assert!(got.is_infinite());}
}
'''
