from pathlib import Path
from collections import defaultdict, Counter
import sys,re,json,hashlib
sys.path.insert(0,'/tmp/catia-numeric-audit-hbktitbw/deps')
from tree_sitter import Language, Parser
import tree_sitter_rust
parser=Parser(Language(tree_sitter_rust.language()))
root=Path.cwd();out=Path(sys.argv[1]) if len(sys.argv)>1 else Path('/tmp/cadmpeg-numeric-audit-4');out.mkdir(parents=True,exist_ok=True);functions=[];errors=[];files=[]
excluded={'tests','test_support','test_only','golden_tests','integration_tests'}
def tokens(node,data):
 if node.type in ('line_comment','block_comment'):return []
 if not node.children:return [(node.type,data[node.start_byte:node.end_byte].decode())]
 return [v for child in node.children for v in tokens(child,data)]
def walk(node,data,path,context=()):
 attrs=[]
 for child in node.children:
  if child.type=='attribute_item':attrs.append(data[child.start_byte:child.end_byte].decode());continue
  attr=' '.join(attrs);attrs=[]
  if re.search(r'#\[test\]|cfg\(test\)',attr):continue
  n=child.child_by_field_name('name');name=data[n.start_byte:n.end_byte].decode() if n else ''
  if child.type=='mod_item' and (name in excluded or name.endswith('_tests')):continue
  ctx=context
  if child.type=='impl_item':
   n=child.child_by_field_name('type');ctx+=('impl '+data[n.start_byte:n.end_byte].decode(),) if n else ()
  if child.type=='function_item':
   body=child.child_by_field_name('body')
   if not body:continue
   vals=tokens(body,data);ident={};shape=[]
   for kind,value in vals:
    if kind in ('identifier','field_identifier'):value=ident.setdefault(value,'I'+str(len(ident)))
    shape.append((kind,value))
   hashof=lambda v:hashlib.sha256(json.dumps(v).encode()).hexdigest()
   functions.append({'crate':path.parts[1],'path':str(path),'name':name,'context':list(context),'line':data.count(b'\n',0,child.start_byte)+1,'start':child.start_byte,'end':child.end_byte,'body_start':body.start_byte,'count':len(vals),'exact':hashof(vals),'shape':hashof(shape)})
   ctx+=(name,)
  walk(child,data,path,ctx)
for p in sorted(Path('crates').rglob('*.rs')):
 if any(x in excluded for x in p.parts) or p.stem in excluded or p.stem.endswith('_tests') or 'target' in p.parts:continue
 data=p.read_bytes();tree=parser.parse(data)
 if tree.root_node.has_error:errors.append(str(p))
 files.append({'path':str(p),'sha256':hashlib.sha256(data).hexdigest()})
 walk(tree.root_node,data,p)
(out/'functions.json').write_text(json.dumps(functions))
(out/'sources.json').write_text(json.dumps(files))
coverage=Counter(f['crate'] for f in functions)
(out/'coverage.json').write_text(json.dumps(dict(sorted(coverage.items())),indent=2))
for mode in ['exact','shape']:
 groups=defaultdict(list)
 for f in functions:
  if f['count']>=25:groups[f[mode]].append(f)
 groups=sorted([v for v in groups.values() if len(v)>1],key=lambda v:-v[0]['count'])
 (out/f'{mode}-groups.json').write_text(json.dumps(groups))
 (out/f'{mode}-groups.txt').write_text('\n\n'.join(str(g[0]['count'])+' tokens\n'+'\n'.join(f"{f['path']}:{f['line']} {'::'.join(f['context']+[f['name']])}" for f in g) for g in groups))
print('files',len(files),'functions',len(functions),'parse errors',errors,'coverage',dict(sorted(coverage.items())))
