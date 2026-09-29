import re,sys,os,collections
# Detect `P |> WHEN { ... Tag => ... P.field ... }` : reading a payload field through the scrutinee inside a bare-tag arm.
tokre=re.compile(r'--[^\n]*|TEXT\s*\{[^}]*\}|"[^"]*"|\|>|=>|[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*|\S')
tot=collections.Counter(); perfile=collections.Counter(); samples=[]
for root in sys.argv[1:]:
    files=[os.path.join(dp,f) for dp,dn,fn in os.walk(root) for f in fn if f.endswith('.bn')] if os.path.isdir(root) else [root]
    for p in files:
        src=open(p).read()
        toks=[(m.group(0),src.count('\n',0,m.start())+1) for m in tokre.finditer(src) if not m.group(0).startswith('--')]
        n=len(toks)
        for i in range(n-3):
            P=toks[i][0]
            if not re.match(r'[a-z_]',P) or toks[i+1][0]!='|>' or toks[i+2][0]!='WHEN' or toks[i+3][0]!='{': continue
            # walk block
            depth=0; j=i+3; arms=[]; cur=None
            while j<n:
                t=toks[j][0]
                if t in '{([': depth+=1
                elif t in '})]':
                    depth-=1
                    if depth==0: break
                elif depth==1 and t=='=>':
                    # pattern is previous token(s)
                    pat=toks[j-1][0]
                    bare = re.match(r'[A-Z][A-Za-z0-9]*$',pat) and toks[j-2][0] not in (']',)
                    cur={'pat':pat,'bare':bool(bare),'line':toks[j][1],'reads':[]}
                    arms.append(cur)
                elif cur is not None and (t.startswith(P+'.') ):
                    cur['reads'].append(t[len(P)+1:].split('.')[0])
                j+=1
            for a in arms:
                if a['bare'] and a['reads']:
                    tot['bare-arm-reads-scrutinee']+=1; perfile[p]+=1
                    if len(samples)<400: samples.append(f"{p}:{a['line']}: {P} |> WHEN {{ {a['pat']} => ...{P}.{a['reads'][0]} }}")
for s in samples[:25]: print(s)
print(dict(tot))
for f,c in perfile.most_common(20): print(c,f)
