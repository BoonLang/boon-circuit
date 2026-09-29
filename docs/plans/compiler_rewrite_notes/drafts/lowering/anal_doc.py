import json, sys, collections
p=json.load(open(sys.argv[1])); d=p['document']; ex=d['expressions']
print("exprs",len(ex),"functions",len(d['functions']),"templates",len(d['templates']),"materializations",len(d['materializations']),"bindings",len(d['view_bindings']),"constants",len(d['constants']),"names",len(d['names']))
kinds=collections.Counter(e['op']['kind'] for e in ex)
print("op kinds",kinds.most_common())
classes=collections.Counter(e['value_class'] for e in ex)
print("classes",classes)
# templates per compiler_expr_id
tc=collections.Counter(t['compiler_expr_id'] for t in d['templates'])
print("distinct constructor source exprs",len(tc),"templates",len(d['templates']),"max copies",max(tc.values()), "hist", collections.Counter(tc.values()).most_common(10))
# compiler_id copies overall
cc=collections.Counter(e['compiler_id'] for e in ex)
print("distinct compiler ids",len(cc),"max copies",max(cc.values()))
# function body sizes: reachable from body within function
calls=[e for e in ex if e['op']['kind']=='call']
fc=collections.Counter(e['op']['function'] for e in calls)
print("call nodes",len(calls),"distinct callee functions",len(fc), "calls per fn hist", collections.Counter(fc.values()).most_common(6))
# row expressions
re=p['row_expressions']; print('row_expressions', len(re['nodes']) if isinstance(re,dict) else len(re))
print("constants(plan)",len(p['constants']))
print("regions",[(r['kind'],len(r['ops'])) for r in p['regions']])
print("pulse_batches",len(p.get('pulse_batches',[])),"activations",len(p.get('activations',[])),"source_routes",len(p['source_routes']))
sl=p['storage_layout']; print("scalar slots",len(sl['scalar_slots']),"list slots",len(sl['list_slots']), "row fields", sum(len(l.get('row_fields',[])) for l in sl['list_slots']))
print("list_indexes",len(p.get('list_indexes',[])),"list_dataflow",len(p.get('list_dataflow',[])))
print("outputs",len(p['outputs']),"effects",len(p['effects']),"host_ports",len(p.get('host_ports',[])))
per=p['persistence']; print("memory",len(per['memory']),"lists",len(per['lists']),"collections",len(per.get('collections',[])))
dm=p['debug_map']; print("debug_map keys", {k:(len(v) if isinstance(v,list) else v) for k,v in dm.items()})
# sizes per top-level key in compact json
for k,v in p.items():
    print("size",k,len(json.dumps(v,separators=(',',':'))))
