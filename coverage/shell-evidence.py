"""Validate raw Bashcov line counters and TAP; retain JUnit without inventing unsupported metrics."""
import json,pathlib,re,sys,xml.etree.ElementTree as ET
folder=pathlib.Path(sys.argv[1]);raw=json.loads((folder/'raw-lines.json').read_text());assert len(raw['files'])==10
values=[hit for lines in raw['files'].values() for hit in lines if hit is not None]
missing={path:[index+1 for index,hit in enumerate(lines) if hit==0] for path,lines in raw['files'].items() if any(hit==0 for hit in lines)}
records=re.findall(r'^(not ok|ok) (\d+) - (.*)$',(folder/'run.log').read_text(),re.M)
plans=re.findall(r'^1\.\.(\d+)$',(folder/'run.log').read_text(),re.M)
assert records and plans==[str(len(records))],(len(records),plans)
assert [int(x[1]) for x in records]==list(range(1,len(records)+1))
root=ET.Element('testsuites');suite=ET.SubElement(root,'testsuite',name='bash-unit',tests=str(len(records)),failures=str(sum(kind=='not ok' for kind,_,_ in records)),errors='0',skipped='0')
for kind,number,name in records:
 case=ET.SubElement(suite,'testcase',name=number+' '+name)
 if kind=='not ok':ET.SubElement(case,'failure').text=name
ET.ElementTree(root).write(folder/'unit.xml',encoding='utf-8',xml_declaration=True)
print(json.dumps({'lines':{'covered':sum(hit>0 for hit in values),'valid':len(values)},'branches':'unsupported by Bashcov; no percentage claimed','functions':'unsupported by Bashcov; no percentage claimed','tests':len(records),'missing':missing},indent=2))
if missing or any(kind=='not ok' for kind,_,_ in records):raise SystemExit(1)
