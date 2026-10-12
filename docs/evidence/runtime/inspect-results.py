from pathlib import Path
from pypdf import PdfReader
import json,hashlib
root=Path(__file__).resolve().parent

def inspect(name):
 p=root/name; r=PdfReader(p)
 refs={page.indirect_reference.idnum:i+1 for i,page in enumerate(r.pages)}
 def dest(d):
  if isinstance(d,list):
   return {'page':refs.get(getattr(d[0],'idnum',None)), 'view':[str(x) for x in d[1:]]}
  return str(d)
 def bookmarks(obj):
  out=[]
  if obj is None:return out
  while obj:
   obj=obj.get_object()
   action=obj.get('/A'); action=action.get_object() if action else {}
   row={'title':str(obj.get('/Title')), 'destination':dest(obj.get('/Dest',action.get('/D')))}
   if obj.get('/First'):row['children']=bookmarks(obj['/First'])
   out.append(row);obj=obj.get('/Next')
  return out
 pages=[]
 for pge in r.pages:
  annots=[]
  for a in pge.get('/Annots',[]):
   a=a.get_object();annots.append({'subtype':str(a.get('/Subtype')),'destination':dest(a.get('/Dest')), 'action':str(a.get('/A'))})
  pages.append({'text':pge.extract_text().strip(),'media_box':list(pge.mediabox),'crop_box':list(pge.cropbox),'rotation':pge.rotation,'annotations':annots})
 outline=r.trailer['/Root'].get('/Outlines');outline=outline.get_object() if outline else {}
 return {'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'pages':pages,'bookmarks':bookmarks(outline.get('/First')),'named_destinations':{k:r.get_destination_page_number(v)+1 for k,v in r.named_destinations.items()},'metadata':dict(r.metadata or {})}
result={'evidence_kind':'controlled-runtime-observation (not an REA Evidence record)','application':'/Applications/Adobe Acrobat DC/Adobe Acrobat.app','version':'26.002.22008','entry_point':'AppleScript CARO/inpg insert pages; not the Combine button','scope':'Synthetic fixtures only; inserted two pages after page one, bookmarks true/false; no debugger or network trace','results':{n:inspect(n) for n in ['rea-base.pdf','rea-source.pdf','rea-with-bookmarks.pdf','rea-without-bookmarks.pdf']}}
(root/'observations.json').write_text(json.dumps(result,indent=2,default=str)+'\n')
for n in ['rea-with-bookmarks.pdf','rea-without-bookmarks.pdf']:
 r=result['results'][n]
 assert [p['text'] for p in r['pages']]==['BASE PAGE','SOURCE FIRST','SOURCE SECOND']
 assert [p['rotation'] for p in r['pages']]==[0,90,90]
 assert r['pages'][1]['media_box']==[0,0,600,300]
 assert r['pages'][1]['crop_box']==[10,20,590,280]
 assert len(r['bookmarks'])==(2 if n=='rea-with-bookmarks.pdf' else 1)
print(json.dumps(result['results'],indent=2,default=str))
