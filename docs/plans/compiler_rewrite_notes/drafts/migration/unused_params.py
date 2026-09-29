import re,sys,os,collections
tot=0; hits=[]
for root in sys.argv[1:]:
    files=[os.path.join(dp,f) for dp,dn,fn in os.walk(root) for f in fn if f.endswith('.bn')] if os.path.isdir(root) else [root]
    for p in files:
        src=re.sub(r'--[^\n]*','',open(p).read())
        src=re.sub(r'TEXT\s*\{[^}]*\}','TEXT{}',src)
        for m in re.finditer(r'(?m)^FUNCTION\s+([a-z_][a-z0-9_]*)\s*\(([^)]*)\)\s*\{',src):
            name=m.group(1); params=[x.strip().split(':')[0].strip() for x in m.group(2).split(',') if x.strip()]
            # body: until next line starting with '}' at column 0
            end=re.search(r'(?m)^\}',src[m.end():])
            body=src[m.end(): m.end()+(end.start() if end else len(src))]
            for prm in params:
                if not re.search(r'(?<![A-Za-z0-9_.])'+re.escape(prm)+r'(?![A-Za-z0-9_])',body):
                    hits.append(f"{p}:{src.count(chr(10),0,m.start())+1}: FUNCTION {name} param `{prm}` unused")
for h in hits: print(h)
print(len(hits))
