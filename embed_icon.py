"""把图标写入目标 exe 的资源节，让资源管理器/任务栏也显示自定义图标。

程序本身不依赖本脚本：运行时窗口图标由 app.ico 提供。
本脚本是可选步骤，用于让 exe **文件本身**也带上图标。

用法（两种来源都支持）:
    python embed_icon.py app.ico   target.exe
    python embed_icon.py source.exe target.exe
"""
import ctypes
import struct
import sys
from ctypes import wintypes

k32 = ctypes.windll.kernel32

RT_ICON = 3
RT_GROUP_ICON = 14
LOAD_LIBRARY_AS_DATAFILE = 0x00000002

k32.LoadLibraryExW.restype = wintypes.HMODULE
k32.LoadLibraryExW.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]

k32.EnumResourceNamesW.restype = wintypes.BOOL
k32.EnumResourceNamesW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p, wintypes.LPARAM]

k32.FindResourceW.restype = ctypes.c_void_p
k32.FindResourceW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p]

k32.SizeofResource.restype = wintypes.DWORD
k32.SizeofResource.argtypes = [wintypes.HMODULE, ctypes.c_void_p]

k32.LoadResource.restype = ctypes.c_void_p
k32.LoadResource.argtypes = [wintypes.HMODULE, ctypes.c_void_p]

k32.LockResource.restype = ctypes.c_void_p
k32.LockResource.argtypes = [ctypes.c_void_p]

k32.BeginUpdateResourceW.restype = wintypes.HANDLE
k32.BeginUpdateResourceW.argtypes = [wintypes.LPCWSTR, wintypes.BOOL]

k32.UpdateResourceW.restype = wintypes.BOOL
k32.UpdateResourceW.argtypes = [
    wintypes.HANDLE, ctypes.c_void_p, ctypes.c_void_p, wintypes.WORD,
    ctypes.c_void_p, wintypes.DWORD,
]

k32.EndUpdateResourceW.restype = wintypes.BOOL
k32.EndUpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.BOOL]

k32.FreeLibrary.argtypes = [wintypes.HMODULE]


# 统一内部结构：list of dict(width, height, colorcount, planes, bitcount, data)
def icons_from_ico(path):
    raw = open(path, "rb").read()
    reserved, typ, count = struct.unpack("<HHH", raw[:6])
    if typ != 1:
        raise ValueError("不是有效的 .ico 文件")
    out = []
    for i in range(count):
        off = 6 + i * 16
        w, h, cc, _res, planes, bits, size, imgoff = struct.unpack("<BBBBHHII", raw[off:off + 16])
        out.append(dict(width=w, height=h, colorcount=cc,
                        planes=planes, bitcount=bits,
                        data=raw[imgoff:imgoff + size]))
    return out


def _enum_names(hmod, rtype):
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HMODULE, ctypes.c_void_p,
                        ctypes.c_void_p, wintypes.LPARAM)
    def cb(_h, _t, name, _l):
        found.append(ctypes.cast(name, ctypes.c_void_p).value)
        return True

    k32.EnumResourceNamesW(hmod, ctypes.c_void_p(rtype), cb, 0)
    return found


def _read_res(hmod, rtype, rname):
    h = k32.FindResourceW(hmod, ctypes.c_void_p(rname), ctypes.c_void_p(rtype))
    if not h:
        return None
    size = k32.SizeofResource(hmod, ctypes.c_void_p(h))
    hr = k32.LoadResource(hmod, ctypes.c_void_p(h))
    ptr = k32.LockResource(ctypes.c_void_p(hr))
    return ctypes.string_at(ptr, size) if ptr else None


def icons_from_exe(path):
    hmod = k32.LoadLibraryExW(path, None, LOAD_LIBRARY_AS_DATAFILE)
    if not hmod:
        raise ValueError(f"无法加载 {path}")
    try:
        groups = _enum_names(hmod, RT_GROUP_ICON)
        if not groups:
            raise ValueError("该 exe 没有图标资源")
        gdata = _read_res(hmod, RT_GROUP_ICON, groups[0])
        count = struct.unpack("<H", gdata[4:6])[0]
        out = []
        for i in range(count):
            off = 6 + i * 14
            w, h, cc, _r, planes, bits, _size, nid = struct.unpack("<BBBBHHIH", gdata[off:off + 14])
            data = _read_res(hmod, RT_ICON, nid)
            if data:
                out.append(dict(width=w, height=h, colorcount=cc,
                                planes=planes, bitcount=bits, data=data))
        return out
    finally:
        k32.FreeLibrary(hmod)


def write_icons(dst, items):
    # 生成 RT_GROUP_ICON 数据（GRPICONDIR + GRPICONDIRENTRY[]）
    grp = bytearray(struct.pack("<HHH", 0, 1, len(items)))
    for idx, it in enumerate(items, start=1):
        grp += struct.pack("<BBBBHHIH",
                           it["width"], it["height"], it["colorcount"], 0,
                           it["planes"], it["bitcount"], len(it["data"]), idx)

    hupd = k32.BeginUpdateResourceW(dst, False)
    if not hupd:
        raise OSError(f"无法打开目标 exe 进行资源更新: {dst}")

    ok = True
    for idx, it in enumerate(items, start=1):
        buf = ctypes.create_string_buffer(it["data"], len(it["data"]))
        if not k32.UpdateResourceW(hupd, ctypes.c_void_p(RT_ICON),
                                   ctypes.c_void_p(idx), 0, buf, len(it["data"])):
            ok = False

    gbuf = ctypes.create_string_buffer(bytes(grp), len(grp))
    if not k32.UpdateResourceW(hupd, ctypes.c_void_p(RT_GROUP_ICON),
                               ctypes.c_void_p(1), 0, gbuf, len(grp)):
        ok = False

    k32.EndUpdateResourceW(hupd, False if ok else True)
    return ok


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2

    src, dst = sys.argv[1], sys.argv[2]
    try:
        items = icons_from_ico(src) if src.lower().endswith(".ico") else icons_from_exe(src)
    except Exception as e:
        print(f"读取图标失败: {e}")
        return 1

    if not items:
        print("没有可用图标")
        return 1

    try:
        ok = write_icons(dst, items)
    except Exception as e:
        print(f"写入失败: {e}")
        return 1

    if ok:
        sizes = ", ".join(str(it["width"] or 256) for it in items)
        print(f"已写入 {len(items)} 个图标尺寸 ({sizes}) -> {dst}")
        return 0
    print("写入失败")
    return 1


if __name__ == "__main__":
    sys.exit(main())
