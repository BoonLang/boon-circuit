import json,sys,collections
d=json.load(open(sys.argv[1]))
ex={e['id']:e for e in d['executable']['expressions']}
def desc(i):
    e=ex[i]; k=e['kind']
    s=k['kind']
    for key in ('binding_path','name','path','function','callee','value'):
        if key in k: s+=f" {key}={str(k[key])[:60]}"
    if e.get('resource_binding_path'): s+=f" @{e['resource_binding_path']}"
    return s
def tyk(t):
    if isinstance(t,str): return t
    return next(iter(t.keys()))
cnt=collections.Counter()
for e in ex.values():
    k=e['kind']
    if k['kind']=='then':
        m=ex[k['input']]['flow_type']['mode']
        cnt[m]+=1
        if m=='Continuous': print('THEN on Continuous input:', desc(k['input']))
    if k['kind']=='latest':
        arms=k.get('arms') or k.get('inputs') or k.get('branches')
        if arms:
            kinds=set(tyk(ex[a]['flow_type']['ty']) for a in arms)
            if len(kinds)>1: print('LATEST mixed kinds', kinds, [desc(a) for a in arms][:4])
print(cnt)
