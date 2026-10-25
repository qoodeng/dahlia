"""Read installed Stickies' compiled resource; never instantiate objects or read notes."""
import hashlib
import json
import struct
from pathlib import Path

path = Path('/System/Applications/Stickies.app/Contents/Resources/Base.lproj/SNDocument.nib')
data = path.read_bytes()
assert data[:10] == b'NIBArchive'
version, coder, objects, object_offset, keys, key_offset, values, value_offset, classes, class_offset = struct.unpack_from('<10I', data, 10)
assert (version, coder) == (1, 10)

def integer(offset):
    value, shift = 0, 0
    while True:
        byte = data[offset]
        offset += 1
        value |= (byte & 127) << shift
        shift += 7
        if byte & 128:
            return value, offset

offset, names = key_offset, []
for _ in range(keys):
    length, offset = integer(offset)
    names.append(data[offset:offset + length].decode())
    offset += length
assert offset == value_offset

offset, entries = value_offset, []
for _ in range(values):
    key, offset = integer(offset)
    kind = data[offset]
    offset += 1
    if kind in (0, 1, 2, 3, 6, 7, 10):
        fmt = {0: 'B', 1: 'H', 2: 'I', 3: 'Q', 6: 'f', 7: 'd', 10: 'I'}[kind]
        value = struct.unpack_from('<' + fmt, data, offset)[0]
        offset += struct.calcsize(fmt)
    elif kind in (4, 5, 9):
        value = {4: True, 5: False, 9: None}[kind]
    elif kind == 8:
        length, offset = integer(offset)
        value = data[offset:offset + length].hex()
        offset += length
    else:
        raise ValueError(f'Unknown NIB value type {kind}')
    entries.append((names[key], kind, value))
assert offset == class_offset

offset, class_names = class_offset, []
for _ in range(classes):
    length, offset = integer(offset)
    extra, offset = integer(offset)
    offset += extra * 4
    class_names.append(data[offset:offset + length].rstrip(b'\0').decode())
    offset += length
assert offset == len(data)

offset, windows = object_offset, []
for index in range(objects):
    class_index, offset = integer(offset)
    first, offset = integer(offset)
    count, offset = integer(offset)
    fields = entries[first:first + count]
    if any(key == 'NSWindowStyleMask' for key, _, _ in fields):
        windows.append({'object': index, 'class': class_names[class_index],
                        'fields': {key: {'type': kind, 'value': value} for key, kind, value in fields}})
assert offset == key_offset
print(json.dumps({'path': str(path), 'sha256': hashlib.sha256(data).hexdigest(),
                  'archive_version': version, 'coder_version': coder, 'windows': windows}, indent=2))
