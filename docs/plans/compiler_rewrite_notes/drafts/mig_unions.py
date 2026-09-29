import json,sys,collections
d=json.load(open(sys.argv[1]))
ex={e['id']:e for e in d['executable']['expressions']}
def kind(t):
    if isinstance(t,str): return t
    k=next(iter(t)); v=t[k]
    if k=='Object': return 'Object'
    if k=='VariantSet':
        # tagged object vs bare tag
        return 'Tag'
    return k
def fields(t):
    if isinstance(t,dict) and 'Object' in t: return tuple(sorted(t['Object']['fields'].keys()))
    return None
stats=collections.Counter(); examples=collections.defaultdict(list)
def arms_of(e):
    k=e['kind']
    if k['kind']=='when': return [a['output'] for a in k['arms']]
    if k['kind']=='list': return k['items']
    if k['kind']=='latest': return k['branches']
    return None
for e in ex.values():
    arms=arms_of(e)
    if not arms or len(arms)<2: continue
    tys=[ex[a]['flow_type']['ty'] for a in arms if ex[a]['kind']['kind']!='absent']
    kinds=set(kind(t) for t in tys)
    kinds.discard('Absent')
    tag=e['kind']['kind']
    if len(kinds)>1:
        stats[tag+':mixed-kind']+=1; examples[tag+':mixed-kind'].append((e.get('resource_binding_path'),sorted(kinds)))
    else:
        fs=set(fields(t) for t in tys)
        if len(fs)>1 and None not in fs:
            stats[tag+':record-shapes']+=1; examples[tag+':record-shapes'].append((e.get('resource_binding_path'),[list(f) for f in fs][:3]))
    t=e['flow_type']['ty']
    if isinstance(t,dict) and 'Object' in t and t['Object'].get('open'):
        stats[tag+':open-object-fallback']+=1
print(sys.argv[1].split('/')[-1], dict(stats))
if '-v' in sys.argv:
    for k,v in examples.items():
        c=collections.Counter(str(x) for x in v)
        print(' ',k)
        for s,n in c.most_common(12): print('    ',n,s[:260])
