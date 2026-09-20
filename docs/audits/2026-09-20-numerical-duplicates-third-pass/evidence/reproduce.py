#!/usr/bin/env python3
from pathlib import Path
import hashlib,json,re,subprocess,sys,tempfile
ROOT=Path.cwd(); OUT=Path(sys.argv[1]) if len(sys.argv)>1 else Path(tempfile.mkdtemp(prefix='cadmpeg-numeric-third-pass-'));OUT.mkdir(parents=True,exist_ok=True);print('Evidence directory:',OUT)
manifest=[]
def read(path):
 p=Path('crates')/path;s=p.read_text();manifest.append({'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()});return s
def fn(s,name):
 m=re.search(r'(?m)^\s*(?:pub(?:\([^\n]*\))? )?fn '+name+r'\b',s);assert m,name
 a=s.index('{',m.start());i=a+1;depth=1
 while depth:depth+=(s[i]=='{')-(s[i]=='}');i+=1
 return re.sub(r'^pub\([^)]*\) ', '',s[m.start():i].strip())+'\n'
s='''#![allow(dead_code,unused_imports)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{SolvedCurveGeometry,sampled::PolylineCurve,nurbs::{NurbsCurve,NurbsSurface,NurbsSurfaceAxis,NurbsSurfaceLanes}};
use cadmpeg_ir::sketches::{SketchGeometry,SketchGeometryDefinition};
use cadmpeg_ir::scalar::{Angle,Length};
use cadmpeg_ir::transform::Transform;
fn plane(a:f64,b:f64)->NurbsSurface {let axis=NurbsSurfaceAxis::new(1,vec![0.,0.,1.,1.],false);NurbsSurface::from_lanes(axis.clone(),axis,NurbsSurfaceLanes::new(vec![vec![Point3::new(0.,0.,0.),Point3::new(0.,b,0.)],vec![Point3::new(a,0.,0.),Point3::new(a,b,0.)]],None),false).unwrap()}
fn line(a:[f64;2],b:[f64;2])->SketchGeometry{SketchGeometryDefinition::Line{start:Point2::new(a[0],a[1]),end:Point2::new(b[0],b[1])}.try_into().unwrap()}
fn arc(c:[f64;2],r:f64)->SketchGeometry{SketchGeometryDefinition::Arc{center:Point2::new(c[0],c[1]),radius:Length::new(r).unwrap(),start_angle:Angle::new(0.).unwrap(),end_angle:Angle::new(std::f64::consts::TAU).unwrap()}.try_into().unwrap()}
'''
ir=read('cadmpeg-ir/src/eval.rs');sum_source=read('cadmpeg-ir/src/math/sum.rs')
s+='mod math {pub use cadmpeg_ir::math::*;pub mod sum {'+sum_source[:sum_source.rindex('#[cfg(test)]')].replace('//!','//')+'}}\n'
s+='mod ir {use super::*;use cadmpeg_ir::eval::{curve_point_solved,nurbs_curve_parameter_near_point};use crate::math::sum::ExactSignedSum;#[derive(Debug,Clone,Copy)]struct ScalarSweepDifferential{value:f64,derivative:f64}\n'
s+=''.join(fn(ir,n) for n in ['direct_curve_parameter_near_point','inverse_affine_point','polyline_samples','polyline_parameter_near_point','scalar_unary_sweep_law_differential','finite_sweep_differential'])
s+='''
#[test]fn line_false_witness(){let g=SolvedCurveGeometry::Line(cadmpeg_ir::geometry::analytic::LineCurve::try_new(Point3::new(0.,0.,0.),Vector3::new(1.,0.,0.)).unwrap());let got=direct_curve_parameter_near_point(&g,Point3::new(0.,1e-200,0.),0.,0.);println!("IR analytic line: returned={got:?}; actual residual=1e-200; tolerance=0");assert_eq!(got,Some(0.));}
#[test]fn remaining_unary_laws(){for op in ["ARCTAN","ARCOT","ARCSEC","ARCCSC","ARCCSCH"]{let got=scalar_unary_sweep_law_differential(op,ScalarSweepDifferential{value:1e200,derivative:1e300}).unwrap();println!("IR {op}: derivative={} expected magnitude~1e-100",got.derivative);assert_eq!(got.derivative,0.);}let got=scalar_unary_sweep_law_differential("COTH",ScalarSweepDifferential{value:400.,derivative:1e300}).unwrap();println!("IR COTH(400): derivative={} expected~-1.47e-47",got.derivative);assert_eq!(got.derivative,0.);for op in ["SECH","CSCH"]{let got=scalar_unary_sweep_law_differential(op,ScalarSweepDifferential{value:720.,derivative:1e300});println!("IR {op}(720): {got:?}; expected value~4.064e-313 derivative~-4.064e-13");assert!(got.is_none_or(|d|d.derivative==0.));}}
}
'''
f=read('cadmpeg-codec-f3d/src/design/geometry.rs')
s+='mod f3d {use super::*;enum ProfileBoundarySegment{Line{start:Point2,end:Point2},Arc{center:Point2,radius:f64,start_angle:f64,end_angle:f64}}\n'+''.join(fn(f,n) for n in ['segments_intersect','arc_intersection_points','directed_angle_parameter','point_distance'])+'''
#[test]fn separated_segments(){let a=1e-100;let got=segments_intersect((Point2::new(0.,0.),Point2::new(a,0.)),(Point2::new(0.,2.*a),Point2::new(a,2.*a)));println!("F3D separated parallel segments intersect={got}; separation={:e}",2.*a);assert!(got);}
#[test]fn crossing_circles(){for r in [1.,1e-200,1e200]{let arc=|x|ProfileBoundarySegment::Arc{center:Point2::new(x,0.),radius:r,start_angle:0.,end_angle:std::f64::consts::TAU};let got=arc_intersection_points(&arc(0.),&arc(r));println!("F3D crossing circles radius={r:e}: {got:?}; expected 2 points");if r==1.{assert_eq!(got.unwrap().len(),2)}else{assert!(got.is_none_or(|p|p.len()!=2));}}}
#[test]fn distance_range(){for a in [1e-200,1e200]{let got=point_distance(Point2::new(0.,0.),Point2::new(a,0.));println!("F3D distance={got:e}, expected={a:e}");assert_ne!(got,a);}}
}
'''
c=read('cadmpeg-codec-creo/src/decode/sketch/intersect.rs')
s+='mod creo {use super::*;const EPS_RADIUS_NONZERO:f64=1e-12;const EPS_RADIAL_RESIDUAL:f64=1e-10;const EPS_PARAMETER_BOUND:f64=1e-10;const EPS_CENTER_DISTANCE:f64=1e-12;const EPS_HEIGHT_RESIDUAL:f64=1e-9;const EPS_LINE_INTERSECTION:f64=1e-12;'+''.join(fn(c,n) for n in ['intersect_section_line_arc','intersect_tangent_section_arcs','section_line_origin_direction','intersect_section_lines'])+'''
#[test]fn missed_circle(){let r=1e-6;let got=intersect_section_line_arc(&line([-r,2.*r],[r,2.*r]),&arc([0.,0.],r));println!("Creo missed circle: {got:?}; closest distance=2e-6; radius=1e-6");assert_eq!(got,Some([0.,2.*r]));}
#[test]fn nontangent_circles(){let r=1e-6;let got=intersect_tangent_section_arcs(&arc([0.,0.],r),&arc([r,0.],r));println!("Creo two crossing circles claimed tangent={got:?}; true intersections=(0.5e-6,+/-sqrt(3)*0.5e-6)");assert_eq!(got,Some([r*0.5,0.]));}
#[test]fn small_perpendicular_lines(){let r=1e-7;let got=intersect_section_lines(&line([-r,0.],[r,0.]),&line([0.,-r],[0.,r]));println!("Creo perpendicular lines crossing origin: {got:?}; expected Some([0,0])");assert!(got.is_none());}
}
'''
c=read('cadmpeg-codec-catia/src/families/standard/decode.rs')
s+='mod catia {use super::*;const NURBS_SURFACE_MEMBERSHIP_TOLERANCE:f64=2e-3;const NURBS_SURFACE_SEEDS_PER_SPAN:usize=3;const NURBS_SURFACE_MAX_SEEDS:usize=256;const NURBS_SURFACE_REFINEMENT_ITERATIONS:usize=24;const NURBS_SURFACE_BACKTRACK_STEPS:usize=8;'+''.join(fn(c,n) for n in ['nurbs_surface_parameter_domain','refine_nurbs_surface_point','nurbs_surface_point_distance_squared','nurbs_surface_witness_distance','nurbs_surface_axis_samples','nurbs_surface_start_grid'])+'''
#[test]fn anisotropic_surface(){let u=NurbsSurfaceAxis::new(1,vec![0.,0.,1e-10,1e-10],false);let v=NurbsSurfaceAxis::new(1,vec![0.,0.,1.,1.],false);let sf=NurbsSurface::from_lanes(u,v,NurbsSurfaceLanes::new(vec![vec![Point3::new(0.,0.,0.),Point3::new(0.,1.,0.)],vec![Point3::new(1.,0.,0.),Point3::new(1.,1.,0.)]],None),false).unwrap();let p=Point3::new(0.3,0.4,0.);let got=nurbs_surface_witness_distance(&sf,p).unwrap();let exact=cadmpeg_ir::eval::nurbs_surface_point(&sf,3e-11,0.4).unwrap();println!("CATIA point on plane: best squared residual={got:e}; known UV=(3e-11,0.4); exact point={exact:?}");assert!(got>0.01);assert!(exact.distance(p)<NURBS_SURFACE_MEMBERSHIP_TOLERANCE);}}
'''
n=read('cadmpeg-codec-nx/src/decode/offset.rs')
s+='mod nx {use super::*;'+''.join(fn(n,k) for k in ['determinant_3x3','null_vector_3x4','lift_periodic_parameter'])+'''
#[test]fn tangent_scale(){for a in [1.,1e-5,1e100]{let got=null_vector_3x4([[a,0.,-a,0.],[0.,a,0.,0.],[0.,0.,0.,-a]]);println!("NX rank-three matrix scale={a:e}: nullvector={got:?}; expected a normalized (1,0,1,0)");assert_eq!(got.is_some(),a==1.);}}
#[test]fn finite_phase(){let got=lift_periodic_parameter(-1e308,1e308,1e307);println!("NX periodic lift={got}; finite congruent nearest representative is 1e308");assert!(got.is_infinite());}}
'''
sl=read('cadmpeg-codec-sldprt/src/resolved_features/transforms.rs');lr=read('cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs')
s+='mod sld {use super::*;const SKETCH_POINT_TOLERANCE:f64=1e-9;'+fn(sl,'quantize')+fn(lr,'line_line_distance')+fn(lr,'line_direction')+'''
#[test]fn saturated_keys(){let a=quantize(Point2::new(1e14,0.),1e-6);let b=quantize(Point2::new(2e14,0.),1e-6);println!("SLD separated points 1e14 and 2e14 quantum=1e-6: keys={a:?},{b:?}");assert_eq!(a,b);}
#[test]fn nonparallel_distance(){let got=line_line_distance([[0.,0.],[1e200,0.]],[[0.,1.],[0.,1e200]]);println!("SLD perpendicular lines admitted as parallel: distance={got:?}; expected None");assert_eq!(got,Some(1.));}
}
'''
for module,path,fun in [('iges','cadmpeg-codec-iges/src/entities/evaluation.rs','distance'),('nx_distance','cadmpeg-codec-nx/src/intersection.rs','distance')]:
 src=read(path);s+='mod '+module+' {use super::*;'+fn(src,fun)+'''
#[test]fn residual_range(){for a in [1e-200,1e200]{let got=distance(Point3::new(0.,0.,0.),Point3::new(a,0.,0.));println!("'''+module+''' distance={got:e}, expected={a:e}");assert_ne!(got,a);}}}
'''
src=read('cadmpeg-codec-inventor/src/sketch.rs')
s+='mod inventor {use super::*;const EPS_SKETCH_LINE_CARRIER_MATCHES_E10:f64=1e-10;'+fn(src,'line_carrier_matches')+'''
#[test]fn finite_parallel(){let got=line_carrier_matches([0.,0.],[1e200,1e200],[1e200,1e200],[2e200,2e200]);println!("Inventor exact collinear finite carrier accepted={got}");assert!(!got);}}
'''

src=read('cadmpeg-codec-step/src/geometry.rs')
s+='mod step {use super::*;use cadmpeg_ir::transform::Transform2;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10:f64=1e-10;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12:f64=1e-12;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10:f64=1e-10;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12:f64=1e-12;'+fn(src,'similarity_transform')+fn(src,'similarity_transform_2d')+'''
#[test]fn small_shear(){let a=1e-10;let t=Transform::affine([[a,0.5*a,0.,0.],[0.,0.75f64.sqrt()*a,0.,0.],[0.,0.,a,0.]]).unwrap();let t2=Transform2::affine([[a,0.5*a,0.],[0.,0.75f64.sqrt()*a,0.]]).unwrap();let got=similarity_transform(&t);let got2=similarity_transform_2d(&t2);println!("STEP 60-degree columns accepted as similarity: 3d={got}, 2d={got2}");assert!(got&&got2);}}
'''
src=read('cadmpeg-codec-rhino/src/surfaces.rs')
s+='mod rhino {use super::*;'+fn(src,'periodic_knots')+fn(src,'map_parameter')+'''
#[test]fn plane_parameter_range(){for a in [1e-200,1e200]{let got=map_parameter(a,[0.,a],[0.,a]);println!("Rhino identity plane parameter map: got={got:e}, expected={a:e}");assert_ne!(got,a);}}
#[test]fn nonperiodic_knots(){let k=[-1e308,-1e308,-9e307,9e307,1e308,1e308];let got=periodic_knots(&k,3,5);println!("Rhino nonperiodic knots admitted periodic={got}; paired gaps 0 and 1e307 differ");assert!(got);}}
'''


src=read('cadmpeg-codec-creo/src/decode/sweep/profiles.rs')
s+='mod creo_profile {use super::*;'+''.join(fn(src,n) for n in ['point_on_profile_arc','line_arc_intersect','arcs_intersect'])+'''
#[test]fn large_intersections(){let a=1e200;let circle=([0.,0.],a,0.,std::f64::consts::TAU);let line_hit=line_arc_intersect([[-2.*a,0.],[2.*a,0.]],circle,1e-9);let circle_hit=arcs_intersect(circle,([a,0.],a,0.,std::f64::consts::TAU),1e-9);println!("Creo profile: line-circle={line_hit}, circle-circle={circle_hit}; both have two intersections");assert!(!line_hit&&!circle_hit);}}
'''
src=read('cadmpeg-codec-sldprt/src/resolved_features/typed_relations.rs');loci=read('cadmpeg-codec-sldprt/src/resolved_features/relation_loci.rs')
s+='mod sld_ellipse {use super::*;const EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9:f64=1e-9;struct SketchEntity{geometry:SketchGeometry}const SKETCH_POINT_TOLERANCE:f64=1e-9;const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12:f64=1e-12;const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9:f64=1e-9;'+fn(src,'sketch_entity_contains_point')+fn(loci,'same_dimension_length')+'''
#[test]fn off_ellipse_accepted(){let geometry=SketchGeometryDefinition::Ellipse{center:Point2::new(-1e308,0.),major_angle:Angle::new(0.).unwrap(),major_radius:Length::new(1.).unwrap(),minor_radius:Length::new(0.5).unwrap(),bounds:None}.try_into().unwrap();let got=sketch_entity_contains_point(&SketchEntity{geometry},Point2::new(1e308,0.));println!("SLD ellipse centered at -1e308 contains +1e308={got}; radii=(1,0.5)");assert!(got);}}
'''
src=read('cadmpeg-codec-catia/src/families/freeform/mod.rs')
# Minimal chart scaffolding: the rigid mapping uses the production expressions.
# The failing case returns before a chart is made. The control case exercises it.
s+='''mod catia_chart {use super::*;use cadmpeg_ir::geometry::{SurfaceGeometry,SolvedSurfaceGeometry};const CONSOLIDATED_SITE_TOLERANCE:f64=2e-3;enum ConsolidatedCarrierChart<'a>{Rigid{linear:[[f64;2];2],offset:[f64;2]},Unused(&'a ())}impl ConsolidatedCarrierChart<'_>{fn point(&self,[u,v]:[f64;2])->[f64;2]{match self{Self::Rigid{linear,offset}=>[linear[0][0]*u+linear[0][1]*v+offset[0],linear[1][0]*u+linear[1][1]*v+offset[1]],Self::Unused(_)=>unreachable!()}}}
'''+fn(src,'solve_planar_chart_rechart')+'''
#[test]fn identity_rechart(){let target=SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.)).unwrap()));for a in [1.,1e200]{let sites=[[a,0.],[0.,a],[-a,0.],[0.,-a]];let loci=sites.map(|p|Point3::new(p[0],p[1],0.));let got=solve_planar_chart_rechart(&sites,&loci,&target);println!("CATIA identity chart scale={a:e}: accepted={}",got.is_some());assert_eq!(got.is_some(),a==1.);}}}
'''

(OUT/'probe.rs').write_text(s);(OUT/'manifest.json').write_text(json.dumps(list({x['path']:x for x in manifest}.values()),indent=2)+'\n')
libs={n:max(Path('target/debug/deps').glob('lib'+n+'-*.rlib'),key=lambda p:p.stat().st_mtime) for n in ['cadmpeg_ir']}
cmd=['rustc','--edition=2021','--test',str(OUT/'probe.rs'),'-L','dependency=target/debug/deps','-o',str(OUT/'probe')]
for n,p in libs.items():cmd+=['--extern',n+'='+str(p)]
(OUT/'command.json').write_text(json.dumps(cmd)+'\n')
(OUT/'libraries.json').write_text(json.dumps([{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in libs.values()],indent=2)+'\n')
with (OUT/'compile.log').open('w') as log:r=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT)
(OUT/'compile.exit').write_text(str(r.returncode)+'\n');print('compile exit',r.returncode)
if r.returncode:print((OUT/'compile.log').read_text());sys.exit(r.returncode)
with (OUT/'run.log').open('w') as log:r=subprocess.run([str(OUT/'probe'),'--nocapture','--test-threads=1'],stdout=log,stderr=subprocess.STDOUT)
(OUT/'run.exit').write_text(str(r.returncode)+'\n');print((OUT/'run.log').read_text());sys.exit(r.returncode)
