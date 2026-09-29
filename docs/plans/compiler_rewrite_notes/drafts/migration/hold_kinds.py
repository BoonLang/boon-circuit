import json,sys,collections
d=json.load(open(sys.argv[1]))
ex={e['id']:e for e in d['executable']['expressions']}
def kind(t):
    if isinstance(t,str): return t
    k=next(iter(t)); 
    return 'Tag' if k=='VariantSet' else k
def fields(t):
    if isinstance(t,dict) and 'Object' in t: return tuple(sorted(t['Object']['fields'].keys()))
    return None
for e in ex.values():
    k=e['kind']
    if k['kind']!='hold': continue
    items=[k['initial']]+k['updates']
    tys=[ex[i]['flow_type']['ty'] for i in items]
    ks=set(kind(t) for t in tys); ks.discard('Absent')
    fs=set(fields(t) for t in tys if fields(t) is not None)
    if len(ks)>1 or len(fs)>1: print(k.get('binding_path'), sorted(ks), [list(f) for f in fs][:3])
