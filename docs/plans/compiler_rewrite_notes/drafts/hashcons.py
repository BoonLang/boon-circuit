import json, sys
sys.setrecursionlimit(100000)
def run(f):
    p=json.load(open(f)); d=p['document']; ex=d['expressions']
    memo={}; table={}
    IDB={'constructor','materialize','call'}
    def canon(i):
        if i in memo: return memo[i]
        e=ex[i]; op=e['op']; k=op['kind']
        def sub(v):
            if isinstance(v,dict): return tuple(sorted((kk,sub(vv)) for kk,vv in v.items()))
            if isinstance(v,list): return tuple(sub(x) for x in v)
            return v
        # replace expression refs: keys named value/input/result/left/right/output/body/expression(document)/branches/items
        def rewrite(v,key=None):
            if isinstance(v,dict):
                return tuple(sorted((kk,rewrite(vv,kk)) for kk,vv in v.items()))
            if isinstance(v,list):
                return tuple(rewrite(x,key) for x in v)
            if isinstance(v,int) and key in ('value','input','result','left','right','output','branches') and k!='constant':
                return ('E',canon(v))
            return v
        body=rewrite(op)
        if k in IDB: body=(body,'uniq',i)
        if k=='select': body=(body,'sel',e['compiler_id'])
        key=(body,e['value_class'])
        if key not in table: table[key]=len(table)
        memo[i]=table[key]; return memo[i]
    # canonicalize everything reachable from root and templates
    for i in range(len(ex)): canon(i)
    print(f, "exprs",len(ex),"hash-consed(conservative)",len(table))
for f in sys.argv[1:]: run(f)
