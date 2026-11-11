"""只读校验固定发布包，仅提取完整加固专项所需的两套 Android 资源。"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import zipfile

PACKAGE_SHA256 = "a24819d7c70120dda06bd6488869375fa55dc865c34dd1359b68364fc118eea6"
MEMBERS = {
    "usr/lib/MocikaShield/resources/resources.zip": 21,
    "usr/lib/MocikaShield/resources/resources-api19.zip": 19,
}


def payload(package):
    """严格读取已固定摘要的 Debian AR 容器，不解包其他条目。"""
    if hashlib.sha256(package).hexdigest() != PACKAGE_SHA256:
        raise ValueError("发布包摘要不匹配")
    if package[:8] != b"!<arch>\n":
        raise ValueError("发布包不是 AR 容器")
    cursor, found = 8, None
    while cursor < len(package):
        header = package[cursor:cursor + 60]
        if len(header) != 60 or header[58:] != b"`\n":
            raise ValueError("AR 条目头损坏")
        size = int(header[48:58].strip())
        start, end = cursor + 60, cursor + 60 + size
        if size < 0 or end > len(package):
            raise ValueError("AR 条目越界")
        if header[:16].rstrip(b" /") == b"data.tar.gz":
            if found is not None:
                raise ValueError("重复数据归档")
            found = package[start:end]
        cursor = end + size % 2
    if found is None or cursor != len(package):
        raise ValueError("缺少数据归档或边界错误")
    return found


def resources(data):
    result = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive:
            if member.name not in MEMBERS:
                continue
            if member.name in result or not member.isfile() or not 0 < member.size <= 32 * 1024 * 1024:
                raise ValueError("资源重复、类型或大小无效")
            content = archive.extractfile(member).read()
            with zipfile.ZipFile(io.BytesIO(content)) as zipped:
                entries = [entry for entry in zipped.infolist() if entry.filename == "metadata.json"]
                if len(entries) != 1 or entries[0].file_size > 65536:
                    raise ValueError("资源元数据缺失、重复或过大")
                metadata = json.loads(zipped.read(entries[0]))
            if not isinstance(metadata, dict) or metadata.get("version") != "1.4.3" or metadata.get("min_android_api") != MEMBERS[member.name]:
                raise ValueError("资源版本或兼容类型不匹配")
            result[member.name] = content
    if set(result) != set(MEMBERS):
        raise ValueError("两套资源不完整")
    return result


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.package.stat().st_size > 64 * 1024 * 1024:
        raise ValueError("发布包超出专项读取预算")
    selected = resources(payload(args.package.read_bytes()))
    args.output.mkdir(parents=True, exist_ok=False)
    for name, content in selected.items():
        (args.output / Path(name).name).write_bytes(content)
    print("固定发布包摘要及两套 Android 资源校验通过；未安装或执行发布包")


if __name__ == "__main__":
    main()
