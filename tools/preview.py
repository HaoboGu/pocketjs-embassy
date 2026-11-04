"""Convert the native preview's PPM output to PNG using the Python standard library."""
from pathlib import Path
import struct
import zlib

root = Path(__file__).resolve().parent.parent
magic, dimensions, maximum, rgb = (root / "target/preview.ppm").read_bytes().split(b"\n", 3)
assert magic == b"P6" and maximum == b"255"
width, height = map(int, dimensions.split())
assert len(rgb) == width * height * 3
rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
png = bytearray(b"\x89PNG\r\n\x1a\n")
for kind, data in (
    (b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)),
    (b"IDAT", zlib.compress(rows)),
    (b"IEND", b""),
):
    png += struct.pack(">I", len(data)) + kind + data
    png += struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
path = root / "target/preview.png"
path.write_bytes(png)
print(path)
