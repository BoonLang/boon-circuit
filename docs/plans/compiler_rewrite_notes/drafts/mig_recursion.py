import re,os,sys,collections
# build function graph per example directory (module = file stem), detect cycles
graphs=collections.defaultdict(dict)
for dp,dn,fn in os.walk('examples'):
    for f in fn:
        if not f.endswith('.bn'): continue
        p=os.path.join(dp,f); src=open(p).read()
        src=re.sub(r'--[^\n]*','',src)
        root=p.split('/')[1] if '/' in p[len('examples/'):] else p
        mod=f[:-3]
        parts=[(m.start(),m.group(1)) for m in re.finditer(r'^FUNCTION\s+([a-z_][a-z0-9_]*)\s*\(',src,re.M)]
        for i,(s,name) in enumerate(parts):
            e=parts[i+1][0] if i+1<len(parts) else len(src)
            body=src[s:e]
            body=body[body.index('(')+1:]
            calls=set()
            for m in re.finditer(r'(?<![\w.])((?:[A-Z][A-Za-z0-9]*/)?[a-z_][a-z0-9_]*)\s*\(',body):
                c=m.group(1)
                if '/' in c:
                    ns,fn_=c.split('/')
                    calls.add((ns,fn_))
                else: calls.add((mod,c))
            graphs[root][(mod,name)]=calls
def sccs(g):
    idx={};low={};st=[];on=set();res=[];i=[0]
    sys.setrecursionlimit(100000)
    def sc(v):
        idx[v]=low[v]=i[0];i[0]+=1;st.append(v);on.add(v)
        for w in g.get(v,()):
            if w not in g: continue
            if w not in idx: sc(w);low[v]=min(low[v],low[w])
            elif w in on: low[v]=min(low[v],idx[w])
        if low[v]==idx[v]:
            comp=[]
            while True:
                w=st.pop();on.discard(w);comp.append(w)
                if w==v:break
            res.append(comp)
    for v in g:
        if v not in idx: sc(v)
    return res
for root,g in graphs.items():
    for c in sccs(g):
        if len(c)>1 or (c[0] in g.get(c[0],())):
            print(root, 'CYCLE', c)
print('functions:',sum(len(g) for g in graphs.values()))
