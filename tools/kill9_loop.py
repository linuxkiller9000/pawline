import os, pty, subprocess, time, select, signal, json, sys
BIN="target/debug/pawline"; JREL=".local/share/pawline/journal.jsonl"
def drain(mm, dur):
    out=b""; end=time.time()+dur
    while time.time()<end:
        r,_,_=select.select([mm],[],[],0.02)
        if r:
            try: out+=os.read(mm,65536)
            except OSError: break
    return out
def spawn(env):
    m,s=pty.openpty()
    p=subprocess.Popen([BIN,"--no-ui"],stdin=s,stdout=s,stderr=s,env=env,close_fds=True); os.close(s); return p,m
def run_once(delay,n=300):
    root,data="/tmp/k9_root","/tmp/k9_data"
    os.system("rm -rf %s %s"%(root,data)); os.makedirs(root); os.makedirs(data)
    env=dict(os.environ,XDG_RUNTIME_DIR="/tmp/xrt_fd",PAWLINE_ROOT=root,PAWLINE_DATA=data,PATH="/tmp/zx:"+os.environ["PATH"])
    p,m=spawn(env); drain(m,5)
    t0=time.time(); i=0
    while time.time()-t0<delay and i<n:
        i+=1; os.write(m,("make a folder called k%d\n"%i).encode()); drain(m,0.005)
    os.kill(p.pid,signal.SIGKILL); p.wait()
    at_kill=len([d for d in os.listdir(root) if d.startswith("k")])
    p2,m2=spawn(env); out=drain(m2,6)
    os.write(m2,b"quit\n"); drain(m2,1); p2.terminate()
    rec=[l for l in out.decode(errors="replace").splitlines() if "recover" in l.lower()]
    final=len([d for d in os.listdir(root) if d.startswith("k")])
    opn={};com=0;ab=0;bad=0;ints=0
    jp=os.path.join(root,JREL)
    if os.path.exists(jp):
        for l in open(jp):
            l=l.strip()
            if not l: continue
            try: r=json.loads(l)
            except Exception: bad+=1; continue
            if "intent" in r: ints+=1; opn[r["intent"]["id"]]=1
            elif "commit" in r: com+=1; opn.pop(r["commit"],None)
            elif "abort" in r: ab+=1; opn.pop(r["abort"],None)
    print("delay=%.2f sent=%d at_kill=%d final=%d recov_msgs=%d | intents=%d commits=%d aborts=%d unsettled=%d corrupt=%d | disk==commits:%s"%(delay,i,at_kill,final,len(rec),ints,com,ab,len(opn),bad,final==com),flush=True)
    if rec: print("   e.g.",rec[0][:120],flush=True)
for d in sys.argv[1:]: run_once(float(d))
