"""Offline public certificates for SAN/CN parser branches; discard private keys."""
import os
from pathlib import Path
import subprocess
import tempfile
with tempfile.TemporaryDirectory(prefix='unit-certificate-') as name:
    tmp = Path(name)
    for stem,subject,extensions in [
        ('legacy-cn','/CN=legacy-unit.invalid', ''),
        ('ip-san','/O=Unit Fixture', 'subjectAltName=IP:127.0.0.1\n'),
    ]:
        config = tmp / 'request.conf'
        config.write_text('[req]\ndistinguished_name=dn\nx509_extensions=ext\n[dn]\n[ext]\n'+extensions)
        subprocess.run(['/usr/bin/openssl','req','-new','-x509','-newkey','ec',
            '-pkeyopt','ec_paramgen_curve:prime256v1','-nodes','-days','30',
            '-config',str(config),'-subj',subject,'-keyout',str(tmp/'key.pem'),
            '-out',str(Path('server/tests/fixtures')/(stem+'.pem'))], check=True)
print('Created two public parser fixtures; temporary private keys discarded')
