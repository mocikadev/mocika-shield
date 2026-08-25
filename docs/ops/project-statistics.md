# 项目维护统计

本文档说明 Mocika Shield 仅供维护者使用的聚合统计方案。项目不再提供公开统计看板，README 和 GitHub Pages 均不展示下载、星标、访客、克隆或匿名使用趋势。

## 目标与边界

维护统计用于辅助判断：

- 各平台发布包的大致下载情况
- 新版本发布后的下载变化
- 仓库近期访问趋势
- 开启匿名统计的桌面工具产生的启动、加固、签名和固定失败阶段汇总

这些数据都不是精确用户数：

- 下载次数包含重复下载和维护者测试下载
- GitHub 独立访客与独立克隆是最近 14 天窗口内的近似值
- 克隆数据可能包含机器人、CI 和重复拉取，只作为排查线索
- 星标代表公开关注，不等于实际使用
- 匿名数据只覆盖允许并成功完成上报的客户端

## 数据与存储

| 指标 | 来源 | 用途 |
|------|------|------|
| 发布包及平台下载量 | GitHub Releases | 观察版本和平台需求 |
| 访问与克隆 | GitHub Traffic | 维护分析，不公开展示 |
| 星标、复刻、未关闭事项 | GitHub Repository | 维护分析 |
| 启动、加固、签名与固定失败阶段计数 | 匿名统计 Worker / D1 | 按桌面工具版本观察使用趋势与异常集中情况 |

校验和文件不计入发布包下载量。采集结果保存到 `stats` 分支的 `data/history.json`，不进入 `main` 提交历史，也不生成 HTML、SVG 或其他公开页面。

## 自动化流程

工作流每天北京时间上午 9 点运行，也支持手动执行：

```text
读取 stats 分支历史数据
        ↓
调用 GitHub API 和匿名统计接口
        ↓
按 UTC 日期追加或替换当日快照
        ↓
保存 data/history.json
        ↓
提交并推送 stats 分支
```

同一天重复执行会替换当天快照。Traffic 权限不足或匿名统计接口不可用时，对应数据明确标记不可用，不会错误写成零，也不阻断其他指标采集。

## 权限与安全

如需读取 GitHub Traffic，使用只限本仓库、只读管理元数据的细粒度令牌，并保存为 Actions 密钥 `STATS_TOKEN`。不要复用维护者日常使用的宽权限令牌。

匿名统计只保存随机匿名标识、日期、桌面工具版本、平台、架构和聚合计数；失败仅保存固定操作阶段，不保存错误原文。不包含 APK、证书、密码、路径、包名、最终用户标识或项目内容。随机标识只用于同一桌面工具安装实例的日去重，不读取硬件、账户、网络或系统唯一标识。Worker 令牌不得写入前端、仓库文件或公开响应。

`/stats/trend` 始终保留按日期的 `data`，新版响应额外提供 `versions` 与 `failure_breakdown`。维护脚本遇到旧响应或细分字段缺失时，明确写为不可用，不将历史空白解释为零或重新归属版本。

## 本地验证

```bash
GITHUB_TOKEN="$(gh auth token)" \
python3 scripts/project_stats.py \
  --repository mocikadev/mocika-shield \
  --history-file /tmp/mocika-stats/history.json \
  --output-dir /tmp/mocika-stats

python3 -m unittest scripts.tests.test_project_stats
```

生成结果应仅包含 `data/history.json`。
