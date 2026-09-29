import re,sys,collections
# find `style: [` records, collect top-level field names and whether material: uses a Theme call
tok=re.compile(r'--[^\n]*|TEXT\s*\{[^}]*\}|"[^"]*"|\.\.\.|[A-Za-z_][A-Za-z0-9_/]*|[\[\](){}:,]|\n|\S')
def scan(path, theme_prefix):
    src=open(path).read()
    toks=[(m.group(0),src.count('\n',0,m.start())+1) for m in tok.finditer(src) if not m.group(0).startswith('--')]
    out=[]
    for i,(t,l) in enumerate(toks):
        if t=='style' and i+2<len(toks) and toks[i+1][0]==':' and toks[i+2][0]=='[':
            depth=0; j=i+2; fields=[]; spreads=[]; mat=None; k0=j
            start_line=l
            while j<len(toks):
                tj=toks[j][0]
                if tj in '([{': depth+=1
                elif tj in ')]}':
                    depth-=1
                    if depth==0: break
                elif depth==1:
                    if tj=='...' : spreads.append(toks[j+1][0])
                    elif re.match(r'[a-z_][a-z0-9_]*$',tj) and j+1<len(toks) and toks[j+1][0]==':' and toks[j-1][0] in ('[',',','\n'):
                        fields.append(tj)
                        if tj=='material':
                            # does the value contain a Theme call?
                            d=0;k=j+2; s=[]
                            while k<len(toks):
                                tk=toks[k][0]
                                if tk in '([{': d+=1
                                elif tk in ')]}':
                                    if d==0: break
                                    d-=1
                                elif d==0 and (tk==',' or tk=='\n'): 
                                    if s and s[-1] not in ('|>',): break
                                s.append(tk); k+=1
                            mat=' '.join(x for x in s if x!='\n')[:80]
                j+=1
            if mat and theme_prefix in mat:
                coll=[f for f in fields if f in ('border','border_width','background','outline','borders')]
                sp=[s for s in spreads if 'material' in s or 'Frame' in s]
                out.append((start_line,coll,spreads,mat))
    return out
tot=collections.Counter()
for p in sys.argv[2:]:
    for (l,coll,sp,mat) in scan(p, sys.argv[1]):
        key='collide' if coll else 'clean'
        tot[key]+=1
        if coll or sp: print(f"{p}:{l}: collide={coll} spreads={sp} material={mat}")
print(tot)
