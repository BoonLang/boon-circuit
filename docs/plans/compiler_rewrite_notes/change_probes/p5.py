import subprocess,sys,re
BIN='/home/martinkavik/repos/boon-circuit/target/release/boon_cli'
def run(src, steps, roots):
    # steps: list of (id, event_source or None, payload dict)
    out=[]
    for n in range(len(steps)):
        lines=[f'name = "x"', f'source = "{src}"', '']
        for i,(sid,ev,payload) in enumerate(steps[:n+1]):
            lines.append('[[step]]'); lines.append(f'id = "{sid}"')
            if ev:
                extra=''.join(f', {k} = "{v}"' for k,v in payload.items())
                lines.append(f'expected_source_event = {{ source = "{ev}"{extra} }}')
            if i==n:
                lines.append('  [step.expect_root_text]')
                for r in roots: lines.append(f'  "{r}" = "?"')
            lines.append('')
        open('tmp.scn','w').write('\n'.join(lines))
        p=subprocess.run([BIN,'run',src,'--scenario','tmp.scn'],capture_output=True,text=True)
        msg=p.stderr.strip() or p.stdout.strip()
        vals=dict(re.findall(r"root `([^`]+)` expected `\?`, got `([^`]*)`",msg))
        errs=[s for s in msg.split('; ') if 'expected `?`' not in s]
        out.append((steps[n][0], vals, errs))
        print(steps[n][0], vals, errs if errs else '')
run(sys.argv[1], eval(sys.argv[2]), sys.argv[3].split(','))
