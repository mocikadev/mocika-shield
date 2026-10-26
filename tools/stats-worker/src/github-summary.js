const GITHUB_REPOSITORY = "mocikadev/mocika-shield";

export function isDownloadAsset(name) {
  return !/(checksum|checksums|sha256)/i.test(name || "");
}

export function classifyPlatform(name) {
  const value = String(name || "").toLowerCase();
  if (value.includes("windows") || value.endsWith(".exe") || value.endsWith(".msi")) return "Windows";
  if (value.includes("macos") || value.endsWith(".dmg") || value.endsWith(".pkg")) return "macOS";
  if (value.includes("linux") || value.endsWith(".appimage") || value.endsWith(".deb") || value.endsWith(".rpm")) return "Linux";
  return "其他";
}

async function githubGet(path, env) {
  const headers = {
    "Accept": "application/vnd.github+json",
    "User-Agent": "mocika-shield-stats-worker",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  if (env.GITHUB_TOKEN) headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;
  const response = await fetch(`https://api.github.com/repos/${GITHUB_REPOSITORY}${path}`, {
    headers, signal: AbortSignal.timeout(10000),
  });
  if (!response.ok) throw new Error(`GitHub API 请求失败：${response.status}`);
  return response.json();
}

export function buildSummary(repository, releases, usage, now = new Date()) {
  const platformDownloads = { Windows: 0, macOS: 0, Linux: 0, "其他": 0 };
  let downloads = 0;
  for (const release of releases || []) {
    if (release.draft) continue;
    for (const asset of release.assets || []) {
      if (!isDownloadAsset(asset.name)) continue;
      const count = Number(asset.download_count || 0);
      downloads += count;
      platformDownloads[classifyPlatform(asset.name)] += count;
    }
  }
  const today = now.toISOString().slice(0, 10);
  const todayUsage = usage.find((item) => item.usage_date === today) || null;
  const completeUsage = usage.filter((item) => item.usage_date < today).slice(-14).at(-1) || null;
  return {
    updated_at: now.toISOString(),
    repository: repository ? {
      stars: Number(repository.stargazers_count || 0),
      forks: Number(repository.forks_count || 0),
      open_issues: Number(repository.open_issues_count || 0),
    } : null,
    downloads: releases ? { total: downloads, platforms: platformDownloads } : null,
    usage: { today: todayUsage, latest_complete: completeUsage },
    availability: { repository: repository !== null, downloads: releases !== null, usage: true },
  };
}

export async function getCurrentSummary(env, usagePromise) {
  const [repository, releases, usage] = await Promise.allSettled([
    githubGet("", env),
    githubGet("/releases?per_page=100", env),
    usagePromise,
  ]);
  if (usage.status === "rejected") throw usage.reason;
  return buildSummary(
    repository.status === "fulfilled" ? repository.value : null,
    releases.status === "fulfilled" ? releases.value : null,
    usage.value,
  );
}
