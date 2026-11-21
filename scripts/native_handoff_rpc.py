#!/usr/bin/env python3
"""Submit the supported local Herdr live-handoff request once. Never auto-retry."""
import argparse,hashlib,json,pathlib,socket,sys,uuid
p=argparse.ArgumentParser()
p.add_argument('--socket',required=True)
p.add_argument('--candidate',required=True)
p.add_argument('--sha256',required=True)
p.add_argument('--expected-version',required=True)
p.add_argument('--expected-protocol',type=int,default=22)
a=p.parse_args()
candidate=pathlib.Path(a.candidate)
if not candidate.is_absolute() or not candidate.is_file():p.error('candidate must be an existing absolute file')
h=hashlib.sha256()
with candidate.open('rb') as f:
 for part in iter(lambda:f.read(1024*1024),b''):h.update(part)
if h.hexdigest()!=a.sha256.lower():p.error('candidate SHA-256 mismatch; request not sent')
endpoint=pathlib.Path(a.socket)
if not endpoint.is_absolute():p.error('socket must be absolute')
request={'id':'native-handoff-'+uuid.uuid4().hex,'method':'server.live_handoff','params':{'import_exe':str(candidate),'expected_protocol':a.expected_protocol,'expected_version':a.expected_version}}
try:
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(45);client.connect(str(endpoint));client.sendall((json.dumps(request)+'\n').encode());response=b''
  while not response.endswith(b'\n'):
   chunk=client.recv(65536)
   if not chunk:break
   response+=chunk
   if len(response)>1024*1024:raise RuntimeError('response exceeded bound')
 reply=json.loads(response)
 if reply.get('id')!=request['id']:raise RuntimeError('response id mismatch')
 print(json.dumps(reply))
 raise SystemExit(1 if 'error' in reply else 0)
except (OSError,ValueError,RuntimeError) as error:
 print(json.dumps({'id':request['id'],'state':'uncertain','error':str(error),'instruction':'Do not retry automatically. Inspect service, pane identity, processes and readiness first.'}),file=sys.stderr)
 raise SystemExit(2)
