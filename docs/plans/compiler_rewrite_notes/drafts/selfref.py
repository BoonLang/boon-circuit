import re,sys,os
tok_re = re.compile(r'--[^\n]*|TEXT\s*\{[^}]*\}|"[^"]*"|[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?|\|>|=>|\.\.\.|[\[\](){}:,./]|\S')
def scan(path):
    src=open(path).read()
    toks=[]
    for m in tok_re.finditer(src):
        t=m.group(0)
        if t.startswith('--'): continue
        line=src.count('\n',0,m.start())+1
        toks.append((t,line))
    hits=[]
    # stack of (opener, kind)
    stack=[]
    i=0
    n=len(toks)
    # find record-literal or BLOCK fields: ident ':' where top of stack is '[' or BLOCK-'{'
    prev_kw=None
    for i,(t,line) in enumerate(toks):
        if t in '([{':
            kind=t
            if t=='{' and i>0 and toks[i-1][0]=='BLOCK': kind='BLOCK'
            elif t=='{' : kind='{'
            stack.append((kind,i))
        elif t in ')]}':
            if stack: stack.pop()
        elif t==':' and i>0 and stack and stack[-1][0] in ('[','BLOCK'):
            name=toks[i-1][0]
            if not re.match(r'[a-z_][a-z0-9_]*$',name): continue
            if i>1 and toks[i-2][0] in ('.','/'): continue
            # value extent: until next token at same depth that is ident followed by ':' or closing
            depth=0; j=i+1; binders=set()
            while j<n:
                tj=toks[j][0]
                if tj in '([{': depth+=1
                elif tj in ')]}':
                    if depth==0: break
                    depth-=1
                elif depth==0 and j+1<n and toks[j+1][0]==':' and re.match(r'[a-z_]',tj) and toks[j-1][0] not in ('.','/','(',','):
                    # next field at same depth (newline-separated)
                    if toks[j][1]!=toks[j-1][1] or toks[j-1][0]==',':
                        break
                elif depth==0 and tj==',' : break
                # binders
                if tj=='HOLD' and j+1<n: binders.add(toks[j+1][0])
                if tj in ('map','retain','remove','any','all','count','find','filter','sort_by','then_by','every','sum','find_map') and j+2<n and toks[j+1][0]=='(':
                    binders.add(toks[j+2][0])
                if tj=='=>' and re.match(r'[a-z_]',toks[j-1][0]) and toks[j-2][0] not in ('.',):
                    binders.add(toks[j-1][0])
                if tj==name and toks[j-1][0] not in ('.','/') and not (j+1<n and toks[j+1][0] in (':','(')) and name not in binders:
                    hits.append((line,name,toks[j][1]))
                    break
                j+=1
    return hits
root=sys.argv[1]
for dp,dn,fn in os.walk(root):
    for f in fn:
        if f.endswith('.bn'):
            p=os.path.join(dp,f)
            for h in scan(p):
                print(f"{p}:{h[0]}: field `{h[1]}` references itself (line {h[2]})")
