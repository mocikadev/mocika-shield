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
- 版本号由客户端声明；匹配公开 Release 只说明版本号相同，不证明使用官方安装包。未匹配也可能是自行构建，不据此判定恶意请求。

## 当前维护查询与计数规则

查看最新状态时分别查询两个来源，不使用历史快照冒充实时数：

- GitHub API：仓库关注、公开 Release 安装包下载和 Traffic。校验和及草稿不计下载；不凭下载分布扣除机器人或维护者下载，无法可靠识别时保留原始次数并说明限制。
- Worker `/stats/trend`：匿名日趋势、版本分布、固定失败分类和滚动活跃；必要时经现有授权直接查询 D1。应用分享和错误报告只走维护者私有 D1 查询，不进入公开趋势或 stats 历史。
- 任何来源失败只将对应指标标记不可用，不把不可用填零，不让 GitHub 异常影响匿名入库。

日活 `active_devices` 实为随机匿名安装实例数：对合并后的数据按 UTC 日期、`COUNT(DISTINCT anonymous_id)` 去重。近 7／30 日的 `activity_windows` 在整个窗口内独立去重，含今日，日期范围由 `start_date/end_date` 明示；与只保存完整日的日趋势不同，窗口是采集时的即时结果，不能解释为完整周或自然月。

新旧表同标识、日期、版本重叠时优先第二代表；不同版本的事件次数保留相加，实例数去重。各版本日活之和、每日活跃之和均不能作为总日活或周／月活。一个实例可能使用多版本，不等于一个自然人或组织；不能精确剔除未登记的自测。

维护快照每个 `version_trend` 行新增 `release_version_match`：`true` 匹配本次采集的已公开 Release，`false` 未匹配，Release 列表不可用时为 `null`。保留完整版本号，查看时将匹配与未匹配分开列出，不把二者解释为已鉴定的官方／非官方安装来源。

`active_devices_definition=distinct_installation_per_utc_period` 标识新口径。旧客户端仍可正常上报；旧接口缺少窗口时 `activity_windows_available=false`、`activity_windows=null`，不以日趋势加总补算。已有历史快照不回填，新旧口径须按该标识区分。可用字段缺失和没有使用记录不是同一件事。

### 兼容汇总接口

`/stats/summary` 仅保留给旧消费者，不是维护查看的主入口。GitHub 仓库和下载查询分别有超时与失败隔离；失败字段为 `null`，`availability` 指明来源状态，使用统计正常时仍返回 200。D1 查询异常返回受控 503，不缓存错误响应，不公开原始异常。

2026-10-26 已用实时 Tail 确认旧接口 500 的直接原因：Worker 没有 GitHub 密钥，上游请求出现 403，入口直接返回 Promise 而未等待，导致异步拒绝绕过 `try/catch`。D1 与趋势接口当时正常。修复不要求增加 GitHub 令牌，不改变上报地址和数据库结构。部署与验证记录见[本次维护计划](../superpowers/plans/2026-10-26-maintenance-statistics.md)。

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

匿名统计只保存随机匿名标识、日期、桌面工具版本、平台、架构和聚合计数；失败仅保存固定操作阶段，不保存错误原文。不包含 APK、证书、密码、路径、包名、最终用户标识或项目内容。随机标识只用于同一桌面工具安装实例的日期／窗口去重，不读取硬件、账户、网络或系统唯一标识。Worker 令牌不得写入前端、仓库文件或公开响应。

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
