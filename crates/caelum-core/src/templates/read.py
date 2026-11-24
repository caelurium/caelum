import base64
import errno
import json
import os
import re
import stat


def files():
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_DIRECTORY
    root = os.open(".", flags)
    try:
        for name in (".caelum", "commands"):
            try:
                directory = os.open(name, flags, dir_fd=root)
            except OSError as error:
                if error.errno in (errno.ENOENT, errno.ENOTDIR, errno.ELOOP):
                    return []
                raise
            os.close(root)
            root = directory
        names = []
        with os.scandir(root) as directory:
            for i, entry in enumerate(directory):
                if i >= 4096:
                    raise ValueError("Too many entries in .caelum/commands")
                if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}\.md", entry.name):
                    names.append(entry.name)
        result = []
        for name in sorted(names):
            try:
                fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=root)
            except OSError:
                continue
            try:
                meta = os.fstat(fd)
                if not stat.S_ISREG(meta.st_mode) or meta.st_size > 65536:
                    continue
                with os.fdopen(fd, "rb", closefd=False) as stream:
                    data = stream.read(65537)
                if len(data) > 65536:
                    continue
                try:
                    data.decode("utf-8")
                except UnicodeDecodeError:
                    continue
            finally:
                os.close(fd)
            result.append((name[:-3], base64.b64encode(data).decode("ascii")))
            if len(result) == 128:
                break
        return result
    finally:
        os.close(root)


print(json.dumps(files(), ensure_ascii=False))
