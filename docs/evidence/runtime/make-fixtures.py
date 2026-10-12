from pathlib import Path
from pypdf import PdfWriter
from pypdf.generic import NameObject, NumberObject, DictionaryObject, DecodedStreamObject, ArrayObject, FloatObject, TextStringObject
import shutil, hashlib, json
root=Path(__file__).resolve().parent

def make(name,labels,w,h,rotate):
    writer=PdfWriter()
    for label in labels:
        page=writer.add_blank_page(width=w,height=h)
        font=writer._add_object(DictionaryObject({NameObject('/Type'):NameObject('/Font'),NameObject('/Subtype'):NameObject('/Type1'),NameObject('/BaseFont'):NameObject('/Helvetica')}))
        page[NameObject('/Resources')]=DictionaryObject({NameObject('/Font'):DictionaryObject({NameObject('/F1'):font})})
        stream=DecodedStreamObject(); stream.set_data(f'BT /F1 18 Tf 40 80 Td ({label}) Tj ET'.encode())
        page[NameObject('/Contents')]=writer._add_object(stream)
    parent=writer._pages.get_object()
    parent[NameObject('/MediaBox')]=ArrayObject([NumberObject(x) for x in [0,0,w,h]])
    parent[NameObject('/CropBox')]=ArrayObject([NumberObject(x) for x in [10,20,w-10,h-20]])
    parent[NameObject('/Rotate')]=NumberObject(rotate)
    for page in writer.pages:
        for key in ['/MediaBox','/CropBox','/Rotate']: page.pop(key,None)
    writer.add_outline_item(name+' bookmark',len(labels)-1)
    writer.add_named_destination('shared-destination',len(labels)-1)
    writer.pages[0][NameObject('/Annots')]=ArrayObject([writer._add_object(DictionaryObject({NameObject('/Type'):NameObject('/Annot'),NameObject('/Subtype'):NameObject('/Link'),NameObject('/Rect'):ArrayObject([NumberObject(x) for x in [40,70,220,100]]),NameObject('/Border'):ArrayObject([NumberObject(0)]*3),NameObject('/Dest'):TextStringObject('shared-destination')}))])
    with (root/name).open('wb') as f: writer.write(f)
make('rea-base.pdf',['BASE PAGE'],400,500,0)
make('rea-source.pdf',['SOURCE FIRST','SOURCE SECOND'],600,300,90)
for name in ['rea-with-bookmarks.pdf','rea-without-bookmarks.pdf']: shutil.copyfile(root/'rea-base.pdf',root/name)
(root/'fixture-hashes.json').write_text(json.dumps({p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in root.glob('*.pdf')},indent=2)+'\n')
