"""Check that a Windows executable embeds exactly the images of an .ico file.

    python -I scripts/check-exe-icon.py <file.exe> <icon.ico> [--report-only]

Parses the PE resource tree (RT_ICON = 3, RT_GROUP_ICON = 14) with the
standard library only and compares every image of the .ico byte for byte
with the icon images inside the executable. Exit code 1 if any image of the
.ico is missing (unless --report-only).
"""

import struct
import sys

RT_ICON, RT_GROUP_ICON = 3, 14


def ico_images(data: bytes) -> list[bytes]:
    _, kind, count = struct.unpack_from("<HHH", data, 0)
    if kind != 1:
        raise ValueError("not an .ico file")
    out = []
    for i in range(count):
        size, offset = struct.unpack_from("<II", data, 6 + 16 * i + 8)
        out.append(data[offset : offset + size])
    return out


def pe_resources(data: bytes) -> dict[int, list[bytes]]:
    if data[:2] != b"MZ":
        raise ValueError("not a PE file")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise ValueError("bad PE signature")
    coff = pe + 4
    nsect, = struct.unpack_from("<H", data, coff + 2)
    opt_size, = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    magic, = struct.unpack_from("<H", data, opt)
    dd = opt + (112 if magic == 0x20B else 96)
    res_rva, _ = struct.unpack_from("<II", data, dd + 8 * 2)
    sections = []
    for i in range(nsect):
        s = opt + opt_size + 40 * i
        vsize, va, rawsize, rawptr = struct.unpack_from("<IIII", data, s + 8)
        sections.append((va, max(vsize, rawsize), rawptr))

    def off(rva: int) -> int:
        for va, size, raw in sections:
            if va <= rva < va + size:
                return rva - va + raw
        raise ValueError(f"RVA {rva:#x} outside sections")

    if res_rva == 0:
        return {}
    base = off(res_rva)

    def entries(d: int):
        named, ids = struct.unpack_from("<HH", data, base + d + 12)
        for k in range(named + ids):
            name, target = struct.unpack_from("<II", data, base + d + 16 + 8 * k)
            yield name, target

    def leaves(d: int):
        for _, target in entries(d):
            if target & 0x80000000:
                yield from leaves(target & 0x7FFFFFFF)
            else:
                rva, size = struct.unpack_from("<II", data, base + target)
                yield data[off(rva) : off(rva) + size]

    out: dict[int, list[bytes]] = {}
    for name, target in entries(0):
        if not name & 0x80000000 and target & 0x80000000:
            out[name] = list(leaves(target & 0x7FFFFFFF))
    return out


def main() -> int:
    exe_path, ico_path = sys.argv[1], sys.argv[2]
    report_only = "--report-only" in sys.argv[3:]
    images = ico_images(open(ico_path, "rb").read())
    res = pe_resources(open(exe_path, "rb").read())
    embedded = set(res.get(RT_ICON, []))
    found = sum(1 for img in images if img in embedded)
    groups = len(res.get(RT_GROUP_ICON, []))
    print(f"{exe_path}: {len(embedded)} icon images, {groups} icon groups; "
          f"{found}/{len(images)} images of {ico_path} embedded byte for byte")
    if found != len(images) and not report_only:
        print("ERROR: the executable does not embed the current icon", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
