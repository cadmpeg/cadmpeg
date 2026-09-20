from pathlib import Path
import re,subprocess,json,hashlib
root=Path('/tmp/cadmpeg-audit-fixes'); manifest=[]
def source(path):
 p=Path('crates')/path;s=p.read_text();manifest.append({'path':str(p),'sha256':hashlib.sha256(s.encode()).hexdigest()});return s
def extract(src,name):
 m=re.search(r'(?m)^\s*(?:pub(?:\([^\n]*\))? )?fn '+re.escape(name)+r'\b',src);assert m,name
 a=src.index('{',m.start());i=a+1;depth=1
 while depth:depth+=(src[i]=='{')-(src[i]=='}');i+=1
 return src[m.start():i].strip()+'\n'
def module(name,path,names,tests,prefix='',replace=None):
 src=source(path);out='mod '+name+' { use super::*;\n'+prefix+'\n'
 out+=''.join(extract(src,n) for n in names)
 out+='#[cfg(test)]mod tests {use super::*;\n'
 for path,test in tests:
  fn=extract(source(path),test)
  if replace:
   for old,new in replace.items():fn=fn.replace(old,new)
  out+='#[test]\n'+fn
 return out+'}}\n'
s='''#![allow(dead_code,unused_imports)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_core::CodecError;
'''
s+=module('f3d_rotation','cadmpeg-codec-f3d/src/design/feature_project.rs',['matrix_axis_angle'],[('cadmpeg-codec-f3d/src/design/feature_project/tests/timeline.rs','numerical_audit_half_turn_recovers_axis_with_zero_x')], 'const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12:f64=1e-12; const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8:f64=1e-8;',{'crate::design::feature_project::':'super::'})
s+=module('f3d_intersection','cadmpeg-codec-f3d/src/design/geometry.rs',['line_arc_intersection_points','directed_angle_parameter'],[('cadmpeg-codec-f3d/src/design/geometry/tests.rs','numerical_audit_line_circle_intersections_are_scale_invariant')], 'enum ProfileBoundarySegment {Line{start:Point2,end:Point2},Arc{center:Point2,radius:f64,start_angle:f64,end_angle:f64}}')
s+=module('creo','cadmpeg-codec-creo/src/feature/definitions.rs',['trim_line_circle_intersection'],[('cadmpeg-codec-creo/src/feature/definitions.rs','numerical_audit_trim_line_circle_rejects_disjoint_small_carriers')], 'const TRIM_INTERSECTION_EPS:f64=1e-12; const TRIM_COORDINATE_EPS:f64=1e-9;')
s+=module('nx','cadmpeg-codec-nx/src/decode/offset.rs',['least_squares_step'],[('cadmpeg-codec-nx/src/decode/offset.rs','numerical_audit_least_squares_checks_rank_independent_of_column_scale')])
frame=source('cadmpeg-codec-freecad/src/native/frame.rs');frame=frame[:frame.index('#[cfg(test)]')];frame=frame.replace('use serde::{Deserialize, Serialize};','').replace(', Serialize, Deserialize','');frame=re.sub(r'#\[serde\([^\n]*\)\]\n','',frame);frame='\n'.join(line for line in frame.splitlines() if not line.startswith('//!'))
s+=module('freecad','cadmpeg-codec-freecad/src/placement.rs',['placement_components'],[('cadmpeg-codec-freecad/src/placement.rs','numerical_audit_quaternion_frame_is_invariant_under_nonzero_scaling')],frame)
s+=module('sld_angles','cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs',['line_direction','line_line_angle','dynamic_line_line_angle'],[('cadmpeg-codec-sldprt/src/resolved_features/relation_records.rs','numerical_audit_line_angles_are_finite_for_large_directions')], 'const SKETCH_POINT_TOLERANCE:f64=1e-9;')
holes=source('cadmpeg-codec-sldprt/src/resolved_features/holes.rs');a=holes.index('#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]\nenum GridCoordinate');b=holes.index('fn hole_axis_key',a)
s+=module('sld_holes','cadmpeg-codec-sldprt/src/resolved_features/holes.rs',['canonical_axis','carrier_placements','hole_axis_key'],[('cadmpeg-codec-sldprt/src/resolved_features/holes/tests/hole_axis.rs','numerical_audit_hole_carriers_keep_distinct_large_coordinate_axes')], 'use std::collections::HashMap;use cadmpeg_ir::features::holes::HolePlacement;const EPS_HOLE_POSITION:f64=1e-8;const EPS_HOLE_EXACT_GEOMETRY:f64=1e-12;\n'+holes[a:b],{'crate::resolved_features::holes::':'super::'})
s+=module('rhino_mesh','cadmpeg-codec-rhino/src/mesh.rs',['triangulate_faces','unique_face_vertices'],[('cadmpeg-codec-rhino/src/mesh.rs','numerical_audit_quad_uses_shorter_large_diagonal')])
s+=module('iges_orientation','cadmpeg-codec-iges/src/entities/surfaces.rs',['similarity_orientation'],[('cadmpeg-codec-iges/src/entities/surfaces/tests.rs','numerical_audit_similarity_orientation_survives_uniform_scale')], 'const EPS_SURFACES_SIMILARITY_ORIENTATION_E10:f64=1e-10;')
real=source('cadmpeg-codec-iges/src/binary.rs');s+='''mod iges_real {use super::*;
fn malformed(message:&str)->CodecError {CodecError::Malformed(message.into())}
struct BitReader<'a>{bytes:&'a[u8],byte:usize,bit:u8}
impl<'a> BitReader<'a>{
'''+extract(real[real.index("impl<'a> BitReader<'a>"):],'new')+''.join(extract(real,n) for n in ['read_bits','align_zero','read_real'])+'''}
#[derive(Default)]struct BitWriter{bytes:Vec<u8>,bit:u8}
impl BitWriter {'''+extract(real,'push_bits')+'''}
#[cfg(test)]mod tests{use super::*;#[test]
'''+extract(real,'numerical_audit_binary_real_keeps_representable_exponent_boundary')+'}}\n'
s+=module('sld_integer','cadmpeg-codec-sldprt/src/history/configuration.rs',['align_configuration_parameter_kinds'],[('cadmpeg-codec-sldprt/src/history/tests/configuration.rs','configuration_numeric_override_inherits_parameter_dimension')], 'use std::collections::{BTreeMap,HashMap}; use cadmpeg_ir::features::ParameterValue; use cadmpeg_ir::scalar::{Length,Angle}; const EPS_CONFIGURATION_ALIGN_CONFIGURATION_PARAMETER_KINDS_E9:f64=1e-9;'+extract(source('cadmpeg-codec-sldprt/src/history/parameters/eval.rs'),'exact_integer_f64'))
(root/'codec-probes.rs').write_text(s);(root/'codec-probes-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
libs={name:max(Path('target/debug/deps').glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime) for name in ['cadmpeg_ir','cadmpeg_core']}
cmd=['rustc','--edition=2021','--test',str(root/'codec-probes.rs'),'-L','dependency=target/debug/deps','-o',str(root/'codec-probes')]
for name,path in libs.items():cmd+=['--extern',name+'='+str(path)]
(root/'codec-probes.command').write_text(' '.join(cmd)+'\n')
with (root/'codec-probes-build.log').open('w') as log:r=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT)
(root/'codec-probes-build.exit').write_text(str(r.returncode)+'\n');print('build',r.returncode,flush=True)
if r.returncode:print((root/'codec-probes-build.log').read_text());raise SystemExit(r.returncode)
with (root/'codec-probes.log').open('w') as log:r=subprocess.run([str(root/'codec-probes')],stdout=log,stderr=subprocess.STDOUT)
(root/'codec-probes.exit').write_text(str(r.returncode)+'\n');print((root/'codec-probes.log').read_text());raise SystemExit(r.returncode)
