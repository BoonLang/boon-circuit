#!/usr/bin/env python3
"""Poor-man's sampling profiler: drive gdb/MI, interrupt the inferior at random
intervals, record the full unwound stack (via .eh_frame CFI) each time.
No perf events needed (perf_event_paranoid stays untouched).

usage: gdbprof.py <out.jsonl> <runs> <mean_interval_ms> -- <program args...>
Run from the repo root.
"""
import os, sys, time, random, re, signal, subprocess, threading, queue, json

BIN = './target/release/boon_cli'
FRAME_RE = re.compile(r'frame=\{level="(\d+)",addr="(0x[0-9a-f]+)"(?:,func="([^"]*)")?')


def run_one(prog_args, mean_ms, out_redirect, max_frames=400):
    gdb = subprocess.Popen(['gdb', '-q', '-nx', '--interpreter=mi2', BIN],
                           stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=subprocess.DEVNULL, text=True, bufsize=1)
    q = queue.Queue()

    def reader():
        for line in gdb.stdout:
            q.put(line)
        q.put(None)
    threading.Thread(target=reader, daemon=True).start()

    def send(cmd):
        gdb.stdin.write(cmd + '\n')
        gdb.stdin.flush()

    def read_until(pred, timeout=60):
        lines = []
        deadline = time.time() + timeout
        while True:
            line = q.get(timeout=max(0.01, deadline - time.time()))
            if line is None:
                raise EOFError(''.join(lines[-5:]))
            lines.append(line)
            if pred(line):
                return lines

    read_until(lambda l: l.startswith('(gdb)'))
    for cmd in ['-gdb-set pagination off',
                '-gdb-set startup-with-shell on',
                '-interpreter-exec console "handle SIGINT stop print nopass"',
                '-exec-arguments ' + ' '.join(prog_args) + ' > ' + out_redirect]:
        send(cmd)
        read_until(lambda l: l.startswith('^done') or l.startswith('^error'))
    send('-exec-run')
    pid = None
    lines = read_until(lambda l: l.startswith('*running'))
    for l in lines:
        m = re.search(r'thread-group-started,id="i1",pid="(\d+)"', l)
        if m:
            pid = int(m.group(1))
    if pid is None:
        raise RuntimeError('no pid: ' + ''.join(lines))
    samples = []
    exited = False
    while not exited:
        time.sleep(random.uniform(0.3, 1.7) * mean_ms / 1000.0)
        # drain: did it exit already?
        try:
            while True:
                l = q.get_nowait()
                if l is None or l.startswith('*stopped') and 'exited' in l:
                    exited = True
                    break
        except queue.Empty:
            pass
        if exited:
            break
        try:
            os.kill(pid, signal.SIGINT)
        except ProcessLookupError:
            break
        lines = read_until(lambda l: l.startswith('*stopped'))
        if 'exited' in lines[-1]:
            break
        send('-stack-list-frames 0 %d' % max_frames)
        lines = read_until(lambda l: l.startswith('^done') or l.startswith('^error'))
        frames = []
        for l in lines:
            if l.startswith('^done'):
                for m in FRAME_RE.finditer(l):
                    frames.append((m.group(3) or '??', m.group(2)))
        samples.append(frames)
        send('-exec-continue')
        lines = read_until(lambda l: l.startswith('*running') or (l.startswith('*stopped') and 'exited' in l))
        if lines[-1].startswith('*stopped'):
            break
    try:
        send('-gdb-exit')
        gdb.wait(timeout=10)
    except Exception:
        gdb.kill()
    return samples


def main():
    out = sys.argv[1]
    runs = int(sys.argv[2])
    mean_ms = float(sys.argv[3])
    assert sys.argv[4] == '--'
    prog = sys.argv[5:]
    with open(out, 'w') as f:
        for r in range(runs):
            t0 = time.time()
            s = run_one(prog, mean_ms, '/dev/null')
            for frames in s:
                f.write(json.dumps({'run': r, 'frames': frames}) + '\n')
            print(f'run {r}: {len(s)} samples in {time.time()-t0:.1f}s', file=sys.stderr)


if __name__ == '__main__':
    main()
