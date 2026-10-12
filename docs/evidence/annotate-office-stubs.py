"""Read-only annotation of captured LLDB assembly for the analyzed Acrobat build.

This is supplementary shell evidence, not an REA evidence producer. The input
path, ARM64 slice offset, and instruction layout are deliberately build-specific.
It never launches or attaches to Acrobat. Addresses are unslid virtual addresses.
"""
from pathlib import Path
import re
import struct
import hashlib

TARGET = Path('/Applications/Adobe Acrobat DC/Adobe Acrobat.app/Contents/Frameworks/Acrobat.framework/Versions/A/Acrobat')
SLICE_OFFSET = 83263488
artifact = TARGET.read_bytes()
assert hashlib.sha256(artifact).hexdigest() == 'ddbe975f487125b09dc8038fe91a6d1f106b070ba62069f8cc6ec2928fe27bd1'
data = artifact[SLICE_OFFSET:]
source = Path(__file__).with_name('office-converters-arm64.txt')

def u32(address):
    return struct.unpack_from('<I', data, address)[0]

def selector(address):
    adrp, ldr = u32(address), u32(address + 4)
    assert adrp & 0x9f00001f == 0x90000001
    assert ldr & 0xffc003ff == 0xf9400021
    imm = ((adrp >> 29) & 3) | (((adrp >> 5) & 0x7ffff) << 2)
    if imm & (1 << 20):
        imm -= 1 << 21
    slot = (address & ~0xfff) + (imm << 12) + (((ldr >> 10) & 0xfff) << 3)
    # DYLD_CHAINED_PTR_64_OFFSET unbound rebase targets in this build's selrefs.
    pointer = struct.unpack_from('<Q', data, slot)[0]
    assert pointer >> 63 == 0
    target = pointer & ((1 << 36) - 1)
    name = data[target:data.index(b'\0', target)].decode('ascii')
    return name, slot, target

annotated = []
for line in source.read_text().splitlines():
    match = re.search(r'\b(?:bl|b)\s+(0x[0-9a-f]+)', line)
    if match and 0x3906620 <= int(match[1], 16) < 0x3917a20:
        name, slot, target = selector(int(match[1], 16))
        line += f' ; ObjC selector {name} [selref={slot:#x}, string={target:#x}]'
    annotated.append(line)
source.with_name('office-converters-annotated-arm64.txt').write_text('\n'.join(annotated) + '\n')
