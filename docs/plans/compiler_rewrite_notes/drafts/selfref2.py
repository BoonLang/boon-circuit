import re,sys,os,collections
tok_re = re.compile(r'--[^\n]*|TEXT\s*\{[^}]*\}|"[^"]*"|[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?|\|>|=>|\.\.\.|==|[\[\](){}:,./]|\S')
LISTFN={'map','retain','remove','any','all','count','find','filter','sort_by','then_by','every','sum','find_map','key_by','group_by','flat_map','fold'}
def lex(src):
    out=[]
    for m in tok_re.finditer(src):
        t=m.group(0)
        if t.startswith('--'): continue
        out.append((t,src.count('\n',0,m.start())+1))
    return out
def scan(path):
    toks=lex(open(path).read()); n=len(toks)
    stack=[]; hits=[]
    for i,(t,line) in enumerate(toks):
        if t in '([{':
            kind='BLOCK' if (t=='{' and i>0 and toks[i-1][0]=='BLOCK') else t
            stack.append(kind)
        elif t in ')]}':
            if stack: stack.pop()
        elif t==':' and stack and stack[-1] in ('[','BLOCK'):
            name=toks[i-1][0]
            if not re.match(r'[a-z_][a-z0-9_]*$',name) or (i>1 and toks[i-2][0] in ('.','/')): continue
            depth=0; j=i+1; binders=set(); gated=False; exact=False
            # exact bare copy
            if j+1<n and toks[j][0]==name and toks[j+1][0] in (',',']','}',')') or (j+1<n and toks[j][0]==name and toks[j+1][1]!=toks[j][1] and toks[j+1][0]!='|>'):
                hits.append((line,name,'copy',line)); continue
            gate_depth=None
            while j<n:
                tj,lj=toks[j]
                if tj in '([{':
                    depth+=1
                elif tj in ')]}':
                    if depth==0: break
                    depth-=1
                elif depth==0 and tj==',': break
                elif depth==0 and j>i+1 and lj!=toks[j-1][1] and tj!='|>' and not tj in ('==','+','-','*','/','.'):
                    # new line at same depth, not a continuation
                    break
                if tj=='HOLD' and j+1<n: binders.add(toks[j+1][0])
                if tj in LISTFN and j+2<n and toks[j+1][0]=='(': binders.add(toks[j+2][0])
                if tj=='=>' and re.match(r'[a-z_]',toks[j-1][0]) and toks[j-2][0]!='.': binders.add(toks[j-1][0])
                if tj in ('THEN',) : gated=True
                if tj==name and toks[j-1][0] not in ('.','/') and not (j+1<n and toks[j+1][0] in (':','(')) and name not in binders:
                    hits.append((line,name,'gated' if gated else 'direct',lj)); break
                j+=1
    return hits
tot=collections.Counter(); files=collections.Counter()
for dp,dn,fn in os.walk(sys.argv[1]):
    for f in sorted(fn):
        if f.endswith('.bn'):
            p=os.path.join(dp,f)
            for h in scan(p):
                tot[h[2]]+=1; files[p]+=1
                if h[2]!='copy' or '-v' in sys.argv: print(f"{p}:{h[0]}: {h[2]} `{h[1]}` (ref line {h[3]})")
print(tot); 
for p,c in files.most_common(40): print(c,p)
