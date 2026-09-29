import re,sys,os,collections
# fields defined as `name:\n  Tag |> HOLD` or `name: Tag |> HOLD` (bare tag init) ; then count `.name.<field>` / `name.<field>` reads
res=collections.Counter(); hits=[]
for root in sys.argv[1:]:
    files=[os.path.join(dp,f) for dp,dn,fn in os.walk(root) for f in fn if f.endswith('.bn')] if os.path.isdir(root) else [root]
    srcs={p:re.sub(r'--[^\n]*','',open(p).read()) for p in files}
    allsrc='\n'.join(srcs.values())
    for p,src in srcs.items():
        for m in re.finditer(r'(?m)^\s*([a-z_][a-z0-9_]*):\s*\n?\s*([A-Z][A-Za-z0-9]*)\s*\|>\s*HOLD\b',src):
            name,tag=m.group(1),m.group(2)
            if tag in ('True','False'): continue
            reads=re.findall(r'(?<![A-Za-z0-9_])'+name+r'\.([a-z_][a-z0-9_]*)',allsrc)
            if reads:
                line=src.count('\n',0,m.start())+1
                hits.append((p,line,name,tag,collections.Counter(reads).most_common(4)))
for h in hits: print(f"{h[0]}:{h[1]}: {h[2]} init {h[3]} read fields {h[4]}")
print(len(hits))
