import json,os,pathlib,shutil,socket,subprocess,sys,time,hashlib

base=pathlib.Path(os.environ.get('HERDR_SMOKE_BASE','/out/handoff-smoke')); base.mkdir()
systemd_mode='--systemd' in sys.argv
binary=base/'bin';binary.mkdir()
old=binary/'old';candidate=binary/'candidate'
for source,dest in [(sys.argv[1],old),(sys.argv[2],candidate),(sys.executable,binary/'claude-bin')]:shutil.copyfile(source,dest);dest.chmod(0o755)
config=base/'config';runtime=base/'runtime';state=base/'state';home=base/'home'
for p in [config/'herdr',runtime,state,home]:p.mkdir(parents=True,exist_ok=True)
(config/'herdr'/'config.toml').write_text('onboarding = false\n')
session='feedback-smoke';endpoint=config/'herdr'/'sessions'/session/'herdr.sock'
env={k:v for k,v in os.environ.items() if not k.startswith('HERDR_')}
env.update(HOME=str(home),SHELL='/bin/bash',XDG_CONFIG_HOME=str(config),XDG_RUNTIME_DIR=str(runtime),XDG_STATE_HOME=str(state))
log=(base/'server.log').open('wb');server=None;unit=None;drop=None
def ctl(*args):return subprocess.check_output(['systemctl','--user',*args],text=True).strip()
def unit_state():return dict(x.split('=',1) for x in ctl('show',unit.name,'-p','ActiveState','-p','SubState','-p','MainPID','-p','ExitType','-p','KillMode').splitlines())
if systemd_mode:
 unit=pathlib.Path.home()/'.config/systemd/user'/('herdr-feedback-native-smoke-'+str(os.getpid())+'.service');drop=unit.with_name(unit.name+'.d')
 assert not unit.exists()
 settings='\n'.join('Environment='+k+'='+env[k] for k in ['HOME','SHELL','XDG_CONFIG_HOME','XDG_RUNTIME_DIR','XDG_STATE_HOME'])
 unit.write_text('[Unit]\nDescription=Owned Herdr native cross-version fixture\n[Service]\nType=simple\nRestart=on-failure\nKillMode=control-group\nUnsetEnvironment=HERDR_SOCKET_PATH HERDR_CLIENT_SOCKET_PATH HERDR_SESSION HERDR_ENV\n'+settings+'\nExecStart='+str(old)+' --session '+session+' server\n')
 ctl('daemon-reload');ctl('start',unit.name)
else:server=subprocess.Popen([str(old),'--session',session,'server'],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
def request(method,params=None,allow_error=False):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(40);s.connect(str(endpoint));s.sendall((json.dumps({'id':'smoke-'+method,'method':method,'params':params or {}})+'\n').encode());data=b''
        while not data.endswith(b'\n'):
            part=s.recv(65536)
            if not part:break
            data+=part
    response=json.loads(data)
    if 'error' in response:
        if allow_error:return response
        raise RuntimeError(response)
    return response['result']
def wait_ready():
    for _ in range(200):
        try:return request('ping',{})
        except (OSError,ValueError):time.sleep(.1)
    raise RuntimeError('smoke socket did not become ready')
def pane_info(pane):return request('pane.get',{'pane_id':pane})['pane']
def process_info(pane):return request('pane.process_info',{'pane_id':pane})['process_info']
def heartbeat():return json.loads((base/'heartbeat.json').read_text())
receipt={'fixture_only':True,'session':session,'source':'42ffed3039c2965a32cb951657db2c965e6b55f8','binaries':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in [('old',old),('candidate',candidate)]}}
try:
    wait_ready()
    panes=request('pane.list',{})['panes']
    if not panes:
        request('workspace.create',{'name':'Feedback native fixture','cwd':str(home)})
        panes=request('pane.list',{})['panes']
    assert len(panes)==1,panes
    pane=panes[0]['pane_id']
    fixture_id='00000000-0000-4000-8000-000000000001'
    request('pane.report_agent_session',{'pane_id':pane,'source':'herdr:claude','agent':'claude','agent_session_id':fixture_id,'session_start_source':'startup'})
    job=base/'job.py'
    job.write_text("import os,time,json,pathlib\np=pathlib.Path("+repr(str(base/'heartbeat.json'))+")\ni=0\nwhile True:\n p.with_suffix('.tmp').write_text(json.dumps({'pid':os.getpid(),'counter':i}))\n p.with_suffix('.tmp').replace(p)\n print('HANDOFF_HEARTBEAT',i,flush=True)\n i+=1\n time.sleep(.2)\n")
    time.sleep(.5)
    request('pane.send_input',{'pane_id':pane,'text':str(binary/'claude-bin')+' '+str(job),'keys':['enter']})
    for _ in range(100):
        if (base/'heartbeat.json').exists():break
        time.sleep(.1)
    before={'pane':pane_info(pane),'process':process_info(pane),'heartbeat':heartbeat(),'agent':request('agent.get',{'target':pane},allow_error=True)}
    assert before['agent'].get('error',{}).get('code')=='agent_not_found',before
    if systemd_mode:
        receipt['unit_before']=unit_state();assert receipt['unit_before']['ExitType']=='main'
        drop.mkdir();(drop/'exit-type.conf').write_text('[Service]\nExitType=cgroup\n');ctl('daemon-reload');receipt['unit_reloaded']=unit_state();assert receipt['unit_reloaded']['ExitType']=='cgroup'
    receipt['before']=before
    for name,exe in [('candidate',candidate),('rollback',old)]:
        expected_version=subprocess.check_output([str(exe),'--version'],text=True).strip().removeprefix('herdr ')
        request('server.live_handoff',{'import_exe':str(exe),'expected_protocol':22,'expected_version':expected_version})
        wait_ready();time.sleep(.7)
        after={'pane':pane_info(pane),'process':process_info(pane),'heartbeat':heartbeat()}
        if name=='candidate':
            for _ in range(200):
                agent=request('agent.get',{'target':pane},allow_error=True)
                if agent.get('agent',{}).get('agent')=='claude':break
                time.sleep(.1)
            assert agent.get('agent',{}).get('agent')=='claude',agent
            after['agent']=agent
        if systemd_mode:
            after['unit']=unit_state();assert after['unit']['ActiveState']=='active';assert after['unit']['KillMode']=='control-group'
        assert after['pane']['pane_id']==before['pane']['pane_id'],after
        assert after['pane']['agent_session']==before['pane']['agent_session'],after
        assert after['process']['shell_pid']==before['process']['shell_pid'],after
        assert after['process']['foreground_process_group_id']==before['process']['foreground_process_group_id'],after
        assert after['heartbeat']['pid']==before['heartbeat']['pid'],after
        assert after['heartbeat']['counter']>before['heartbeat']['counter'],after
        os.kill(after['process']['shell_pid'],0);os.kill(after['heartbeat']['pid'],0)
        receipt[name]=after;print(name+' PASS shell='+str(after['process']['shell_pid'])+' job='+str(after['heartbeat']['pid']),flush=True)
        before=after
    receipt['passed']=True
finally:
    (base/'receipt.json').write_text(json.dumps(receipt,indent=2))
    if systemd_mode:
        subprocess.run(['systemctl','--user','stop',unit.name],check=False,capture_output=True)
        receipt['unit_after_stop']=unit_state();(base/'receipt.json').write_text(json.dumps(receipt,indent=2))
        if drop and drop.exists():(drop/'exit-type.conf').unlink();drop.rmdir()
        unit.unlink();ctl('daemon-reload')
    else:
        try:request('server.stop',{})
        except Exception:pass
        try:server.wait(timeout=5)
        except subprocess.TimeoutExpired:server.terminate()
    log.close()
print(json.dumps(receipt),flush=True)
