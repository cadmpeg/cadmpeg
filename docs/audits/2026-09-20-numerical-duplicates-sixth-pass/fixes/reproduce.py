# Extract current owners and execute scale and incidence regressions.
# Run from the repository root with tree-sitter and tree-sitter-rust installed.
from pathlib import Path
import sys,re,json,hashlib,subprocess,tempfile
from tree_sitter import Language,Parser
import tree_sitter_rust
parser=Parser(Language(tree_sitter_rust.language()))
ROOT=Path.cwd(); OUT=Path(tempfile.mkdtemp(prefix='cadmpeg-sixth-fix-probes-')); manifest=[]
print("Evidence directory:", OUT, flush=True)
def source(p):
 p=ROOT/'crates'/p;b=p.read_bytes();manifest.append(dict(path=str(p.relative_to(ROOT)),sha256=hashlib.sha256(b).hexdigest()));return b.decode()
def fn(s,name):
 b=s.encode();stack=[parser.parse(b).root_node]
 while stack:
  n=stack.pop()
  if n.type=='function_item' and n.child_by_field_name('name').text.decode()==name:return re.sub(r'^pub(?:\([^)]*\))?\s+','',b[n.start_byte:n.end_byte].decode())+'\n'
  stack.extend(reversed(n.children))
 raise ValueError(name)
def consts(s):return '\n'.join(re.findall(r'(?m)^const [A-Z][A-Z0-9_]*: f64 = [^;]+;',s))+'\n'
def test(p,name):return '#[test]\n'+fn(source(p),name)
s='''#![allow(dead_code,unused_imports,unused_variables)]
mod math {pub use cadmpeg_ir::math::*;
'''+f'#[path="{OUT}/sum.rs"]pub mod sum;'+'''}
use math::*;
use cadmpeg_ir::geometry::{CurveGeometry,SolvedCurveGeometry};
use cadmpeg_ir::geometry::analytic::{CircleCurve,EllipseCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::sketches::{SketchGeometry,SketchGeometryDefinition};
'''
(OUT/'sum.rs').write_text(source('cadmpeg-ir/src/math/sum.rs').replace('#[cfg(test)]\nmod tests;', ''))
f=source('cadmpeg-asm/src/brep/geometry.rs');t=source('cadmpeg-asm/src/brep/tests.rs')
s+='mod asm {use super::*;'+consts(f)+''.join(fn(f,n) for n in ['rational_four_arc_circle','reduce_homogeneous_bezier_to_quadratic','point_sum_difference','point_vector'])+''.join(fn(t,n) for n in ['exact_circle_directrix','degree_elevated_circle'])+'#[test]\n'+fn(t,'circle_recognition_is_invariant_under_common_weight_scale')+'}\n'
f=source('cadmpeg-ir/src/math/planar.rs')
s+='mod planar {use super::*;use math::sum::ExactSignedSum;'+''.join(fn(f,n) for n in ['scaled_displacement','line_circle_parameters','circle_intersections','orientation'])+'use std::cmp::Ordering;'+''.join('#[test]\n'+fn(f,n) for n in ['distant_diagonal_line_preserves_exact_circle_incidence','distant_line_origin_does_not_change_circle_intersections','intersections_preserve_independent_direction_and_position_scales','intersections_and_orientation_preserve_scale'])+'}\n'
f=source('cadmpeg-ir/src/validate/sketches.rs')
s+='mod ir_parallel {use super::*;'+consts(f)+'''struct PlanarParallelLines {first:[Point2;2],second:[Point2;2],distance:f64}
'''+''.join(fn(f,n) for n in ['planar_parallel_lines','planar_parallel_line_distance','planar_parallel_line_span_distance'])+'mod tests {use super::*; const TEST_LINEAR_TOLERANCE:f64=1e-6;'+test('cadmpeg-ir/src/validate/sketches/tests.rs','extreme_lines_preserve_parallelism_and_span_separation')+'}}\n'
f=source('cadmpeg-ir/src/eval.rs');t=source('cadmpeg-ir/src/eval/tests/offset_frames.rs')
s+='mod ir_offset {use super::*;'+consts(f)+fn(f,'fitted_nurbs_offset_candidate')+'#[test]\n'+fn(t,'offset_frames_reject_extreme_perpendicular_tangents')+'}\n'
f=source('cadmpeg-codec-catia/src/families/standard/decode.rs')
s+='mod catia {use super::*;const ANALYTIC_CURVE_ENDPOINT_TOLERANCE:f64=2e-3;'+fn(f,'standard_analytic_curve_angle')+'mod tests {mod circle {'+test('cadmpeg-codec-catia/src/families/standard/decode/tests/circle.rs','analytic_curve_angles_preserve_extreme_radii')+'}}}\n'
f=source('cadmpeg-codec-f3d/src/design/dimensions.rs')
s+='mod f3d {use super::*;'+consts(f)+fn(f,'reflect_point')+'mod tests {mod numerical {'+test('cadmpeg-codec-f3d/src/design/dimensions/tests/numerical_ranges.rs','reflection_preserves_finite_points_at_extreme_scales')+'}}}\n'
f=source('cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs')
s+='mod sld {use super::*;const SKETCH_POINT_TOLERANCE:f64=1e-9;'+fn(f,'line_direction')+fn(f,'line_line_angle')+'mod tests {'+'#[test]\n'+fn(f,'line_angles_retain_shallow_and_near_opposite_angles')+'}}\n'
f=source('cadmpeg-codec-creo/src/decode/analytic/equations.rs')
a=f.index('#[derive(Clone, Copy)]');b=f.index('impl CarrierEquation',a)
types=f[a:b];types=re.sub(r'pub\([^)]*\)', 'pub',types)
s+='mod decode {pub mod analytic {pub mod equations {'+types+'}}}\n'
f=source('cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates.rs');v=source('cadmpeg-codec-creo/src/vecmath.rs')
s+='mod creo {use super::*;use crate::decode::analytic::equations::*;use crate::planar::line_circle_parameters;'+consts(f)+consts(v)+''.join(fn(v,n) for n in ['dot','cross','normalize','normalize_with_length'])+''.join(fn(f,n) for n in ['parallel_cylinder_generator_candidates','coaxial_cylinder_sphere_circle_candidates','parallel_plane_cylinder_generator_candidates','coaxial_cylinder_torus_circle_candidates','axis_normal_plane_torus_circle_candidates','coaxial_cone_sphere_circle_candidates','coaxial_cone_torus_circle_candidates'])+'mod tests {'+test('cadmpeg-codec-creo/src/decode/surfaces/intersection_candidates/tests.rs','intersection_candidate_multiplicity_is_invariant_under_length_scale')+'}}\n'
# Equation helper references this module-local constant in its original owner.
s=s.replace('pub fn circular_cone(cone: ConeEquation) -> bool {','pub fn circular_cone(cone: ConeEquation) -> bool { const EPS_NEAR_ZERO:f64=1e-12;')
f=source('cadmpeg-codec-nx/src/decode/pcurves.rs')
s+='mod nx {use super::*;use cadmpeg_ir::geometry::pcurve::PcurveGeometry;'+fn(f,'reverse_analytic_pcurve_over_range')+'mod tests {use super::*;'+test('cadmpeg-codec-nx/src/decode/pcurves/reversal_tests.rs','analytic_reversal_preserves_finite_coefficients_at_extreme_parameters')+'}}\n'
f=source('cadmpeg-codec-rhino/src/brep.rs');expr=re.search(r'tolerance = tolerance\s*(\.max\(delta\[0\][^;]+);',fn(f,'parse_legacy_major2')).group(1)
s+='mod rhino_tolerance {fn measured(delta:[f64;3])->f64 {let tolerance=0.0_f64;tolerance'+expr+'}'+'''#[test]fn legacy_tolerance_retains_large_finite_distance(){assert_eq!(measured([1e200,0.,0.]),1e200);assert!((measured([3e200,4e200,0.])/1e200-5.).abs()<4.*f64::EPSILON);}}'''
f=source('cadmpeg-codec-rhino/src/writer.rs')
s+='''mod rhino {use super::*;struct Vertex{point:Point3}struct Loop{coedges:Vec<usize>}struct WritableModel<'a>{vertices:Vec<Vertex>,loops:Vec<Loop>,edges:Vec<(usize,usize)>,marker:std::marker::PhantomData<&'a ()>}impl WritableModel<'_>{fn endpoints(&self,i:usize)->(usize,usize){self.edges[i]}}'''+fn(f,'planar_solid_orientation')+'''
fn tetra(scale:f64,t:f64)->WritableModel<'static>{let vertices=vec![(0.,0.,0.),(2.,0.,0.),(0.,1.,0.),(0.,0.,1.)].into_iter().map(|(x,y,z)|Vertex{point:Point3::new(scale*x+t,scale*y+t,scale*z+t)}).collect();let mut edges=vec![];let mut loops=vec![];for face in [[0,2,1],[0,1,3],[0,3,2],[1,2,3]]{let first=edges.len();for j in 0..3{edges.push((face[j],face[(j+1)%3]));}loops.push(Loop{coedges:(first..first+3).collect()});}WritableModel{vertices,loops,edges,marker:std::marker::PhantomData}}
#[test]fn solid_orientation_preserves_scale_translation_and_sense(){for(scale,t)in[(1.,0.),(1.,1e8),(1e200,0.),(1e-200,0.)]{let mut model=tetra(scale,t);assert_eq!(planar_solid_orientation(&model),1);for l in &mut model.loops{l.coedges.reverse();}assert_eq!(planar_solid_orientation(&model),2);}}}
'''
f=source('cadmpeg-codec-iges/src/entities/conics.rs')
exprs=re.findall(r'let Some\(focal_distance\) =\s*(cadmpeg_ir::math::product_quotient\(.*?\)\s*\.map\(f64::abs\)) else',fn(f,'project'),re.S);assert len(exprs)==2
s+='mod iges {'
for i,expr in enumerate(exprs):s+=f'fn focal_{i}(coefficient:f64)->Option<f64> {{let coeff_a=&coefficient;let coeff_c=&coefficient;let coeff_e=&-coefficient;let coeff_d=&-coefficient;let factor=1.0;let scale_x=1.0;let scale_y=1.0; {expr} }}\n'
s+='''#[test]fn parabola_focal_distance_preserves_coefficient_scale(){for focal in [focal_0,focal_1]{for scale in [1e-300,1.,1e308]{assert_eq!(focal(scale),Some(0.25));}}}}'''
s=s.replace('fn line_circle_parameters(', 'pub(crate) fn line_circle_parameters(')
(OUT/'regressions.rs').write_text(s)
lib=max((ROOT/'target/debug/deps').glob('libcadmpeg_ir*.rlib'),key=lambda p:p.stat().st_mtime)
manifest.append(dict(cached_library=str(lib.relative_to(ROOT)),sha256=hashlib.sha256(lib.read_bytes()).hexdigest()))
(OUT/'regression-sources.json').write_text(json.dumps(manifest,indent=2)+'\n')
cmd=['rustc','--edition=2021','--test',str(OUT/'regressions.rs'),'--extern','cadmpeg_ir='+str(lib),'-L','dependency='+str(ROOT/'target/debug/deps'),'-o',str(OUT/'regressions')]
for name,command in [('regression-build',cmd),('regressions',[str(OUT/'regressions'),'--nocapture'])]:
 (OUT/(name+'.command.json')).write_text(json.dumps(command))
 with (OUT/(name+'.log')).open('w')as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 (OUT/(name+'.exit')).write_text(str(r.returncode)+'\n');print((OUT/(name+'.log')).read_text());print(name,'exit',r.returncode)
 if r.returncode:raise SystemExit(r.returncode)
