"""Retain exact invocation, source revision, exit status and unmodified stdout/stderr."""
import datetime
import json
import pathlib
import subprocess
import sys
output=pathlib.Path(sys.argv[1]);output.mkdir(parents=True,exist_ok=True)
metadata={'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip(),'worktree':str(pathlib.Path.cwd()),'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':sys.argv[2:]}
with (output/'command.json').open('w') as handle:json.dump(metadata,handle,indent=2)
with (output/'run.log').open('w') as handle:code=subprocess.run(sys.argv[2:],stdout=handle,stderr=subprocess.STDOUT).returncode
metadata.update(unit_command_exit_code=code,finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat());(output/'command.json').write_text(json.dumps(metadata,indent=2)+'\n')
sys.exit(code)
