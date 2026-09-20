from pathlib import Path
import re,json,hashlib,subprocess,sys
OUT=Path(sys.argv[1]) if len(sys.argv)>1 else Path('/tmp/cadmpeg-numeric-audit-4/probes'); OUT.mkdir(parents=True,exist_ok=True)
manifest=[]
def read(path):
 p=Path('crates')/path; b=p.read_bytes(); manifest.append({'path':str(p),'sha256':hashlib.sha256(b).hexdigest()});return b.decode()
def fn(s,name):
 m=re.search(r'(?m)^\s*(?:pub(?:\([^\n]*\))? )?fn '+name+r'\b',s);assert m,name
 a=s.index('{',m.start());i=a+1;depth=1
 while depth:depth+=(s[i]=='{')-(s[i]=='}');i+=1
 return re.sub(r'^pub\([^)]*\) ', '',s[m.start():i].strip())+'\n'
s='''#![allow(dead_code,unused_imports)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{SurfaceGeometry,SolvedSurfaceGeometry,nurbs::NurbsSurface};
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::sketches::{SketchGeometry,SketchGeometryDefinition,SpatialSketchGeometry,SpatialSketchGeometryDefinition};
use cadmpeg_ir::scalar::{Angle,Length};
fn line(a:[f64;2],b:[f64;2])->SketchGeometry {SketchGeometryDefinition::Line {start:Point2::new(a[0],a[1]),end:Point2::new(b[0],b[1])}.try_into().unwrap()}
'''
ir=read('cadmpeg-ir/src/eval.rs');ss=read('cadmpeg-ir/src/math/sum.rs');s+='mod math {pub use cadmpeg_ir::math::*;pub mod sum {'+ss[:ss.rindex('#[cfg(test)]')].replace('//!','//')+'}}\n'
s+='mod ir {use super::*;use crate::math::sum::ExactSignedSum;#[derive(Debug,Clone,Copy)]struct ScalarSweepDifferential{value:f64,derivative:f64}\n'+fn(ir,'scalar_unary_sweep_law_differential')+fn(ir,'finite_sweep_differential')+'''
#[test]fn remaining_chain_rules(){for (op,x,dx) in [("LN",1e-310,1e-310),("COT",1e-200,1e-200),("CSC",1e-200,1e-200),("ARCSECH",1e-310,1e-310),("EXP",-750.,1e300)]{let got=scalar_unary_sweep_law_differential(op,ScalarSweepDifferential{value:x,derivative:dx});println!("{op} x={x:e} dx={dx:e}: {got:?}");assert!(got.is_none_or(|d|d.derivative==0.));}}
}
'''
c=read('cadmpeg-codec-catia/src/families/standard/decode.rs')
s+='mod catia {use super::*;fn point_on_nurbs_surface(_:Point3,_:&NurbsSurface)->Option<bool>{unreachable!()}'+fn(c,'point_on_surface_if_supported')+'''
#[test]fn axial_cancellation(){let sf=SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(CylinderSurface::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.),1.).unwrap()));for z in [0.,1e8]{let got=point_on_surface_if_supported(Point3::new(1.,0.,z),&sf);println!("CATIA radius-one cylinder, exact point (1,0,{z}): {got:?}");assert_eq!(got,Some(z==0.));}}
}
'''
f=read('cadmpeg-codec-f3d/src/design/dimensions.rs');g=read('cadmpeg-codec-f3d/src/design/geometry.rs')
s+='mod f3d {use super::*;const EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9:f64=1e-9;const EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9:f64=1e-9;const EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9:f64=1e-9;'+''.join(fn(f,n) for n in ['spatial_point_distance_matches','point_lies_on_sketch_geometry','sketch_points_close'])+fn(g,'angle_in_sweep')+'''
#[test]fn incorrect_spatial_dimension(){let p=|x|SpatialSketchGeometryDefinition::Point{position:Point3::new(x,0.,0.)}.try_into().unwrap();let got=spatial_point_distance_matches(&p(0.),&p(1e200),1.);println!("F3D 1e200 separation accepted for dimension 1: {got}");assert!(got);}
#[test]fn short_line_membership(){let got=point_lies_on_sketch_geometry(Point2::new(0.5e-6,1e-4),&line([0.,0.],[1e-6,0.]));println!("F3D point 1e-4 off 1e-6 segment, tolerance 1e-9: accepted={got}");assert!(got);}
#[test]fn remote_ellipse_membership(){let e=SketchGeometryDefinition::Ellipse{center:Point2::new(0.,0.),major_angle:Angle::new(0.).unwrap(),major_radius:Length::new(1.).unwrap(),minor_radius:Length::new(0.5).unwrap(),bounds:None}.try_into().unwrap();let got=point_lies_on_sketch_geometry(Point2::new(1e200,0.),&e);println!("F3D unit ellipse accepts (1e200,0): {got}");assert!(got);}
}
'''
c=read('cadmpeg-codec-creo/src/decode/sweep/profiles.rs')
s+='mod creo {use super::*;'+fn(c,'segments_intersect')+'''
#[test]fn small_crossing_segments(){for a in [1.,1e-5]{let got=segments_intersect([[-a,0.],[a,0.]],[[0.,-a],[0.,a]],1e-9);println!("Creo perpendicular segment half-length={a:e}: intersect={got}");assert_eq!(got,a==1.);}}
}
'''
n=read('cadmpeg-codec-nx/src/decode/offset.rs')
s+='mod nx {use super::*;const EPS_OFFSET_SOLVE_DAMPED_LEAST_SQUARES_4X4_E12:f64=1e-12;'+fn(n,'solve_4x4')+fn(n,'solve_damped_least_squares_4x4')+'''
#[test]fn scaled_rank_deficient_solve(){for a in [1.,1e-200,1e200]{let got=solve_damped_least_squares_4x4([[a,0.,0.,0.],[0.,a,0.,0.],[0.,0.,a,0.],[0.,0.,0.,0.]],[a,a,a,0.]);println!("NX rank-three consistent system scale={a:e}: {got:?}; exact step=(1,1,1,0)");assert_eq!(got.is_some(),a==1.);}}
}
'''
h=read('cadmpeg-codec-sldprt/src/resolved_features/helix.rs')
s+='mod sld {use super::*;const HELIX_MAX_RELATIVE_RESIDUAL:f64=5e-4;'+''.join(fn(h,n) for n in ['fit_helix_polyline','fit_circle_on_axis','solve_three','subtract_axis','solve_four'])+'''
#[test]fn small_helix(){for r in [1.,1e-8]{let pts=(0..=16).map(|i|{let t=i as f64/16.;let angle=t*std::f64::consts::TAU;Point3::new(r*angle.cos(),r*angle.sin(),r*t)}).collect::<Vec<_>>();let got=fit_helix_polyline(&pts,1.,false);println!("SLD exact helix radius={r:e}: {got:?}");assert_eq!(got.is_some(),r==1.);}}
}
'''
# Exact expression-level probes for large owner functions, not complete codec execution.
ircheck=read('cadmpeg-ir/src/validate/sketches.rs')
start=ircheck.index('((second.x - first.x).powi(2)'); block=ircheck[start:ircheck.index('SpatialConstraint::PointLineDistance',start)]
measurement=re.search(r'(\(\(second.x.*?\.sqrt\(\))',block,re.S).group(1)
comparison=re.search(r'let scale = 1.0 \+ measured.max\(expected\);\s*([^\n]+)',block).group(1)
s+='mod expressions {use super::*;const EPS_SKETCHES_CHECK_SKETCHES_E9:f64=1e-9;\n#[test]fn ir_distance_admission(){let first=Point3::new(0.,0.,0.);let second=Point3::new(1e200,0.,0.);let expected=1.;let measured='+measurement+';let scale=1.0+measured.max(expected);let got='+comparison+';println!("IR spatial separation=1e200 expected=1 measured={measured} accepted={got}");assert!(got);}\n'
sl=read('cadmpeg-codec-sldprt/src/resolved_features/transforms.rs');expr=re.search(r'let radius_key = (.*);',sl).group(1)
s+='#[test]fn radius_keys(){let key=|radius:f64|{let quantum=1e-6;'+expr+'};let a=key(1e14);let b=key(2e14);println!("SLD radius keys for 1e14 and 2e14: {a}, {b}");assert_eq!(a,b);}\n'
fc=read('cadmpeg-codec-freecad/src/design.rs');expr=re.search(r'let direction_magnitude = raw_direction.map\(\|direction\| \{(.*?)\}\);',fc,re.S).group(1)
s+='#[test]fn freecad_default_extrusion_length(){for x in [1e200]{let direction=Vector3::new(x,0.,0.);let got='+expr+';println!("FreeCAD extrusion Dir=({x:e},0,0), magnitude={got:e}");assert_ne!(got,x);assert!(direction.unit().is_some());}}\n'
ig=read('cadmpeg-codec-iges/src/writer.rs');assert 'number(1.0 / (major_radius * major_radius))' in ig
s+='''#[test]fn iges_conic_coefficients(){for major_radius in [1e-200_f64,1e200]{let got=1.0/(major_radius*major_radius);println!("IGES conic radius={major_radius:e} coefficient={got}");assert!(got==0.||!got.is_finite());}}
}
'''

s+='mod ir_more {use super::*;use cadmpeg_ir::transform::Transform;'+''.join(fn(ir,n) for n in ['affine_point','affine_vector','affine_orientation','polyline_point','polyline_tangent'])+'''
#[test]fn affine_cancel(){let tr=Transform::affine([[1e308,-1e308,1.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.]]).unwrap();let p=Point3::new(2.,2.,3.);let got=affine_point(tr,p);let robust=tr.apply_point(p).unwrap();println!("IR evaluator affine point={got:?}; shared Transform={robust:?}");assert!(!got.is_finite());assert_eq!(robust.x,3.);let v=affine_vector(tr,Vector3::new(2.,2.,3.));assert!(!v.is_finite());}
#[test]fn reflection_orientation(){for a in [1.,1e-200,1e200]{let tr=Transform::affine([[-a,0.,0.,0.],[0.,a,0.,0.],[0.,0.,a,0.]]).unwrap();let got=affine_orientation(tr);println!("IR scaled reflection scale={a:e}: orientation={got}, expected=-1");assert_eq!(got,if a==1.{-1.}else{1.});}}
#[test]fn polyline_interpolation(){let p=[Point3::new(-1e308,0.,0.),Point3::new(1e308,0.,0.)];let got=polyline_point(&p,&[0.,1.],0.5);println!("IR symmetric polyline midpoint={got:?}, expected origin");assert!(got.is_some_and(|p|!p.is_finite()));let q=polyline_point(&[Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],&[-1e308,1e308],0.);println!("IR finite wide-domain midpoint={q:?}, expected (0.5,0,0)");assert!(q.is_none());}
#[test]fn polar_scale(){use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,PolarHarmonicPcurve};for a in [1.,1e-200,1e200]{let g=PcurveGeometry::PolarHarmonic(PolarHarmonicPcurve::try_new(Point2::new(0.,0.),Point2::new(a,0.),Point2::new(0.,a),0.,0.,0.).unwrap());let p=cadmpeg_ir::eval::pcurve_uv(&g,0.);let d=cadmpeg_ir::eval::pcurve_tangent(&g,0.);println!("IR polar harmonic scale={a:e}: point={p:?}, tangent={d:?}; expected (0,0), (1,0)");assert_eq!(d.is_some(),a==1.);}}
#[test]fn great_circle_derivative(){use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,SphericalGreatCirclePcurve};let g=PcurveGeometry::SphericalGreatCircle(SphericalGreatCirclePcurve::try_new(0.,1.,0.,1e200).unwrap());let d=cadmpeg_ir::eval::pcurve_tangent(&g,0.5).unwrap();println!("IR great-circle slope=1e200 at 0.5: tangent={d:?}; latitude derivative ~-6.225e-201");assert_eq!(d.v,0.);}
}
'''
ps=read('cadmpeg-ir/src/math/planar.rs')
s+='mod planar {use super::*;'+fn(ps,'line_circle_parameters')+fn(ps,'circle_intersections')+'''
#[test]fn finite_intersections_lost(){let roots=line_circle_parameters(Point2::new(0.,0.),Point2::new(1e-200,0.),Point2::new(0.,0.),1.);println!("IR tiny line at unit-circle center: roots={roots:?}, expected +/-1e200");assert!(roots.is_none());let tangent=circle_intersections(Point2::new(-1e308,0.),1e308,Point2::new(1e308,0.),1e308);println!("IR radius 1e308 externally tangent circles: {tangent:?}, expected origin");assert!(tangent.is_none());}
}
'''
src=read('cadmpeg-codec-iges/src/entities/evaluation.rs')
s+='mod iges_eval {use super::*;use cadmpeg_core::decode::alloc_filled;use cadmpeg_ir::geometry::{pcurve::{PcurveGeometry,ParabolaPcurve,PcurveNurbs,PcurveNurbsPoles},CurveGeometry,SolvedCurveGeometry};'+''.join(fn(src,n) for n in ['basis','pcurve','curve'])+'''
#[test]fn parabola_parameterization(){let g=PcurveGeometry::Parabola(ParabolaPcurve::try_new(Point2::new(0.,0.),Point2::new(1.,0.),Point2::new(0.,1.),2.).unwrap());let got=pcurve(&g,1.).unwrap();let neutral=cadmpeg_ir::eval::pcurve_uv(&g,1.).unwrap();println!("IGES parabola f=2 t=1: {got:?}; neutral evaluator={neutral:?}");assert_eq!(got,Point2::new(2.,4.));assert_eq!(neutral,Point2::new(0.125,1.));}
#[test]fn weighted_nurbs_overflow(){let n=PcurveNurbs::new(1,vec![0.,0.,1.,1.],PcurveNurbsPoles::from_lanes(vec![Point2::new(1e200,0.),Point2::new(2e200,0.)],Some(vec![1e200,1e200])).unwrap(),false).unwrap();let g=PcurveGeometry::Nurbs{nurbs:n};let got=pcurve(&g,0.5).unwrap();let neutral=cadmpeg_ir::eval::pcurve_uv(&g,0.5);println!("IGES rational pcurve={got:?}, neutral={neutral:?}");assert!(!got.is_finite());}
}
'''
c=read('cadmpeg-codec-catia/src/families/standard/decode.rs')
s+='mod catia_axis {use super::*;fn unit_vector(v:Vector3)->Option<Vector3>{v.unit_nonzero()}'+''.join(fn(c,n) for n in ['circle_axis_from_carrier','close_length','close_squared'])+'''
#[test]fn disjoint_circle_and_sphere(){let sf=SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(cadmpeg_ir::geometry::analytic::SphereSurface::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.),1.).unwrap()));let got=circle_axis_from_carrier(Point3::new(1e200,0.,0.),1.,&sf);println!("CATIA sphere radius 1, circle radius 1 center=(1e200,0,0): axis={got:?}; expected None");assert!(got.is_some());}
}
'''
conic=read('cadmpeg-codec-iges/src/entities/conics.rs');assert 'coeff_a * coeff_c > 0.0' in conic and 'coeff_a * coeff_c < 0.0' in conic
s+='''mod more_expressions {use super::*;
#[test]fn conic_coefficient_scale(){for a in [1.,1e-200_f64]{let c=a;let f=-a;let ellipse=a*c>0.;let hyperbola=a*c<0.;println!("IGES common-scaled unit circle A=C={a:e}, F={f:e}: ellipse={ellipse} hyperbola={hyperbola}");assert_eq!(ellipse,a==1.);}}
'''
rh=read('cadmpeg-codec-rhino/src/writer.rs');assert 'domain[0] + domain[1] - value' in rh
s+='''#[test]fn rhino_reversed_trim_break(){let domain=[1e308_f64,1.4e308];let value=1.2e308;let got=domain[0]+domain[1]-value;let expected=cadmpeg_ir::math::reflect_parameter(value,domain[0],domain[1]).unwrap();println!("Rhino reversed trim knot={got}, robust={expected:e}");assert!(got.is_infinite());}
}
'''

# The normal-transform probe uses the production admission and transform methods.
mesh=read('cadmpeg-codec-f3d/src/records/mesh.rs');impl_mesh=mesh[mesh.index('impl MeshAffineTransform {'):mesh.index('impl TryFrom<[[f64; 4]; 4]> for MeshAffineTransform')]
mesh_math=read('cadmpeg-codec-f3d/src/design/decode/mesh.rs')
s+='mod f3d_mesh {use super::*;use cadmpeg_core::CodecError;#[derive(Debug,Copy,Clone)]struct MeshAffineTransform([f64;16]);'+impl_mesh+'impl MeshAffineTransform {'+fn(mesh_math,'transform_normal')+'''}
#[test]fn valid_anisotropic_normal(){let tr=MeshAffineTransform::new([1e200,0.,0.,0.,0.,1e200,0.,0.,0.,0.,1e-200,0.,0.,0.,0.,1.]).unwrap();let got=tr.transform_normal([0.,0.,1.]);println!("F3D admitted anisotropic transform, input Z normal: {got:?}; expected Z normal");assert!(got.is_err());}
}
'''
asm=read('cadmpeg-asm/src/edit.rs');assert 'transform.rows()[0][3] / (header_scale * LEN_TO_MM)' in asm
s+='''mod asm {#[test]fn finite_translation_patch(){let header_scale=1e308_f64;let translation=1e308_f64;let len_to_mm=10.;let got=translation/(header_scale*len_to_mm);let expected=(translation/header_scale)/len_to_mm;println!("ASM native translation={got}, expected={expected}");assert_eq!(got,0.);assert_eq!(expected,0.1);}}
'''

pcurve_source=read('cadmpeg-ir/src/geometry/pcurve.rs');assert 'parabola.focal_distance() * u_scale' in pcurve_source
s+='''mod coordinate_scaling {use super::*;use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,ParabolaPcurve};
#[test]fn parabola_parameter_is_not_preserved(){let mut g=PcurveGeometry::Parabola(ParabolaPcurve::try_new(Point2::new(0.,0.),Point2::new(1.,0.),Point2::new(0.,1.),2.).unwrap());let before=cadmpeg_ir::eval::pcurve_uv(&g,1.).unwrap();g.try_scale_coordinates([3.,3.]).unwrap();let after=cadmpeg_ir::eval::pcurve_uv(&g,1.).unwrap();let expected=Point2::new(3.*before.u,3.*before.v);println!("IR parabola coordinate scale 3 at fixed t=1: before={before:?}, after={after:?}, expected={expected:?}");assert_eq!(after,Point2::new(1./24.,1.));assert_eq!(expected,Point2::new(0.375,3.));assert_ne!(after,expected);}
}
'''

(OUT/'probe.rs').write_text(s);(OUT/'manifest.json').write_text(json.dumps(list({x['path']:x for x in manifest}.values()),indent=2)+'\n')
libs={n:max(Path('target/debug/deps').glob('lib'+n+'-*.rlib'),key=lambda p:p.stat().st_mtime) for n in ['cadmpeg_ir','cadmpeg_core']}
cmd=['rustc','--edition=2021','--test',str(OUT/'probe.rs'),'-L','dependency=target/debug/deps','-o',str(OUT/'probe')]
for n,p in libs.items():cmd+=['--extern',n+'='+str(p)]
(OUT/'command.json').write_text(json.dumps(cmd)+'\n');(OUT/'libraries.json').write_text(json.dumps([{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in libs.values()],indent=2)+'\n')
with (OUT/'compile.log').open('w') as log:r=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT)
(OUT/'compile.exit').write_text(str(r.returncode)+'\n');print('compile exit',r.returncode)
if r.returncode:print((OUT/'compile.log').read_text());sys.exit(r.returncode)
with (OUT/'run.log').open('w') as log:r=subprocess.run([str(OUT/'probe'),'--nocapture','--test-threads=1'],stdout=log,stderr=subprocess.STDOUT)
(OUT/'run.exit').write_text(str(r.returncode)+'\n');print((OUT/'run.log').read_text());sys.exit(r.returncode)
