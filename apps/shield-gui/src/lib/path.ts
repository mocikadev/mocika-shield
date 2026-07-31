export function basename(path: string) {
  const normalized = path.replace(/\\/g, "/");
  return normalized.split("/").filter(Boolean).pop() ?? path;
}

export function dirname(path: string) {
  const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return slash >= 0 ? path.slice(0, slash) : ".";
}

export function fileStem(path: string) {
  const name = basename(path);
  const dot = name.toLowerCase().lastIndexOf(".apk");
  if (dot > 0) {
    return name.slice(0, dot);
  }
  const lastDot = name.lastIndexOf(".");
  return lastDot > 0 ? name.slice(0, lastDot) : name || "app";
}

export function joinPath(parent: string, filename: string) {
  if (!parent || parent === ".") {
    return filename;
  }
  const sep = parent.includes("\\") && !parent.includes("/") ? "\\" : "/";
  return `${parent.replace(/[\\/]+$/, "")}${sep}${filename}`;
}

export function protectedOutputPath(input: string) {
  if (!input) {
    return "";
  }
  return joinPath(dirname(input), `${fileStem(input)}_protected.apk`);
}

export function signedOutputPath(input: string) {
  if (!input) {
    return "";
  }
  return joinPath(dirname(input), `${fileStem(input)}_signed.apk`);
}

export function protectedOutputFilename(input: string, signed: boolean) {
  if (!input) return "";
  return `${fileStem(input)}_protected${signed ? "_signed" : ""}.apk`;
}

export function normalizeApkFilename(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return "";
  return trimmed.toLowerCase().endsWith(".apk") ? trimmed : `${trimmed}.apk`;
}

export function validateOutputFilename(value: string) {
  const normalized = normalizeApkFilename(value);
  if (!normalized) return "empty" as const;
  if (/[\\/:*?"<>|]/.test(normalized) || normalized === "." || normalized === "..") {
    return "invalid" as const;
  }
  return null;
}

export function isApk(path: string) {
  return path.toLowerCase().endsWith(".apk");
}
